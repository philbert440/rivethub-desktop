use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, img, prelude::*, px};
use model::session::SessionSummary;
use model::topics::Card;

use super::Workspace;
use crate::brand;
use crate::side_panel::{DraggedAgent, DraggedSession};
use crate::text::{ago, now_ms, one_line};
use crate::theme::Theme;

const CARD_SESSIONS: usize = 3;

impl Workspace {
    pub(super) fn everything(&self, cx: &mut Context<Self>) -> AnyElement {
        let board = self.board();
        if board.forming.is_empty() && board.recent.is_empty() {
            let line = self
                .problem
                .clone()
                .unwrap_or_else(|| "Start a session and it will find its topic.".into());
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(img(brand::ghost()).size(px(96.)))
                .child(div().text_color(Theme::ink()).child("Nothing active"))
                .child(div().text_color(Theme::ink_dim()).child(line))
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
                self.problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .when(!board.forming.is_empty(), |page| {
                page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(Self::lane_title(
                            "FORMING",
                            format!("{} sessions without a topic yet", board.forming.len()),
                        ))
                        .child(
                            div().flex().flex_wrap().gap_3().children(
                                board
                                    .forming
                                    .iter()
                                    .enumerate()
                                    .map(|(index, session)| self.forming_tile(index, session, now, cx)),
                            ),
                        ),
                )
            })
            .when(!board.recent.is_empty(), |page| {
                page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(Self::lane_title("TOPICS", "most recent first".to_owned()))
                        .child(
                            div().flex().flex_wrap().gap_3().children(
                                board
                                    .recent
                                    .iter()
                                    .enumerate()
                                    .map(|(index, card)| self.topic_card(index, card, now, cx)),
                            ),
                        ),
                )
            })
            .into_any_element()
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
        let waiting = card
            .sessions
            .iter()
            .filter(|session| self.feed.needs_you(session))
            .count();
        let count = card.sessions.len();
        let noun = if count == 1 { "session" } else { "sessions" };
        div()
            .id(("topic", index))
            .w(px(320.))
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
            .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                this.place(dragged.0.id.clone(), dropped_session.clone(), cx);
            }))
            .on_drop(cx.listener(move |this, dragged: &DraggedAgent, window, cx| {
                this.start_with(dragged.0.clone(), Some(dropped_agent.clone()), window, cx);
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open_topic(topic.clone(), window, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(card.topic.name.clone()),
                    )
                    .when(waiting > 0, |row| row.child(Self::needs_you_chip())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(format!("{count} {noun} · {}", ago(now, card.active_ms))),
            )
            .children(card.sessions.iter().take(CARD_SESSIONS).map(|session| {
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
            }))
            .into_any_element()
    }

    fn forming_tile(
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
            .id(("forming", index))
            .w(px(320.))
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
                    .child(session.model.clone().unwrap_or_else(|| "claude".into()))
                    .child(ago(now, session.updated_at_ms))
                    .children(self.subagents.get(&session.id).map(|count| {
                        let noun = if *count == 1 { "subagent" } else { "subagents" };
                        format!("↳ {count} {noun}")
                    }))
                    .child(div().flex_1())
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
