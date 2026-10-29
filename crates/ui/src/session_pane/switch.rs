use gpui::{ClickEvent, Context, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};
use model::agents::EFFORTS;

use super::SessionPane;
use crate::theme::Theme;

/// Which of the composer's switches is showing its choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Model,
    Effort,
}

fn chip(id: SharedString, label: String, selected: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(28.))
        .px_3()
        .rounded(px(14.))
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .cursor_pointer()
        .map(|chip| {
            if selected {
                chip.bg(Theme::selected()).text_color(Theme::on_selected())
            } else {
                chip.bg(Theme::raised())
                    .text_color(Theme::ink_dim())
                    .hover(|chip| chip.text_color(Theme::ink()))
            }
        })
        .child(label)
}

impl SessionPane {
    /// The model the next turn runs: the one picked here, else the one the session runs.
    fn shown_model(&self) -> Option<&str> {
        self.picked.model.as_deref().or(self.summary.model.as_deref())
    }

    fn shown_effort(&self) -> Option<&str> {
        self.picked.effort.as_deref().or(self.summary.effort.as_deref())
    }

    /// The label the harness offers for a model, or its id when the harness doesn't offer it.
    pub(super) fn model_label(&self, id: &str) -> String {
        self.models
            .iter()
            .find(|choice| choice.id == id)
            .map_or_else(|| id.to_owned(), |choice| choice.label.clone())
    }

    fn can_pick_model(&self) -> bool {
        self.switches.model && !self.models.is_empty()
    }

    fn toggle_picker(&mut self, picker: Picker, cx: &mut Context<Self>) {
        self.picking = (self.picking != Some(picker)).then_some(picker);
        cx.notify();
    }

    /// The model and effort chips, shown only for what rh says the running session can change.
    pub(super) fn switch_chips(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let mut chips = Vec::new();
        if self.can_pick_model() {
            let label = self
                .shown_model()
                .map_or_else(|| "Model".to_owned(), |model| self.model_label(model));
            chips.push(
                chip(
                    "switch-model".into(),
                    format!("{label} ▾"),
                    self.picking == Some(Picker::Model),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_picker(Picker::Model, cx)))
                .into_any_element(),
            );
        }
        if self.switches.effort {
            let label = self.shown_effort().unwrap_or("Effort");
            chips.push(
                chip(
                    "switch-effort".into(),
                    format!("{label} ▾"),
                    self.picking == Some(Picker::Effort),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_picker(Picker::Effort, cx)))
                .into_any_element(),
            );
        }
        chips
    }

    /// The choices of the open switch; picking one keeps it for every turn sent after.
    pub(super) fn picker_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let choices: Vec<gpui::AnyElement> = match self.picking? {
            Picker::Model if self.can_pick_model() => {
                let shown = self.shown_model();
                self.models
                    .iter()
                    .enumerate()
                    .map(|(index, choice)| {
                        let id = choice.id.clone();
                        chip(
                            SharedString::from(format!("pick-model-{index}")),
                            choice.label.clone(),
                            shown == Some(choice.id.as_str()),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.picked.model = Some(id.clone());
                            this.picking = None;
                            cx.notify();
                        }))
                        .into_any_element()
                    })
                    .collect()
            }
            Picker::Effort if self.switches.effort => {
                let shown = self.shown_effort();
                EFFORTS
                    .iter()
                    .map(|effort| {
                        chip(
                            SharedString::from(format!("pick-effort-{effort}")),
                            (*effort).to_owned(),
                            shown == Some(*effort),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.picked.effort = Some((*effort).to_owned());
                            this.picking = None;
                            cx.notify();
                        }))
                        .into_any_element()
                    })
                    .collect()
            }
            Picker::Model | Picker::Effort => return None,
        };
        Some(
            div()
                .px_4()
                .pt_3()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .children(choices),
        )
    }
}
