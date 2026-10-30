use std::collections::HashSet;

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, IntoElement, MouseButton, ParentElement, Styled, div,
    prelude::*, px,
};
use model::SessionId;
use model::TopicId;
use model::session::SessionSummary;
use model::shelf::Shelf;
use model::topics::{Directory, Topic};

use super::launch::Launch;
use super::{ViewInMemory, Workspace};
use crate::brand;
use crate::memory::Focus;
use crate::side_panel::{DraggedAgent, DraggedSession};
use crate::text::{ago_phrase, now_ms, one_line};
use crate::theme::Theme;

/// How many of a step's harnesses get a quick start.
const QUICK_STARTS: usize = 3;

impl Workspace {
    /// The open topic at its current step: its heading, its path and subtopics, what needs you,
    /// quick starts, and its sessions, Active then History.
    pub(super) fn topic_view(&self, root: &TopicId, cx: &mut Context<Self>) -> AnyElement {
        let step_id = self.step_in(root);
        let Some(step) = self.topic(&step_id) else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(Theme::ink_dim())
                .child("This topic is gone. Ctrl ↑ goes back to Everything.")
                .into_any_element();
        };
        let inside = self.shelf(&step_id);
        let live = self.live();
        let now = now_ms();
        let (dropped_session, dropped_agent) = (step_id.clone(), step_id.clone());
        div()
            .id("topic-view")
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .flex()
            .flex_col()
            .gap_5()
            .on_mouse_down(
                MouseButton::Right,
                Self::on_right_click(Focus::Topic(step_id.clone()), cx),
            )
            .drag_over::<DraggedSession>(|style, _, _, _| style.bg(Theme::selected()))
            .drag_over::<DraggedAgent>(|style, _, _, _| style.bg(Theme::selected()))
            .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                this.place(dragged.0.id.clone(), dropped_session.clone(), cx);
            }))
            .on_drop(cx.listener(move |this, dragged: &DraggedAgent, window, cx| {
                this.start_with(dragged.0.clone(), Some(dropped_agent.clone()), window, cx);
            }))
            .child(self.topic_heading(step, &inside, now, cx))
            .child(self.topic_path(root, &step_id, cx))
            .children(self.needs_you_strip(&inside, cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap_6()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(520.))
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(Self::quick_start(&step_id, &inside, cx))
                            .child(self.active_list(&step_id, &inside, &live, now, cx))
                            .children(self.history_list(&step_id, &inside, now, cx)),
                    )
                    .child(div().flex_none().w(px(340.)).child(self.rail.clone())),
            )
            .into_any_element()
    }

    /// The step's name (double-click or the pencil renames it in place), its wiki page in Memory,
    /// its menu, and a line on where it works and how busy it is.
    fn topic_heading(
        &self,
        step: &Topic,
        inside: &Shelf<'_>,
        now: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (double_clicked, penciled, menu, wiki) =
            (step.id.clone(), step.id.clone(), step.id.clone(), step.id.clone());
        let title = match self.retitle_field(&step.id, cx) {
            Some(field) => div().min_w(px(320.)).child(field).into_any_element(),
            None => div()
                .id("topic-title")
                .text_2xl()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Theme::ink())
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    if event.click_count() == 2 {
                        this.begin_retitle(double_clicked.clone(), window, cx);
                    }
                }))
                .child(step.name.clone())
                .into_any_element(),
        };
        let last = inside
            .active
            .iter()
            .chain(&inside.history)
            .map(|session| session.updated_at_ms)
            .max();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(title)
                    .child(
                        div()
                            .id("topic-rename")
                            .size(px(36.))
                            .rounded(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|button| button.bg(Theme::card()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.begin_retitle(penciled.clone(), window, cx);
                            }))
                            .child(brand::icon("icons/rename.svg", 16., Theme::ink_dim())),
                    )
                    .child(div().flex_1())
                    .child(
                        Self::outlined("topic-wiki", "Wiki page in Memory ↗").on_click(cx.listener(
                            move |_, _: &ClickEvent, _, cx| cx.emit(ViewInMemory(Focus::Topic(wiki.clone()))),
                        )),
                    )
                    .child(Self::outlined("topic-menu", "⋯").on_click(cx.listener(
                        move |this, event: &ClickEvent, window, cx| {
                            this.open_menu(Focus::Topic(menu.clone()), event.position(), window, cx);
                        },
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x_4()
                    .gap_y_1()
                    .text_sm()
                    .text_color(Theme::ink_dim())
                    .children(step.directory.as_ref().map(Self::directory))
                    .child(format!(
                        "{} {} · {} active",
                        inside.len(),
                        if inside.len() == 1 { "session" } else { "sessions" },
                        inside.active.len()
                    ))
                    .children(last.map(|at| format!("last active {}", ago_phrase(now, at)))),
            )
    }

    fn directory(directory: &Directory) -> impl IntoElement {
        let (path, place) = match directory {
            Directory::Local { node, path } => (path.clone(), format!("on {node}")),
            Directory::Shared { path } => (path.clone(), "shared".to_owned()),
        };
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .font_family(Theme::MONO)
                    .text_color(Theme::ink())
                    .child(path),
            )
            .child(place)
    }

    pub(super) fn outlined(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(36.))
            .px_4()
            .rounded(px(18.))
            .border_1()
            .border_color(Theme::line())
            .flex()
            .items_center()
            .cursor_pointer()
            .text_sm()
            .text_color(Theme::ink())
            .hover(|button| button.bg(Theme::card()))
            .child(label)
    }

    /// The step's sessions waiting on you, each a click from its question.
    fn needs_you_strip(&self, inside: &Shelf<'_>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let waiting: Vec<&SessionSummary> = inside
            .active
            .iter()
            .copied()
            .filter(|session| self.feed.needs_you(session))
            .collect();
        if waiting.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .px_4()
                .py_3()
                .rounded(px(16.))
                .bg(Theme::warn_container())
                .border_1()
                .border_color(Theme::warn_line())
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(Theme::warn())
                        .child(format!("NEEDS YOU · {}", waiting.len())),
                )
                .children(waiting.into_iter().enumerate().map(|(index, session)| {
                    let open = session.clone();
                    let harness = session.id.harness().display_name().to_owned();
                    let asks = match self.previews.get(&session.id) {
                        Some(preview) if preview.kind.waits() => {
                            format!("{harness} · {}", Self::preview_line(preview))
                        }
                        _ => harness,
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().flex_none().size(px(8.)).rounded(px(4.)).bg(Theme::warn()))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(Theme::ink())
                                .child(one_line(session.title.as_deref())),
                        )
                        .child(
                            div()
                                .flex_none()
                                .max_w(px(520.))
                                .truncate()
                                .text_sm()
                                .text_color(Theme::ink())
                                .child(asks),
                        )
                        .child(
                            div()
                                .id(("answer", index))
                                .h(px(30.))
                                .px_4()
                                .rounded(px(15.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .bg(Theme::warn())
                                .text_sm()
                                .text_color(Theme::warn_container())
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.open(open.clone(), window, cx);
                                }))
                                .child("Answer"),
                        )
                }))
                .into_any_element(),
        )
    }

    /// New session, plus one-click starts in this step for the harnesses its sessions run most,
    /// each on the node it last ran on.
    fn quick_start(step: &TopicId, inside: &Shelf<'_>, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                div()
                    .id("topic-new-session")
                    .h(px(40.))
                    .px_4()
                    .rounded(px(20.))
                    .border_1()
                    .border_dashed()
                    .border_color(Theme::line())
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|button| button.bg(Theme::card()))
                    .text_color(Theme::ink())
                    .on_click(cx.listener(Self::start))
                    .child(brand::icon("icons/plus.svg", 16., Theme::ink_dim()))
                    .child("New session"),
            )
            .children(
                inside
                    .usual(QUICK_STARTS)
                    .into_iter()
                    .enumerate()
                    .map(|(index, latest)| {
                        let (harness, node, topic) =
                            (latest.id.harness().clone(), latest.node.clone(), step.clone());
                        let label = format!("{} here", harness.display_name());
                        div()
                            .id(("quick-start", index))
                            .h(px(40.))
                            .px_4()
                            .rounded(px(20.))
                            .bg(Theme::card())
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|button| button.bg(Theme::raised()))
                            .text_color(Theme::ink())
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.start_session(
                                    Launch::In(topic.clone()),
                                    node.clone(),
                                    harness.clone(),
                                    None,
                                    window,
                                    cx,
                                );
                            }))
                            .child(label)
                    }),
            )
    }

    /// "now" or "5m" for a live session, "idle 3h" for a quiet Active one.
    pub(super) fn active_age(now: i64, session: &SessionSummary, live: &HashSet<SessionId>) -> String {
        let ago = crate::text::ago(now, session.updated_at_ms);
        if live.contains(&session.id) {
            ago
        } else {
            format!("idle {ago}")
        }
    }
}
