use std::cmp::Reverse;

use gpui::{
    AnyElement, ClickEvent, Context, Div, IntoElement, ParentElement, Stateful, Styled, div, prelude::*, px,
};
use model::SessionId;
use model::session::SessionSummary;
use model::topics::{Atlas, Topic};

use super::{MemoryEvent, MemoryPage};
use crate::text::{ago, now_ms};
use crate::theme::Theme;

/// A topic page lists this many of its sessions until asked for all of them.
const SHOWN: usize = 5;

/// The outlined button that leaves Memory for the workspace.
pub(super) fn link_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(32.))
        .px_4()
        .rounded(px(16.))
        .border_1()
        .border_color(Theme::accent())
        .flex()
        .items_center()
        .cursor_pointer()
        .text_sm()
        .text_color(Theme::accent())
        .hover(|button| button.bg(Theme::card()))
        .child(label)
}

impl MemoryPage {
    pub(super) fn known_session(&self, id: &SessionId) -> Option<&SessionSummary> {
        self.listed.sessions.iter().find(|session| &session.id == id)
    }

    pub(super) fn topic_of(&self, session: &SessionSummary) -> Option<&Topic> {
        let listed = &self.listed;
        let atlas = Atlas::new(&listed.topics, &listed.sessions, &listed.placements);
        let topic = atlas.topic_of(session)?;
        listed.topics.iter().find(|known| &known.id == topic)
    }

    fn workspace_button(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let root = self.opened_root()?;
        Some(
            link_button("memory-open-topic", "Open in Workspace").on_click(cx.listener(
                move |_, _: &ClickEvent, _, cx| {
                    cx.emit(MemoryEvent::OpenTopic(root.clone()));
                },
            )),
        )
    }

    /// A line of the topic page's head with its Open in Workspace button at the end.
    pub(super) fn beside_workspace_button(&self, line: Div, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_start()
            .gap_4()
            .child(line.flex_1().min_w_0())
            .children(self.workspace_button(cx))
    }

    /// The open topic's sessions, newest first, each opening its own page in Memory.
    pub(super) fn sessions_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let root = self.opened_root()?;
        let listed = &self.listed;
        let atlas = Atlas::new(&listed.topics, &listed.sessions, &listed.placements);
        let mut sessions: Vec<&SessionSummary> = listed
            .sessions
            .iter()
            .filter(|session| !session.inside_parent() && atlas.within(session, &root))
            .collect();
        if sessions.is_empty() {
            return None;
        }
        sessions.sort_by_key(|session| Reverse(session.updated_at_ms));
        let total = sessions.len();
        let shown = if self.all_sessions {
            total
        } else {
            total.min(SHOWN)
        };
        let now = now_ms();
        let toggle = (total > SHOWN).then(|| {
            let label = if self.all_sessions {
                "Show fewer".to_owned()
            } else {
                format!("Show all {total}")
            };
            div()
                .id("memory-topic-sessions-all")
                .px_4()
                .py_1()
                .cursor_pointer()
                .text_xs()
                .text_color(Theme::accent())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.all_sessions = !this.all_sessions;
                    cx.notify();
                }))
                .child(label)
        });
        Some(
            div()
                .pt_4()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .pb_1()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(div().text_lg().text_color(Theme::ink()).child("Sessions"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .child(total.to_string()),
                        ),
                )
                .children(
                    sessions
                        .into_iter()
                        .take(shown)
                        .enumerate()
                        .map(|(index, session)| Self::session_row(index, session, now, cx)),
                )
                .children(toggle)
                .into_any_element(),
        )
    }

    fn session_row(
        index: usize,
        session: &SessionSummary,
        now: i64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let open = session.clone();
        let note = |text: String| {
            div()
                .flex_none()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(text)
        };
        div()
            .id(("memory-topic-session", index))
            .px_4()
            .py_2()
            .rounded(px(14.))
            .bg(Theme::card())
            .cursor_pointer()
            .hover(|row| row.bg(Theme::raised()))
            .flex()
            .items_center()
            .gap_3()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.open_session(open.clone(), cx);
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(Theme::ink())
                    .child(session.name()),
            )
            .child(note(session.id.harness().display_name().to_owned()))
            .child(note(ago(now, session.updated_at_ms)))
    }
}
