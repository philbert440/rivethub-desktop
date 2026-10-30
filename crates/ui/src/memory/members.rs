use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::SessionId;
use model::TopicId;
use model::session::SessionSummary;
use model::tidy::Proposal;
use model::topics::{FiledBy, FiledConversation};

use super::{Focus, MemoryPage};
use crate::text::{ago, now_ms};
use crate::theme::Theme;

/// A proposal lists this many of the sessions it would file until asked for all of them.
const LISTED: usize = 40;

/// One of the sessions or conversations a proposal would file.
#[derive(Debug, Clone)]
enum Member {
    Session(SessionId),
    Conversation(String),
}

/// The last few characters of an id, enough to tell sessions apart: the random end of a uuid,
/// whose start is a timestamp in time-ordered ids.
fn short(id: &str) -> &str {
    id.char_indices().rev().nth(7).map_or(id, |(at, _)| &id[at..])
}

impl MemoryPage {
    /// What a session or conversation is called and when it was last active, from what this app
    /// lists or else the proposal's own samples, which name every member a backfill files.
    fn member_facts(&self, proposal: &Proposal, member: &Member) -> (Option<String>, Option<i64>) {
        let id = match member {
            Member::Session(session) => session.to_string(),
            Member::Conversation(id) => id.clone(),
        };
        let sample = proposal.samples.iter().find(|sample| sample.id == id);
        let (title, at) = match member {
            Member::Session(session) => self.known_session(session).map(|known| {
                (
                    known.title.clone(),
                    Some(known.updated_at_ms).filter(|at| *at > 0),
                )
            }),
            Member::Conversation(id) => self
                .listed
                .conversations
                .iter()
                .find(|known| &known.id == id)
                .map(|known| (known.title.clone(), known.last_active_ms)),
        }
        .unwrap_or_default();
        let named = |title: &Option<String>| title.clone().filter(|title| !title.trim().is_empty());
        let title = named(&title).or_else(|| sample.and_then(|sample| named(&sample.title)));
        let at = at.or_else(|| sample.and_then(|sample| sample.last_active_ms));
        (title, at)
    }

    fn open_member(&mut self, member: &Member, title: Option<String>, cx: &mut Context<Self>) {
        match member {
            Member::Session(session) => {
                let summary = self
                    .known_session(session)
                    .cloned()
                    .unwrap_or_else(|| SessionSummary::remembered(session.clone(), title, 0, None));
                self.focus(Focus::Session(Box::new(summary)), cx);
            }
            Member::Conversation(id) => {
                let filed = self
                    .listed
                    .conversations
                    .iter()
                    .find(|known| &known.id == id)
                    .cloned();
                let shown = filed.clone().unwrap_or_else(|| FiledConversation {
                    id: id.clone(),
                    topic: TopicId::new(String::new()),
                    title,
                    channel: String::new(),
                    last_active_ms: None,
                    by: FiledBy::You,
                    node: None,
                });
                self.open_conversation(shown, filed.is_some(), cx);
            }
        }
    }

    /// The sessions and conversations a proposal would file, each opening its page in Memory.
    pub(super) fn members(&self, proposal: &Proposal, cx: &mut Context<Self>) -> AnyElement {
        let Some((sessions, conversations)) = proposal.change.members() else {
            return div().into_any_element();
        };
        let members: Vec<Member> = sessions
            .iter()
            .cloned()
            .map(Member::Session)
            .chain(conversations.iter().cloned().map(Member::Conversation))
            .collect();
        let all = self.review.members.get(&proposal.id).copied().unwrap_or_default();
        let shown = if all {
            members.len()
        } else {
            members.len().min(LISTED)
        };
        let now = now_ms();
        let rows = members.iter().take(shown).enumerate().map(|(index, member)| {
            let (title, at) = self.member_facts(proposal, member);
            let (kind, fallback) = match member {
                Member::Session(session) => (
                    session.harness().display_name().to_owned(),
                    format!("Session {}", short(session.native().as_str())),
                ),
                Member::Conversation(id) => {
                    ("Conversation".to_owned(), format!("Conversation {}", short(id)))
                }
            };
            let (opened, named) = (member.clone(), title.clone());
            div()
                .id(("member", index))
                .min_h(px(28.))
                .px_2()
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .hover(|row| row.bg(Theme::card()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open_member(&opened, named.clone(), cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(if title.is_some() {
                            Theme::ink()
                        } else {
                            Theme::ink_dim()
                        })
                        .child(title.unwrap_or(fallback)),
                )
                .children(at.map(|at| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child(ago(now, at))
                }))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child(kind),
                )
        });
        let more = (members.len() > LISTED).then(|| {
            let id = proposal.id.clone();
            div()
                .id("members-all")
                .px_2()
                .pt_1()
                .cursor_pointer()
                .text_xs()
                .text_color(Theme::accent())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some(all) = this.review.members.get_mut(&id) {
                        *all = !*all;
                    }
                    cx.notify();
                }))
                .child(if all {
                    "Show fewer".to_owned()
                } else {
                    format!("Show all {}", members.len())
                })
        });
        div()
            .id(gpui::SharedString::from(format!("members-{}", proposal.id)))
            .py_1()
            .flex()
            .flex_col()
            .children(rows)
            .children(more)
            .into_any_element()
    }
}
