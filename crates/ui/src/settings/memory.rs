use gpui::{AnyElement, Div, IntoElement, ParentElement, Styled, div, prelude::*};
use model::health::Status;

use super::SettingsPage;
use super::parts::{Load, compact, dim, dot, note, pending, row, section, warn_chip};
use crate::theme::Theme;

impl SettingsPage {
    pub(super) fn memory_tab(&self) -> Vec<Div> {
        vec![
            section("Health", self.health()),
            section(
                "Filing",
                vec![
                    note(
                        "The categorizer",
                        "After a conversation's first summary, it files the conversation under a topic: one \
                         whose folder it worked in, one its title or summary names, or the filing model's pick. \
                         It never moves what you placed, and you can move or unfile what it placed.",
                    ),
                    note(
                        "Tidying",
                        "Once a week it proposes merges, moves, renames and new topics from what it filed. \
                         Nothing changes until you approve a proposal.",
                    ),
                    note(
                        "Backfill",
                        "Conversations from before the categorizer are filed the same way by running rh memory \
                         cleanup on a node: a dry run first, and what it proposes waits for your approval too.",
                    ),
                    note(
                        "The filing model",
                        "Filing and tidying use the model of the categorizer's default agent, set in the \
                         node's config.yaml.",
                    ),
                ],
            ),
        ]
    }

    fn health(&self) -> Vec<AnyElement> {
        let health = match &self.memory {
            Load::Ready(health) => health,
            other => return pending(other),
        };
        let (tone, label) = match health.status {
            Status::Ok => (Theme::accent(), "Memory checks OK"),
            Status::Degraded | Status::Error => (Theme::warn(), "Memory needs attention"),
        };
        vec![
            row()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(dot(tone))
                        .child(label)
                        .when(!health.semantic, |line| line.child(warn_chip("keywords only"))),
                )
                .child(dim(format!(
                    "{} sessions · {} messages",
                    compact(health.conversations),
                    compact(health.messages)
                )))
                .into_any_element(),
        ]
    }
}
