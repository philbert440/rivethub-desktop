use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, Context, Hsla, IntoElement, ParentElement, Styled, Task, Window, div, prelude::*,
    px,
};
use model::markdown::Block;
use model::memory::{SessionMemory, Summary, SummaryKind, Vector};
use model::session::SessionSummary;
use model::topics::FiledBy;

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
    session: SessionSummary,
    memory: Option<SessionMemory>,
    blocks: HashMap<String, Vec<Block>>,
    expanded: HashSet<String>,
    loading: bool,
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

fn badge(text: impl Into<String>, ink: Hsla) -> impl IntoElement {
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
            memory: None,
            blocks: HashMap::new(),
            expanded: HashSet::new(),
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

    fn load_session(&mut self, cx: &mut Context<Self>) {
        let Some(viewing) = self.viewing.as_mut() else {
            return;
        };
        viewing.loading = true;
        let id = viewing.session.id.clone();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let memory = hub.session_memory(id.clone()).await;
            this.update(cx, |this, cx| {
                let Some(viewing) = this.viewing.as_mut().filter(|viewing| viewing.session.id == id) else {
                    return;
                };
                viewing.loading = false;
                match memory {
                    Ok(memory) if viewing.memory.as_ref() == Some(&memory) => {}
                    Ok(memory) => {
                        viewing.blocks = memory
                            .summaries
                            .iter()
                            .map(|summary| (summary.id.clone(), model::markdown::parse(summary.text.trim())))
                            .collect();
                        viewing.memory = Some(memory);
                        this.problem = None;
                        cx.notify();
                    }
                    Err(err) => this.failed(&err, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn session_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(viewing) = &self.viewing else {
            return div().into_any_element();
        };
        let body = match &viewing.memory {
            None => div()
                .text_color(Theme::ink_dim())
                .child("Loading…")
                .into_any_element(),
            Some(memory) => div()
                .flex()
                .flex_col()
                .gap_4()
                .child(Self::parts(memory))
                .child(Self::summaries(viewing, memory, window, cx))
                .into_any_element(),
        };
        div()
            .id("memory-session")
            .size_full()
            .overflow_y_scroll()
            .py_8()
            .flex()
            .justify_center()
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
                    .child(body),
            )
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
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .child(format!("active {}", ago_phrase(now_ms(), session.updated_at_ms))),
                            ),
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
        let summaries = u32::try_from(memory.summaries.len()).unwrap_or(u32::MAX);
        let embedded_summaries = u32::try_from(memory.embedded_summaries()).unwrap_or(u32::MAX);
        let kinds = |kind| u32::try_from(memory.summaries_of(kind)).unwrap_or(u32::MAX);
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
                    (kinds(SummaryKind::Root), "root", "roots"),
                    (kinds(SummaryKind::Branch), "branch", "branches"),
                    (kinds(SummaryKind::Leaf), "leaf", "leaves"),
                ]),
            ))
    }

    fn summaries(
        viewing: &Viewing,
        memory: &SessionMemory,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if memory.summaries.is_empty() {
            let note = if memory.messages.total == 0 {
                "Memory hasn't captured anything from this session."
            } else {
                "Memory hasn't summarized this session yet."
            };
            return div()
                .pt_2()
                .text_color(Theme::ink_dim())
                .child(note)
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child("SUMMARIES"),
            )
            .children(
                memory
                    .outline()
                    .into_iter()
                    .enumerate()
                    .map(|(index, (depth, summary))| {
                        Self::summary(index, depth, summary, viewing, window, cx)
                    }),
            )
            .into_any_element()
    }

    fn summary(
        index: usize,
        depth: usize,
        summary: &Summary,
        viewing: &Viewing,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = viewing.expanded.contains(&summary.id);
        let blocks = viewing.blocks.get(&summary.id).map_or(&[][..], Vec::as_slice);
        let shown = if open {
            blocks
        } else {
            &blocks[..blocks.len().min(1)]
        };
        // Its own scope when open, or the paragraph keeps the clamped layout and runs off the card.
        let scope = format!("summary-{}-{}", summary.id, if open { "open" } else { "short" });
        let text = markdown_view::blocks(shown, &scope, Theme::ink().into(), window);
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
                if let Some(viewing) = this.viewing.as_mut()
                    && !viewing.expanded.remove(&toggle)
                {
                    viewing.expanded.insert(toggle.clone());
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
