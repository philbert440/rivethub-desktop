use gpui::{
    AnyElement, App, ClickEvent, FontStyle, FontWeight, Hsla, InteractiveText, IntoElement, ParentElement,
    RenderOnce, SharedString, StrikethroughStyle, Styled, StyledText, TextRun, UnderlineStyle, Window, div,
    font, prelude::*, px,
};
use model::markdown::{Block, Run};

use crate::copy;
use crate::theme::Theme;

/// Inline text, whose web links open in the browser when clicked.
fn styled(runs: &[Run], base: Hsla, bold_all: bool, id: &str, window: &Window) -> AnyElement {
    let text = styled_text(runs, base, bold_all, window);
    let mut ranges = Vec::new();
    let mut urls = Vec::new();
    let mut offset = 0;
    for run in runs {
        if let Some(url) = run.url() {
            ranges.push(offset..offset + run.text.len());
            urls.push(url.to_owned());
        }
        offset += run.text.len();
    }
    if urls.is_empty() {
        return text.into_any_element();
    }
    InteractiveText::new(SharedString::from(format!("text-{id}")), text)
        .on_click(ranges, move |index, _, cx| {
            if let Some(url) = urls.get(index) {
                cx.stop_propagation();
                cx.open_url(url);
            }
        })
        .into_any_element()
}

fn styled_text(runs: &[Run], base: Hsla, bold_all: bool, window: &Window) -> StyledText {
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
                color: if run.url().is_some() {
                    Theme::accent().into()
                } else {
                    base
                },
                background_color: run.code.then(|| Theme::card().into()),
                underline: run.url().is_some().then(|| UnderlineStyle {
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

/// The narrowest a table column gets; a table wider than its space scrolls sideways.
const TABLE_COLUMN: f32 = 140.;

/// Whether a code block's long lines run on past the edge instead of wrapping, kept between frames.
#[derive(Debug, Clone, Copy, Default)]
struct Unwrapped(bool);

/// A fenced block of code under a bar with its language, a Wrap toggle and a Copy that confirms.
#[derive(IntoElement)]
struct CodeBlock {
    id: SharedString,
    language: Option<String>,
    text: String,
}

impl CodeBlock {
    fn button(&self, name: &str, label: &'static str, lit: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(SharedString::from(format!("{}-{name}", self.id)))
            .cursor_pointer()
            .text_color(if lit { Theme::accent() } else { Theme::ink_dim() })
            .hover(|button| button.text_color(Theme::ink()))
            .child(label)
    }
}

impl RenderOnce for CodeBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let unwrapping = window.use_keyed_state(self.id.clone(), cx, |_, _| Unwrapped::default());
        let unwrapped = unwrapping.read(cx).0;
        let copied = copy::state(format!("{}-copied", self.id), window, cx);
        let done = copied.read(cx).just();
        let text = self.text.trim_end_matches('\n').to_owned();
        let copy = {
            let text = text.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| copy::copy(&copied, &text, cx)
        };
        let wrap = move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            unwrapping.update(cx, |unwrapped, cx| {
                unwrapped.0 = !unwrapped.0;
                cx.notify();
            });
        };
        let body = div()
            .id(SharedString::from(format!("{}-body", self.id)))
            .px_4()
            .pb_4()
            .font_family(Theme::MONO)
            .text_size(px(12.))
            .line_height(px(19.))
            .text_color(Theme::ink())
            .map(|body| {
                if unwrapped {
                    body.overflow_x_scroll()
                        .child(div().whitespace_nowrap().child(text))
                } else {
                    body.child(text)
                }
            });
        div()
            .w_full()
            .rounded(px(16.))
            .bg(Theme::code())
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px(px(14.))
                    .py(px(9.))
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(self.language.clone().unwrap_or_else(|| "text".to_owned()))
                    .child(div().flex_1())
                    .child(self.button("wrap", "Wrap", !unwrapped).on_click(wrap))
                    .child(
                        self.button("copy", if done { "Copied" } else { "Copy" }, done)
                            .on_click(copy),
                    ),
            )
            .child(body)
    }
}

pub fn blocks(blocks: &[Block], scope: &str, base: Hsla, window: &Window) -> Vec<AnyElement> {
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| render(block, &format!("{scope}-{index}"), base, window))
        .collect()
}

fn list(start: Option<u64>, items: &[Vec<Block>], scope: &str, base: Hsla, window: &Window) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(items.iter().enumerate().map(|(index, item)| {
            let marker = start.map_or_else(|| "•".to_owned(), |first| format!("{}.", first + index as u64));
            div()
                .w_full()
                .flex()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .min_w(px(18.))
                        .text_color(Theme::ink_dim())
                        .child(marker),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(blocks(item, &format!("{scope}-{index}"), base, window)),
                )
        }))
        .into_any_element()
}

fn render(block: &Block, scope: &str, base: Hsla, window: &Window) -> AnyElement {
    match block {
        Block::Paragraph(runs) => div()
            .w_full()
            .child(styled(runs, base, false, scope, window))
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
                .child(styled(runs, Theme::ink().into(), true, scope, window))
                .into_any_element()
        }
        Block::List { start, items } => list(*start, items, scope, base, window),
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
        Block::Code { language, text } => CodeBlock {
            id: SharedString::from(format!("code-{scope}")),
            language: language.clone(),
            text: text.clone(),
        }
        .into_any_element(),
        Block::Table { head, rows } => {
            let columns = rows.iter().map(Vec::len).fold(head.len(), usize::max);
            let wide = px(TABLE_COLUMN * u16::try_from(columns).map_or(f32::MAX, f32::from));
            div()
                .id(SharedString::from(format!("{scope}-table")))
                .w_full()
                .rounded(px(12.))
                .border_1()
                .border_color(Theme::line())
                .overflow_x_scroll()
                .child(
                    div()
                        .w_full()
                        .min_w(wide)
                        .child(
                            div()
                                .flex()
                                .bg(Theme::card())
                                .children(head.iter().enumerate().map(|(column, cell)| {
                                    div().flex_1().min_w_0().px_4().py(px(10.)).child(styled(
                                        cell,
                                        Theme::ink().into(),
                                        true,
                                        &format!("{scope}-head-{column}"),
                                        window,
                                    ))
                                })),
                        )
                        .children(rows.iter().enumerate().map(|(line, row)| {
                            div().flex().border_t_1().border_color(Theme::line()).children(
                                row.iter().enumerate().map(|(column, cell)| {
                                    div().flex_1().min_w_0().px_4().py(px(10.)).child(styled(
                                        cell,
                                        base,
                                        false,
                                        &format!("{scope}-{line}-{column}"),
                                        window,
                                    ))
                                }),
                            )
                        })),
                )
                .into_any_element()
        }
        Block::Rule => div().w_full().h(px(1.)).bg(Theme::line()).into_any_element(),
    }
}
