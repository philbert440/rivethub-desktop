use app::Hub;
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, prelude::*, px, rgb,
};
use model::HarnessId;
use model::agents::{Agent, AgentDraft};
use model::harness::HarnessOffer;

use crate::input::TextInput;
use crate::theme::Theme;

const COLORS: [u32; 6] = [
    0x006c_dbac,
    0x008e_cbff,
    0x00f5_c56b,
    0x00ff_b4ab,
    0x00c9_b2ff,
    0x00a7_b2ab,
];
const EFFORTS: [&str; 5] = ["off", "low", "medium", "high", "xhigh"];

#[derive(Debug, Clone)]
pub enum AgentEditorEvent {
    Saved,
    Deleted,
    Closed,
}

pub struct AgentEditor {
    hub: Hub,
    id: Option<String>,
    draft: AgentDraft,
    name: Entity<TextInput>,
    folder: Entity<TextInput>,
    instructions: Entity<TextInput>,
    nodes: Vec<String>,
    offers: Vec<HarnessOffer>,
    problem: Option<String>,
    busy: bool,
}

impl EventEmitter<AgentEditorEvent> for AgentEditor {}

impl AgentEditor {
    pub fn new(hub: Hub, agent: Option<&Agent>, cx: &mut Context<Self>) -> Self {
        let draft = agent.map(AgentDraft::from_agent).unwrap_or_default();
        let field = |placeholder: &'static str, text: Option<&str>, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(placeholder, cx);
                if let Some(text) = text {
                    input.set_text(text, cx);
                }
                input
            })
        };
        let name = field("Name", agent.map(|agent| agent.name.as_str()), cx);
        let folder = field(
            "Working folder, such as /home/you/work/project",
            draft.directory.as_deref(),
            cx,
        );
        let instructions = field(
            "Instructions added to the harness's own system prompt",
            draft.instructions.as_deref(),
            cx,
        );
        let editor = Self {
            hub: hub.clone(),
            id: agent.map(|agent| agent.id.clone()),
            draft,
            name,
            folder,
            instructions,
            nodes: Vec::new(),
            offers: Vec::new(),
            problem: None,
            busy: false,
        };
        cx.spawn(async move |this, cx| {
            let nodes = hub.nodes().await.unwrap_or_default();
            this.update(cx, |this, cx| {
                if this.draft.node.is_none() {
                    this.draft.node = nodes.first().cloned();
                }
                this.nodes = nodes;
                this.load_offers(cx);
            })
            .ok();
        })
        .detach();
        editor
    }

    fn load_offers(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.draft.node.clone() else {
            return;
        };
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let offers = hub.harnesses_on(node).await;
            this.update(cx, |this, cx| {
                match offers {
                    Ok(offers) => {
                        if this.draft.harness.is_none() {
                            this.draft.harness = offers.first().map(|offer| offer.id.clone());
                        }
                        this.offers = offers;
                    }
                    Err(err) => this.problem = Some(format!("couldn't read that node's harnesses: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().to_owned();
        if name.is_empty() {
            self.problem = Some("Give the agent a name.".into());
            cx.notify();
            return;
        }
        if self.draft.harness.is_none() {
            self.problem = Some("Pick a harness.".into());
            cx.notify();
            return;
        }
        let folder = self.folder.read(cx).text().to_owned();
        if !folder.is_empty() && !folder.starts_with('/') {
            self.problem = Some("Use an absolute working folder, starting with /.".into());
            cx.notify();
            return;
        }
        let instructions = self.instructions.read(cx).text().to_owned();
        let mut draft = self.draft.clone();
        draft.name = name;
        draft.directory = Some(folder).filter(|folder| !folder.is_empty());
        draft.instructions = Some(instructions).filter(|text| !text.is_empty());
        self.busy = true;
        self.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        let id = self.id.clone();
        cx.spawn(async move |this, cx| {
            let saved = hub.save_agent(id, draft).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match saved {
                    Ok(_) => cx.emit(AgentEditorEvent::Saved),
                    Err(err) => this.problem = Some(format!("saving failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.id.clone() else {
            return;
        };
        self.busy = true;
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let deleted = hub.delete_agent(id).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match deleted {
                    Ok(()) => cx.emit(AgentEditorEvent::Deleted),
                    Err(err) => this.problem = Some(format!("deleting failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn label(text: &'static str) -> impl IntoElement {
        div().text_xs().text_color(Theme::ink_dim()).child(text)
    }

    fn chip(id: (&'static str, usize), label: SharedString, selected: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(30.))
            .px_3()
            .rounded(px(15.))
            .flex()
            .items_center()
            .cursor_pointer()
            .border_1()
            .map(|chip| {
                if selected {
                    chip.bg(Theme::selected())
                        .border_color(Theme::accent())
                        .text_color(Theme::on_selected())
                } else {
                    chip.bg(Theme::card())
                        .border_color(Theme::card())
                        .text_color(Theme::ink())
                        .hover(|chip| chip.bg(Theme::raised()))
                }
            })
            .child(label)
    }

    fn field(input: &Entity<TextInput>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .bg(Theme::card())
            .border_1()
            .border_color(Theme::line())
            .child(input.clone())
    }

    fn colors(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .gap(px(6.))
            .children(COLORS.iter().enumerate().map(|(index, color)| {
                let color = *color;
                let selected = self.draft.color == color;
                div()
                    .id(("agent-color", index))
                    .size(px(26.))
                    .rounded(px(13.))
                    .cursor_pointer()
                    .bg(rgb(color))
                    .border_2()
                    .border_color(if selected { Theme::ink() } else { Theme::surface() })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.draft.color = color;
                        cx.notify();
                    }))
            }))
    }

    fn nodes_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(self.nodes.iter().enumerate().map(|(index, node)| {
                let selected = self.draft.node.as_deref() == Some(node.as_str());
                let pick = node.clone();
                Self::chip(("agent-node", index), node.clone().into(), selected).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        if this.draft.node.as_deref() != Some(pick.as_str()) {
                            this.draft.node = Some(pick.clone());
                            this.draft.harness = None;
                            this.draft.model = None;
                            this.offers.clear();
                            this.load_offers(cx);
                        }
                    },
                ))
            }))
    }

    fn harness_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(self.offers.iter().enumerate().map(|(index, offer)| {
                let selected = self.draft.harness.as_ref() == Some(&offer.id);
                let pick: HarnessId = offer.id.clone();
                Self::chip(
                    ("agent-harness", index),
                    offer.id.as_str().to_owned().into(),
                    selected,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.draft.harness = Some(pick.clone());
                    this.draft.model = None;
                    cx.notify();
                }))
            }))
    }

    fn model_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let offer = self
            .offers
            .iter()
            .find(|offer| Some(&offer.id) == self.draft.harness.as_ref())?;
        if offer.models.is_empty() {
            return None;
        }
        Some(
            div().flex().flex_col().gap_1().child(Self::label("Model")).child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(offer.models.iter().enumerate().map(|(index, choice)| {
                        let selected = self.draft.model.as_deref() == Some(choice.id.as_str())
                            || (self.draft.model.is_none() && choice.default);
                        let pick = choice.id.clone();
                        Self::chip(("agent-model", index), choice.label.clone().into(), selected).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.draft.model = Some(pick.clone());
                                cx.notify();
                            }),
                        )
                    })),
            ),
        )
    }

    fn effort_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap(px(6.))
            .children(EFFORTS.iter().enumerate().map(|(index, effort)| {
                let selected = self.draft.effort.as_deref() == Some(*effort);
                Self::chip(("agent-effort", index), (*effort).into(), selected).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        this.draft.effort = Some((*effort).to_owned());
                        cx.notify();
                    },
                ))
            }))
    }

    fn fields(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(Self::label("Name"))
                            .child(Self::field(&self.name)),
                    )
                    .child(self.colors(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Self::label("Runs on"))
                    .child(self.nodes_row(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Self::label("Harness · what that node offers"))
                    .child(self.harness_row(cx)),
            )
            .children(self.model_row(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Self::label("Effort"))
                    .child(self.effort_row(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Self::label(
                        "Working folder · a topic's folder wins over this one",
                    ))
                    .child(Self::field(&self.folder)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Self::label("Instructions"))
                    .child(Self::field(&self.instructions)),
            )
    }

    fn button(id: &'static str, label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(36.))
            .px_4()
            .rounded(px(18.))
            .flex()
            .items_center()
            .cursor_pointer()
            .map(|button| {
                if primary {
                    button.bg(Theme::accent()).text_color(Theme::on_accent())
                } else {
                    button
                        .text_color(Theme::ink_dim())
                        .hover(|button| button.text_color(Theme::ink()))
                }
            })
            .child(label)
    }
}

impl Focusable for AgentEditor {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.name.focus_handle(cx)
    }
}

impl Render for AgentEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.id.is_some();
        div()
            .w(px(620.))
            .p_6()
            .rounded(px(28.))
            .bg(Theme::raised())
            .border_1()
            .border_color(Theme::line())
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().text_color(Theme::ink()).child(if editing {
                "Edit agent"
            } else {
                "New agent"
            }))
            .child(self.fields(cx))
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().text_sm().text_color(Theme::warn()).child(problem)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(editing, |row| {
                        row.child(
                            Self::button("agent-delete", "Delete agent", false)
                                .text_color(Theme::warn())
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.delete(cx))),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Self::button("agent-cancel", "Cancel", false).on_click(
                            cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(AgentEditorEvent::Closed)),
                        ),
                    )
                    .child(
                        Self::button("agent-save", if editing { "Save" } else { "Add agent" }, true)
                            .when(self.busy, |button| button.opacity(0.5))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if !this.busy {
                                    this.save(cx);
                                }
                            })),
                    ),
            )
    }
}
