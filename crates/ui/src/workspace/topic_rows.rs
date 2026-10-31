use std::cmp::Reverse;
use std::collections::HashSet;

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, IntoElement, MouseButton, ParentElement, Styled, Window,
    div, prelude::*, px, uniform_list,
};
use model::preview::{Preview, PreviewKind};
use model::session::SessionSummary;
use model::shelf::Shelf;
use model::topics::{Atlas, FiledConversation};
use model::{SessionId, TopicId};

use super::{ViewInMemory, Workspace};
use crate::memory::Focus;
use crate::side_panel::{DragGhost, DraggedSession};
use crate::text::{ago, now_ms, one_line};
use crate::theme::Theme;

/// How many History sessions show before Show all.
const HISTORY_SHOWN: usize = 5;
/// Past this many, all of History scrolls in a box of its own that draws only the rows in view,
/// since thousands of rows drawn at once take a second a frame.
const HISTORY_DRAWN: usize = 60;
/// How tall that box is.
const HISTORY_BOX: f32 = 640.;

/// A row of History: a session, or a conversation filed here that is no harness session.
enum Past<'a> {
    Session(&'a SessionSummary),
    Conversation(&'a FiledConversation),
}

impl Past<'_> {
    fn at_ms(&self) -> i64 {
        match self {
            Self::Session(session) => session.updated_at_ms,
            Self::Conversation(conversation) => conversation.last_active_ms.unwrap_or_default(),
        }
    }
}

impl Workspace {
    /// The step's Active sessions, the ones needing you first, then newest first.
    pub(super) fn active_list(
        &self,
        step: &TopicId,
        inside: &Shelf<'_>,
        live: &HashSet<SessionId>,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut active = inside.active.clone();
        active.sort_by_key(|session| !self.feed.needs_you(session));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Self::list_heading(
                format!("ACTIVE · {}", active.len()),
                "live, or active in the last 24 hours",
            ))
            .when(active.is_empty(), |list| {
                list.child(
                    div()
                        .text_sm()
                        .text_color(Theme::ink_dim())
                        .child("Nothing active here. Drag a session in, or start one."),
                )
            })
            .children(active.into_iter().enumerate().map(|(index, session)| {
                let age = Self::active_age(now, session, live);
                self.session_row(("active", index), session, step, age, true, cx)
            }))
            .into_any_element()
    }

    /// The step's History, newest first. Conversations filed here that are no harness session sit
    /// among the sessions by when they were last active.
    fn past<'a>(&'a self, step: &TopicId, inside: &Shelf<'a>) -> Vec<Past<'a>> {
        let atlas = Atlas::new(&self.topics, &[], &self.placements);
        let mut past: Vec<Past<'a>> = inside
            .history
            .iter()
            .copied()
            .map(Past::Session)
            .chain(
                self.conversations
                    .iter()
                    .filter(|conversation| atlas.lineage(&conversation.topic).contains(&step))
                    .map(Past::Conversation),
            )
            .collect();
        past.sort_by_key(|row| Reverse(row.at_ms()));
        past
    }

    fn past_row(
        &self,
        index: usize,
        row: &Past<'_>,
        step: &TopicId,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            Past::Session(session) => {
                let age = ago(now, session.updated_at_ms);
                self.session_row(("history", index), session, step, age, false, cx)
            }
            Past::Conversation(conversation) => {
                Self::conversation_row(("history", index), conversation, now, cx)
            }
        }
    }

    /// History's first few rows, or all of them once asked; a long History then scrolls in a box
    /// of its own that draws only the rows in view.
    pub(super) fn history_list(
        &self,
        step: &TopicId,
        inside: &Shelf<'_>,
        now: i64,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let past = self.past(step, inside);
        if past.is_empty() {
            return None;
        }
        let total = past.len();
        let all = self.screen.all_history;
        let rows = if all && total > HISTORY_DRAWN {
            let step = step.clone();
            let rows = uniform_list(
                "history-all",
                total,
                cx.processor(move |this: &mut Self, range: std::ops::Range<usize>, _, cx| {
                    let now = now_ms();
                    let inside = this.shelf(&step);
                    let past = this.past(&step, &inside);
                    range
                        .filter_map(|index| {
                            let row = past.get(index)?;
                            Some(div().pb_1().child(this.past_row(index, row, &step, now, cx)))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .h(px(HISTORY_BOX));
            // The wheel over the box scrolls the box alone; GPUI would move the page along with it.
            div()
                .id("history-all-box")
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(rows)
                .into_any_element()
        } else {
            let shown = if all { total } else { HISTORY_SHOWN };
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(
                    past.iter()
                        .take(shown)
                        .enumerate()
                        .map(|(index, row)| self.past_row(index, row, step, now, cx)),
                )
                .into_any_element()
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(Self::list_heading(
                    format!("HISTORY · {total}"),
                    "quiet for a day, or marked as history; newest first",
                ))
                .child(rows)
                .when(total > HISTORY_SHOWN, |list| {
                    let label = if all {
                        "Show fewer".to_owned()
                    } else {
                        format!("Show all {total}")
                    };
                    list.child(
                        div()
                            .id("history-all-toggle")
                            .mt_1()
                            .px_3()
                            .text_sm()
                            .text_color(Theme::ink_dim())
                            .cursor_pointer()
                            .hover(|more| more.text_color(Theme::ink()))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.screen.all_history = !this.screen.all_history;
                                cx.notify();
                            }))
                            .child(label),
                    )
                })
                .into_any_element(),
        )
    }

    /// A conversation in History: a bot chat, a voice call or an import, which opens in Memory.
    fn conversation_row(
        id: (&'static str, usize),
        conversation: &FiledConversation,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = conversation.clone();
        let title = match conversation
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            Some(title) => one_line(Some(title)),
            None => format!("{} conversation", conversation.channel).into(),
        };
        div()
            .id(id)
            .px_4()
            .py(px(9.))
            .rounded(px(14.))
            .border_1()
            .border_color(Theme::surface())
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .hover(|row| row.bg(Theme::card()))
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                cx.emit(ViewInMemory(Focus::Conversation(Box::new(shown.clone()))));
            }))
            .child(
                div()
                    .flex_none()
                    .size(px(9.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(Theme::ink_dim()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(Theme::ink())
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(match &conversation.node {
                        Some(node) => format!("{} · {node} · in Memory", conversation.channel),
                        None => format!("{} · in Memory", conversation.channel),
                    }),
            )
            .child(
                div().flex_none().text_xs().text_color(Theme::ink_dim()).child(
                    conversation
                        .last_active_ms
                        .map(|at| ago(now, at))
                        .unwrap_or_default(),
                ),
            )
            .into_any_element()
    }

    fn list_heading(title: String, note: &'static str) -> impl IntoElement {
        div()
            .flex()
            .items_baseline()
            .gap_3()
            .px_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(Theme::ink_dim())
                    .child(title),
            )
            .child(div().text_xs().text_color(Theme::ink_dim()).child(note))
    }

    /// One session: an Active one as a card, a History one as a plain line. A click opens it, ⋯ or
    /// a right-click gives its menu, and it drags onto a topic to file it there.
    fn session_row(
        &self,
        id: (&'static str, usize),
        session: &SessionSummary,
        step: &TopicId,
        age: String,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let waiting = self.feed.needs_you(session);
        let (open, menu) = (session.clone(), session.clone());
        let row = div()
            .id(id)
            .px_4()
            .py(px(if active { 12. } else { 9. }))
            .rounded(px(14.))
            .border_1()
            .border_color(if waiting {
                Theme::warn_line()
            } else {
                Theme::surface()
            })
            .flex()
            .flex_col()
            .gap_1()
            .cursor_pointer()
            .on_drag(DraggedSession(session.clone()), |dragged, _, _, cx| {
                cx.new(|_| DragGhost(one_line(dragged.0.title.as_deref())))
            })
            .on_mouse_down(
                MouseButton::Right,
                Self::on_right_click(Focus::Session(Box::new(session.clone())), cx),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open_filed(open.clone(), window, cx);
            }));
        let row = if active {
            row.bg(Theme::card()).hover(|row| row.bg(Theme::raised()))
        } else {
            row.hover(|row| row.bg(Theme::card()))
        };
        let preview = active.then(|| self.previews.get(&session.id)).flatten();
        let line = div()
            .flex()
            .items_center()
            .gap_3()
            .child(self.status_dot(session, active))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(Theme::ink())
                    .child(one_line(session.title.as_deref())),
            )
            .children(self.session_glyph(&session.id))
            .child(
                div()
                    .flex_none()
                    .max_w(px(320.))
                    .truncate()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(self.session_meta(session, step)),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(if waiting { Theme::warn() } else { Theme::ink_dim() })
                    .child(if waiting { "waiting on you".to_owned() } else { age }),
            )
            .child(
                div()
                    .id((id.0, id.1 + 10_000))
                    .flex_none()
                    .size(px(28.))
                    .rounded(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(Theme::ink_dim())
                    .hover(|button| button.bg(Theme::raised()).text_color(Theme::ink()))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_menu(
                            Focus::Session(Box::new(menu.clone())),
                            event.position(),
                            window,
                            cx,
                        );
                    }))
                    .child("⋯"),
            );
        row.child(line)
            .children(preview.map(|preview| {
                div()
                    .pl(px(21.))
                    .truncate()
                    .text_sm()
                    .text_color(if preview.kind.waits() {
                        Theme::warn()
                    } else {
                        Theme::ink_dim()
                    })
                    .child(Self::preview_line(preview))
            }))
            .into_any_element()
    }

    /// Opens a listed session in the pane; one no node lists any more opens in Memory instead.
    fn open_filed(&mut self, session: SessionSummary, window: &mut Window, cx: &mut Context<Self>) {
        if self.sessions.iter().any(|known| known.id == session.id) {
            self.open(session, window, cx);
        } else {
            cx.emit(ViewInMemory(Focus::Session(Box::new(session))));
        }
    }

    /// A preview as the line under a session's title.
    pub(super) fn preview_line(preview: &Preview) -> String {
        let prose = || model::markdown::plain(&preview.text);
        match preview.kind {
            PreviewKind::Running => preview.text.clone(),
            PreviewKind::Approval => format!("Waiting for approval: {}", preview.text),
            PreviewKind::Asking => format!("Asks: “{}”", prose()),
            PreviewKind::Said => {
                let said = prose();
                if said.is_empty() {
                    said
                } else {
                    format!("“{said}”")
                }
            }
        }
    }

    fn status_dot(&self, session: &SessionSummary, active: bool) -> impl IntoElement {
        let dot = div().flex_none().size(px(9.)).rounded(px(5.));
        if active {
            dot.bg(self.dot(session))
        } else {
            dot.border_1().border_color(Theme::ink_dim())
        }
    }

    /// "Claude · Opus · ct115 · Memory": the harness, its model, the node it runs on (this one's
    /// name for a session listed here, the capturing node's for one only memory keeps), and the
    /// subtopic it sits in when that's below the step.
    fn session_meta(&self, session: &SessionSummary, step: &TopicId) -> String {
        let mut parts = vec![session.id.harness().display_name().to_owned()];
        if session.model.is_some() {
            parts.push(self.model_name(session));
        }
        let listed = self.sessions.iter().any(|known| known.id == session.id);
        if let Some(node) = session
            .node
            .clone()
            .or_else(|| listed.then(|| self.hub.local_node()).flatten())
        {
            parts.push(node);
        }
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        if let Some(own) = atlas.topic_of(session).filter(|own| *own != step)
            && let Some(topic) = self.topic(own)
        {
            parts.push(topic.name.clone());
        }
        parts.join(" · ")
    }
}
