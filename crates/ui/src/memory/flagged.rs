use std::collections::HashSet;

use gateway::GatewayError;
use gpui::{AnyElement, ClickEvent, Context, ParentElement, Styled, div, prelude::*, px};
use model::flagged::{FlaggedAction, FlaggedGroup};
use model::memory::Verdict;

use super::MemoryPage;
use super::proposals::verdict_button;
use crate::text::{ago_phrase, now_ms};
use crate::theme::Theme;

/// The jobs memory stopped retrying, as the node groups them.
#[derive(Debug, Default)]
pub(super) struct Flagged {
    /// `None` until the node answers, and from a node that keeps none for this client.
    groups: Option<Vec<FlaggedGroup>>,
    /// The jobs a retry or dismiss is on its way for, until the list shows what became of them.
    acting: HashSet<i64>,
}

impl MemoryPage {
    pub(super) fn load_flagged(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.flagged().await;
            this.update(cx, |this, cx| this.flagged_loaded(loaded, cx)).ok();
        })
        .detach();
    }

    fn flagged_loaded(
        &mut self,
        loaded: Result<Option<Vec<FlaggedGroup>>, GatewayError>,
        cx: &mut Context<Self>,
    ) {
        match loaded {
            Ok(groups) => self.flagged.groups = groups,
            Err(err) => self.failed(&err, cx),
        }
        cx.notify();
    }

    /// Retries or dismisses the job a group shows, then lists the flagged jobs again, and reads the
    /// workers' failing counts again, which count it too.
    fn act_on_flagged(&mut self, id: i64, action: FlaggedAction, cx: &mut Context<Self>) {
        if !self.flagged.acting.insert(id) {
            return;
        }
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let done = hub.act_on_flagged(id, action).await;
            let loaded = hub.flagged().await;
            this.update(cx, |this, cx| {
                this.flagged.acting.remove(&id);
                match done {
                    Ok(()) => this.load_stats(cx),
                    Err(err) => this.failed(&err, cx),
                }
                this.flagged_loaded(loaded, cx);
            })
            .ok();
        })
        .detach();
    }

    /// One cause: its error, task and count, then the newest job it failed, with Dismiss and Retry
    /// for that job.
    fn flagged_group(
        &self,
        index: usize,
        group: &FlaggedGroup,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sample = &group.sample;
        let id = sample.id;
        let jobs = match group.count {
            1 => "1 job".to_owned(),
            count => format!("{count} jobs"),
        };
        let failures = match sample.failures {
            1 => "failed once".to_owned(),
            count => format!("failed {count} times"),
        };
        div()
            .id(("flagged", index))
            .px_4()
            .py_3()
            .rounded(px(16.))
            .bg(Theme::card())
            .flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(Theme::ink()).child(group.cause.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(format!("{} · {jobs}", group.task)),
                    )
                    .child(
                        div()
                            .mt_1()
                            .px_3()
                            .py_2()
                            .rounded(px(12.))
                            .bg(Theme::code())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_xs()
                            .children(sample.subject().map(|subject| {
                                div()
                                    .font_family(Theme::MONO)
                                    .text_color(Theme::ink_dim())
                                    .truncate()
                                    .child(subject.to_owned())
                            }))
                            .child(
                                div()
                                    .font_family(Theme::MONO)
                                    .text_color(Theme::ink())
                                    .child(sample.error.clone()),
                            )
                            .child(div().text_color(Theme::ink_dim()).child(format!(
                                "{failures} · last {}",
                                ago_phrase(now, sample.last_failed_ms)
                            ))),
                    ),
            )
            .when(!self.flagged.acting.contains(&id), |row| {
                row.child(
                    verdict_button(("flagged-dismiss", index), "Dismiss", Verdict::Reject).on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.act_on_flagged(id, FlaggedAction::Dismiss, cx);
                        }),
                    ),
                )
                .child(
                    verdict_button(("flagged-retry", index), "Retry", Verdict::Accept).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.act_on_flagged(id, FlaggedAction::Retry, cx);
                        },
                    )),
                )
            })
            .into_any_element()
    }

    /// The jobs memory gave up on, one group per cause; nothing from a node that keeps none for
    /// this client.
    pub(super) fn flagged_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let groups = self.flagged.groups.as_ref()?;
        let now = now_ms();
        let body = if groups.is_empty() {
            div()
                .pt_4()
                .text_color(Theme::ink_dim())
                .child("Nothing flagged.")
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(
                    groups
                        .iter()
                        .enumerate()
                        .map(|(index, group)| self.flagged_group(index, group, now, cx)),
                )
                .into_any_element()
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_3()
                        .child(div().text_xl().text_color(Theme::ink()).child("Flagged"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .child("jobs that kept failing, so memory stopped retrying them"),
                        ),
                )
                .child(body)
                .into_any_element(),
        )
    }
}
