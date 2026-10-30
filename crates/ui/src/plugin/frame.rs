mod adopt;
mod layout;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use app::Hub;
use gpui::{
    Context, Entity, EventEmitter, IntoElement, ParentElement, Pixels, Point, Render, ScrollHandle, Styled,
    Subscription, Window, div, prelude::*,
};
use model::plugin::view::{Action, Answer, Button, ButtonStyle, Confirm, View};
use model::plugin::{Context as About, Params, PluginRef};

use super::canvas::CanvasView;
use super::form::FormView;

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

/// The key of the block at `index` in a list of blocks reached by `parent`, so local state
/// (a tab chosen, a section folded, a column sorted) stays with its block across fetches.
pub(super) fn child_key(parent: &str, index: usize) -> String {
    if parent.is_empty() {
        index.to_string()
    } else {
        format!("{parent}.{index}")
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
