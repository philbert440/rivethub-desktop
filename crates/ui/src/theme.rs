use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Rgba, rgb, rgba};

static LIGHT: AtomicBool = AtomicBool::new(false);

fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

fn pick(dark: u32, light_tone: u32) -> Rgba {
    rgb(if light() { light_tone } else { dark })
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
        pick(0x000f_1512, 0x00e9_f0eb)
    }
    pub fn panel() -> Rgba {
        pick(0x0014_1a17, 0x00f2_f7f3)
    }
    pub fn surface() -> Rgba {
        pick(0x0018_1e1b, 0x00fb_fdfb)
    }
    pub fn ink() -> Rgba {
        pick(0x00de_e4df, 0x0017_1d1a)
    }
    pub fn ink_dim() -> Rgba {
        pick(0x00a7_b2ab, 0x0056_615b)
    }
    pub fn accent() -> Rgba {
        pick(0x006c_dbac, 0x0000_6c4c)
    }
    pub fn card() -> Rgba {
        pick(0x0023_2a26, 0x00e4_ebe6)
    }
    pub fn raised() -> Rgba {
        pick(0x002b_332e, 0x00db_e4de)
    }
    pub fn warn() -> Rgba {
        pick(0x00f5_c56b, 0x008a_5100)
    }
    pub fn warn_container() -> Rgba {
        pick(0x002a_2618, 0x00fb_efd6)
    }
    pub fn warn_line() -> Rgba {
        pick(0x005c_4d24, 0x00e3_c78c)
    }
    pub fn code() -> Rgba {
        pick(0x0011_1714, 0x00ed_f2ee)
    }
    pub fn line() -> Rgba {
        pick(0x0035_3d39, 0x00c6_d0c9)
    }
    pub fn selected() -> Rgba {
        pick(0x001f_4535, 0x00c4_ecd8)
    }
    pub fn on_selected() -> Rgba {
        pick(0x00a6_f2cd, 0x0000_2114)
    }
    pub fn removed() -> Rgba {
        pick(0x00ff_8a80, 0x00ba_1a1a)
    }
    pub fn added_bg() -> Rgba {
        pick(0x0016_3326, 0x00d3_efe0)
    }
    pub fn removed_bg() -> Rgba {
        pick(0x003a_1f1f, 0x00fa_dcd8)
    }
    pub fn on_accent() -> Rgba {
        pick(0x0000_3825, 0x00ff_ffff)
    }
    /// Dims the workspace behind a modal.
    pub fn scrim() -> Rgba {
        if light() {
            rgba(0x0f28_1c59)
        } else {
            rgba(0x0509_07b3)
        }
    }

    /// Switches every colour above to the light or the dark theme; windows redraw to show it.
    pub fn set_light(light: bool) {
        LIGHT.store(light, Ordering::Relaxed);
    }
}

/// Whether the desktop is using a light colour scheme.
pub fn desktop_is_light(window: &gpui::Window) -> bool {
    matches!(
        window.appearance(),
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
    )
}

/// Shows the theme `appearance` asks for in `window`'s light or dark desktop.
pub fn follow(appearance: model::prefs::Appearance, window: &gpui::Window) {
    Theme::set_light(appearance.is_light(desktop_is_light(window)));
}
