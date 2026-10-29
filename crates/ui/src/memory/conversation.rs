use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::topics::{FiledBy, FiledConversation};

use super::{Focus, MemoryEvent, MemoryPage};
use crate::text::{ago_phrase, now_ms};
use crate::theme::Theme;

impl MemoryPage {
    /// Shows a filed conversation that is no harness session: what it is and where it's filed.
    pub(super) fn open_conversation(&mut self, conversation: FiledConversation, cx: &mut Context<Self>) {
        self.viewing = None;
        self.problem = None;
        self.remembering = Some(conversation);
        cx.notify();
    }

    fn unfile_conversation(&mut self, conversation: String, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let undone = hub.unfile_conversation(conversation).await;
            this.update(cx, |this, cx| {
                match undone {
                    Ok(()) => {
                        if let Some(remembering) = this.remembering.as_mut() {
                            remembering.by = FiledBy::You;
                        }
                        this.problem = Some("Taken out of its topic.".into());
                        cx.emit(MemoryEvent::Refiled);
                    }
                    Err(err) => this.problem = Some(format!("undoing it failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn conversation_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(conversation) = &self.remembering else {
            return div().into_any_element();
        };
        let title = conversation
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| format!("{} conversation", conversation.channel));
        let topic = self
            .listed
            .topics
            .iter()
            .find(|known| known.id == conversation.topic)
            .map(|topic| {
                let focus = topic.id.clone();
                div()
                    .id("conversation-topic")
                    .px_2()
                    .rounded(px(9.))
                    .bg(Theme::surface())
                    .cursor_pointer()
                    .text_xs()
                    .text_color(Theme::accent())
                    .hover(|chip| chip.bg(Theme::card()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.focus(Focus::Topic(focus.clone()), cx);
                    }))
                    .child(format!("in {}", topic.name))
            });
        let undo = (conversation.by == FiledBy::Categorizer).then(|| {
            let id = conversation.id.clone();
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child("by categorizer ·")
                .child(
                    div()
                        .id("conversation-undo")
                        .cursor_pointer()
                        .text_color(Theme::accent())
                        .hover(|link| link.text_color(Theme::ink()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.unfile_conversation(id.clone(), cx);
                        }))
                        .child("Undo"),
                )
        });
        div()
            .id("memory-conversation")
            .size_full()
            .overflow_y_scroll()
            .py_8()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(860.))
                    .px_8()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_2xl().text_color(Theme::ink()).child(title))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .rounded(px(9.))
                                    .bg(Theme::surface())
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .child(conversation.channel.clone()),
                            )
                            .children(topic)
                            .children(undo)
                            .children(conversation.last_active_ms.map(|at| {
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .child(format!("active {}", ago_phrase(now_ms(), at)))
                            })),
                    )
                    .children(
                        self.problem
                            .clone()
                            .map(|problem| div().text_xs().text_color(Theme::ink_dim()).child(problem)),
                    ),
            )
            .into_any_element()
    }
}
