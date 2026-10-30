use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::TopicId;
use model::session::SessionSummary;
use model::topics::{FiledBy, FiledConversation};
use model::wiki::{Mention, Page, mentioned_in, mentioning_topics};

use super::{Focus, MemoryPage, Tab};
use crate::text::{ago, now_ms};
use crate::theme::Theme;

/// "Mentioned in" lists this many conversations until asked for all of them.
const SHOWN: usize = 8;

fn chip(id: (&'static str, usize), label: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py(px(2.))
        .rounded(px(10.))
        .bg(Theme::surface())
        .cursor_pointer()
        .text_xs()
        .text_color(Theme::accent())
        .hover(|chip| chip.bg(Theme::card()))
        .child(label)
}

impl MemoryPage {
    fn topic_name(&self, topic: &TopicId) -> Option<String> {
        self.listed
            .topics
            .iter()
            .find(|known| &known.id == topic)
            .map(|known| known.name.clone())
    }

    /// What links to an entity's page, like Wikipedia's "What links here": the topics its
    /// mentions are filed in, then each conversation that mentions it, the latest first.
    pub(super) fn mentioned_in(&self, page: &Page, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mentions = mentioned_in(&page.citations);
        if mentions.is_empty() {
            return None;
        }
        let topics: Vec<(TopicId, String, usize)> = mentioning_topics(&mentions)
            .into_iter()
            .filter_map(|(topic, count)| self.topic_name(&topic).map(|name| (topic, name, count)))
            .collect();
        let total = mentions.len();
        let shown = if self.browse.all_mentions {
            total
        } else {
            total.min(SHOWN)
        };
        let now = now_ms();
        let toggle = (total > SHOWN).then(|| {
            div()
                .id("mentions-all")
                .px_4()
                .py_1()
                .cursor_pointer()
                .text_xs()
                .text_color(Theme::accent())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.browse.all_mentions = !this.browse.all_mentions;
                    cx.notify();
                }))
                .child(if self.browse.all_mentions {
                    "Show fewer".to_owned()
                } else {
                    format!("Show all {total}")
                })
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
                        .child(div().text_lg().text_color(Theme::ink()).child("Mentioned in"))
                        .child(div().text_xs().text_color(Theme::ink_dim()).child(format!(
                            "{total} {}",
                            if total == 1 {
                                "conversation"
                            } else {
                                "conversations"
                            }
                        ))),
                )
                .when(!topics.is_empty(), |panel| {
                    panel.child(
                        div()
                            .pb_2()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(div().text_xs().text_color(Theme::ink_dim()).child("Topics"))
                            .children(
                                topics
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, (topic, name, count))| {
                                        chip(("mention-topic", index), format!("{name} · {count}")).on_click(
                                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                                this.focus(Focus::Topic(topic.clone()), cx);
                                            }),
                                        )
                                    }),
                            ),
                    )
                })
                .children(
                    mentions
                        .into_iter()
                        .take(shown)
                        .enumerate()
                        .map(|(index, mention)| self.mention_row(index, mention, now, cx)),
                )
                .children(toggle)
                .into_any_element(),
        )
    }

    fn mention_row(&self, index: usize, mention: Mention, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let title = mention
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .or_else(|| {
                mention
                    .session
                    .as_ref()
                    .and_then(|session| self.known_session(session))
                    .map(SessionSummary::name)
            })
            .unwrap_or_else(|| match &mention.channel {
                Some(channel) => format!("{channel} conversation"),
                None => "A conversation".to_owned(),
            });
        let mut notes: Vec<String> = Vec::new();
        if let Some(name) = mention.topic.as_ref().and_then(|topic| self.topic_name(topic)) {
            notes.push(name);
        } else {
            notes.push("not filed".to_owned());
        }
        if mention.summaries > 1 {
            notes.push(format!("{} summaries", mention.summaries));
        }
        if let Some(at) = mention.latest_ms {
            notes.push(ago(now, at));
        }
        div()
            .id(("mention", index))
            .px_4()
            .py_2()
            .rounded(px(14.))
            .bg(Theme::card())
            .cursor_pointer()
            .hover(|row| row.bg(Theme::raised()))
            .flex()
            .items_center()
            .gap_3()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_mention(&mention, cx)))
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
                    .child(notes.join(" · ")),
            )
            .into_any_element()
    }

    /// Opens what a mention names in Memory: its session's page, or the conversation's.
    fn open_mention(&mut self, mention: &Mention, cx: &mut Context<Self>) {
        let at = mention.latest_ms.unwrap_or_default();
        if let Some(session) = &mention.session {
            let summary = self.known_session(session).cloned().unwrap_or_else(|| {
                SessionSummary::remembered(session.clone(), mention.title.clone(), at, None)
            });
            self.focus(Focus::Session(Box::new(summary)), cx);
            return;
        }
        let filed = self
            .listed
            .conversations
            .iter()
            .find(|known| known.id == mention.conversation)
            .cloned();
        let shown = filed.clone().unwrap_or_else(|| FiledConversation {
            id: mention.conversation.clone(),
            topic: TopicId::new(String::new()),
            title: mention.title.clone(),
            channel: mention.channel.clone().unwrap_or_default(),
            last_active_ms: mention.latest_ms,
            by: FiledBy::You,
            node: None,
        });
        self.open_conversation(shown, filed.is_some(), cx);
    }

    /// The topic page's Mentions line: the entities its sessions refer to, each opening its page.
    pub(super) fn topic_mentions(&self, root: &TopicId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mentions = self.library.as_ref()?.mentions.get(root)?;
        if mentions.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(div().text_xs().text_color(Theme::ink_dim()).child("Mentions"))
                .children(mentions.iter().enumerate().map(|(index, mention)| {
                    let entity = mention.entity.clone();
                    chip(("topic-mention", index), mention.name.clone()).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.select(Tab::Entities, cx);
                            this.open_entity(entity.clone(), cx);
                        },
                    ))
                }))
                .into_any_element(),
        )
    }
}
