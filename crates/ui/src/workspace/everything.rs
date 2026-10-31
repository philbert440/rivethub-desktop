use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, MouseButton, ParentElement, Styled, div, img, prelude::*,
    px,
};
use model::session::SessionSummary;
use model::topics::Card;

use super::topic_edit::DraggedTopic;
use super::{Arrival, Workspace};
use crate::brand;
use crate::memory::Focus;
use crate::side_panel::{DragGhost, DraggedAgent, DraggedSession};
use crate::text::{ago, now_ms, one_line};
use crate::theme::Theme;

const CARD_SESSIONS: usize = 3;
const UNCATEGORIZED_SHOWN: usize = 6;
pub(super) const TILE_WIDTH: f32 = 300.;

impl Workspace {
    pub(super) fn everything(&self, cx: &mut Context<Self>) -> AnyElement {
        let board = self.board();
        if board.uncategorized.is_empty() && board.recent.is_empty() {
            let (heading, hint) = if self.arrival == Arrival::Arrived {
                ("Nothing active", "Start a session and it will find its topic.")
            } else {
                ("Finding your sessions…", "Asking each node what it runs.")
            };
            let line = self.problems().unwrap_or_else(|| hint.into());
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(img(brand::ghost()).size(px(96.)))
                .child(div().text_color(Theme::ink()).child(heading))
                .child(div().text_color(Theme::ink_dim()).child(line))
                .child(self.new_topic_card(cx))
                .into_any_element();
        }
        let now = now_ms();
        div()
            .id("everything")
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .flex()
            .flex_col()
            .gap_6()
            .children(
                self.problems()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .when(!board.uncategorized.is_empty(), |page| {
                page.child(self.uncategorized_lane(&board.uncategorized, now, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(Self::lane_title("TOPICS", "most recent first".to_owned()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_3()
                            .children(
                                board
                                    .recent
                                    .iter()
                                    .enumerate()
                                    .map(|(index, card)| self.topic_card(index, card, now, cx)),
                            )
                            .child(self.new_topic_card(cx)),
                    ),
            )
            .into_any_element()
    }

    fn uncategorized_lane(
        &self,
        sessions: &[&SessionSummary],
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = sessions.len();
        let noun = if count == 1 { "session" } else { "sessions" };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.uncategorized_title(format!("{count} {noun} without a topic yet"), cx))
            .when(!self.uncategorized_folded, |lane| {
                lane.child(
                    div().flex().flex_wrap().gap_3().children(
                        sessions
                            .iter()
                            .take(if self.uncategorized_open {
                                usize::MAX
                            } else {
                                UNCATEGORIZED_SHOWN
                            })
                            .enumerate()
                            .map(|(index, session)| self.uncategorized_tile(index, session, now, cx)),
                    ),
                )
            })
            .when(
                !self.uncategorized_folded && count > UNCATEGORIZED_SHOWN,
                |lane| {
                    let label = if self.uncategorized_open {
                        "Show fewer".to_owned()
                    } else {
                        format!("Show all {count}")
                    };
                    lane.child(
                        div()
                            .id("uncategorized-more")
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .cursor_pointer()
                            .hover(|more| more.text_color(Theme::ink()))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.uncategorized_open = !this.uncategorized_open;
                                cx.notify();
                            }))
                            .child(label),
                    )
                },
            )
            .into_any_element()
    }

    /// The Uncategorized lane's title, which folds the lane to just itself.
    fn uncategorized_title(&self, note: String, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("uncategorized-title")
            .flex()
            .gap_2()
            .items_baseline()
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.uncategorized_folded = !this.uncategorized_folded;
                cx.notify();
            }))
            .child(
                div()
                    .w(px(12.))
                    .text_color(Theme::ink_dim())
                    .child(if self.uncategorized_folded { "▸" } else { "▾" }),
            )
            .child(Self::lane_title("UNCATEGORIZED", note))
    }

    fn lane_title(title: &'static str, note: String) -> impl IntoElement {
        div()
            .flex()
            .gap_3()
            .items_baseline()
            .child(div().text_color(Theme::ink()).child(title))
            .child(div().text_xs().text_color(Theme::ink_dim()).child(note))
    }

    fn topic_card(&self, index: usize, card: &Card<'_>, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let topic = card.topic.id.clone();
        let dropped_session = topic.clone();
        let dropped_agent = topic.clone();
        let dropped_topic = topic.clone();
        let dragged = DraggedTopic {
            topic: topic.clone(),
            name: card.topic.name.clone().into(),
        };
        let waiting = card
            .sessions
            .iter()
            .filter(|session| self.feed.needs_you(session))
            .count();
        let active = ago(now, card.active_ms);
        div()
            .id(("topic", index))
            .w(px(TILE_WIDTH))
            .p_4()
            .rounded(px(16.))
            .bg(Theme::card())
            .border_1()
            .border_color(if waiting > 0 {
                Theme::warn_line()
            } else {
                Theme::card()
            })
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap_2()
            .drag_over::<DraggedSession>(|style, _, _, _| style.border_color(Theme::accent()))
            .drag_over::<DraggedAgent>(|style, _, _, _| style.border_color(Theme::accent()))
            .drag_over::<DraggedTopic>(|style, _, _, _| style.border_color(Theme::accent()))
            .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                this.place(dragged.0.id.clone(), dropped_session.clone(), cx);
            }))
            .on_drop(cx.listener(move |this, dragged: &DraggedAgent, window, cx| {
                this.start_with(dragged.0.clone(), Some(dropped_agent.clone()), window, cx);
            }))
            .on_drop(cx.listener(move |this, dragged: &DraggedTopic, _, cx| {
                this.nest(dragged.topic.clone(), Some(dropped_topic.clone()), cx);
            }))
            .on_drag(dragged, |dragged, _, _, cx| {
                cx.new(|_| DragGhost(dragged.name.clone()))
            })
            .on_mouse_down(
                MouseButton::Right,
                Self::on_right_click(Focus::Topic(card.topic.id.clone()), cx),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open_topic(topic.clone(), window, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(match self.retitle_field(&card.topic.id, cx) {
                        Some(field) => div()
                            .id(("retitle", index))
                            .flex_1()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(field)
                            .into_any_element(),
                        None => div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(card.topic.name.clone())
                            .into_any_element(),
                    })
                    .children(self.topic_glyph(card.topic))
                    .when(waiting > 0, |row| row.child(Self::needs_you_chip())),
            )
            .child(div().text_xs().text_color(Theme::ink_dim()).child(format!(
                "{} active · {} in history · {active}",
                card.sessions.len(),
                card.history + self.conversations_within(&card.topic.id)
            )))
            .children(
                card.sessions
                    .iter()
                    .take(CARD_SESSIONS)
                    .map(|session| self.card_session(session)),
            )
            .into_any_element()
    }

    fn card_session(&self, session: &SessionSummary) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .size(px(6.))
                    .rounded(px(3.))
                    .bg(self.dot(session)),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(one_line(session.title.as_deref())),
            )
            .children(self.session_glyph(&session.id))
    }

    fn suggestion_chip(
        &self,
        index: usize,
        session: &SessionSummary,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let suggestion = self.suggestions.get(&session.id)?;
        let label = suggestion
            .topic
            .as_ref()
            .and_then(|topic| self.topic(topic))
            .map_or_else(
                || format!("+ {}", suggestion.name),
                |topic| format!("→ {}", topic.name),
            );
        let accept = session.id.clone();
        Some(
            div()
                .id(("suggest", index))
                .max_w(px(150.))
                .truncate()
                .px_2()
                .rounded(px(9.))
                .border_1()
                .border_color(Theme::accent())
                .text_color(Theme::accent())
                .hover(|chip| chip.bg(Theme::selected()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.accept_suggestion(accept.clone(), cx);
                }))
                .child(label)
                .into_any_element(),
        )
    }

    fn uncategorized_tile(
        &self,
        index: usize,
        session: &SessionSummary,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let summary = session.clone();
        let placing = session.id.clone();
        let waiting = self.feed.needs_you(session);
        let picker = self
            .placing
            .as_ref()
            .filter(|placing| placing.session == session.id)
            .map(|placing| placing.picker.clone());
        div()
            .id(("uncategorized", index))
            .w(px(TILE_WIDTH))
            .p_4()
            .rounded(px(16.))
            .bg(Theme::card())
            .border_1()
            .border_color(if waiting {
                Theme::warn_line()
            } else {
                Theme::card()
            })
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap_2()
            .on_drag(DraggedSession(session.clone()), |dragged, _, _, cx| {
                cx.new(|_| crate::side_panel::DragGhost(one_line(dragged.0.title.as_deref())))
            })
            .on_mouse_down(
                MouseButton::Right,
                Self::on_right_click(Focus::Session(Box::new(session.clone())), cx),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open(summary.clone(), window, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .size(px(8.))
                            .rounded(px(4.))
                            .bg(self.dot(session)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(one_line(session.title.as_deref())),
                    )
                    .when(waiting, |row| row.child(Self::needs_you_chip())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(150.))
                            .truncate()
                            .child(self.model_name(session)),
                    )
                    .child(div().flex_none().child(ago(now, session.updated_at_ms)))
                    .children(self.subagents.get(&session.id).map(|count| {
                        let noun = if *count == 1 { "subagent" } else { "subagents" };
                        div().flex_none().child(format!("↳ {count} {noun}"))
                    }))
                    .child(div().flex_1())
                    .children(self.suggestion_chip(index, session, cx))
                    .child(
                        div()
                            .id(("place", index))
                            .px_2()
                            .rounded(px(9.))
                            .border_1()
                            .border_color(Theme::line())
                            .hover(|chip| chip.text_color(Theme::ink()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                this.begin_placing(placing.clone(), window, cx);
                            }))
                            .child("→ Topic"),
                    ),
            )
            .children(picker.map(|picker| {
                div()
                    .id(("picker", index))
                    .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                    .child(picker)
            }))
            .into_any_element()
    }
}
