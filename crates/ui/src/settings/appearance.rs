use gpui::{ClickEvent, Context, Div, IntoElement, ParentElement, Styled, Window, div, prelude::*, px};
use model::prefs::Appearance;

use super::SettingsPage;
use super::parts::{labelled, row, section};
use crate::theme::{self, Theme};

impl SettingsPage {
    pub(super) fn appearance(&self, window: &Window, cx: &mut Context<Self>) -> Vec<Div> {
        let chosen = self.hub.preferences().appearance;
        let detail = match chosen {
            Appearance::Dark => "The dark Material theme, the app's own.",
            Appearance::Light => "The light Material theme.",
            Appearance::System if theme::desktop_is_light(window) => {
                "Following the desktop, which is light now."
            }
            Appearance::System => "Following the desktop, which is dark now.",
        };
        let options = div()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(px(18.))
            .bg(Theme::panel())
            .children(Appearance::ALL.map(|option| {
                let here = option == chosen;
                div()
                    .id(label(option))
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
                                .hover(|chip| chip.bg(Theme::raised()))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        let mut prefs = this.hub.preferences();
                        prefs.appearance = option;
                        this.hub.set_preferences(prefs);
                        theme::follow(option, window);
                        window.refresh();
                        cx.notify();
                    }))
                    .child(label(option))
            }));
        vec![section(
            "Theme",
            vec![
                row()
                    .child(labelled("Colours", detail))
                    .child(options)
                    .into_any_element(),
            ],
        )]
    }
}

fn label(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Dark => "Dark",
        Appearance::Light => "Light",
        Appearance::System => "Match desktop",
    }
}
