use gpui::{ClickEvent, Context, Div, IntoElement, ParentElement, Window, prelude::*};
use model::prefs::Appearance;

use super::SettingsPage;
use super::parts::{labelled, row, section, segment, segments};
use crate::appearance;
use crate::theme::desktop_is_light;

impl SettingsPage {
    pub(super) fn appearance(&self, window: &Window, cx: &mut Context<Self>) -> Vec<Div> {
        let chosen = self.hub.preferences().appearance;
        let omarchy = appearance::omarchy_theme(cx).map(|theme| theme.name.clone());
        let detail = match (chosen, &omarchy) {
            (Appearance::Dark, _) => "The dark Material theme, the app's own.".to_owned(),
            (Appearance::Light, _) => "The light Material theme.".to_owned(),
            (Appearance::System, _) if desktop_is_light(window) => {
                "Following the desktop, which is light now.".to_owned()
            }
            (Appearance::System, _) => "Following the desktop, which is dark now.".to_owned(),
            (Appearance::Omarchy, Some(Some(name))) => {
                format!("Following Omarchy's {name} theme. Switch themes in Omarchy and RivetHub follows.")
            }
            (Appearance::Omarchy, Some(None)) => {
                "Following the Omarchy theme. Switch themes in Omarchy and RivetHub follows.".to_owned()
            }
            (Appearance::Omarchy, None) => {
                "No Omarchy theme is set up here, so the dark theme shows.".to_owned()
            }
        };
        // Omarchy is offered where there's a theme to follow, or while it's still the choice.
        let offered = Appearance::ALL.into_iter().filter(|option| {
            *option != Appearance::Omarchy || omarchy.is_some() || chosen == Appearance::Omarchy
        });
        let options = segments().children(offered.map(|option| {
            segment(label(option), label(option), option == chosen).on_click(cx.listener(
                move |this, _: &ClickEvent, window, cx| {
                    let mut prefs = this.hub.preferences();
                    prefs.appearance = option;
                    this.hub.set_preferences(prefs);
                    if option == Appearance::Omarchy {
                        appearance::read_omarchy(cx);
                    }
                    appearance::apply(&this.hub, window, cx);
                    cx.notify();
                },
            ))
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
        Appearance::Omarchy => "Omarchy",
    }
}
