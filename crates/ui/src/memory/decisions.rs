use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, SharedString, Styled,
    Subscription, Window, div, prelude::*, px,
};
use model::TopicId;
use model::decisions::{Decision, Group, text};

use super::MemoryPage;
use crate::input::{Submitted, TextInput};
use crate::text::{ago, now_ms};
use crate::theme::Theme;

/// What the open decision input changes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Add(TopicId),
    Edit { topic: TopicId, decision: String },
    Supersede { topic: TopicId, decision: String },
}

impl Target {
    fn is(&self, decision: &Decision) -> bool {
        match self {
            Self::Add(_) => false,
            Self::Edit { decision: id, .. } | Self::Supersede { decision: id, .. } => *id == decision.id,
        }
    }

    fn action(&self) -> &'static str {
        match self {
            Self::Add(_) => "Add",
            Self::Edit { .. } => "Save",
            Self::Supersede { .. } => "Supersede",
        }
    }
}

/// A decision being added, edited or superseded in place on the page.
pub(super) struct Deciding {
    target: Target,
    input: Entity<TextInput>,
    busy: bool,
    problem: Option<String>,
    _submitted: Subscription,
}

fn link(id: SharedString, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .rounded(px(10.))
        .cursor_pointer()
        .text_xs()
        .text_color(Theme::accent())
        .hover(|link| link.bg(Theme::selected()))
        .child(label)
}

impl MemoryPage {
    pub(super) fn load_decisions(&self, root: TopicId, cx: &mut Context<Self>) {
        let Some(topics) = self.library.as_ref().map(|library| library.topics.clone()) else {
            return;
        };
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.page_decisions(topics, root.clone()).await;
            this.update(cx, |this, cx| {
                if this.opened_root().as_ref() != Some(&root) {
                    return;
                }
                match loaded {
                    Ok(groups) => this.decisions = Some(groups),
                    Err(err) if err.is_missing_route() => {
                        this.problem = Some("Decisions need a newer rh on this node.".into());
                    }
                    Err(err) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The root topic of the open topic page.
    fn opened_root(&self) -> Option<TopicId> {
        let super::Opened::Topic(topic) = self.opened.as_ref()? else {
            return None;
        };
        let library = self.library.as_ref()?;
        let placements = model::topics::Placements::new();
        Some(
            model::topics::Atlas::new(&library.topics, &[], &placements)
                .root(topic)
                .clone(),
        )
    }

    fn begin(&mut self, target: Target, current: &str, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match target {
            Target::Add(_) => "What was decided",
            Target::Edit { .. } => "The decision",
            Target::Supersede { .. } => "What replaces it (optional)",
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(placeholder, cx);
            input.set_text(current, cx);
            input
        });
        let submitted = cx.subscribe(&input, |this, _, Submitted(typed), cx| {
            this.commit(Some(typed), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.deciding = Some(Deciding {
            target,
            input,
            busy: false,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.deciding = None;
        cx.notify();
    }

    /// Sends the open input's text to rh. Adding and editing need text; superseding without any
    /// marks the decision superseded with nothing in its place.
    fn commit(&mut self, typed: Option<&str>, cx: &mut Context<Self>) {
        let Some(deciding) = self.deciding.as_mut() else {
            return;
        };
        if deciding.busy {
            return;
        }
        let typed = typed.and_then(text);
        let hub = self.hub.clone();
        let target = deciding.target.clone();
        deciding.busy = true;
        deciding.problem = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let written = match (target.clone(), typed) {
                (Target::Add(topic), Some(typed)) => hub.add_decision(topic, typed).await.map(drop),
                (Target::Edit { topic, decision }, Some(typed)) => {
                    hub.edit_decision(topic, decision, typed).await.map(drop)
                }
                (Target::Supersede { topic, decision }, typed) => {
                    hub.supersede_decision(topic, decision, typed).await
                }
                (Target::Add(_) | Target::Edit { .. }, None) => {
                    Err(GatewayError::Conflict("A decision needs some text.".into()))
                }
            };
            this.update(cx, |this, cx| {
                if this.deciding.as_ref().map(|open| &open.target) != Some(&target) {
                    return;
                }
                match written {
                    Ok(()) => {
                        this.deciding = None;
                        if let Some(root) = this.opened_root() {
                            this.load_decisions(root, cx);
                        }
                    }
                    Err(err) => {
                        if let Some(open) = this.deciding.as_mut() {
                            open.busy = false;
                            open.problem = Some(match err {
                                GatewayError::Conflict(message) => message,
                                other => other.to_string(),
                            });
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn editor(deciding: &Deciding, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .py_1()
                            .rounded(px(12.))
                            .bg(Theme::card())
                            .border_1()
                            .border_color(Theme::accent())
                            .child(deciding.input.clone()),
                    )
                    .child(
                        link("decision-commit".into(), deciding.target.action())
                            .when(deciding.busy, |link| link.opacity(0.4))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                let typed = this
                                    .deciding
                                    .as_ref()
                                    .map(|open| open.input.read(cx).text().to_owned());
                                this.commit(typed.as_deref(), cx);
                            })),
                    )
                    .child(
                        link("decision-cancel".into(), "Cancel")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel(cx))),
                    ),
            )
            .children(
                deciding
                    .problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .into_any_element()
    }

    fn decision_row(&self, decision: &Decision, now: i64, cx: &mut Context<Self>) -> AnyElement {
        if let Some(deciding) = self
            .deciding
            .as_ref()
            .filter(|deciding| deciding.target.is(decision))
        {
            let editing = matches!(deciding.target, Target::Edit { .. });
            return div()
                .flex()
                .flex_col()
                .gap_1()
                .when(!editing, |row| {
                    row.child(
                        div()
                            .text_color(Theme::ink_dim())
                            .line_through()
                            .child(decision.text.clone()),
                    )
                })
                .child(Self::editor(deciding, cx))
                .into_any_element();
        }
        let group = SharedString::from(format!("decision-{}", decision.id));
        let active = decision.is_active();
        div()
            .group(group.clone())
            .flex()
            .items_baseline()
            .gap_2()
            .child(div().text_color(Theme::ink_dim()).child("•"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(if active { Theme::ink() } else { Theme::ink_dim() })
                    .when(!active, gpui::Styled::line_through)
                    .child(decision.text.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(if active {
                        format!("{} · {}", decision.by, ago(now, decision.updated_ms))
                    } else {
                        "superseded".to_owned()
                    }),
            )
            .when(active, |row| {
                let (edit, supersede) = (decision.clone(), decision.clone());
                row.child(
                    div()
                        .flex()
                        .flex_none()
                        .invisible()
                        .group_hover(group, gpui::Styled::visible)
                        .child(
                            link(format!("edit-{}", decision.id).into(), "Edit").on_click(cx.listener(
                                move |this, _: &ClickEvent, window, cx| {
                                    let target = Target::Edit {
                                        topic: edit.topic.clone(),
                                        decision: edit.id.clone(),
                                    };
                                    this.begin(target, &edit.text, window, cx);
                                },
                            )),
                        )
                        .child(
                            link(format!("supersede-{}", decision.id).into(), "Supersede").on_click(
                                cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    let target = Target::Supersede {
                                        topic: supersede.topic.clone(),
                                        decision: supersede.id.clone(),
                                    };
                                    this.begin(target, "", window, cx);
                                }),
                            ),
                        ),
                )
            })
            .into_any_element()
    }

    fn decision_group(&self, index: usize, group: &Group, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let adding = self
            .deciding
            .as_ref()
            .filter(|deciding| deciding.target == Target::Add(group.topic.clone()));
        let topic = group.topic.clone();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(index > 0, |column| {
                column.child(
                    div()
                        .pt_2()
                        .text_sm()
                        .text_color(Theme::ink())
                        .child(group.name.clone()),
                )
            })
            .children(
                group
                    .decisions
                    .iter()
                    .map(|decision| self.decision_row(decision, now, cx)),
            )
            .child(match adding {
                Some(deciding) => Self::editor(deciding, cx),
                None => div()
                    .flex()
                    .child(
                        link(
                            SharedString::from(format!("add-decision-{index}")),
                            "+ Add a decision",
                        )
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, window, cx| {
                                this.begin(Target::Add(topic.clone()), "", window, cx);
                            },
                        )),
                    )
                    .into_any_element(),
            })
            .into_any_element()
    }

    /// The open topic page's decisions, the root's first and then each sub-topic's, each one
    /// editable or supersedable in place.
    pub(super) fn decisions_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let groups = self.decisions.as_ref()?;
        let now = now_ms();
        Some(
            div()
                .pt_4()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_lg().text_color(Theme::ink()).child("Decisions"))
                .children(
                    groups
                        .iter()
                        .enumerate()
                        .map(|(index, group)| self.decision_group(index, group, now, cx)),
                )
                .into_any_element(),
        )
    }
}
