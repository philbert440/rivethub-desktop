use gateway::GatewayError;
use gpui::{
    Context, Focusable, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, Window, div,
    prelude::*, px,
};
use model::session::SessionSummary;
use model::topics::{Atlas, Directory};
use model::{Altitude, HarnessId, TopicId};

use super::{NewSession, Workspace};
use crate::agent_editor::{AgentEditor, AgentEditorEvent};
use crate::launch_choice::Choice;
use crate::launcher::{Launcher, LauncherEvent, Target};
use crate::theme::Theme;

pub(super) enum Launch {
    Uncategorized,
    In(TopicId),
    New(String),
}

impl Workspace {
    /// The New session picker over a backdrop; clicking anywhere outside the picker closes it.
    pub(super) fn launcher(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (launcher, _) = self.launching.as_ref()?;
        let picker = div()
            .absolute()
            .top(px(52.))
            .right(px(20.))
            .w(px(460.))
            .p_3()
            .rounded(px(18.))
            .bg(Theme::raised())
            .border_1()
            .border_color(Theme::line())
            .flex()
            .flex_col()
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child("Start a session"),
            )
            .child(launcher.clone());
        Some(
            div()
                .id("launcher-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.launching = None;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }),
                )
                .child(picker),
        )
    }

    pub(super) fn edit_agent(
        &mut self,
        agent: Option<&model::agents::Agent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hub = self.hub.clone();
        let editor = cx.new(|cx| AgentEditor::new(hub, agent, cx));
        let events = cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &AgentEditorEvent, window, cx| {
                this.editing_agent = None;
                if matches!(event, AgentEditorEvent::Saved | AgentEditorEvent::Deleted) {
                    this.panel.update(cx, |panel, cx| panel.load_agents(cx));
                }
                window.focus(&this.focus, cx);
                cx.notify();
            },
        );
        window.focus(&editor.focus_handle(cx), cx);
        self.editing_agent = Some((editor, events));
        cx.notify();
    }

    pub(super) fn new_session(&mut self, _: &NewSession, window: &mut Window, cx: &mut Context<Self>) {
        self.launch(window, cx);
    }

    pub(super) fn start(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.launch(window, cx);
    }

    fn launch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preset = match &self.altitude {
            Altitude::Topic(root) => self
                .topic(&self.step_in(root))
                .map(|known| (known.id.clone(), known.name.clone())),
            _ => None,
        };
        let topics = self.topics.clone();
        let hub = self.hub.clone();
        let launcher = cx.new(|cx| Launcher::new(&hub, topics, preset, cx));
        let events = cx.subscribe_in(&launcher, window, |this, _, event: &LauncherEvent, window, cx| {
            this.launching = None;
            let LauncherEvent::Start { choice, target } = event else {
                window.focus(&this.focus, cx);
                cx.notify();
                return;
            };
            let launch = match target {
                Target::Uncategorized => Launch::Uncategorized,
                Target::In(topic) => Launch::In(topic.clone()),
                Target::New(name) => Launch::New(name.clone()),
            };
            match (**choice).clone() {
                Choice::Agent(agent) => this.start_agent(agent, launch, window, cx),
                Choice::Harness { node, harness, model } => {
                    this.start_session(launch, node, harness, model, window, cx);
                }
            }
        });
        window.focus(&launcher.focus_handle(cx), cx);
        self.launching = Some((launcher, events));
        cx.notify();
    }

    pub(super) fn start_session(
        &mut self,
        launch: Launch,
        node: Option<String>,
        harness: HarnessId,
        model: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.failed = None;
        let (topic, node, cwd) = self.start_place(&launch, node);
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_session(node, harness, model, topic, cwd).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| this.started(started, launch, window, cx))
                    .ok();
            })
            .ok();
        })
        .detach();
    }

    /// Where a session launched into a topic starts: filed in the topic, so rh hands it the topic's
    /// startup memory, and in the topic's working directory (its own or the nearest above it), on
    /// that directory's node when the directory is local to one.
    fn start_place(
        &self,
        launch: &Launch,
        node: Option<String>,
    ) -> (Option<TopicId>, Option<String>, Option<String>) {
        let Launch::In(topic) = launch else {
            return (None, node, None);
        };
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        match atlas.directory(topic) {
            Some(Directory::Local { node, path }) => {
                (Some(topic.clone()), Some(node.clone()), Some(path.clone()))
            }
            Some(Directory::Shared { path }) => (Some(topic.clone()), node, Some(path.clone())),
            None => (Some(topic.clone()), node, None),
        }
    }

    pub(super) fn start_with(
        &mut self,
        agent: model::agents::Agent,
        topic: Option<TopicId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_agent(agent, topic.map_or(Launch::Uncategorized, Launch::In), window, cx);
    }

    fn start_agent(
        &mut self,
        agent: model::agents::Agent,
        launch: Launch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.failed = None;
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_with(agent).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| this.started(started, launch, window, cx))
                    .ok();
            })
            .ok();
        })
        .detach();
    }

    fn started(
        &mut self,
        started: Result<SessionSummary, GatewayError>,
        launch: Launch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match started {
            Ok(summary) => {
                match launch {
                    Launch::In(topic) => self.place(summary.id.clone(), topic, cx),
                    Launch::New(name) => self.place_in_new(summary.id.clone(), name, cx),
                    Launch::Uncategorized => {}
                }
                self.sessions.push(summary.clone());
                self.refile_sessions();
                self.open_new(summary, window, cx);
            }
            Err(err) => {
                self.failed = Some(err.to_string());
                cx.notify();
            }
        }
    }
}
