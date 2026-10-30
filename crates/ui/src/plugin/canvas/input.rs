use std::time::Instant;

use gpui::{
    Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, ScrollWheelEvent,
    Window, px,
};
use model::plugin::board::Hit;
use model::plugin::canvas::{End, Event, Placed, Point, Rect};

use super::{
    ANCHOR_SLACK, Activate, CanvasView, ClearSelection, DRAG_START, DeleteSelection, FitView, Gesture,
    OpenMenu, SLACK, TOOLTIP_DELAY, ZOOM_STEP, ZoomIn, ZoomOut,
};

impl CanvasView {
    pub(super) fn down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.open_menu.take().is_some() {
            cx.notify();
            return;
        }
        let local = self.local(event.position);
        let at = self.board.to_canvas(local);
        let zoom = self.zoom();
        let hit = self
            .board
            .hit(at, ANCHOR_SLACK / zoom, !self.read_only)
            .filter(|hit| matches!(hit, Hit::Anchor { .. }))
            .or_else(|| self.board.hit(at, SLACK / zoom, false));
        if event.button == MouseButton::Right {
            self.open_menu_at(event.position, at, hit.as_ref(), cx);
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let toggle = event.modifiers.shift || event.modifiers.platform;
        match hit {
            Some(Hit::Anchor { node, anchor }) if self.board.starts_connectors(&node, &anchor) => {
                self.gesture = Some(Gesture::Connect {
                    node,
                    anchor,
                    now: at,
                    target: None,
                });
            }
            Some(hit) if event.click_count >= 2 => self.tell(Event::Activated(hit.item().to_owned()), cx),
            Some(hit) => {
                let id = hit.item().to_owned();
                if toggle || !self.board.is_selected(&id) {
                    self.board.select(&id, toggle);
                    self.tell(Event::Selected(self.board.selection().to_vec()), cx);
                }
                if !toggle && !self.read_only && matches!(hit, Hit::Node(_) | Hit::Anchor { .. }) {
                    let from = self.board.draggable();
                    self.gesture = Some(Gesture::Drag {
                        start: event.position,
                        now: from.clone(),
                        from,
                        moved: false,
                    });
                }
            }
            None if event.modifiers.shift => self.gesture = Some(Gesture::Box { start: at, now: at }),
            None => {
                self.gesture = Some(Gesture::Pan {
                    last: event.position,
                    moved: false,
                });
            }
        }
        cx.notify();
    }

    pub(super) fn delete_selection(&mut self, _: &DeleteSelection, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.gesture.is_some() {
            return;
        }
        let deleted = self.board.delete_selection();
        if !deleted.is_empty() {
            self.tell(Event::Deleted(deleted), cx);
            cx.notify();
        }
    }

    /// Activates the one item selected, as a double-click on it does.
    pub(super) fn activate(&mut self, _: &Activate, _: &mut Window, cx: &mut Context<Self>) {
        if let [only] = self.board.selection() {
            let item = only.clone();
            self.tell(Event::Activated(item), cx);
        }
    }

    pub(super) fn clear_selection(&mut self, _: &ClearSelection, _: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        if self.board.clear_selection() {
            self.tell(Event::Selected(Vec::new()), cx);
        }
        cx.notify();
    }

    pub(super) fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(ZOOM_STEP, cx);
    }

    pub(super) fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(1. / ZOOM_STEP, cx);
    }

    pub(super) fn fit_view(&mut self, _: &FitView, _: &mut Window, cx: &mut Context<Self>) {
        let size = self.size();
        self.board.fit(size);
        cx.notify();
    }

    fn open_menu_at(
        &mut self,
        at: gpui::Point<Pixels>,
        on: Point,
        hit: Option<&Hit>,
        cx: &mut Context<Self>,
    ) {
        let item = hit.map(|hit| hit.item().to_owned());
        let entries = match &item {
            Some(id) => self
                .board
                .scene()
                .item(id)
                .map(|item| item.menu().to_vec())
                .unwrap_or_default(),
            None => self.menu.clone(),
        };
        if entries.is_empty() {
            return;
        }
        if let Some(id) = item.as_deref()
            && !self.board.is_selected(id)
        {
            self.board.select(id, false);
            self.tell(Event::Selected(self.board.selection().to_vec()), cx);
        }
        self.open_menu = Some(OpenMenu {
            at,
            on,
            item,
            entries,
        });
        cx.notify();
    }

    pub(super) fn moved(&mut self, event: &MouseMoveEvent, over: bool, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        let at = self.board.to_canvas(local);
        let zoom = self.zoom();
        match self.gesture.as_mut() {
            Some(Gesture::Pan { last, moved }) => {
                let by = event.position - *last;
                *last = event.position;
                *moved = true;
                self.board.pan(Point::new(f32::from(by.x), f32::from(by.y)));
            }
            Some(Gesture::Drag {
                start,
                from,
                now,
                moved,
            }) => {
                let by = event.position - *start;
                let by = Point::new(f32::from(by.x), f32::from(by.y));
                if !*moved && by.length() < DRAG_START {
                    return;
                }
                *moved = true;
                *now = self.board.dragged(from, by * (1. / zoom));
                self.board.place(now);
            }
            Some(Gesture::Box { now, .. }) => *now = at,
            Some(Gesture::Connect { now, target, .. }) => {
                *now = at;
                *target = self
                    .board
                    .hit(at, ANCHOR_SLACK / zoom, true)
                    .filter(|hit| matches!(hit, Hit::Anchor { .. }))
                    .or_else(|| self.board.hit(at, 0., false));
            }
            None => {
                let pointer = over.then_some(event.position);
                let hover = over
                    .then(|| {
                        self.board
                            .hit(at, ANCHOR_SLACK / zoom, !self.read_only)
                            .filter(|hit| matches!(hit, Hit::Anchor { .. }))
                            .or_else(|| self.board.hit(at, SLACK / zoom, false))
                    })
                    .flatten();
                let tooltip_moves = self.tooltip && pointer != self.pointer;
                self.pointer = pointer;
                if hover != self.hover {
                    self.hover = hover;
                    self.rest(cx);
                    cx.notify();
                } else if tooltip_moves {
                    cx.notify();
                }
                return;
            }
        }
        cx.notify();
    }

    /// Shows the hovered item's tooltip once the pointer has rested on it.
    fn rest(&mut self, cx: &mut Context<Self>) {
        self.tooltip = false;
        let resting = self.hover.clone();
        self.resting = resting.is_some().then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TOOLTIP_DELAY).await;
                this.update(cx, |this, cx| {
                    if this.hover == resting {
                        this.tooltip = true;
                        cx.notify();
                    }
                })
                .ok();
            })
        });
    }

    pub(super) fn up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        match gesture {
            Gesture::Pan { moved: false, .. } => {
                if self.board.clear_selection() {
                    self.tell(Event::Selected(Vec::new()), cx);
                }
            }
            Gesture::Drag {
                from,
                now,
                moved: true,
                ..
            } => {
                let dropped: Vec<Placed> = now.into_iter().filter(|spot| !from.contains(spot)).collect();
                if !dropped.is_empty() {
                    let at = Instant::now();
                    self.dropped
                        .retain(|(spot, _)| !dropped.iter().any(|drop| drop.id == spot.id));
                    self.dropped.extend(dropped.iter().map(|spot| (spot.clone(), at)));
                    self.tell(Event::Moved(dropped), cx);
                }
            }
            Gesture::Pan { .. } | Gesture::Drag { .. } => {}
            Gesture::Box { start, now } => {
                if self.board.select_within(Rect::between(start, now), false) {
                    self.tell(Event::Selected(self.board.selection().to_vec()), cx);
                }
            }
            Gesture::Connect {
                node, anchor, target, ..
            } => {
                if let Some(to) = self.board.connection((&node, &anchor), target.as_ref()) {
                    let from = End {
                        node,
                        anchor: Some(anchor),
                    };
                    self.board.propose(from.clone(), to.clone());
                    self.tell(Event::Connected { from, to }, cx);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn scrolled(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(20.));
        let by = Point::new(f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control || event.modifiers.platform {
            self.board
                .zoom_at(self.local(event.position), (by.y * 0.002).exp());
        } else {
            self.board.pan(by);
        }
        cx.notify();
    }

    pub(super) fn pinched(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        self.board.zoom_at(self.local(event.position), 1. + event.delta);
        cx.notify();
    }

    pub(super) fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let center = self.size() * 0.5;
        self.board.zoom_at(center, factor);
        cx.notify();
    }

    pub(super) fn choose(&mut self, choice: String, cx: &mut Context<Self>) {
        let Some(menu) = self.open_menu.take() else {
            return;
        };
        self.tell(
            Event::Menu {
                item: menu.item,
                choice,
                at: menu.on,
            },
            cx,
        );
        cx.notify();
    }
}
