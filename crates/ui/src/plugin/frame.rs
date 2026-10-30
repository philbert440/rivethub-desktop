use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use app::Hub;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, ImageFormat, IntoElement, ParentElement,
    Pixels, Point, Render, ScrollHandle, Styled, Subscription, Window, anchored, deferred, div, prelude::*,
    px,
};
use model::plugin::view::{Action, Answer, Block, Button, ButtonStyle, CanvasBlock, Confirm, Field, View};
use model::plugin::{Context as About, Params, PluginRef};

use super::canvas::{CanvasTarget, CanvasView, Replied};
use super::form::{FormSubmitted, FormView};
use crate::theme::Theme;
use crate::{brand, shell};

/// Where a view is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Page,
    Dialog,
    Card,
}

pub enum FrameEvent {
    /// Actions for the plug-in host to run, in order.
    Run(Vec<Action>),
    /// A button asks before its action runs.
    Confirm(Confirm, Pressed),
}

/// A button's action, and the form it belongs to.
#[derive(Debug, Clone)]
pub struct Pressed {
    pub action: Action,
    pub form: Option<String>,
}

enum Shown {
    Waiting,
    Ready(View),
    Failed(String),
}

pub(super) struct FormState {
    pub(super) view: Entity<FormView>,
    /// What the plug-in said when it refused the form last.
    pub(super) refusal: Option<String>,
    buttons: Vec<Button>,
    _submitted: Subscription,
}

pub(super) enum Picture {
    Loading,
    Ready(Arc<gpui::Image>),
    Failed,
}

/// What a view holds that the frame keeps between fetches.
#[derive(Default)]
struct Found<'a> {
    forms: Vec<(&'a str, &'a [Field], &'a [Button])>,
    canvases: Vec<&'a CanvasBlock>,
    images: Vec<&'a str>,
    logs: Vec<String>,
}

/// The key of the block at `index` in a list of blocks reached by `parent`, so local state
/// (a tab chosen, a section folded, a column sorted) stays with its block across fetches.
pub(super) fn child_key(parent: &str, index: usize) -> String {
    if parent.is_empty() {
        index.to_string()
    } else {
        format!("{parent}.{index}")
    }
}

fn gather<'a>(blocks: &'a [Block], key: &str, found: &mut Found<'a>) {
    for (index, block) in blocks.iter().enumerate() {
        let key = child_key(key, index);
        match block {
            Block::Section { blocks, .. } => gather(blocks, &key, found),
            Block::Split {
                primary, secondary, ..
            } => {
                gather(primary, &format!("{key}p"), found);
                gather(secondary, &format!("{key}s"), found);
            }
            Block::Tabs { tabs, .. } => {
                for tab in tabs {
                    gather(&tab.blocks, &format!("{key}:{}", tab.id), found);
                }
            }
            Block::Form(form) => found.forms.push((&form.id, &form.fields, &form.buttons)),
            Block::Canvas(canvas) => found.canvases.push(canvas),
            Block::Image { path, .. } => found.images.push(path),
            Block::Code { follow: true, .. } => found.logs.push(key),
            _ => {}
        }
    }
}

/// One of a plug-in's views on screen, as a page, a dialog or a topic card: it fetches the view,
/// keeps what the person does locally, and runs the plug-in's actions through its host.
pub struct Frame {
    pub(super) hub: Hub,
    pub(super) plugin: PluginRef,
    pub(super) view: String,
    pub(super) params: Option<Params>,
    pub(super) context: About,
    pub(super) placement: Placement,
    /// How many pages lie under this one.
    depth: usize,
    shown: Shown,
    loading: bool,
    generation: u64,
    pub(super) busy: bool,
    problem: Option<String>,
    pub(super) tabs: HashMap<String, String>,
    pub(super) folded: HashMap<String, bool>,
    pub(super) sorts: HashMap<String, (usize, bool)>,
    pub(super) forms: HashMap<String, FormState>,
    pub(super) canvases: HashMap<String, (Entity<CanvasView>, Subscription)>,
    pub(super) pictures: HashMap<String, Picture>,
    pub(super) logs: HashMap<String, ScrollHandle>,
    pub(super) menu: Option<(Point<Pixels>, Vec<Button>)>,
    scroll: ScrollHandle,
}

impl EventEmitter<FrameEvent> for Frame {}

impl Frame {
    pub fn new(
        hub: Hub,
        plugin: PluginRef,
        view: String,
        params: Option<Params>,
        context: About,
        placement: Placement,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut frame = Self {
            hub,
            plugin,
            view,
            params,
            context,
            placement,
            depth: 0,
            shown: Shown::Waiting,
            loading: false,
            generation: 0,
            busy: false,
            problem: None,
            tabs: HashMap::new(),
            folded: HashMap::new(),
            sorts: HashMap::new(),
            forms: HashMap::new(),
            canvases: HashMap::new(),
            pictures: HashMap::new(),
            logs: HashMap::new(),
            menu: None,
            scroll: ScrollHandle::new(),
        };
        frame.load(cx);
        frame
    }

    pub fn plugin(&self) -> &PluginRef {
        &self.plugin
    }

    pub fn view_name(&self) -> &str {
        &self.view
    }

    pub fn set_depth(&mut self, depth: usize, cx: &mut Context<Self>) {
        self.depth = depth;
        cx.notify();
    }

    /// Fetches the view again, keeping what is on screen until the new one arrives.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.generation += 1;
        let generation = self.generation;
        let asked = Instant::now();
        let hub = self.hub.clone();
        let request = (
            self.plugin.clone(),
            self.view.clone(),
            self.params.clone(),
            self.context.clone(),
        );
        cx.spawn(async move |this, cx| {
            let (plugin, view, params, context) = request;
            let fetched = hub.plugin_view(plugin, view, params, context).await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match fetched {
                    Ok(view) => {
                        this.problem = None;
                        this.adopt(&view, asked, cx);
                        this.shown = Shown::Ready(view);
                    }
                    Err(err) if matches!(this.shown, Shown::Ready(_)) => this.problem = Some(err.to_string()),
                    Err(err) => this.shown = Shown::Failed(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Shows another view, or the same one with other params, in place of this one. Local state
    /// stays when the view is the same, so a canvas keeps where it was.
    pub fn retarget(&mut self, view: String, params: Option<Params>, cx: &mut Context<Self>) {
        if view != self.view {
            self.tabs.clear();
            self.folded.clear();
            self.sorts.clear();
            self.forms.clear();
            self.canvases.clear();
            self.logs.clear();
            self.shown = Shown::Waiting;
            self.scroll = ScrollHandle::new();
        }
        self.view = view;
        self.params = params;
        self.load(cx);
    }

    /// Brings the frame's forms, canvases and images in line with a view fetched as `asked`.
    fn adopt(&mut self, view: &View, asked: Instant, cx: &mut Context<Self>) {
        let mut found = Found::default();
        gather(&view.blocks, "", &mut found);
        self.forms
            .retain(|id, _| found.forms.iter().any(|(form, ..)| form == id));
        for (id, fields, buttons) in found.forms {
            let unchanged = self
                .forms
                .get(id)
                .is_some_and(|state| state.view.read(cx).given() == fields);
            if let Some(state) = self.forms.get_mut(id).filter(|_| unchanged) {
                state.buttons = buttons.to_vec();
                continue;
            }
            let form = cx.new(|cx| FormView::new(fields.to_vec(), &[], cx));
            let named = id.to_owned();
            let submitted = cx.subscribe(&form, move |this, _, _: &FormSubmitted, cx| {
                this.submit(&named, cx);
            });
            self.forms.insert(
                id.to_owned(),
                FormState {
                    view: form,
                    refusal: None,
                    buttons: buttons.to_vec(),
                    _submitted: submitted,
                },
            );
        }
        self.canvases
            .retain(|id, _| found.canvases.iter().any(|canvas| &canvas.id == id));
        for block in found.canvases {
            let target = CanvasTarget {
                plugin: self.plugin.clone(),
                canvas: block.id.clone(),
                params: self.params.clone(),
                context: self.context.clone(),
            };
            if let Some((canvas, _)) = self.canvases.get(&block.id) {
                canvas.update(cx, |canvas, cx| canvas.renew(block, target, asked, cx));
                continue;
            }
            let hub = self.hub.clone();
            let canvas = cx.new(|cx| CanvasView::new(hub, block, target, cx));
            let replied = cx.subscribe(&canvas, |_, _, Replied(actions), cx| {
                cx.emit(FrameEvent::Run(actions.clone()));
            });
            self.canvases.insert(block.id.clone(), (canvas, replied));
        }
        for path in found.images {
            if !self.pictures.contains_key(path) {
                self.fetch_picture(path.to_owned(), cx);
            }
        }
        for key in found.logs {
            self.logs.entry(key).or_default().scroll_to_bottom();
        }
    }

    fn fetch_picture(&mut self, path: String, cx: &mut Context<Self>) {
        self.pictures.insert(path.clone(), Picture::Loading);
        let hub = self.hub.clone();
        let plugin = self.plugin.clone();
        cx.spawn(async move |this, cx| {
            let fetched = hub.plugin_file(plugin, path.clone()).await;
            this.update(cx, |this, cx| {
                let format = |media_type: Option<&str>| media_type.and_then(ImageFormat::from_mime_type);
                let picture = match fetched {
                    Ok((bytes, media_type)) => format(media_type.as_deref())
                        .map_or(Picture::Failed, |format| {
                            Picture::Ready(Arc::new(gpui::Image::from_bytes(format, bytes)))
                        }),
                    Err(_) => Picture::Failed,
                };
                this.pictures.insert(path, picture);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A button was pressed: asks first when it says to, then runs its action.
    pub(super) fn press(&mut self, button: &Button, form: Option<&str>, cx: &mut Context<Self>) {
        if button.disabled {
            return;
        }
        self.menu = None;
        let pressed = Pressed {
            action: button.action.clone(),
            form: form.map(str::to_owned),
        };
        match &button.confirm {
            Some(confirm) => cx.emit(FrameEvent::Confirm(confirm.clone(), pressed)),
            None => self.run_pressed(pressed, cx),
        }
    }

    pub fn run_pressed(&mut self, pressed: Pressed, cx: &mut Context<Self>) {
        match pressed.action {
            Action::Call { action, params } => self.call(action, params, pressed.form, cx),
            other => cx.emit(FrameEvent::Run(vec![other])),
        }
    }

    /// Enter in a form's field presses its primary button, or its only one.
    fn submit(&mut self, form: &str, cx: &mut Context<Self>) {
        let Some(state) = self.forms.get(form) else {
            return;
        };
        let button = state
            .buttons
            .iter()
            .find(|button| button.style == ButtonStyle::Primary)
            .or_else(|| state.buttons.first().filter(|_| state.buttons.len() == 1))
            .cloned();
        if let Some(button) = button {
            self.press(&button, Some(form), cx);
        }
    }

    fn call(&mut self, action: String, params: Option<Params>, form: Option<String>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let values = match form.as_deref().and_then(|form| self.forms.get(form)) {
            Some(state) => {
                if !state.view.update(cx, FormView::check) {
                    return;
                }
                state.view.read(cx).values(cx)
            }
            None => BTreeMap::default(),
        };
        self.busy = true;
        self.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        let (plugin, context) = (self.plugin.clone(), self.context.clone());
        cx.spawn(async move |this, cx| {
            let answered = hub.plugin_action(plugin, action, params, values, context).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                let state = form.as_deref().and_then(|form| this.forms.get_mut(form));
                match (answered, state) {
                    (Ok(Answer::Done(actions)), state) => {
                        if let Some(state) = state {
                            state.refusal = None;
                        }
                        cx.emit(FrameEvent::Run(actions));
                    }
                    (Ok(Answer::Refused { message, fields }), Some(state)) => {
                        state.refusal = Some(message);
                        state.view.update(cx, |form, cx| form.refused(fields, cx));
                    }
                    (Ok(Answer::Refused { message, .. }), None) => this.problem = Some(message),
                    (Err(err), _) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_menu(&mut self, at: Point<Pixels>, buttons: Vec<Button>, cx: &mut Context<Self>) {
        if !buttons.is_empty() {
            self.menu = Some((at, buttons));
            cx.notify();
        }
    }

    fn back(cx: &mut Context<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(FrameEvent::Run(vec![Action::Back])))
    }

    fn icon_button(id: &'static str, icon: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(36.))
            .flex_none()
            .rounded(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .child(brand::icon(brand::symbol(Some(icon)), 18., Theme::ink()))
    }

    fn body(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let level = u8::from(self.placement == Placement::Dialog);
        let mut body: Vec<AnyElement> = self
            .problem
            .iter()
            .map(|problem| {
                div()
                    .text_xs()
                    .text_color(Theme::danger())
                    .child(problem.clone())
                    .into_any_element()
            })
            .collect();
        match &self.shown {
            Shown::Waiting => body.push(
                div()
                    .text_sm()
                    .text_color(Theme::ink_dim())
                    .child("Loading…")
                    .into_any_element(),
            ),
            Shown::Failed(problem) => body.push(
                div()
                    .p_4()
                    .rounded(px(16.))
                    .bg(Theme::danger_container())
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_color(Theme::danger()).child(problem.clone()))
                    .child(
                        div().flex().child(
                            div()
                                .id("frame-retry")
                                .h(px(30.))
                                .px_4()
                                .rounded(px(15.))
                                .border_1()
                                .border_color(Theme::line())
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_sm()
                                .hover(|button| button.bg(Theme::raised()))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.load(cx)))
                                .child("Try again"),
                        ),
                    )
                    .into_any_element(),
            ),
            Shown::Ready(view) => body.extend(self.blocks(&view.blocks, "", level, window, cx)),
        }
        body
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match &self.shown {
            Shown::Ready(view) => view
                .toolbar
                .iter()
                .enumerate()
                .map(|(index, button)| self.button(button, format!("toolbar-{index}").into(), None, cx))
                .collect(),
            Shown::Waiting | Shown::Failed(_) => Vec::new(),
        }
    }

    fn heading(&self) -> (String, Option<String>) {
        match &self.shown {
            Shown::Ready(view) => (view.title.clone(), view.subtitle.clone()),
            Shown::Waiting | Shown::Failed(_) => (String::new(), None),
        }
    }

    fn page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (title, subtitle) = self.heading();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                shell::header()
                    .when(self.depth > 0, |header| {
                        header.child(Self::icon_button("frame-back", "arrow_back").on_click(Self::back(cx)))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(shell::title(title).truncate())
                            .children(subtitle.map(|subtitle| {
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .truncate()
                                    .child(subtitle)
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .rounded(px(8.))
                            .bg(Theme::card())
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(self.plugin.node.clone()),
                    )
                    .child(div().flex_1())
                    .when(self.loading && matches!(self.shown, Shown::Ready(_)), |header| {
                        header.child(div().text_xs().text_color(Theme::ink_dim()).child("Updating…"))
                    })
                    .children(self.toolbar(cx)),
            )
            .child(
                shell::tile().child(
                    div()
                        .id("plugin-page")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(
                            div()
                                .p_6()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .children(self.body(window, cx)),
                        ),
                ),
            )
            .into_any_element()
    }

    fn dialog(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (title, subtitle) = self.heading();
        div()
            .w(px(560.))
            .max_h(window.viewport_size().height * 0.85)
            .rounded(px(28.))
            .bg(Theme::card())
            .border_1()
            .border_color(Theme::line())
            .flex()
            .flex_col()
            .occlude()
            .child(
                div()
                    .px_6()
                    .pt_5()
                    .pb_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(shell::title(title))
                            .children(subtitle.map(|subtitle| {
                                div().text_xs().text_color(Theme::ink_dim()).child(subtitle)
                            })),
                    )
                    .children(self.toolbar(cx))
                    .child(Self::icon_button("dialog-close", "close").on_click(Self::back(cx))),
            )
            .child(
                div()
                    .id("plugin-dialog")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px_6()
                    .pb_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(self.body(window, cx)),
            )
            .into_any_element()
    }

    fn menu_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (at, buttons) = self.menu.as_ref()?;
        Some(deferred(
            anchored().position(*at).child(
                div()
                    .min_w(px(200.))
                    .p_1()
                    .rounded(px(16.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                        this.menu = None;
                        cx.notify();
                    }))
                    .children(buttons.iter().enumerate().map(|(index, button)| {
                        let pressed = button.clone();
                        let ink = if button.style == ButtonStyle::Danger {
                            Theme::danger()
                        } else {
                            Theme::ink()
                        };
                        div()
                            .id(("row-menu", index))
                            .px_3()
                            .py_2()
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(ink)
                            .map(|item| {
                                if button.disabled {
                                    item.opacity(0.5)
                                } else {
                                    item.cursor_pointer().hover(|item| item.bg(Theme::selected()))
                                }
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.press(&pressed, None, cx);
                            }))
                            .children(
                                button
                                    .icon
                                    .as_deref()
                                    .map(|icon| brand::icon(brand::symbol(Some(icon)), 16., ink)),
                            )
                            .child(button.label.clone())
                    })),
            ),
        ))
    }
}

impl Render for Frame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.placement {
            Placement::Page => self.page(window, cx),
            Placement::Dialog => self.dialog(window, cx),
            Placement::Card => div()
                .flex()
                .flex_col()
                .gap_3()
                .children(self.body(window, cx))
                .into_any_element(),
        };
        div()
            .when(self.placement == Placement::Page, Styled::size_full)
            .child(content)
            .children(self.menu_overlay(cx))
    }
}
