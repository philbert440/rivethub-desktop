use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, prelude::*, px,
};
use model::plugin::view::{self, Align, Button, Column, Empty, ListItem, Row};

use super::{container, inline, symbol};
use crate::plugin::frame::{Frame, Pressed};
use crate::text::now_ms;
use crate::theme::Theme;

impl Frame {
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

    pub(super) fn list(
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

    pub(super) fn table(
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
}
