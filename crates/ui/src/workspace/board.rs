use std::collections::{HashMap, HashSet};

use gpui::Context;
use model::history::Phase;
use model::session::SessionSummary;
use model::shelf::Shelf;
use model::topics::{Activity, Atlas, Board, Topic, Windows};
use model::{Altitude, SessionId, TopicId};

use super::{ACTIVE_MS, RECENT_MS, Workspace};
use crate::side_panel::PastTopic;
use crate::text::now_ms;

impl Workspace {
    pub(super) fn board(&self) -> Board<'_> {
        let windows = Windows {
            now_ms: now_ms(),
            active_ms: ACTIVE_MS,
            recent_ms: RECENT_MS,
        };
        model::topics::board(
            &self.topics,
            &self.filed_sessions,
            &self.placements,
            &self.marks,
            &self.live(),
            windows,
        )
    }

    /// The sessions that are live: running, waiting on you, or open in the pane.
    pub(super) fn live(&self) -> HashSet<SessionId> {
        self.sessions
            .iter()
            .filter(|session| {
                self.feed.is_active(session)
                    || self.feed.needs_you(session)
                    || self.on_screen() == Some(&session.id)
            })
            .map(|session| session.id.clone())
            .collect()
    }

    /// The open topic's sessions at `step`, Active then History.
    pub(super) fn shelf(&self, step: &TopicId) -> Shelf<'_> {
        let live = self.live();
        let activity = Activity {
            now_ms: now_ms(),
            window_ms: ACTIVE_MS,
            live: &live,
            marks: &self.marks,
        };
        Shelf::of(
            &Atlas::new(&self.topics, &self.filed_sessions, &self.placements),
            step,
            &activity,
        )
    }

    /// The part of the open topic the topic screen shows: the subtopic stepped into, else the topic.
    pub(super) fn step_in(&self, root: &TopicId) -> TopicId {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        self.screen
            .step
            .as_ref()
            .filter(|step| atlas.lineage(step).contains(&root))
            .map_or_else(|| root.clone(), Clone::clone)
    }

    /// Shows the open topic's current step on the rail, and asks what the step's Active sessions
    /// are doing now.
    pub(super) fn sync_step(&self, cx: &mut Context<Self>) {
        let Some((step, active)) = self.show_step(cx) else {
            return;
        };
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let previews = hub.previews(active).await;
            this.update(cx, |this, cx| {
                let current = match &this.altitude {
                    Altitude::Topic(root) => this.step_in(root) == step,
                    _ => false,
                };
                if current {
                    this.previews = previews;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Points the rail at the open topic's current step and its sessions, newest first; the step
    /// and its Active sessions, when a topic is open.
    pub(super) fn show_step(&self, cx: &mut Context<Self>) -> Option<(TopicId, Vec<SessionSummary>)> {
        let Altitude::Topic(root) = &self.altitude else {
            return None;
        };
        let step = self.step_in(root);
        let inside = self.shelf(&step);
        let active: Vec<SessionSummary> = inside.active.iter().map(|session| (*session).clone()).collect();
        let mut sessions: Vec<SessionSummary> = active
            .iter()
            .cloned()
            .chain(inside.history.iter().map(|session| (*session).clone()))
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        self.rail
            .update(cx, |rail, cx| rail.show(step.clone(), sessions, cx));
        Some((step, active))
    }

    pub(super) fn sync_panel(&self, cx: &mut Context<Self>) {
        let board = self.board();
        let history: Vec<SessionSummary> = board.history.iter().map(|session| (*session).clone()).collect();
        let phases: HashMap<SessionId, Phase> = history
            .iter()
            .map(|session| {
                let phase = Phase::of(
                    self.feed.needs_you(session),
                    self.feed.is_active(session),
                    session.status.is_finished(),
                );
                (session.id.clone(), phase)
            })
            .collect();
        let past: Vec<PastTopic> = board
            .past
            .iter()
            .map(|card| PastTopic {
                id: card.topic.id.clone(),
                name: card.topic.name.clone(),
                sessions: card.sessions.len() + card.history,
                active_ms: card.active_ms,
            })
            .collect();
        let topics = self.topics.clone();
        let known = self.sessions.clone();
        self.panel.update(cx, |panel, cx| {
            panel.set_sessions(history, phases, known, cx);
            panel.set_topics(topics, past, cx);
        });
    }

    pub(super) fn home_of(&self, session: &SessionSummary) -> Option<TopicId> {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        atlas.topic_of(session).map(|topic| atlas.root(topic).clone())
    }

    /// Closed, with none of its sessions active since.
    pub(super) fn closed(&self, topic: &Topic) -> bool {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        let newest = self
            .sessions
            .iter()
            .filter(|session| atlas.within(session, &topic.id))
            .map(|session| session.updated_at_ms)
            .max()
            .unwrap_or(0);
        topic.is_closed(newest)
    }

    /// How many conversations that are no harness session are filed under the topic or below it.
    pub(super) fn conversations_within(&self, topic: &TopicId) -> usize {
        let atlas = Atlas::new(&self.topics, &[], &self.placements);
        self.conversations
            .iter()
            .filter(|conversation| atlas.lineage(&conversation.topic).contains(&topic))
            .count()
    }
}
