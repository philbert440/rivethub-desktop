use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Rgba, SharedString, Styled, Window, div, img, prelude::*, px, relative,
};
use model::plugin::view::{
    self, Align, Block, Button, ButtonStyle, Column, Empty, Inline, ListItem, Property, Row, Tab, TimeStyle,
    Tone,
};

use super::frame::{Frame, Picture, Placement, Pressed, child_key};
use crate::text::{ago_phrase, bytes, clock, duration, now_ms};
use crate::theme::Theme;
use crate::{brand, markdown_view};

/// The narrowest a page is for a split to sit side by side.
const SIDE_BY_SIDE: f32 = 1100.;

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
            Block::WebPanel => Self::web_panel(level),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a split's two sides and where it is drawn"
    )]
    fn split(
        &self,
        primary: &[Block],
        secondary: &[Block],
        ratio: Option<f32>,
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let wide = self.placement == Placement::Page && window.viewport_size().width >= px(SIDE_BY_SIDE);
        let primary = self.stack(primary, &format!("{key}p"), level, window, cx);
        let secondary = self.stack(secondary, &format!("{key}s"), level, window, cx);
        if wide {
            div()
                .flex()
                .items_start()
                .gap_4()
                .child(
                    primary
                        .w(relative(ratio.unwrap_or(0.5).clamp(0.2, 0.8)))
                        .flex_none(),
                )
                .child(secondary.flex_1())
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(primary)
                .child(secondary)
                .into_any_element()
        }
    }

    fn markdown(text: &str, key: &str, window: &Window) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_sm()
            .children(markdown_view::blocks(
                &model::markdown::parse(text),
                key,
                Theme::ink().into(),
                window,
            ))
            .into_any_element()
    }

    fn code(&self, text: &str, key: &str) -> AnyElement {
        let scroll = self.logs.get(key).cloned().unwrap_or_default();
        div()
            .id(SharedString::from(format!("code-{key}")))
            .max_h(px(360.))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .p_3()
            .rounded(px(12.))
            .bg(Theme::code())
            .font_family(Theme::MONO)
            .text_xs()
            .text_color(Theme::ink())
            .child(text.trim_end().to_owned())
            .into_any_element()
    }

    fn web_panel(level: u8) -> AnyElement {
        div()
            .p_4()
            .rounded(px(16.))
            .bg(container(level))
            .flex()
            .items_center()
            .gap_3()
            .text_sm()
            .text_color(Theme::ink_dim())
            .child(symbol(Some("language"), 18., Theme::ink_dim()))
            .child("This part of the plug-in is a web panel, which RivetHub desktop doesn't show yet.")
            .into_any_element()
    }

    fn placeholder(alt: &str, state: &str) -> AnyElement {
        div()
            .h(px(160.))
            .rounded(px(12.))
            .bg(Theme::code())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .text_xs()
            .text_color(Theme::ink_dim())
            .child(symbol(Some("image"), 24., Theme::ink_dim()))
            .child(alt.to_owned())
            .child(state.to_owned())
            .into_any_element()
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

    #[expect(
        clippy::too_many_arguments,
        reason = "a section's parts, as the contract lists them"
    )]
    fn section(
        &self,
        (title, subtitle): (Option<&str>, Option<&str>),
        actions: &[Button],
        blocks: &[Block],
        collapsed: Option<bool>,
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let folded = collapsed.map(|start| self.folded.get(key).copied().unwrap_or(start));
        let owned = key.to_owned();
        let header = div()
            .id(SharedString::from(format!("section-{key}")))
            .flex()
            .items_center()
            .gap_3()
            .when_some(folded, |header, folded| {
                header
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.folded.insert(owned.clone(), !folded);
                        cx.notify();
                    }))
                    .child(symbol(
                        Some(if folded { "chevron_right" } else { "expand_more" }),
                        18.,
                        Theme::ink_dim(),
                    ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(title.map(|title| {
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(Theme::ink())
                            .child(title.to_owned())
                    }))
                    .children(subtitle.map(|subtitle| {
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(subtitle.to_owned())
                    })),
            )
            .child(self.buttons(actions, key, None, cx));
        let open = folded != Some(true);
        div()
            .p_4()
            .rounded(px(20.))
            .bg(container(level))
            .flex()
            .flex_col()
            .gap_3()
            .when(
                title.is_some() || subtitle.is_some() || !actions.is_empty() || folded.is_some(),
                |section| section.child(header),
            )
            .when(open, |section| {
                section.child(self.stack(blocks, key, level + 1, window, cx))
            })
            .into_any_element()
    }

    fn tabs(
        &self,
        tabs: &[Tab],
        selected: Option<&str>,
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let set = tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let current = self
            .tabs
            .get(&set)
            .map(String::as_str)
            .or(selected)
            .filter(|id| tabs.iter().any(|tab| tab.id == *id))
            .or_else(|| tabs.first().map(|tab| tab.id.as_str()))
            .unwrap_or_default()
            .to_owned();
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(px(18.))
            .bg(Theme::panel())
            .children(tabs.iter().enumerate().map(|(index, tab)| {
                let here = tab.id == current;
                let (set, id) = (set.clone(), tab.id.clone());
                div()
                    .id(SharedString::from(format!("{key}-tab-{index}")))
                    .h(px(30.))
                    .px_4()
                    .rounded(px(15.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .map(|chip| {
                        if here {
                            chip.bg(Theme::selected()).text_color(Theme::on_selected())
                        } else {
                            chip.text_color(Theme::ink_dim())
                                .hover(|chip| chip.bg(Theme::card()))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.tabs.insert(set.clone(), id.clone());
                        cx.notify();
                    }))
                    .child(tab.title.clone())
                    .children(tab.badge.clone().map(|badge| {
                        div()
                            .px_2()
                            .rounded(px(9.))
                            .bg(Theme::raised())
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(badge)
                    }))
            }));
        let body = tabs
            .iter()
            .find(|tab| tab.id == current)
            .map(|tab| self.stack(&tab.blocks, &format!("{key}:{}", tab.id), level, window, cx));
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().flex().child(bar))
            .children(body)
            .into_any_element()
    }

    fn properties(items: &[Property]) -> AnyElement {
        let now = now_ms();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(items.iter().map(|item| {
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_sm()
                    .child(
                        div()
                            .w(px(140.))
                            .flex_none()
                            .text_color(Theme::ink_dim())
                            .child(item.label.clone()),
                    )
                    .child(div().flex().min_w_0().child(inline(&item.value, now)))
            }))
            .into_any_element()
    }

    fn progress(value: Option<f32>, label: Option<&str>, key: &str) -> AnyElement {
        let track = div()
            .relative()
            .h(px(6.))
            .w_full()
            .rounded_full()
            .bg(Theme::line())
            .overflow_hidden();
        let track = match value {
            Some(value) => track
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(Theme::accent())
                        .w(relative(value.clamp(0., 1.))),
                )
                .into_any_element(),
            None => track
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .h_full()
                        .w(relative(0.3))
                        .rounded_full()
                        .bg(Theme::accent())
                        .with_animation(
                            SharedString::from(format!("progress-{key}")),
                            Animation::new(Duration::from_millis(1400)).repeat(),
                            |bar, moved| bar.left(relative(moved * 1.3 - 0.3)),
                        ),
                )
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(track)
            .children(label.map(|label| {
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(label.to_owned())
            }))
            .into_any_element()
    }

    fn notice(
        &self,
        tone: Tone,
        title: Option<&str>,
        text: &str,
        actions: &[Button],
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (ink, fill) = Theme::tone(tone);
        let icon = match tone {
            Tone::Neutral | Tone::Info => "info",
            Tone::Success => "check_circle",
            Tone::Warning => "warning",
            Tone::Danger => "error",
        };
        div()
            .p_4()
            .rounded(px(16.))
            .bg(fill)
            .flex()
            .gap_3()
            .child(symbol(Some(icon), 20., ink))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(title.map(|title| {
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(ink)
                            .child(title.to_owned())
                    }))
                    .child(div().text_sm().text_color(Theme::ink()).child(text.to_owned()))
                    .when(!actions.is_empty(), |body| {
                        body.child(self.buttons(actions, key, None, cx).pt_2())
                    }),
            )
            .into_any_element()
    }

    fn empty(&self, empty: Option<&Empty>, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(empty) = empty else {
            return div()
                .p_6()
                .text_sm()
                .text_color(Theme::ink_dim())
                .child("Nothing here yet.")
                .into_any_element();
        };
        div()
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(Theme::ink())
                    .child(empty.title.clone()),
            )
            .children(
                empty
                    .text
                    .clone()
                    .map(|text| div().text_xs().text_color(Theme::ink_dim()).child(text)),
            )
            .children(
                empty
                    .button
                    .as_ref()
                    .map(|button| self.button(button, format!("{key}-empty").into(), None, cx)),
            )
            .into_any_element()
    }

    /// A row that runs `action` when clicked and opens `menu` on a right-click.
    fn row(
        id: SharedString,
        action: Option<&view::Action>,
        menu: &[Button],
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let pressed = action.cloned();
        let buttons = menu.to_vec();
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .when_some(pressed, |row, action| {
                row.cursor_pointer()
                    .hover(|row| row.bg(Theme::raised()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let pressed = Pressed {
                            action: action.clone(),
                            form: None,
                        };
                        this.run_pressed(pressed, cx);
                    }))
            })
            .when(!buttons.is_empty(), |row| {
                row.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.open_menu(event.position, buttons.clone(), cx);
                    }),
                )
            })
    }

    fn list(
        &self,
        items: &[ListItem],
        empty: Option<&Empty>,
        key: &str,
        level: u8,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = div().rounded(px(20.)).bg(container(level)).overflow_hidden();
        if items.is_empty() {
            return frame.child(self.empty(empty, key, cx)).into_any_element();
        }
        let now = now_ms();
        frame
            .flex()
            .flex_col()
            .children(items.iter().enumerate().map(|(index, item)| {
                Self::row(
                    format!("{key}-item-{index}").into(),
                    item.action.as_ref(),
                    &item.menu,
                    cx,
                )
                .min_h(px(56.))
                .py_2()
                .when(index > 0, |row| row.border_t_1().border_color(Theme::line()))
                .children(
                    item.icon
                        .as_deref()
                        .map(|icon| symbol(Some(icon), 20., Theme::ink_dim())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .text_color(Theme::ink())
                                .truncate()
                                .child(item.title.clone()),
                        )
                        .children(item.subtitle.clone().map(|subtitle| {
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .truncate()
                                .child(subtitle)
                        })),
                )
                .children(item.trailing.iter().map(|trailing| inline(trailing, now)))
                .when(!item.menu.is_empty(), |row| {
                    row.child(Self::menu_button(
                        format!("{key}-item-menu-{index}").into(),
                        &item.menu,
                        cx,
                    ))
                })
            }))
            .into_any_element()
    }

    fn table(
        &self,
        columns: &[Column],
        rows: &[Row],
        empty: Option<&Empty>,
        key: &str,
        level: u8,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = div().rounded(px(20.)).bg(container(level)).overflow_hidden();
        let sort = self.sorts.get(key).copied();
        let menus = rows.iter().any(|row| !row.menu.is_empty());
        let cell = |align: Align| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .map(|cell| match align {
                    Align::Start => cell,
                    Align::Center => cell.justify_center(),
                    Align::End => cell.justify_end(),
                })
        };
        let header = div()
            .h(px(40.))
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(Theme::ink_dim())
            .children(columns.iter().enumerate().map(|(index, column)| {
                let owned = key.to_owned();
                let arrow = match sort {
                    Some((sorted, descending)) if sorted == index => Some(if descending {
                        "arrow_downward"
                    } else {
                        "arrow_upward"
                    }),
                    _ => None,
                };
                cell(column.align)
                    .id(SharedString::from(format!("{key}-column-{index}")))
                    .gap_1()
                    .when(column.sortable, |cell| {
                        cell.cursor_pointer()
                            .hover(|cell| cell.text_color(Theme::ink()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let descending =
                                    matches!(this.sorts.get(&owned), Some((at, false)) if *at == index);
                                this.sorts.insert(owned.clone(), (index, descending));
                                cx.notify();
                            }))
                    })
                    .child(column.title.clone())
                    .children(arrow.map(|arrow| symbol(Some(arrow), 14., Theme::ink_dim())))
            }))
            .when(menus, |header| header.child(div().w(px(30.)).flex_none()));
        if rows.is_empty() {
            return frame
                .child(header)
                .child(self.empty(empty, key, cx))
                .into_any_element();
        }
        let order = match sort {
            Some((column, descending)) => view::sorted(rows, column, descending),
            None => (0..rows.len()).collect(),
        };
        let now = now_ms();
        frame
            .flex()
            .flex_col()
            .child(header)
            .children(
                order
                    .into_iter()
                    .filter_map(|index| rows.get(index).map(|row| (index, row)))
                    .map(|(index, row)| {
                        Self::row(
                            format!("{key}-row-{index}").into(),
                            row.action.as_ref(),
                            &row.menu,
                            cx,
                        )
                        .h(px(44.))
                        .border_t_1()
                        .border_color(Theme::line())
                        .text_sm()
                        .children(columns.iter().enumerate().map(|(at, column)| {
                            cell(column.align).children(row.cells.get(at).map(|value| inline(value, now)))
                        }))
                        .when(menus, |line| {
                            line.child(if row.menu.is_empty() {
                                div().w(px(30.)).flex_none().into_any_element()
                            } else {
                                Self::menu_button(format!("{key}-row-menu-{index}").into(), &row.menu, cx)
                            })
                        })
                    }),
            )
            .into_any_element()
    }

    fn breadcrumbs(crumbs: &[view::Crumb], key: &str, cx: &mut Context<Self>) -> AnyElement {
        let last = crumbs.len().saturating_sub(1);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .text_sm()
            .children(crumbs.iter().enumerate().map(|(index, crumb)| {
                let label = div()
                    .id(SharedString::from(format!("{key}-crumb-{index}")))
                    .px_1()
                    .rounded(px(6.))
                    .text_color(if index == last {
                        Theme::ink()
                    } else {
                        Theme::ink_dim()
                    })
                    .when_some(crumb.action.clone(), |label, action| {
                        label
                            .cursor_pointer()
                            .hover(|label| label.text_color(Theme::accent()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let pressed = Pressed {
                                    action: action.clone(),
                                    form: None,
                                };
                                this.run_pressed(pressed, cx);
                            }))
                    })
                    .child(crumb.label.clone());
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(index > 0, |crumb| {
                        crumb.child(symbol(Some("chevron_right"), 16., Theme::ink_dim()))
                    })
                    .child(label)
            }))
            .into_any_element()
    }

    fn form(&self, form: &view::Form, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.forms.get(&form.id) else {
            return div().into_any_element();
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(state.view.clone())
            .children(
                state
                    .refusal
                    .clone()
                    .map(|refusal| div().text_sm().text_color(Theme::danger()).child(refusal)),
            )
            .when(!form.buttons.is_empty(), |body| {
                body.child(self.buttons(&form.buttons, key, Some(&form.id), cx).justify_end())
            })
            .into_any_element()
    }
}
