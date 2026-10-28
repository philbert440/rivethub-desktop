use app::Hub;
use gpui::{
    Context, Div, Entity, Focusable, IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    img, prelude::*, px,
};
use model::Page;

use crate::memory::MemoryPage;
use crate::settings::SettingsPage;
use crate::workspace::Workspace;
use crate::{brand, theme::Theme};

const OPEN_WIDTH: f32 = 208.;
const RAIL_WIDTH: f32 = 56.;

/// The window's root: the left sidebar and whichever page it has selected.
pub struct Shell {
    page: Page,
    sidebar_open: bool,
    workspace: Entity<Workspace>,
    memory: Entity<MemoryPage>,
    settings: Entity<SettingsPage>,
}

impl Shell {
    pub fn new(hub: Hub, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = cx.new(|cx| Workspace::new(hub, window, cx));
        window.focus(&workspace.focus_handle(cx), cx);
        Self {
            page: Page::Workspace,
            sidebar_open: true,
            workspace,
            memory: cx.new(|_| MemoryPage::default()),
            settings: cx.new(|_| SettingsPage),
        }
    }

    fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page == self.page {
            return;
        }
        self.page = page;
        if page == Page::Workspace {
            window.focus(&self.workspace.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn nav_item(
        &self,
        page: Page,
        icon: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let here = self.page == page;
        let ink = if here {
            Theme::on_selected()
        } else {
            Theme::ink_dim()
        };
        div()
            .id(label)
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(22.))
            .cursor_pointer()
            .text_color(ink)
            .map(|item| {
                if self.sidebar_open {
                    item.px(px(12.))
                } else {
                    item.w(px(44.)).justify_center()
                }
            })
            .map(|item| {
                if here {
                    item.bg(Theme::selected())
                } else {
                    item.hover(|item| item.bg(Theme::card()))
                }
            })
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.show(page, window, cx);
            }))
            .child(brand::icon(icon, 20., ink))
            .when(self.sidebar_open, |item| item.child(label))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.sidebar_open;
        let toggle = div()
            .id("toggle-sidebar")
            .size(px(36.))
            .flex_none()
            .rounded(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::card()))
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .child(brand::icon(
                if open {
                    "icons/sidebar-collapse.svg"
                } else {
                    "icons/sidebar-expand.svg"
                },
                18.,
                Theme::ink_dim(),
            ));
        let mark = div()
            .flex()
            .items_center()
            .gap_2()
            .child(img(brand::rh()).size(px(32.)))
            .when(open, |mark| {
                mark.child(div().text_base().text_color(Theme::ink()).child("RivetHub"))
            });
        div()
            .w(px(if open { OPEN_WIDTH } else { RAIL_WIDTH }))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .pt_3()
            .pb_3()
            .bg(Theme::window())
            .map(|bar| if open { bar.px_3() } else { bar.items_center() })
            .child(if open {
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb_3()
                    .child(mark)
                    .child(toggle)
            } else {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .pb_3()
                    .child(mark)
                    .child(toggle)
            })
            .child(self.nav_item(Page::Workspace, "icons/workspace.svg", "Workspace", cx))
            .child(self.nav_item(Page::Memory, "icons/memory.svg", "Memory", cx))
            .child(div().flex_1())
            .child(self.nav_item(Page::Settings, "icons/settings.svg", "Settings", cx))
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match self.page {
            Page::Workspace => self.workspace.clone().into_any_element(),
            Page::Memory => self.memory.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .bg(Theme::window())
            .font_family(Theme::SANS)
            .text_color(Theme::ink())
            .text_sm()
            .child(self.sidebar(cx))
            .child(div().flex_1().min_w_0().h_full().child(page))
    }
}

/// The bar across the top of every page.
pub fn header() -> Div {
    div().h(px(52.)).flex_none().flex().items_center().gap_4().px_5()
}

pub fn title(text: impl Into<SharedString>) -> Div {
    div().text_lg().text_color(Theme::ink()).child(text.into())
}

/// The rounded surface each page draws its content on.
pub fn tile() -> Div {
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .mr_3()
        .mb_3()
        .rounded(px(28.))
        .bg(Theme::surface())
        .overflow_hidden()
}
