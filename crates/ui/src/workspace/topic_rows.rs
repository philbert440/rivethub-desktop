use std::collections::HashSet;

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, IntoElement, MouseButton, ParentElement, Styled, div,
    prelude::*, px,
};
use model::session::SessionSummary;
use model::shelf::Shelf;
use model::topics::Atlas;
use model::{SessionId, TopicId};

use super::Workspace;
use crate::memory::Focus;
use crate::side_panel::{DragGhost, DraggedSession};
use crate::text::{ago, one_line};
use crate::theme::Theme;

/// How many History sessions show before Show all.
const HISTORY_SHOWN: usize = 5;

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

    /// The step's History, newest first: its first few, or all of it once asked.
    pub(super) fn history_list(
        &self,
        step: &TopicId,
        inside: &Shelf<'_>,
        now: i64,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if inside.history.is_empty() {
            return None;
        }
        let total = inside.history.len();
        let shown = if self.screen.all_history {
            total
        } else {
            HISTORY_SHOWN
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
                .children(
                    inside
                        .history
                        .iter()
                        .take(shown)
                        .enumerate()
                        .map(|(index, session)| {
                            let age = ago(now, session.updated_at_ms);
                            self.session_row(("history", index), session, step, age, false, cx)
                        }),
                )
                .when(total > HISTORY_SHOWN, |list| {
                    let label = if self.screen.all_history {
                        "Show fewer".to_owned()
                    } else {
                        format!("Show all {total}")
                    };
                    list.child(
                        div()
                            .id("history-all")
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
            .items_center()
            .gap_3()
            .cursor_pointer()
            .on_drag(DraggedSession(session.clone()), |dragged, _, _, cx| {
                cx.new(|_| DragGhost(one_line(dragged.0.title.as_deref())))
            })
            .on_mouse_down(
                MouseButton::Right,
                Self::on_right_click(Focus::Session(Box::new(session.clone())), cx),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open(open.clone(), window, cx);
            }));
        let row = if active {
            row.bg(Theme::card()).hover(|row| row.bg(Theme::raised()))
        } else {
            row.hover(|row| row.bg(Theme::card()))
        };
        row.child(self.status_dot(session, active))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(Theme::ink())
                    .child(one_line(session.title.as_deref())),
            )
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
            )
            .into_any_element()
    }

    fn status_dot(&self, session: &SessionSummary, active: bool) -> impl IntoElement {
        let dot = div().flex_none().size(px(9.)).rounded(px(5.));
        if active {
            dot.bg(self.dot(session))
        } else {
            dot.border_1().border_color(Theme::ink_dim())
        }
    }

    /// "Claude · Opus · ct115 · Memory": the harness, its model, the node, and the subtopic it sits
    /// in when that's below the step.
    fn session_meta(&self, session: &SessionSummary, step: &TopicId) -> String {
        let mut parts = vec![session.id.harness().display_name().to_owned()];
        if session.model.is_some() {
            parts.push(self.model_name(session));
        }
        if let Some(node) = &session.node {
            parts.push(node.clone());
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
