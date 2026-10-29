use std::cmp::Reverse;

use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, MouseButton, ParentElement, Styled, div, prelude::*, px,
};
use model::session::SessionSummary;
use model::topics::{Atlas, Topic, in_history};
use model::{SessionId, TopicId, lineage};

use super::Workspace;
use crate::brand;
use crate::memory::Focus;
use crate::side_panel::{DraggedAgent, DraggedSession};
use crate::text::{ago, now_ms, one_line};
use crate::theme::Theme;

struct Section<'a> {
    topic: &'a Topic,
    sessions: Vec<SessionSummary>,
    history: Vec<SessionSummary>,
    stubs: Vec<(&'a SessionSummary, Option<&'a Topic>)>,
}

impl Workspace {
    fn sections<'a>(&'a self, root: &TopicId) -> Vec<Section<'a>> {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        let mut topics: Vec<&Topic> = self
            .topics
            .iter()
            .filter(|topic| atlas.root(&topic.id) == root)
            .collect();
        topics.sort_by_key(|topic| (topic.parent.is_some(), topic.name.to_lowercase()));
        topics
            .into_iter()
            .map(|topic| {
                let mut sessions: Vec<SessionSummary> = self
                    .sessions
                    .iter()
                    .filter(|session| atlas.topic_of(session) == Some(&topic.id))
                    .cloned()
                    .collect();
                sessions
                    .sort_by_key(|session| (!self.feed.needs_you(session), Reverse(session.updated_at_ms)));
                let (history, sessions) = sessions
                    .into_iter()
                    .partition(|session| in_history(atlas.origin(session), &self.marks));
                let stubs = self
                    .sessions
                    .iter()
                    .filter(|session| {
                        let parent = session.parent.as_ref().and_then(|parent| {
                            self.sessions.iter().find(|candidate| &candidate.id == parent)
                        });
                        parent.is_some_and(|parent| atlas.topic_of(parent) == Some(&topic.id))
                            && atlas.topic_of(session).is_some_and(|own| own != &topic.id)
                    })
                    .map(|session| (session, atlas.topic_of(session).and_then(|own| self.topic(own))))
                    .collect();
                Section {
                    topic,
                    sessions,
                    history,
                    stubs,
                }
            })
            .collect()
    }

    pub(super) fn topic_view(&self, root: &TopicId, cx: &mut Context<Self>) -> AnyElement {
        if self.topic(root).is_none() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(Theme::ink_dim())
                .child("This topic is gone. Ctrl ↑ goes back to Everything.")
                .into_any_element();
        }
        let now = now_ms();
        let sections = self.sections(root);
        let counts = self.counts(sections.iter().flat_map(|section| &section.sessions));
        let dropped_session = root.clone();
        let dropped_agent = root.clone();
        let mut row = 0_usize;
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
                Self::on_right_click(Focus::Topic(root.clone()), cx),
            )
            .drag_over::<DraggedSession>(|style, _, _, _| style.bg(Theme::selected()))
            .drag_over::<DraggedAgent>(|style, _, _, _| style.bg(Theme::selected()))
            .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                this.place(dragged.0.id.clone(), dropped_session.clone(), cx);
            }))
            .on_drop(cx.listener(move |this, dragged: &DraggedAgent, window, cx| {
                this.start_with(dragged.0.clone(), Some(dropped_agent.clone()), window, cx);
            }))
            .children(counts)
            .children(
                sections
                    .into_iter()
                    .map(|section| self.section_view(section, root, &mut row, now, cx)),
            )
            .child(Self::new_session_bar(cx))
            .into_any_element()
    }

    /// A short bar under the list; it starts a session in this topic.
    fn new_session_bar(cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("topic-new-session")
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py(px(10.))
            .rounded(px(14.))
            .border_1()
            .border_dashed()
            .border_color(Theme::line())
            .cursor_pointer()
            .hover(|bar| bar.bg(Theme::card()))
            .text_color(Theme::ink_dim())
            .on_click(cx.listener(Self::start))
            .child(brand::icon("icons/plus.svg", 16., Theme::ink_dim()))
            .child("New session")
    }

    fn section_view(
        &self,
        section: Section<'_>,
        root: &TopicId,
        row: &mut usize,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let heading = (section.topic.id != *root).then(|| self.subtopic_heading(section.topic, cx));
        let rows = lineage::nest(&section.sessions, &self.folded);
        let empty = rows.is_empty() && section.stubs.is_empty() && section.history.is_empty();
        let mut lines = Vec::with_capacity(rows.len() + section.stubs.len());
        for line in &rows {
            *row += 1;
            lines.push(self.session_row(*row, line, now, cx));
        }
        for (session, elsewhere) in section.stubs {
            *row += 1;
            lines.push(Self::stub_row(*row, session, elsewhere, cx));
        }
        if !section.history.is_empty() {
            let open = self.history_open.contains(&section.topic.id);
            lines.push(Self::history_toggle(
                *row,
                section.topic.id.clone(),
                section.history.len(),
                open,
                cx,
            ));
            if open {
                for line in &lineage::nest(&section.history, &self.folded) {
                    *row += 1;
                    lines.push(self.session_row(*row, line, now, cx));
                }
            }
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(heading)
            .when(empty, |list| {
                list.child(
                    div()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child("No sessions yet. Drag one here, or an agent to start one."),
                )
            })
            .children(lines)
            .into_any_element()
    }

    /// A subtopic's heading: right-click for its menu, double-click to rename it, and drop a
    /// session on it to file the session there.
    fn subtopic_heading(&self, topic: &Topic, cx: &mut Context<Self>) -> AnyElement {
        if let Some(field) = self.retitle_field(&topic.id, cx) {
            return div().pb_1().child(field).into_any_element();
        }
        let (renamed, filed) = (topic.id.clone(), topic.id.clone());
        div()
            .id(gpui::ElementId::Name(
                format!("subtopic-{}", topic.id.as_str()).into(),
            ))
            .pb_1()
            .px_1()
            .rounded(px(8.))
            .cursor_pointer()
            .text_xs()
            .text_color(Theme::ink_dim())
            .hover(|heading| heading.text_color(Theme::ink()))
            .drag_over::<DraggedSession>(|style, _, _, _| {
                style.bg(Theme::selected()).text_color(Theme::on_selected())
            })
            .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                cx.stop_propagation();
                this.place(dragged.0.id.clone(), filed.clone(), cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                Self::on_right_click(Focus::Topic(topic.id.clone()), cx),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    this.begin_retitle(renamed.clone(), window, cx);
                }
            }))
            .child(topic.name.to_uppercase())
            .into_any_element()
    }

    /// The row that folds a topic's history sessions away, the ones marked as history and not
    /// active since.
    fn history_toggle(
        row: usize,
        topic: TopicId,
        count: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("history", row))
            .mt_1()
            .px_2()
            .py(px(6.))
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|line| line.bg(Theme::card()))
            .text_xs()
            .text_color(Theme::ink_dim())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.history_open.remove(&topic) {
                    this.history_open.insert(topic.clone());
                }
                cx.notify();
            }))
            .child(div().w(px(12.)).child(if open { "▾" } else { "▸" }))
            .child(format!("History · {count}"))
            .into_any_element()
    }

    fn stub_row(
        row: usize,
        session: &SessionSummary,
        elsewhere: Option<&Topic>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = session.clone();
        let place = elsewhere.map_or_else(|| "another topic".to_owned(), |topic| topic.name.clone());
        div()
            .id(("stub", row))
            .pl(px(24.))
            .py(px(6.))
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|line| line.bg(Theme::card()))
            .text_xs()
            .text_color(Theme::ink_dim())
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open(open.clone(), window, cx);
            }))
            .child(format!(
                "↗ {} · side track in {place}",
                one_line(session.title.as_deref())
            ))
            .into_any_element()
    }

    fn session_row(&self, row: usize, line: &lineage::Row, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let session = &line.session;
        let open = session.clone();
        let fold = session.id.clone();
        let waiting = self.feed.needs_you(session);
        let indent = px(20. * f32::from(u8::try_from(line.depth.min(4)).unwrap_or(4)));
        div()
            .id(("topic-session", row))
            .pl(px(12.) + indent)
            .pr_3()
            .py(px(10.))
            .rounded(px(14.))
            .bg(if line.depth == 0 {
                Theme::card()
            } else {
                Theme::surface()
            })
            .border_1()
            .border_color(if waiting {
                Theme::warn_line()
            } else {
                Theme::surface()
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .gap_3()
            .on_drag(DraggedSession(session.clone()), |dragged, _, _, cx| {
                cx.new(|_| crate::side_panel::DragGhost(one_line(dragged.0.title.as_deref())))
            })
            .on_mouse_down(
                MouseButton::Right,
                Self::on_right_click(Focus::Session(Box::new(session.clone())), cx),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open(open.clone(), window, cx);
            }))
            .when(line.depth > 0, |row| {
                row.child(div().text_color(Theme::ink_dim()).child("↳"))
            })
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
            .when(waiting, |row| row.child(Self::needs_you_chip()))
            .child(
                div()
                    .flex_none()
                    .max_w(px(200.))
                    .truncate()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(self.model_name(session)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(36.))
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(ago(now, session.updated_at_ms)),
            )
            .when(line.children > 0, |item| {
                item.child(
                    div()
                        .id(("topic-fold", row))
                        .flex_none()
                        .text_color(Theme::ink_dim())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if !this.folded.remove(&fold) {
                                this.folded.insert(fold.clone());
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .child(if line.collapsed {
                            format!("▸ {}", line.children)
                        } else {
                            "▾".to_owned()
                        }),
                )
            })
            .when(line.depth == 0, |item| {
                item.child(Self::unplace_button(row, session.id.clone(), cx))
            })
            .into_any_element()
    }

    /// Takes the session out of this topic, back to Uncategorized or History.
    fn unplace_button(row: usize, session: SessionId, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("topic-unplace", row))
            .flex_none()
            .px_1()
            .rounded(px(6.))
            .text_color(Theme::ink_dim())
            .hover(|button| button.bg(Theme::raised()).text_color(Theme::ink()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.unplace(session.clone(), cx);
            }))
            .child("✕")
    }
}
