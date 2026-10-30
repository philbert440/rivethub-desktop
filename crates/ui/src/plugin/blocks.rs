mod containers;
mod content;
mod lists;

use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, ParentElement, Rgba, SharedString, Styled, Window, div,
    img, prelude::*, px,
};
use model::plugin::view::{Block, Button, ButtonStyle, Inline, TimeStyle, Tone};

use super::frame::{Frame, Picture, child_key};
use crate::brand;
use crate::text::{ago_phrase, bytes, clock, duration};
use crate::theme::Theme;

/// The fill of a container drawn `level` surfaces up.
fn container(level: u8) -> Rgba {
    if level == 0 {
        Theme::card()
    } else {
        Theme::raised()
    }
}

fn symbol(name: Option<&str>, size: f32, color: Rgba) -> gpui::Svg {
    brand::icon(brand::symbol(name), size, color)
}

fn tone_ink(tone: Option<Tone>) -> Rgba {
    tone.map_or_else(Theme::ink, |tone| Theme::tone(tone).0)
}

pub(super) fn inline(inline: &Inline, now: i64) -> AnyElement {
    match inline {
        Inline::Text {
            text,
            tone,
            icon,
            monospace,
        } => div()
            .flex()
            .items_center()
            .gap_1p5()
            .min_w_0()
            .text_color(tone_ink(*tone))
            .when(*monospace, |text| text.font_family(Theme::MONO).text_xs())
            .children(
                icon.as_deref()
                    .map(|icon| symbol(Some(icon), 16., Theme::ink_dim())),
            )
            .child(div().min_w_0().truncate().child(text.clone()))
            .into_any_element(),
        Inline::Badge { text, tone } => {
            let (ink, fill) = Theme::tone(*tone);
            div()
                .flex_none()
                .px_2()
                .rounded(px(9.))
                .bg(fill)
                .text_xs()
                .text_color(ink)
                .child(text.clone())
                .into_any_element()
        }
        Inline::Time { at_ms, style } => div()
            .flex_none()
            .text_color(Theme::ink_dim())
            .child(match style {
                Some(TimeStyle::Relative) => ago_phrase(now, *at_ms),
                Some(TimeStyle::Absolute) | None => clock(*at_ms),
            })
            .into_any_element(),
        Inline::Duration { ms } => div()
            .flex_none()
            .text_color(Theme::ink_dim())
            .child(duration(*ms))
            .into_any_element(),
        Inline::Bytes(count) => div()
            .flex_none()
            .text_color(Theme::ink_dim())
            .child(bytes(*count))
            .into_any_element(),
    }
}

impl Frame {
    pub(super) fn blocks(
        &self,
        blocks: &[Block],
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        blocks
            .iter()
            .enumerate()
            .map(|(index, block)| self.block(block, &child_key(key, index), level, window, cx))
            .collect()
    }

    fn stack(
        &self,
        blocks: &[Block],
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .min_w_0()
            .children(self.blocks(blocks, key, level, window, cx))
    }

    fn block(
        &self,
        block: &Block,
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match block {
            Block::Section {
                title,
                subtitle,
                actions,
                blocks,
                collapsed,
            } => self.section(
                (title.as_deref(), subtitle.as_deref()),
                actions,
                blocks,
                *collapsed,
                key,
                level,
                window,
                cx,
            ),
            Block::Split {
                primary,
                secondary,
                ratio,
            } => self.split(primary, secondary, *ratio, key, level, window, cx),
            Block::Tabs { tabs, selected } => self.tabs(tabs, selected.as_deref(), key, level, window, cx),
            Block::Text {
                text,
                tone,
                monospace,
            } => div()
                .text_sm()
                .text_color(tone_ink(*tone))
                .when(*monospace, |text| text.font_family(Theme::MONO).text_xs())
                .child(text.clone())
                .into_any_element(),
            Block::Markdown(text) => Self::markdown(text, key, window),
            Block::Code { text, .. } => self.code(text, key),
            Block::Image { path, alt } => match self.pictures.get(path) {
                Some(Picture::Ready(image)) => img(image.clone())
                    .max_w_full()
                    .max_h(px(480.))
                    .rounded(px(12.))
                    .into_any_element(),
                Some(Picture::Failed) => Self::placeholder(alt, "Couldn't load the image"),
                Some(Picture::Loading) | None => Self::placeholder(alt, "Loading…"),
            },
            Block::Properties(items) => Self::properties(items),
            Block::Progress { value, label } => Self::progress(*value, label.as_deref(), key),
            Block::Notice {
                tone,
                title,
                text,
                actions,
            } => self.notice(*tone, title.as_deref(), text, actions, key, cx),
            Block::List { items, empty } => self.list(items, empty.as_ref(), key, level, cx),
            Block::Table { columns, rows, empty } => {
                self.table(columns, rows, empty.as_ref(), key, level, cx)
            }
            Block::Breadcrumbs(crumbs) => Self::breadcrumbs(crumbs, key, cx),
            Block::Actions(buttons) => self.buttons(buttons, key, None, cx).into_any_element(),
            Block::Form(form) => self.form(form, key, cx),
            Block::Canvas(canvas) => self.canvases.get(&canvas.id).map_or_else(
                || div().into_any_element(),
                |(view, _)| view.clone().into_any_element(),
            ),
            Block::WebPanel(panel) => self.panels.get(&panel.id).map_or_else(
                || div().into_any_element(),
                |(view, _)| view.clone().into_any_element(),
            ),
        }
    }

    pub(super) fn button(
        &self,
        button: &Button,
        id: SharedString,
        form: Option<&str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (fill, ink, edge) = match button.style {
            ButtonStyle::Primary => (Some(Theme::accent()), Theme::on_accent(), None),
            ButtonStyle::Secondary => (None, Theme::ink(), Some(Theme::line())),
            ButtonStyle::Danger => (Some(Theme::danger_container()), Theme::danger(), None),
        };
        let off = button.disabled || self.busy;
        let pressed = button.clone();
        let form = form.map(str::to_owned);
        div()
            .id(id)
            .h(px(34.))
            .flex_none()
            .px_4()
            .rounded(px(17.))
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(ink)
            .when_some(fill, Styled::bg)
            .when_some(edge, |button, edge| button.border_1().border_color(edge))
            .map(|element| {
                if off {
                    element.opacity(0.5)
                } else {
                    element
                        .cursor_pointer()
                        .hover(move |element| {
                            if fill.is_some() {
                                element.opacity(0.88)
                            } else {
                                element.bg(Theme::raised())
                            }
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.press(&pressed, form.as_deref(), cx);
                        }))
                }
            })
            .children(button.icon.as_deref().map(|icon| symbol(Some(icon), 16., ink)))
            .child(button.label.clone())
            .into_any_element()
    }

    fn buttons(
        &self,
        buttons: &[Button],
        key: &str,
        form: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        div().flex().flex_wrap().items_center().gap_2().children(
            buttons
                .iter()
                .enumerate()
                .map(|(index, button)| self.button(button, format!("{key}-button-{index}").into(), form, cx)),
        )
    }

    /// The ⋮ that opens a row's menu.
    fn menu_button(id: SharedString, menu: &[Button], cx: &mut Context<Self>) -> AnyElement {
        let buttons = menu.to_vec();
        div()
            .id(id)
            .size(px(30.))
            .flex_none()
            .rounded(px(15.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.open_menu(event.position(), buttons.clone(), cx);
            }))
            .child(symbol(Some("more_vert"), 18., Theme::ink_dim()))
            .into_any_element()
    }
}
