//! The card above the composer while an agent waits on an ask-user question. It steps through the
//! questions one at a time; each takes a pick, several picks, or an answer in your own words, and
//! the answers go to the agent as the session's next turn.

use app::SessionHandle;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Task, Window, div, prelude::*, px,
};
use model::prompt::Prompt;
use model::tools::{Question, answer};

use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

struct Asked {
    tool: Option<String>,
    questions: Vec<Question>,
    picked: Vec<Vec<String>>,
    written: Vec<Entity<TextInput>>,
    at: usize,
    answered: bool,
    /// Enter in an answer's own words moved on, so the next question's box takes the typing.
    refocus: bool,
    _written: Vec<Subscription>,
}

impl Asked {
    fn lone_pick(&self) -> bool {
        matches!(self.questions.as_slice(), [question] if !question.multi_select)
    }

    fn last(&self) -> bool {
        self.at + 1 >= self.questions.len()
    }

    /// Each question's answer: the options picked, then anything written in your own words.
    fn answers(&self, cx: &Context<QuestionCard>) -> Vec<Vec<String>> {
        self.picked
            .iter()
            .zip(&self.written)
            .map(|(picked, written)| {
                let own = written.read(cx).text().trim().to_owned();
                picked
                    .iter()
                    .cloned()
                    .chain((!own.is_empty()).then_some(own))
                    .collect()
            })
            .collect()
    }
}

/// The options an agent recommends, picked before you choose: the first for a one-pick question,
/// all of them for a pick-several one.
fn recommended(question: &Question) -> Vec<String> {
    let mut marked = question
        .options
        .iter()
        .filter(|option| option.recommended())
        .map(|option| option.label.clone());
    if question.multi_select {
        marked.collect()
    } else {
        marked.next().into_iter().collect()
    }
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

    /// Follows the session's open question, keeping your answers while it stays the same one.
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
        self.asked = open.map(|(tool, questions)| {
            let written: Vec<Entity<TextInput>> = questions
                .iter()
                .map(|_| cx.new(|cx| TextInput::new("Or answer in your own words", cx)))
                .collect();
            let subscriptions = written
                .iter()
                .flat_map(|input| {
                    [
                        cx.observe(input, |_, _, cx| cx.notify()),
                        cx.subscribe(input, |this, input, Submitted(text), cx| {
                            input.update(cx, |input, cx| input.set_text(text, cx));
                            this.next(cx);
                            if let Some(asked) = this.asked.as_mut().filter(|asked| !asked.answered) {
                                asked.refocus = true;
                            }
                        }),
                    ]
                })
                .collect();
            Asked {
                tool: tool.id.clone(),
                questions: questions.to_vec(),
                picked: questions.iter().map(recommended).collect(),
                written,
                at: 0,
                answered: false,
                refocus: false,
                _written: subscriptions,
            }
        });
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(asked) = self.asked.as_ref().filter(|asked| !asked.answered) else {
            return;
        };
        let text = answer(&asked.questions, &asked.answers(cx));
        if text.is_empty() {
            return;
        }
        if let Some(asked) = self.asked.as_mut() {
            asked.answered = true;
        }
        self.handle.answer(Prompt::new(text));
        cx.notify();
    }

    /// On to the next question, or the answers go once the last is reached.
    fn next(&mut self, cx: &mut Context<Self>) {
        let Some(asked) = self.asked.as_mut().filter(|asked| !asked.answered) else {
            return;
        };
        if asked.last() {
            self.send(cx);
        } else {
            asked.at += 1;
            cx.notify();
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(asked) = self.asked.as_mut() {
            asked.at = asked.at.saturating_sub(1);
            cx.notify();
        }
    }

    /// Leaves the shown question unanswered and moves on.
    fn skip(&mut self, cx: &mut Context<Self>) {
        let Some(asked) = self.asked.as_mut().filter(|asked| !asked.answered) else {
            return;
        };
        if let Some(picked) = asked.picked.get_mut(asked.at) {
            picked.clear();
        }
        if let Some(written) = asked.written.get(asked.at).cloned() {
            written.update(cx, |input, cx| input.set_text("", cx));
        }
        self.next(cx);
    }

    /// A pick on a one-pick question answers it: a lone question goes at once, and one of several
    /// moves on to the next.
    fn pick(&mut self, label: &str, cx: &mut Context<Self>) {
        let Some(asked) = self.asked.as_mut().filter(|asked| !asked.answered) else {
            return;
        };
        let at = asked.at;
        let (Some(question), Some(picked)) = (asked.questions.get(at), asked.picked.get_mut(at)) else {
            return;
        };
        let multi = question.multi_select;
        if let Some(chosen) = picked.iter().position(|chosen| chosen == label) {
            picked.remove(chosen);
            cx.notify();
            return;
        }
        if !multi {
            picked.clear();
        }
        picked.push(label.to_owned());
        if multi {
            cx.notify();
        } else if asked.lone_pick() {
            self.send(cx);
        } else if asked.last() {
            cx.notify();
        } else {
            self.next(cx);
        }
    }

    fn option(
        index: usize,
        label: &str,
        description: Option<&str>,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picked = label.to_owned();
        div()
            .id(("ask-option", index))
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
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick(&picked, cx)))
            .child(label.to_owned())
            .children(description.map(|description| {
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(description.to_owned())
            }))
            .into_any_element()
    }

    fn link(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .cursor_pointer()
            .text_sm()
            .text_color(Theme::ink_dim())
            .hover(|link| link.text_color(Theme::ink()))
            .child(label)
    }

    /// Back, Skip, and Next or Send. Skip on the last question needs another answered, since the
    /// answers can't go out empty.
    fn footer(asked: &Asked, answers: &[Vec<String>], cx: &mut Context<Self>) -> impl IntoElement {
        let ready = answers.iter().any(|answer| !answer.is_empty());
        let others = answers
            .iter()
            .enumerate()
            .any(|(index, answer)| index != asked.at && !answer.is_empty());
        let label = if asked.last() {
            if asked.questions.len() == 1 {
                "Send answer"
            } else {
                "Send answers"
            }
        } else {
            "Next"
        };
        let shown = !asked.lone_pick() || ready;
        div()
            .flex()
            .items_center()
            .gap_4()
            .when(asked.at > 0, |row| {
                row.child(
                    Self::link("ask-back", "Back")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back(cx))),
                )
            })
            .child(div().flex_1())
            .when(asked.questions.len() > 1 && (!asked.last() || others), |row| {
                row.child(
                    Self::link("ask-skip", "Skip")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.skip(cx))),
                )
            })
            .when(shown, |row| {
                row.child(
                    div()
                        .id("ask-next")
                        .h(px(34.))
                        .px_4()
                        .rounded(px(17.))
                        .flex()
                        .items_center()
                        .map(|button| {
                            if ready || !asked.last() {
                                button
                                    .cursor_pointer()
                                    .bg(Theme::accent())
                                    .text_color(Theme::on_accent())
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.next(cx)))
                            } else {
                                button
                                    .border_1()
                                    .border_color(Theme::line())
                                    .text_color(Theme::ink_dim())
                            }
                        })
                        .child(label),
                )
            })
    }
}

impl Render for QuestionCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(asked) = self.asked.as_mut().filter(|asked| asked.refocus) {
            asked.refocus = false;
            if let Some(written) = asked.written.get(asked.at) {
                let handle = written.focus_handle(cx);
                window.defer(cx, move |window, cx| window.focus(&handle, cx));
            }
        }
        let Some(asked) = self.asked.as_ref().filter(|asked| !asked.answered) else {
            return div();
        };
        let (Some(question), Some(picked), Some(written)) = (
            asked.questions.get(asked.at),
            asked.picked.get(asked.at),
            asked.written.get(asked.at).cloned(),
        ) else {
            return div();
        };
        let answers = asked.answers(cx);
        let options: Vec<AnyElement> = question
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let selected = picked.contains(&option.label);
                Self::option(index, &option.label, option.description.as_deref(), selected, cx)
            })
            .collect();
        let heading: SharedString = if asked.questions.len() == 1 {
            "Waiting for your answer".into()
        } else {
            format!("Question {} of {}", asked.at + 1, asked.questions.len()).into()
        };
        let title = question.question.clone().or_else(|| question.header.clone());
        let footer = Self::footer(asked, &answers, cx);
        div()
            .w_full()
            .p_5()
            .rounded(px(28.))
            .bg(Theme::card())
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xs().text_color(Theme::accent()).child(heading))
            .child(
                div()
                    .id(("ask-question", asked.at))
                    .max_h(window.viewport_size().height * 0.5)
                    .overflow_y_scroll()
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
                    .child(
                        div()
                            .mt_1()
                            .px_3()
                            .py(px(8.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(Theme::line())
                            .child(written),
                    ),
            )
            .child(footer)
    }
}
