use gpui::{Rgba, rgb, rgba};
use model::palette::{DARK, Palette};
use model::plugin::canvas::Role;
use model::plugin::view::Tone;
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

    pub fn outline() -> Rgba {
        pick(|palette| palette.outline)
    }
    pub fn info() -> Rgba {
        pick(|palette| palette.info)
    }
    pub fn info_container() -> Rgba {
        pick(|palette| palette.info_container)
    }
    pub fn danger() -> Rgba {
        pick(|palette| palette.danger)
    }
    pub fn danger_container() -> Rgba {
        pick(|palette| palette.danger_container)
    }

    /// Switches every colour above to `palette`; windows redraw to show it.
    pub fn set(palette: Palette) {
        *PALETTE.write() = palette;
    }

    /// A plug-in's colour role in this theme.
    pub fn role(role: Role) -> Rgba {
        match role {
            Role::Surface => Self::surface(),
            Role::SurfaceContainer => Self::card(),
            Role::SurfaceContainerHigh => Self::raised(),
            Role::Primary | Role::Success => Self::accent(),
            Role::PrimaryContainer => Self::selected(),
            Role::OnPrimary => Self::on_accent(),
            Role::OnPrimaryContainer => Self::on_selected(),
            Role::SecondaryContainer => pick(|palette| palette.secondary_container),
            Role::OnSurface => Self::ink(),
            Role::OnSurfaceVariant => Self::ink_dim(),
            Role::Outline => Self::outline(),
            Role::OutlineVariant => Self::line(),
            Role::Info | Role::Category1 => Self::info(),
            Role::Warning | Role::Category3 => Self::warn(),
            Role::Danger | Role::Category4 => Self::danger(),
            Role::Category2 => pick(|palette| palette.categories[0]),
            Role::Category5 => pick(|palette| palette.categories[1]),
            Role::Category6 => pick(|palette| palette.categories[2]),
        }
    }

    /// A tone's ink and the container it sits on.
    pub fn tone(tone: Tone) -> (Rgba, Rgba) {
        match tone {
            Tone::Neutral => (Self::ink_dim(), Self::raised()),
            Tone::Info => (Self::info(), Self::info_container()),
            Tone::Success => (Self::on_selected(), Self::selected()),
            Tone::Warning => (Self::warn(), Self::warn_container()),
            Tone::Danger => (Self::danger(), Self::danger_container()),
        }
    }
}

/// Whether the desktop is using a light colour scheme.
pub fn desktop_is_light(window: &gpui::Window) -> bool {
    matches!(
        window.appearance(),
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
    )
}
