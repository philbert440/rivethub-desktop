use std::time::Duration;

use app::Hub;
use app::omarchy::{OmarchyTheme, current_theme};
use gpui::{App, Global, Window};
use model::palette;
use model::prefs::Appearance;

use crate::theme::{Theme, desktop_is_light};

/// How often a followed Omarchy theme is read again, to catch a switch in Omarchy.
pub const WATCH: Duration = Duration::from_secs(2);

/// The active Omarchy theme as last read, or `None` on a desktop without Omarchy.
#[derive(Debug, Default)]
pub struct Omarchy(pub Option<OmarchyTheme>);

impl Global for Omarchy {}

/// Reads the active Omarchy theme again.
pub fn read_omarchy(cx: &mut App) {
    cx.set_global(Omarchy(current_theme()));
}

/// The Omarchy theme to follow, when there is one.
pub fn omarchy_theme(cx: &App) -> Option<&OmarchyTheme> {
    cx.try_global::<Omarchy>().and_then(|omarchy| omarchy.0.as_ref())
}

/// Shows the colours the person chose in `window`: the dark or light theme, the desktop's, or
/// the active Omarchy theme's.
pub fn apply(hub: &Hub, window: &mut Window, cx: &mut App) {
    let omarchy = omarchy_theme(cx).and_then(|theme| palette::from_omarchy(&theme.colors));
    let chosen = hub.preferences().appearance;
    Theme::set(palette::chosen(
        chosen,
        desktop_is_light(window),
        omarchy.as_ref(),
    ));
    window.refresh();
}

/// Whether the person follows the Omarchy theme, so it's worth reading again.
pub fn follows_omarchy(hub: &Hub) -> bool {
    hub.preferences().appearance == Appearance::Omarchy
}
