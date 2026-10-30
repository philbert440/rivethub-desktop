use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, Window, div, prelude::*, px,
};
use model::search::SessionMatch;
use model::session::SessionSummary;

use super::drag::{DragGhost, DraggedSession};
use super::{PanelEvent, SidePanel};
use crate::theme::Theme;

impl SidePanel {
    pub(super) fn search(&mut self, query: String, cx: &mut Context<Self>) {
        self.asked = Some(query.clone());
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let found = hub.search(query).await;
            this.update(cx, |this, cx| {
                match found {
                    Ok(found) => {
                        this.results = Some(found);
                        this.problem = None;
                    }
                    Err(err) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn clear(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.asked = None;
        self.results = None;
        cx.notify();
    }

    fn summary_for(&self, found: &SessionMatch) -> SessionSummary {
        self.known
            .iter()
            .find(|known| known.id == found.session)
            .cloned()
            .unwrap_or_else(|| SessionSummary {
                id: found.session.clone(),
                title: None,
                status: model::session::SessionStatus::Idle,
                model: None,
                effort: None,
                cwd: None,
                updated_at_ms: found.latest_ms,
                parent: None,
                node: None,
                blocked: false,
            })
    }

    fn topic_hits(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asked = self.asked.as_deref()?;
        let hits = model::topics::suggest(&self.topics, asked, 5);
        if hits.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .pb_2()
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child("Topics"),
                )
                .children(hits.into_iter().enumerate().map(|(index, topic)| {
                    let open = topic.id.clone();
                    div()
                        .id(("topic-hit", index))
                        .px_2()
                        .py(px(7.))
                        .rounded(px(12.))
                        .cursor_pointer()
                        .hover(|line| line.bg(Theme::card()))
                        .text_color(Theme::ink())
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(PanelEvent::OpenTopic(open.clone()));
                        }))
                        .child(topic.name.clone())
                }))
                .into_any_element(),
        )
    }

    pub(super) fn results(&self, found: &[SessionMatch], cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(self.topic_hits(cx))
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(match found.len() {
                        0 => "No sessions matched".to_owned(),
                        1 => "1 session".to_owned(),
                        count => format!("{count} sessions"),
                    }),
            )
            .children(found.iter().enumerate().map(|(index, hit)| {
                let summary = self.summary_for(hit);
                let open = summary.clone();
                div()
                    .id(("result", index))
                    .px_2()
                    .py_2()
                    .rounded(px(12.))
                    .cursor_pointer()
                    .hover(|row| row.bg(Theme::card()))
                    .on_drag(DraggedSession(summary.clone()), |dragged, _, _, cx| {
                        cx.new(|_| DragGhost(crate::text::one_line(dragged.0.title.as_deref())))
                    })
                    .on_click(
                        cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(PanelEvent::Open(open.clone()))),
                    )
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_color(Theme::ink())
                                    .child(crate::text::one_line(summary.title.as_deref())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::warn())
                                    .child(hit.count.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(hit.snippet.before.clone())
                            .child(div().text_color(Theme::warn()).child(hit.snippet.matched.clone()))
                            .child(hit.snippet.after.clone()),
                    )
            }))
            .into_any_element()
    }
}
