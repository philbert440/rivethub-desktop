mod controls;
mod input;
mod path;
mod picture;

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use app::Hub;
use gpui::{
    Bounds, Context, DispatchPhase, EventEmitter, FocusHandle, HitboxBehavior, IntoElement, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, PinchEvent, Pixels, Render, ScrollWheelEvent, Styled, Task,
    Window, actions, canvas, div, prelude::*, px,
};
use model::plugin::board::{Board, Hit, Outbox};
use model::plugin::canvas::{Event, MenuItem, Op, Placed, Point};
use model::plugin::view::{Action, CanvasBlock};
use model::plugin::{Context as About, Params, PluginRef};

use crate::theme::Theme;
use path::Segment;

actions!(
    plugin_canvas,
    [
        DeleteSelection,
        Activate,
        ClearSelection,
        ZoomIn,
        ZoomOut,
        FitView
    ]
);

/// A canvas's height when its block doesn't say.
const HEIGHT: f32 = 420.;
/// How near the pointer counts as on an item or an anchor, in screen pixels.
const SLACK: f32 = 6.;
const ANCHOR_SLACK: f32 = 9.;
const ANCHOR_RADIUS: f32 = 5.;
/// How far a press moves before it is a drag, in screen pixels.
const DRAG_START: f32 = 3.;
/// How long the pointer rests on an item before its tooltip shows.
const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
const ZOOM_STEP: f32 = 1.25;

/// Where a canvas's events go: its plug-in, its id, and the params and context of its view.
#[derive(Debug, Clone)]
pub struct CanvasTarget {
    pub plugin: PluginRef,
    pub canvas: String,
    pub params: Option<Params>,
    pub context: About,
}

/// The plug-in answered an event with what to do next.
pub struct Replied(pub Vec<Action>);

enum Gesture {
    /// Dragging empty space; a press that never moves clears the selection.
    Pan {
        last: gpui::Point<Pixels>,
        moved: bool,
    },
    Drag {
        start: gpui::Point<Pixels>,
        from: Vec<Placed>,
        now: Vec<Placed>,
        moved: bool,
    },
    Box {
        start: Point,
        now: Point,
    },
    Connect {
        node: String,
        anchor: String,
        now: Point,
        target: Option<Hit>,
    },
}

struct OpenMenu {
    at: gpui::Point<Pixels>,
    on: Point,
    item: Option<String>,
    entries: Vec<MenuItem>,
}

/// A plug-in's canvas, drawn by the app: pan, zoom, selection, dragging and connecting happen
/// here, and the plug-in hears only their results.
pub struct CanvasView {
    hub: Hub,
    focus: FocusHandle,
    target: CanvasTarget,
    board: Board,
    read_only: bool,
    menu: Vec<MenuItem>,
    height: f32,
    outbox: Outbox,
    gesture: Option<Gesture>,
    hover: Option<Hit>,
    pointer: Option<gpui::Point<Pixels>>,
    tooltip: bool,
    resting: Option<Task<()>>,
    open_menu: Option<OpenMenu>,
    bounds: Option<Bounds<Pixels>>,
    paths: HashMap<String, Rc<[Segment]>>,
    /// Where the person dropped nodes, and when, so a fetch asked for before a drop doesn't put
    /// them back.
    dropped: Vec<(Placed, Instant)>,
    problem: Option<String>,
}

impl EventEmitter<Replied> for CanvasView {}

impl CanvasView {
    pub fn new(hub: Hub, block: &CanvasBlock, target: CanvasTarget, cx: &mut Context<Self>) -> Self {
        Self {
            hub,
            focus: cx.focus_handle(),
            target,
            board: Board::new(block.scene.clone()),
            read_only: block.read_only,
            menu: block.menu.clone(),
            height: block.height.unwrap_or(HEIGHT),
            outbox: Outbox::default(),
            gesture: None,
            hover: None,
            pointer: None,
            tooltip: false,
            resting: None,
            open_menu: None,
            bounds: None,
            paths: HashMap::new(),
            dropped: Vec::new(),
            problem: None,
        }
    }

    /// Takes the canvas a fetch of its view, asked for at `asked`, brought, keeping where the view
    /// is and what's selected. Nodes dropped since it was asked for, or being dragged, stay where
    /// the person put them.
    pub fn renew(
        &mut self,
        block: &CanvasBlock,
        target: CanvasTarget,
        asked: Instant,
        cx: &mut Context<Self>,
    ) {
        self.board.renew(block.scene.clone());
        self.dropped.retain(|(_, at)| *at > asked);
        let kept: Vec<Placed> = self.dropped.iter().map(|(spot, _)| spot.clone()).collect();
        self.board.place(&kept);
        if let Some(Gesture::Drag { now, .. }) = &self.gesture {
            self.board.place(now);
        }
        self.read_only = block.read_only;
        self.menu.clone_from(&block.menu);
        self.height = block.height.unwrap_or(HEIGHT);
        self.target = target;
        cx.notify();
    }

    fn size(&self) -> Point {
        self.bounds.map_or(Point::new(800., self.height), |bounds| {
            Point::new(f32::from(bounds.size.width), f32::from(bounds.size.height))
        })
    }

    fn zoom(&self) -> f32 {
        self.board.viewport().map_or(1., |view| view.zoom)
    }

    /// The pointer's place in screen pixels from the canvas's top-left.
    fn local(&self, at: gpui::Point<Pixels>) -> Point {
        let origin = self.bounds.map_or(at, |bounds| bounds.origin);
        Point::new(f32::from(at.x - origin.x), f32::from(at.y - origin.y))
    }

    fn tell(&mut self, event: Event, cx: &mut Context<Self>) {
        self.outbox.push(event);
        self.send(cx);
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(event) = self.outbox.take() else {
            return;
        };
        let hub = self.hub.clone();
        let target = self.target.clone();
        cx.spawn(async move |this, cx| {
            let replied = hub
                .canvas_event(
                    target.plugin,
                    target.canvas,
                    event.clone(),
                    target.params,
                    target.context,
                )
                .await;
            this.update(cx, |this, cx| {
                this.outbox.answered();
                this.board.settle(&event);
                match replied {
                    Ok(reply) => {
                        this.problem = None;
                        for op in &reply.ops {
                            if let Op::Upsert(item) = op {
                                this.dropped.retain(|(spot, _)| spot.id != item.id());
                            }
                        }
                        this.board.apply(reply.ops);
                        if !reply.actions.is_empty() {
                            cx.emit(Replied(reply.actions));
                        }
                    }
                    Err(err) => this.problem = Some(err.to_string()),
                }
                this.send(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let focused = self.focus.is_focused(window);
        let drawing = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                let picture = view.update(cx, |this, _| this.picture(bounds, focused));
                picture.paint(bounds, &hitbox, window, cx);
                window.on_mouse_event({
                    let (view, hitbox) = (view.clone(), hitbox.clone());
                    move |event: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                            view.update(cx, |this, cx| {
                                window.focus(&this.focus, cx);
                                this.down(event, cx);
                            });
                            cx.stop_propagation();
                        }
                    }
                });
                window.on_mouse_event({
                    let (view, hitbox) = (view.clone(), hitbox.clone());
                    move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            let over = hitbox.is_hovered(window);
                            view.update(cx, |this, cx| this.moved(event, over, cx));
                        }
                    }
                });
                window.on_mouse_event({
                    let view = view.clone();
                    move |event: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            view.update(cx, |this, cx| this.up(event, cx));
                        }
                    }
                });
                window.on_mouse_event({
                    let (view, hitbox) = (view.clone(), hitbox.clone());
                    move |event: &ScrollWheelEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                            view.update(cx, |this, cx| this.scrolled(event, cx));
                            cx.stop_propagation();
                        }
                    }
                });
                window.on_mouse_event({
                    let view = view.clone();
                    move |event: &PinchEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                            view.update(cx, |this, cx| this.pinched(event, cx));
                            cx.stop_propagation();
                        }
                    }
                });
            },
        )
        .size_full();
        div()
            .relative()
            .w_full()
            .h(px(self.height))
            .flex_none()
            .key_context("PluginCanvas")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::delete_selection))
            .on_action(cx.listener(Self::activate))
            .on_action(cx.listener(Self::clear_selection))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::fit_view))
            // A click anywhere else takes the keyboard back, so Delete can't reach a selection
            // the person has moved on from.
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                if this.focus.is_focused(window) {
                    window.blur(cx);
                    cx.notify();
                }
            }))
            .child(drawing)
            .child(self.controls(cx))
            .children(self.problem.clone().map(|problem| {
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(12.))
                    .max_w(px(420.))
                    .px_3()
                    .py_1()
                    .rounded(px(12.))
                    .bg(Theme::danger_container())
                    .text_xs()
                    .text_color(Theme::danger())
                    .child(problem)
            }))
            .children(self.tooltip())
            .children(self.menu_overlay(cx))
    }
}
