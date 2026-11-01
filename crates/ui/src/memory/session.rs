use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, Context, Hsla, IntoElement, ListState, ParentElement, Styled, Task, Window, div,
    list, prelude::*, px,
};
use model::memory::{SessionMemory, Summary, Vector};
use model::session::SessionSummary;
use model::topics::FiledBy;

use super::recalled::Recalled;
use super::sessions::link_button;
use super::{Focus, MemoryEvent, MemoryPage};
use crate::markdown_view;
use crate::text::{ago_phrase, clock, now_ms};
use crate::theme::Theme;

const COLUMN: f32 = 860.;
const INDENT: f32 = 24.;
const CLAMP: usize = 3;
const REFRESH: Duration = Duration::from_secs(10);
/// The session Memory is showing, with its summaries parsed once they load.
pub(super) struct Viewing {
    pub(super) session: SessionSummary,
    pub(super) recalled: Option<Recalled>,
    pub(super) loading: bool,
    _refresh: Task<()>,
}

impl Viewing {
    /// Takes the session's latest listing, such as a new title.
    pub(super) fn relist(&mut self, sessions: &[SessionSummary]) {
        if let Some(latest) = sessions.iter().find(|known| known.id == self.session.id) {
            self.session.clone_from(latest);
        }
    }
}

pub(super) fn badge(text: impl Into<String>, ink: Hsla) -> impl IntoElement {
    div()
        .px_2()
        .rounded(px(9.))
        .bg(Theme::surface())
        .text_xs()
        .text_color(ink)
        .child(text.into())
}

fn vector_ink(vector: Vector) -> Hsla {
    match vector {
        Vector::Embedded => Theme::accent().into(),
        Vector::Failed => Theme::warn().into(),
        Vector::Pending | Vector::Skipped => Theme::ink_dim().into(),
    }
}

/// The non-zero counts, each with its one-or-many label, such as "1 reply · 50 tool calls".
fn counts(parts: &[(u32, &str, &str)]) -> String {
    parts
        .iter()
        .filter(|(count, ..)| *count > 0)
        .map(|(count, one, many)| format!("{count} {}", if *count == 1 { one } else { many }))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn span(summary: &Summary) -> String {
    match (summary.from_ms, summary.to_ms) {
        (Some(from), Some(to)) if clock(from) != clock(to) => format!("{} – {}", clock(from), clock(to)),
        (Some(at), _) | (None, Some(at)) => clock(at),
        (None, None) => String::new(),
    }
}

impl MemoryPage {
    /// Shows the session's page, kept current while Memory is on screen.
    pub(super) fn open_session(&mut self, session: SessionSummary, cx: &mut Context<Self>) {
        self.problem = None;
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                let refreshed = this.update(cx, |this, cx| {
                    if this.shown {
                        this.refresh_session(cx);
                    }
                });
                if refreshed.is_err() {
                    break;
                }
            }
        });
        self.viewing = Some(Viewing {
            session,
            recalled: None,
            loading: false,
            _refresh: refresh,
        });
        self.load_session(cx);
        cx.notify();
    }

    /// Reloads the open session's page unless a load is under way, which also retries one that
    /// failed.
    pub(super) fn refresh_session(&mut self, cx: &mut Context<Self>) {
        if self.viewing.as_ref().is_some_and(|viewing| !viewing.loading) {
            self.load_session(cx);
        }
    }

    pub(super) fn session_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(viewing) = &self.viewing else {
            return div().into_any_element();
        };
        if let Some(recalled) = &viewing.recalled {
            return Self::recalled_page(&recalled.rows, cx);
        }
        div()
            .id("memory-session")
            .size_full()
            .overflow_y_scroll()
            .py_8()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .px_8()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.session_head(&viewing.session, cx))
                    .children(
                        self.problem
                            .clone()
                            .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
                    )
                    .child(div().text_color(Theme::ink_dim()).child("Loading…")),
            )
            .into_any_element()
    }

    /// A session's or conversation's memory as rows, its head and then each summary, of which only
    /// those in view are laid out and drawn.
    pub(super) fn recalled_page(rows: &ListState, cx: &mut Context<Self>) -> AnyElement {
        let rows = list(
            rows.clone(),
            cx.processor(|this: &mut Self, row: usize, window: &mut Window, cx| {
                this.read_ahead(row, cx);
                this.recalled_row(row, window, cx)
            }),
        )
        .size_full();
        div()
            .size_full()
            .flex()
            .justify_center()
            .child(div().w_full().max_w(px(COLUMN)).h_full().child(rows))
            .into_any_element()
    }

    fn recalled_row(&self, row: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if row == 0 {
            return div()
                .px_8()
                .pt_8()
                .pb_2()
                .child(self.recalled_head(cx))
                .into_any_element();
        }
        let Some(recalled) = self.recalled() else {
            return div().into_any_element();
        };
        let found = recalled
            .order
            .get(row - 1)
            .and_then(|(depth, place)| Some((*depth, recalled.memory.summaries.get(*place)?)));
        let Some((depth, summary)) = found else {
            if row == recalled.order.len() + 1 {
                return div()
                    .px_8()
                    .pb(px(32.))
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child("Reading more summaries…")
                    .into_any_element();
            }
            return div().into_any_element();
        };
        let last = row == recalled.order.len() && !recalled.more_left();
        div()
            .px_8()
            .pb(px(if last { 32. } else { 8. }))
            .child(Self::summary(row - 1, depth, summary, recalled, window, cx))
            .into_any_element()
    }

    /// The open page's head: what it is, any problem, its parts and the summaries' heading.
    fn recalled_head(&self, cx: &mut Context<Self>) -> AnyElement {
        let (head, noun) = match (&self.viewing, &self.remembering) {
            (Some(viewing), _) => (
                self.session_head(&viewing.session, cx).into_any_element(),
                "session",
            ),
            (None, Some(remembering)) => (self.conversation_head(remembering, cx), "conversation"),
            (None, None) => return div().into_any_element(),
        };
        let Some(recalled) = self.recalled() else {
            return head;
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(head)
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .child(Self::parts(&recalled.memory))
            .child(Self::summaries_heading(&recalled.memory, noun))
            .into_any_element()
    }

    fn session_head(&self, session: &SessionSummary, cx: &mut Context<Self>) -> impl IntoElement {
        let open = session.id.clone();
        let topic = self.topic_of(session).map(|topic| {
            let focus = topic.id.clone();
            div()
                .id("memory-session-topic")
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
        let undo = (topic.is_some() && self.listed.filed_by.get(&session.id) == Some(&FiledBy::Categorizer))
            .then(|| {
                let unfiled = session.id.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child("by categorizer ·")
                    .child(
                        div()
                            .id("memory-session-undo")
                            .cursor_pointer()
                            .text_color(Theme::accent())
                            .hover(|link| link.text_color(Theme::ink()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.unfile(unfiled.clone(), cx);
                            }))
                            .child("Undo"),
                    )
            });
        div()
            .flex()
            .items_start()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_2xl().text_color(Theme::ink()).child(session.name()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(badge(
                                session.id.harness().display_name(),
                                Theme::ink_dim().into(),
                            ))
                            .children(
                                session
                                    .node
                                    .clone()
                                    .map(|node| badge(node, Theme::ink_dim().into())),
                            )
                            .children(topic)
                            .children(undo)
                            // A session memory knows no time for says nothing about when it was active.
                            .when(session.updated_at_ms > 0, |line| {
                                line.child(
                                    div().text_xs().text_color(Theme::ink_dim()).child(format!(
                                        "active {}",
                                        ago_phrase(now_ms(), session.updated_at_ms)
                                    )),
                                )
                            }),
                    ),
            )
            .child(
                link_button("memory-open-session", "Open session").on_click(cx.listener(
                    move |_, _: &ClickEvent, _, cx| {
                        cx.emit(MemoryEvent::OpenSession(open.clone()));
                    },
                )),
            )
    }

    /// Takes a session the categorizer filed back out of its topic; rh keeps it as a correction.
    fn unfile(&mut self, session: model::SessionId, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let undone = hub.unplace(session).await;
            this.update(cx, |this, cx| {
                match undone {
                    Ok(()) => cx.emit(MemoryEvent::Refiled),
                    Err(err) => this.problem = Some(format!("undoing it failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn tile(label: &'static str, value: String, detail: String) -> impl IntoElement {
        div()
            .flex_1()
            .min_w_0()
            .px_4()
            .py_3()
            .rounded(px(16.))
            .bg(Theme::card())
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().text_color(Theme::ink_dim()).child(label))
            .child(div().text_lg().text_color(Theme::ink()).child(value))
            .child(div().text_xs().text_color(Theme::ink_dim()).child(detail))
    }

    /// What the session is made of in memory, and how much of it search can find by meaning.
    fn parts(memory: &SessionMemory) -> impl IntoElement {
        let tally = memory.messages;
        let held = memory.counts();
        let count = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        let summaries = count(held.total());
        let embedded_summaries = count(held.embedded);
        let mut embedding = counts(&[
            (tally.pending, "message waiting", "messages waiting"),
            (tally.failed, "message failed", "messages failed"),
            (tally.skipped, "not embeddable", "not embeddable"),
        ]);
        if summaries > 0 {
            let noun = if summaries == 1 { "summary" } else { "summaries" };
            let summed = format!("{embedded_summaries} of {summaries} {noun}");
            embedding = if embedding.is_empty() {
                summed
            } else {
                format!("{summed} · {embedding}")
            };
        }
        div()
            .flex()
            .gap_3()
            .child(Self::tile(
                "Messages",
                tally.total.to_string(),
                counts(&[
                    (tally.user, "from you", "from you"),
                    (tally.assistant, "reply", "replies"),
                    (tally.tool, "tool call", "tool calls"),
                    (tally.system, "system", "system"),
                ]),
            ))
            .child(Self::tile(
                "Embedded",
                format!("{} of {}", tally.embedded, tally.total),
                embedding,
            ))
            .child(Self::tile(
                "Summaries",
                summaries.to_string(),
                counts(&[
                    (count(held.roots), "root", "roots"),
                    (count(held.branches), "branch", "branches"),
                    (count(held.leaves), "leaf", "leaves"),
                ]),
            ))
    }

    /// The summaries being read: the open session's, or else the open conversation's.
    fn recalled(&self) -> Option<&Recalled> {
        match self.viewing.as_ref() {
            Some(viewing) => viewing.recalled.as_ref(),
            None => self
                .remembering
                .as_ref()
                .and_then(|remembering| remembering.recalled.as_ref()),
        }
    }

    fn recalled_mut(&mut self) -> Option<&mut Recalled> {
        match self.viewing.as_mut() {
            Some(viewing) => viewing.recalled.as_mut(),
            None => self
                .remembering
                .as_mut()
                .and_then(|remembering| remembering.recalled.as_mut()),
        }
    }

    /// SUMMARIES over the list, or why there are none yet.
    fn summaries_heading(memory: &SessionMemory, noun: &str) -> AnyElement {
        if memory.counts().total() == 0 {
            let note = if memory.messages.total == 0 {
                format!("Memory hasn't captured anything from this {noun}.")
            } else {
                format!("Memory hasn't summarized this {noun} yet.")
            };
            return div()
                .pt_2()
                .text_color(Theme::ink_dim())
                .child(note)
                .into_any_element();
        }
        div()
            .pt_2()
            .text_xs()
            .text_color(Theme::ink_dim())
            .child("SUMMARIES")
            .into_any_element()
    }

    fn summary(
        index: usize,
        depth: usize,
        summary: &Summary,
        recalled: &Recalled,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = recalled.expanded.contains(&summary.id);
        let blocks = recalled.blocks.get(&summary.id).map_or(&[][..], Vec::as_slice);
        let shown = if open {
            blocks
        } else {
            &blocks[..blocks.len().min(1)]
        };
        // Its own scope when open, or the paragraph keeps the clamped layout and runs off the card.
        let scope = format!("summary-{}-{}", summary.id, if open { "open" } else { "short" });
        let text = if open {
            markdown_view::blocks(shown, &scope, Theme::ink().into(), window)
        } else {
            markdown_view::clamped_blocks(shown, &scope, Theme::ink().into(), window)
        };
        let toggle = summary.id.clone();
        let indent = px(INDENT * f32::from(u8::try_from(depth.min(4)).unwrap_or(4)));
        div()
            .id(("memory-summary", index))
            .ml(indent)
            .px_4()
            .py_3()
            .rounded(px(16.))
            .bg(Theme::card())
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap_2()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if this
                    .recalled_mut()
                    .is_some_and(|recalled| recalled.toggle(&toggle))
                {
                    this.read_whole(toggle.clone(), cx);
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(badge(summary.kind.label(), Theme::ink().into()))
                    .child(badge(summary.vector.label(), vector_ink(summary.vector)))
                    .child(div().text_xs().text_color(Theme::ink_dim()).child(counts(&[(
                        summary.messages,
                        "message",
                        "messages",
                    )])))
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(Theme::ink_dim()).child(span(summary))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(!open, |text| text.line_clamp(CLAMP).text_ellipsis())
                    .children(text),
            )
            .into_any_element()
    }
}
