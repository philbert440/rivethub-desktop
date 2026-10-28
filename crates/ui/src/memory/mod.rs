use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div, prelude::*, px};

use crate::shell;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Ask,
    Topics,
    Tidy,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Ask, Self::Topics, Self::Tidy];

    fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Topics => "Topics",
            Self::Tidy => "To tidy",
        }
    }
}

/// The Memory page: ask the mesh's memory, read each topic's wiki page, and tidy what needs it.
#[derive(Default)]
pub struct MemoryPage {
    tab: Tab,
}

impl MemoryPage {
    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(px(18.))
            .bg(Theme::panel())
            .children(Tab::ALL.map(|tab| {
                let here = tab == self.tab;
                div()
                    .id(tab.label())
                    .h(px(30.))
                    .px_4()
                    .rounded(px(15.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .map(|chip| {
                        if here {
                            chip.bg(Theme::selected()).text_color(Theme::on_selected())
                        } else {
                            chip.text_color(Theme::ink_dim())
                                .hover(|chip| chip.bg(Theme::card()))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
                    .child(tab.label())
            }))
    }
}

impl Render for MemoryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(shell::header().child(shell::title("Memory")).child(self.tabs(cx)))
            .child(shell::tile())
    }
}
