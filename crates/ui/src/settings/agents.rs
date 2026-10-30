use gpui::{
    AnyElement, ClickEvent, Context, Div, Focusable, IntoElement, ParentElement, Styled, Window, div,
    prelude::*, px, rgb,
};
use model::agents::{Agent, Roster};

use super::parts::{Load, button, chip, dim, field, path, pending, row, section, section_with};
use super::{SettingsEvent, SettingsPage};
use crate::agent_editor::{AgentEditor, AgentEditorEvent};
use crate::theme::Theme;

impl SettingsPage {
    pub(super) fn load_agents(&mut self, cx: &mut Context<Self>) {
        self.roster = Load::Waiting;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let roster = hub.agents().await;
            this.update(cx, |this, cx| {
                this.roster = Load::from(roster);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn agents(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let add = button("new-agent", "New agent")
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.edit_agent(None, window, cx)))
            .into_any_element();
        let roster = match &self.roster {
            Load::Ready(roster) => roster,
            other => return vec![section("Agents", pending(other))],
        };
        let rows = if roster.agents.is_empty() {
            vec![
                row()
                    .child(dim(
                        "No agents yet. An agent is a harness with its own model, instructions and folder.",
                    ))
                    .into_any_element(),
            ]
        } else {
            roster
                .agents
                .iter()
                .enumerate()
                .map(|(index, agent)| self.agent_row(index, agent, &roster.node, cx))
                .collect()
        };
        let storage = storage(roster);
        let mut sections = vec![section_with("Agents", Some(add), rows)];
        if !storage.is_empty() {
            sections.push(section("Where agents work", storage));
        }
        sections
    }

    fn agent_row(&self, index: usize, agent: &Agent, local: &str, cx: &mut Context<Self>) -> AnyElement {
        let what = [
            agent
                .harness
                .as_ref()
                .map(|harness| harness.display_name().to_owned()),
            agent.model.as_ref().map(|model| {
                agent
                    .harness
                    .as_ref()
                    .and_then(|harness| self.hub.model_label(agent.node.as_deref(), harness, model))
                    .unwrap_or_else(|| model.clone())
            }),
            agent.effort.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let away = agent
            .node
            .as_deref()
            .filter(|node| !node.eq_ignore_ascii_case(local));
        let editing = agent.clone();
        row()
            .id(("agent", index))
            .cursor_pointer()
            .hover(|row| row.bg(Theme::raised()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.edit_agent(Some(&editing), window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(8.))
                            .flex_none()
                            .rounded_full()
                            .bg(agent.color.map_or(Theme::ink_dim(), rgb)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(agent.name.clone())
                                    .children(away.map(|node| chip(node.to_owned()))),
                            )
                            .child(
                                dim(if what.is_empty() {
                                    "No harness".to_owned()
                                } else {
                                    what
                                })
                                .text_xs()
                                .whitespace_nowrap(),
                            ),
                    ),
            )
            .children(agent.directory.clone().map(|folder| path(folder).min_w_0()))
            .into_any_element()
    }

    fn edit_agent(&mut self, agent: Option<&Agent>, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| AgentEditor::new(self.hub.clone(), agent, cx));
        let closed = cx.subscribe(&editor, |this, _, event: &AgentEditorEvent, cx| {
            this.editing = None;
            if matches!(event, AgentEditorEvent::Saved | AgentEditorEvent::Deleted) {
                this.load_agents(cx);
                cx.emit(SettingsEvent::AgentsChanged);
            }
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
        self.editing = Some((editor, closed));
        cx.notify();
    }
}

/// Where the node puts the folders its agents work in, and where it keeps the agents themselves.
fn storage(roster: &Roster) -> Vec<AnyElement> {
    let storage = &roster.storage;
    let kept = match storage.shared_presets {
        Some(true) => Some("In the memory database, for every node on it".to_owned()),
        Some(false) => Some(format!("In files on {}", roster.node)),
        None => None,
    };
    [
        storage
            .folder_root
            .clone()
            .map(|root| field("Agent folders", path(format!("{root}/<agent>")))),
        storage
            .shared_folder
            .clone()
            .map(|shared| field("Shared folder", path(shared))),
        kept.map(|kept| field("Agents are kept", dim(kept))),
    ]
    .into_iter()
    .flatten()
    .collect()
}
