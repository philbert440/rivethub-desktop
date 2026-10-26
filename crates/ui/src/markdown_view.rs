use gpui::{
    AnyElement, ClipboardItem, FontStyle, FontWeight, Hsla, IntoElement, ParentElement, SharedString,
    StrikethroughStyle, Styled, StyledText, TextRun, UnderlineStyle, Window, div, font, prelude::*, px,
};
use model::markdown::{Block, Run};

use crate::theme::Theme;

fn styled(runs: &[Run], base: Hsla, bold_all: bool, window: &Window) -> StyledText {
    let prose = window.text_style().font();
    let text: String = runs.iter().map(|run| run.text.as_str()).collect();
    let text_runs = runs
        .iter()
        .filter(|run| !run.text.is_empty())
        .map(|run| {
            let mut face = if run.code {
                font(Theme::MONO)
            } else {
                prose.clone()
            };
            if run.marks.bold || bold_all {
                face.weight = FontWeight::SEMIBOLD;
            }
            if run.marks.italic {
                face.style = FontStyle::Italic;
            }
            TextRun {
                len: run.text.len(),
                font: face,
                color: if run.link.is_some() {
                    Theme::accent().into()
                } else {
                    base
                },
                background_color: run.code.then(|| Theme::card().into()),
                underline: run.link.is_some().then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(Theme::accent().into()),
                    wavy: false,
                }),
                strikethrough: run.marks.strike.then(|| StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(base),
                }),
            }
        })
        .collect();
    StyledText::new(SharedString::from(text)).with_runs(text_runs)
}

fn code_block(language: Option<&str>, text: &str, id: SharedString) -> AnyElement {
    let copied = text.to_owned();
    div()
        .w_full()
        .rounded(px(16.))
        .bg(Theme::code())
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .px(px(14.))
                .py(px(9.))
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(language.unwrap_or("text").to_owned())
                .child(div().flex_1())
                .child(
                    div()
                        .id(id)
                        .cursor_pointer()
                        .hover(|copy| copy.text_color(Theme::ink()))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                        })
                        .child("Copy"),
                ),
        )
        .child(
            div()
                .px_4()
                .pb_4()
                .font_family(Theme::MONO)
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(Theme::ink())
                .child(text.trim_end_matches('\n').to_owned()),
        )
        .into_any_element()
}

pub fn blocks(blocks: &[Block], scope: &str, base: Hsla, window: &Window) -> Vec<AnyElement> {
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| render(block, &format!("{scope}-{index}"), base, window))
        .collect()
}

fn render(block: &Block, scope: &str, base: Hsla, window: &Window) -> AnyElement {
    match block {
        Block::Paragraph(runs) => div()
            .w_full()
            .child(styled(runs, base, false, window))
            .into_any_element(),
        Block::Heading { level, runs } => {
            let size = match level {
                1 => 21.,
                2 => 19.,
                3 => 18.,
                _ => 14.,
            };
            div()
                .w_full()
                .pt_2()
                .text_size(px(size))
                .child(styled(runs, Theme::ink().into(), true, window))
                .into_any_element()
        }
        Block::List { start, items } => div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker =
                    start.map_or_else(|| "•".to_owned(), |first| format!("{}.", first + index as u64));
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(18.))
                            .text_color(Theme::ink_dim())
                            .child(marker),
                    )
                    .child(div().flex_1().flex().flex_col().gap_2().children(blocks(
                        item,
                        &format!("{scope}-{index}"),
                        base,
                        window,
                    )))
            }))
            .into_any_element(),
        Block::Quote(inner) => div()
            .w_full()
            .pl_3()
            .border_l_2()
            .border_color(Theme::line())
            .flex()
            .flex_col()
            .gap_2()
            .children(blocks(inner, scope, Theme::ink_dim().into(), window))
            .into_any_element(),
        Block::Code { language, text } => code_block(
            language.as_deref(),
            text,
            SharedString::from(format!("copy-{scope}")),
        ),
        Block::Table { head, rows } => div()
            .w_full()
            .rounded(px(12.))
            .border_1()
            .border_color(Theme::line())
            .overflow_hidden()
            .child(div().flex().bg(Theme::card()).children(head.iter().map(|cell| {
                div()
                    .flex_1()
                    .px_4()
                    .py(px(10.))
                    .child(styled(cell, Theme::ink().into(), true, window))
            })))
            .children(rows.iter().map(|row| {
                div()
                    .flex()
                    .border_t_1()
                    .border_color(Theme::line())
                    .children(row.iter().map(|cell| {
                        div()
                            .flex_1()
                            .px_4()
                            .py(px(10.))
                            .child(styled(cell, base, false, window))
                    }))
            }))
            .into_any_element(),
        Block::Rule => div().w_full().h(px(1.)).bg(Theme::line()).into_any_element(),
    }
}
