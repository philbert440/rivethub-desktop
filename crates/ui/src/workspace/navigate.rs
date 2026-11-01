use gpui::{AppContext, Context, Focusable, Window};
use model::prefs::SessionView;
use model::session::SessionSummary;
use model::topics::Atlas;
use model::{Altitude, SessionId, TopicId};

use super::{NextNeedingYou, ToggleSidePanel, TopicScreen, Workspace, ZoomOut};
use crate::session_pane::{PaneEvent, SessionPane};

impl Workspace {
    pub(super) fn open(&mut self, summary: SessionSummary, window: &mut Window, cx: &mut Context<Self>) {
        self.open_pane(summary, false, window, cx);
    }

    /// Opens a session started or forked here. With Settings on Terminal its terminal starts at once,
    /// without the confirm an existing session gets: the person just asked for this session.
    pub(super) fn open_new(&mut self, summary: SessionSummary, window: &mut Window, cx: &mut Context<Self>) {
        self.open_pane(summary, true, window, cx);
    }

    fn open_pane(&mut self, summary: SessionSummary, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Rebuilding the pane on show would drop its draft and terminal.
        if self
            .pane
            .as_ref()
            .is_some_and(|pane| pane.read(cx).session_id() == &summary.id)
        {
            return;
        }
        let topic = self.home_of(&summary);
        let session = summary.id.clone();
        let terminal = self.hub.preferences().opens_in == SessionView::Terminal && !summary.inside_parent();
        let pane = cx.new(|cx| SessionPane::new(self.hub.clone(), summary, cx));
        if terminal && new {
            pane.update(cx, |pane, cx| pane.start_terminal(window, cx));
        } else if terminal {
            pane.update(cx, |pane, cx| pane.show_terminal(window, cx));
        } else {
            window.focus(&pane.read(cx).composer().focus_handle(cx), cx);
        }
        self.pane_events = Some(cx.subscribe_in(
            &pane,
            window,
            |this, pane, event: &PaneEvent, window, cx| match event {
                PaneEvent::Renamed(renamed) => this.renamed(renamed, cx),
                PaneEvent::Bell => {
                    let session = pane.read(cx).session_id().clone();
                    this.rang(session, cx);
                }
                PaneEvent::Forked(forked) => {
                    let source = pane.read(cx).summary().clone();
                    this.forked(&source, (**forked).clone(), window, cx);
                }
                PaneEvent::Failed(problem) => {
                    this.failed = Some(problem.clone());
                    cx.notify();
                }
            },
        ));
        self.pane = Some(pane);
        self.placing = None;
        self.altitude = Altitude::Session { topic, session };
        cx.notify();
    }

    /// A fork opens at once, filed where the session it forks from is.
    fn forked(
        &mut self,
        source: &SessionSummary,
        forked: SessionSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        let topic = atlas.topic_of(source).cloned();
        let session = forked.id.clone();
        self.sessions.insert(0, forked.clone());
        if let Some(topic) = topic {
            self.place(session, topic, cx);
        }
        self.open_new(forked, window, cx);
    }

    pub(super) fn renamed(&mut self, renamed: &SessionSummary, cx: &mut Context<Self>) {
        if let Some(known) = self.sessions.iter_mut().find(|known| known.id == renamed.id) {
            known.title.clone_from(&renamed.title);
        }
        self.refile_sessions();
        self.publish(cx);
        if let Some(pane) = self
            .pane
            .clone()
            .filter(|pane| pane.read(cx).session_id() == &renamed.id)
        {
            pane.update(cx, |pane, cx| pane.take_listing(renamed, cx));
        }
        self.sync_panel(cx);
        cx.notify();
    }

    /// Opens a session by id when some reachable node lists it.
    pub fn open_listed(&mut self, id: &SessionId, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(summary) = self.sessions.iter().find(|session| &session.id == id).cloned() else {
            return false;
        };
        self.open(summary, window, cx);
        true
    }

    /// A deleted topic's sessions are back in Uncategorized; leave the topic if it was open.
    pub fn topic_deleted(&mut self, topic: &TopicId, window: &mut Window, cx: &mut Context<Self>) {
        match &mut self.altitude {
            Altitude::Topic(open) if open == topic => {
                self.leave_to(Altitude::Everything, window, cx);
                return;
            }
            Altitude::Session { topic: home, .. } if home.as_ref() == Some(topic) => *home = None,
            _ => {}
        }
        self.load(cx);
    }

    pub fn open_topic(&mut self, topic: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = None;
        self.placing = None;
        self.screen = TopicScreen::default();
        self.retitling = None;
        self.rooting = None;
        self.altitude = Altitude::Topic(topic);
        self.sync_step(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn next_needing_you(
        &mut self,
        _: &NextNeedingYou,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.on_screen().cloned();
        let feed = &self.feed;
        let next = model::attention::next_needing_you(&self.sessions, current.as_ref(), |session| {
            feed.needs_you(session)
        });
        if let Some(next) = next.filter(|next| current.as_ref() != Some(&next.id)).cloned() {
            self.open(next, window, cx);
        }
    }

    pub(super) fn toggle_panel(&mut self, _: &ToggleSidePanel, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_open = !self.panel_open;
        cx.notify();
    }

    pub(super) fn reveal_panel(&mut self, search: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.panel_open = true;
        if search {
            let query = self.panel.read(cx).query().clone();
            window.focus(&query.focus_handle(cx), cx);
        }
        cx.notify();
    }

    pub(super) fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_to(self.altitude.zoom_out(), window, cx);
    }

    pub(super) fn leave_to(&mut self, altitude: Altitude, window: &mut Window, cx: &mut Context<Self>) {
        if altitude == self.altitude {
            return;
        }
        let back_to_its_topic = matches!(
            (&self.altitude, &altitude),
            (Altitude::Session { topic: Some(from), .. }, Altitude::Topic(to)) if from == to
        );
        if !matches!(altitude, Altitude::Session { .. }) && !back_to_its_topic {
            self.screen = TopicScreen::default();
        }
        self.altitude = altitude;
        self.retitling = None;
        self.rooting = None;
        if !matches!(self.altitude, Altitude::Session { .. }) {
            self.pane = None;
            self.placing = None;
            self.load(cx);
        }
        self.sync_step(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
