//! The card above the composer while an agent waits on an ask-user question. Picking an answer sends
//! it as the session's next turn, as the old app did; typing in the composer instead answers in your
//! own words.

use app::SessionHandle;
use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window,
    div, prelude::*, px,
};
use model::prompt::Prompt;
use model::tools::{Question, answer};

use crate::theme::Theme;

struct Asked {
    tool: Option<String>,
    questions: Vec<Question>,
    picked: Vec<Vec<String>>,
    answered: bool,
}

pub struct QuestionCard {
    handle: SessionHandle,
    asked: Option<Asked>,
    _revisions: Task<()>,
}

impl QuestionCard {
    pub fn new(handle: SessionHandle, cx: &mut Context<Self>) -> Self {
        let mut revisions = handle.revisions();
        let watcher = cx.spawn(async move |this, cx| {
            while revisions.changed().await.is_ok() {
                if this.update(cx, Self::sync).is_err() {
                    break;
                }
            }
        });
        let mut card = Self {
            handle,
            asked: None,
            _revisions: watcher,
        };
        card.sync(cx);
        card
    }

    pub fn waiting(&self) -> bool {
        self.asked.as_ref().is_some_and(|asked| !asked.answered)
    }

    /// Follows the session's open question, keeping your picks while it stays the same one.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let shared = self.handle.view();
        let view = shared.read();
        let open = view.open_questions();
        let same = match (&self.asked, open) {
            (Some(asked), Some((tool, questions))) => asked.tool == tool.id && asked.questions == questions,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.asked = open.map(|(tool, questions)| Asked {
            tool: tool.id.clone(),
            questions: questions.to_vec(),
            picked: vec![Vec::new(); questions.len()],
            answered: false,
        });
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(asked) = self.asked.as_mut().filter(|asked| !asked.answered) else {
            return;
        };
        let text = answer(&asked.questions, &asked.picked);
        if text.is_empty() {
            return;
        }
        asked.answered = true;
        self.handle.send(Prompt {
            text,
            attachments: Vec::new(),
        });
        cx.notify();
    }

    fn pick(&mut self, question: usize, label: &str, cx: &mut Context<Self>) {
        let Some(asked) = self.asked.as_mut().filter(|asked| !asked.answered) else {
            return;
        };
        let (Some(asked_question), Some(picked)) =
            (asked.questions.get(question), asked.picked.get_mut(question))
        else {
            return;
        };
        let multi = asked_question.multi_select;
        let one_click = asked.questions.len() == 1 && !multi;
        if let Some(at) = picked.iter().position(|chosen| chosen == label) {
            picked.remove(at);
        } else {
            if !multi {
                picked.clear();
            }
            picked.push(label.to_owned());
        }
        if one_click {
            self.send(cx);
        } else {
            cx.notify();
        }
    }

    fn question(asked: &Asked, index: usize, question: &Question, cx: &mut Context<Self>) -> AnyElement {
        let picked = asked.picked.get(index);
        let options = question.options.iter().enumerate().map(|(option, choice)| {
            let selected = picked.is_some_and(|picked| picked.contains(&choice.label));
            let label = choice.label.clone();
            div()
                .id(("ask-option", option))
                .px_3()
                .py(px(6.))
                .rounded(px(12.))
                .border_1()
                .cursor_pointer()
                .flex()
                .flex_col()
                .map(|row| {
                    if selected {
                        row.bg(Theme::selected())
                            .border_color(Theme::accent())
                            .text_color(Theme::on_selected())
                    } else {
                        row.border_color(Theme::line())
                            .text_color(Theme::ink())
                            .hover(|row| row.bg(Theme::raised()))
                    }
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick(index, &label, cx)))
                .child(choice.label.clone())
                .children(
                    choice
                        .description
                        .clone()
                        .map(|description| div().text_xs().text_color(Theme::ink_dim()).child(description)),
                )
        });
        let title = question.question.clone().or_else(|| question.header.clone());
        div()
            .id(("ask-question", index))
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(title.map(|title| div().text_color(Theme::ink()).child(title)))
            .when(question.multi_select, |block| {
                block.child(
                    div()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child("Select all that apply"),
                )
            })
            .children(options)
            .into_any_element()
    }
}

impl Render for QuestionCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(asked) = self.asked.as_ref().filter(|asked| !asked.answered) else {
            return div();
        };
        let one_click =
            asked.questions.len() == 1 && asked.questions.iter().all(|question| !question.multi_select);
        let ready = asked.picked.iter().any(|picked| !picked.is_empty());
        let questions: Vec<AnyElement> = asked
            .questions
            .iter()
            .enumerate()
            .map(|(index, question)| Self::question(asked, index, question, cx))
            .collect();
        let heading: SharedString = if asked.questions.len() == 1 {
            "Waiting for your answer".into()
        } else {
            format!("Waiting for {} answers", asked.questions.len()).into()
        };
        div()
            .w_full()
            .p_5()
            .rounded(px(28.))
            .bg(Theme::card())
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xs().text_color(Theme::accent()).child(heading))
            .children(questions)
            .when(!one_click, |card| {
                card.child(
                    div().flex().justify_end().child(
                        div()
                            .id("ask-send")
                            .h(px(34.))
                            .px_4()
                            .rounded(px(17.))
                            .flex()
                            .items_center()
                            .map(|button| {
                                if ready {
                                    button
                                        .cursor_pointer()
                                        .bg(Theme::accent())
                                        .text_color(Theme::on_accent())
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send(cx)))
                                } else {
                                    button
                                        .border_1()
                                        .border_color(Theme::line())
                                        .text_color(Theme::ink_dim())
                                }
                            })
                            .child("Send answers"),
                    ),
                )
            })
    }
}
