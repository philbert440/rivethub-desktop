use gpui::{AnyElement, Context, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};

use crate::session_pane::SessionPane;
use crate::theme::Theme;
use crate::transcript::ExchangeView;

const PITCH: f32 = 10.;
const MAX_MARKS: usize = 42;

pub struct Rail<'a> {
    pub exchanges: &'a [ExchangeView],
    pub active: usize,
    pub hovered: Option<usize>,
}

fn preview(view: &ExchangeView, index: usize, total: usize) -> impl IntoElement {
    let prompt = crate::text::one_line(view.exchange.prompt.as_deref());
    let reply: String = view
        .exchange
        .answer
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    div()
        .w(px(300.))
        .max_h(px(100.))
        .overflow_hidden()
        .px_3()
        .py(px(10.))
        .rounded(px(16.))
        .bg(Theme::raised())
        .shadow_lg()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(format!("Turn {} of {total}", index + 1)),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(Theme::ink())
                .truncate()
                .child(prompt),
        )
        .when(!reply.is_empty(), |card| {
            card.child(
                div()
                    .text_size(px(12.))
                    .text_color(Theme::ink_dim())
                    .child(SharedString::from(reply)),
            )
        })
}

pub fn rail(rail: &Rail<'_>, cx: &mut Context<SessionPane>) -> Option<AnyElement> {
    let total = rail.exchanges.len();
    if total < 2 {
        return None;
    }
    let first = rail
        .active
        .saturating_sub(MAX_MARKS / 2)
        .min(total.saturating_sub(MAX_MARKS));
    let shown = first..(first + MAX_MARKS).min(total);
    let marks = shown.clone().map(|index| {
        let active = index == rail.active;
        let hovered = rail.hovered == Some(index);
        let (width, color) = if active {
            (20., Theme::accent())
        } else if hovered {
            (18., Theme::ink())
        } else {
            (12., Theme::ink_dim())
        };
        div()
            .id(("turn", index))
            .h(px(PITCH))
            .w(px(28.))
            .flex()
            .items_center()
            .justify_end()
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                this.hover_turn(hovering.then_some(index), cx);
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.reveal_turn(index, cx)))
            .child(div().h(px(2.)).w(px(width)).rounded(px(1.)).bg(color))
    });
    let card = rail
        .hovered
        .filter(|index| shown.contains(index))
        .and_then(|index| {
            let step = u16::try_from(index - first).map_or(0., f32::from);
            let offset = px(PITCH * step - 20.);
            rail.exchanges.get(index).map(|view| {
                div()
                    .absolute()
                    .right(px(36.))
                    .top(offset)
                    .child(preview(view, index, total))
            })
        });
    Some(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(12.))
            .flex()
            .items_center()
            .child(div().relative().flex().flex_col().children(marks).children(card))
            .into_any_element(),
    )
}
