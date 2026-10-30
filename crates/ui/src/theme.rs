use gpui::{Rgba, rgb, rgba};
use model::palette::{DARK, Palette};
use parking_lot::RwLock;

/// The palette every window draws with now.
static PALETTE: RwLock<Palette> = RwLock::new(DARK);

fn pick(tone: fn(&Palette) -> u32) -> Rgba {
    rgb(tone(&PALETTE.read()))
}

pub struct Theme;

impl Theme {
    pub const MONO: &str = "JetBrains Mono";
    pub const SANS: &str = "Roboto Flex";
    /// The colours an agent can be given.
    pub const AGENT_COLORS: [u32; 6] = [
        0x006c_dbac,
        0x008e_cbff,
        0x00f5_c56b,
        0x00ff_b4ab,
        0x00c9_b2ff,
        0x00a7_b2ab,
    ];

    pub fn window() -> Rgba {
        pick(|palette| palette.window)
    }
    pub fn panel() -> Rgba {
        pick(|palette| palette.panel)
    }
    pub fn surface() -> Rgba {
        pick(|palette| palette.surface)
    }
    pub fn ink() -> Rgba {
        pick(|palette| palette.ink)
    }
    pub fn ink_dim() -> Rgba {
        pick(|palette| palette.ink_dim)
    }
    pub fn accent() -> Rgba {
        pick(|palette| palette.accent)
    }
    pub fn card() -> Rgba {
        pick(|palette| palette.card)
    }
    pub fn raised() -> Rgba {
        pick(|palette| palette.raised)
    }
    pub fn warn() -> Rgba {
        pick(|palette| palette.warn)
    }
    pub fn warn_container() -> Rgba {
        pick(|palette| palette.warn_container)
    }
    pub fn warn_line() -> Rgba {
        pick(|palette| palette.warn_line)
    }
    pub fn code() -> Rgba {
        pick(|palette| palette.code)
    }
    pub fn line() -> Rgba {
        pick(|palette| palette.line)
    }
    pub fn selected() -> Rgba {
        pick(|palette| palette.selected)
    }
    pub fn on_selected() -> Rgba {
        pick(|palette| palette.on_selected)
    }
    pub fn removed() -> Rgba {
        pick(|palette| palette.removed)
    }
    pub fn added_bg() -> Rgba {
        pick(|palette| palette.added_bg)
    }
    pub fn removed_bg() -> Rgba {
        pick(|palette| palette.removed_bg)
    }
    pub fn on_accent() -> Rgba {
        pick(|palette| palette.on_accent)
    }
    /// Dims the workspace behind a modal.
    pub fn scrim() -> Rgba {
        rgba(PALETTE.read().scrim)
    }

    /// Switches every colour above to `palette`; windows redraw to show it.
    pub fn set(palette: Palette) {
        *PALETTE.write() = palette;
    }
}

/// Whether the desktop is using a light colour scheme.
pub fn desktop_is_light(window: &gpui::Window) -> bool {
    matches!(
        window.appearance(),
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
    )
}
