use std::sync::Arc;

use gpui::{
    App, ClickEvent, Context, ExternalPaths, Focusable, IntoElement, ParentElement, Pixels, Styled, Window,
    div, list, prelude::*, px,
};

use super::{PaneEvent, SessionPane};
use crate::theme::Theme;
use crate::transcript::{self, Fold};

impl SessionPane {
    pub fn hover_turn(&mut self, turn: Option<usize>, cx: &mut Context<Self>) {
        if self.hovered_turn != turn {
            self.hovered_turn = turn;
            cx.notify();
        }
    }

    /// Forks the conversation as it stood after the exchange at `index`: a new chat carries every
    /// prompt up to and including that one, with their replies.
    pub fn fork_after(&mut self, index: usize, cx: &mut Context<Self>) {
        let asked = self
            .exchanges
            .iter()
            .take(index + 1)
            .filter(|view| view.exchange.prompt.is_some())
            .count();
        if self.forking.is_some() {
            return;
        }
        let prompts = u32::try_from(asked).unwrap_or(u32::MAX);
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        self.forking = Some(index);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let forked = hub.fork(&summary, prompts).await;
            this.update(cx, |this, cx| {
                this.forking = None;
                cx.notify();
                match forked {
                    Ok(forked) => cx.emit(PaneEvent::Forked(Box::new(forked))),
                    Err(err) => cx.emit(PaneEvent::Failed(format!("forking failed: {err}"))),
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn reveal_turn(&mut self, index: usize, cx: &mut Context<Self>) {
        self.turns.scroll_to_reveal_item(index);
        cx.notify();
    }

    fn active_turn(&self) -> usize {
        let last = self.exchanges.len().saturating_sub(1);
        if self.turns.is_scrolled_to_end() == Some(false) {
            self.turns.logical_scroll_top().item_ix.min(last)
        } else {
            last
        }
    }

    pub(super) fn chat_body(
        &mut self,
        column: Pixels,
        behind: bool,
        footer: gpui::Div,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let shared = self.handle.view();
        let view = shared.read();
        let exchanges = Arc::clone(&self.exchanges);
        let folds = Arc::clone(&self.folds);
        let fetched = Arc::clone(&self.fetched);
        let previews = Arc::clone(&self.previews);
        let plan = view.plan().clone();
        let forking = self.forking;
        let pane = cx.entity().downgrade();
        let cwd = self.summary.cwd.clone();
        let home = std::env::var("HOME").ok();
        let transcript = list(
            self.turns.clone(),
            move |index, window: &mut Window, _: &mut App| {
                exchanges.get(index).map_or_else(
                    || div().into_any_element(),
                    |exchange| {
                        let context = transcript::Context {
                            index,
                            folds: &folds,
                            column,
                            pane: pane.clone(),
                            cwd: cwd.as_deref(),
                            home: home.as_deref(),
                            fetched: &fetched,
                            previews: &previews,
                            plan: &plan,
                            forking: forking == Some(index),
                        };
                        transcript::exchange(exchange, &context, window)
                    },
                )
            },
        )
        .size_full();
        let back = behind.then(|| {
            div().w(column).flex().justify_end().child(
                div()
                    .id("back-to-bottom")
                    .size(px(34.))
                    .rounded(px(17.))
                    .bg(Theme::raised())
                    .text_color(Theme::ink())
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.turns.scroll_to_end();
                        cx.notify();
                    }))
                    .child("↓"),
            )
        });
        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .border_2()
            .border_color(gpui::transparent_black())
            .drag_over::<ExternalPaths>(|style, _, _, _| style.border_color(Theme::accent()))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| this.attach_dropped(dropped, cx)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .pt_2()
                    .child(transcript)
                    .children(
                        wide.then(|| {
                            let rail = crate::turn_rail::Rail {
                                exchanges: &self.exchanges,
                                active: self.active_turn(),
                                hovered: self.hovered_turn,
                            };
                            crate::turn_rail::rail(&rail, cx)
                        })
                        .flatten(),
                    )
                    .child(Self::fade()),
            )
            .children(back)
            .child(
                footer
                    .children(self.questions.read(cx).waiting().then(|| self.questions.clone()))
                    .children(self.plan_panel(&view, cx))
                    .children(self.queue_dock(cx))
                    .child(
                        self.approval_card(&view, &self.composer.focus_handle(cx))
                            .unwrap_or_else(|| self.composer_card(&view, cx)),
                    ),
            )
    }

    fn fade() -> impl IntoElement {
        let mut clear: gpui::Hsla = Theme::surface().into();
        clear.a = 0.;
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(36.))
            .bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(clear, 0.),
                gpui::linear_color_stop(Theme::surface(), 1.),
            ))
    }

    fn tool_at(&self, exchange: usize, step: usize, tool: usize) -> Option<&model::session::ToolCall> {
        match self.exchanges.get(exchange)?.exchange.steps.get(step)? {
            model::chat::Step::Tools(tools) => tools.get(tool),
            _ => None,
        }
    }

    fn fetch_output(&mut self, exchange: usize, step: usize, tool: usize, cx: &mut Context<Self>) {
        let Some(call) = self
            .tool_at(exchange, step, tool)
            .filter(|call| crate::tool_view::fetchable(call))
        else {
            return;
        };
        let (Some(id), name) = (call.id.clone(), call.name.clone()) else {
            return;
        };
        if self.fetched.contains_key(&id) {
            return;
        }
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        cx.spawn(async move |this, cx| {
            let fetched = hub.tool_output(&summary, id.clone(), name).await;
            this.update(cx, |this, cx| {
                if let Ok(Some(output)) = fetched {
                    Arc::make_mut(&mut this.fetched).insert(id, output);
                    if exchange < this.turns.item_count() {
                        this.turns.splice(exchange..exchange + 1, 1);
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn toggle_fold(&mut self, fold: Fold, cx: &mut Context<Self>) {
        Arc::make_mut(&mut self.folds).toggle(fold);
        if let Fold::Tool(exchange, step, tool) = fold {
            self.fetch_output(exchange, step, tool, cx);
        }
        let row = match fold {
            Fold::Exchange(index)
            | Fold::Group(index, _)
            | Fold::Thought(index, _)
            | Fold::Tool(index, _, _) => Some(index),
            Fold::Plan => None,
        };
        if let Some(index) = row.filter(|index| *index < self.turns.item_count()) {
            self.turns.splice(index..index + 1, 1);
        }
        cx.notify();
    }
}
