use gpui::{Rgba, rgb, rgba};
use model::plugin::canvas::Role;
use model::plugin::view::Tone;

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
        rgb(0x000f_1512)
    }
    pub fn panel() -> Rgba {
        rgb(0x0014_1a17)
    }
    pub fn surface() -> Rgba {
        rgb(0x0018_1e1b)
    }
    pub fn ink() -> Rgba {
        rgb(0x00de_e4df)
    }
    pub fn ink_dim() -> Rgba {
        rgb(0x00a7_b2ab)
    }
    pub fn accent() -> Rgba {
        rgb(0x006c_dbac)
    }
    pub fn card() -> Rgba {
        rgb(0x0023_2a26)
    }
    pub fn raised() -> Rgba {
        rgb(0x002b_332e)
    }
    pub fn warn() -> Rgba {
        rgb(0x00f5_c56b)
    }
    pub fn warn_container() -> Rgba {
        rgb(0x002a_2618)
    }
    pub fn warn_line() -> Rgba {
        rgb(0x005c_4d24)
    }
    pub fn code() -> Rgba {
        rgb(0x0011_1714)
    }
    pub fn line() -> Rgba {
        rgb(0x0035_3d39)
    }
    pub fn selected() -> Rgba {
        rgb(0x001f_4535)
    }
    pub fn on_selected() -> Rgba {
        rgb(0x00a6_f2cd)
    }
    pub fn removed() -> Rgba {
        rgb(0x00ff_8a80)
    }
    pub fn added_bg() -> Rgba {
        rgb(0x0016_3326)
    }
    pub fn removed_bg() -> Rgba {
        rgb(0x003a_1f1f)
    }
    pub fn on_accent() -> Rgba {
        rgb(0x0000_3825)
    }
    /// Dims the workspace behind a modal.
    pub fn scrim() -> Rgba {
        rgba(0x0509_07b3)
    }
    pub fn outline() -> Rgba {
        rgb(0x0085_908a)
    }
    pub fn info() -> Rgba {
        rgb(0x008e_cbff)
    }
    pub fn info_container() -> Rgba {
        rgb(0x0016_2a38)
    }
    pub fn danger() -> Rgba {
        rgb(0x00ff_b4ab)
    }
    pub fn danger_container() -> Rgba {
        rgb(0x003a_1f1f)
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
            Role::SecondaryContainer => rgb(0x0034_4b41),
            Role::OnSurface => Self::ink(),
            Role::OnSurfaceVariant => Self::ink_dim(),
            Role::Outline => Self::outline(),
            Role::OutlineVariant => Self::line(),
            Role::Info | Role::Category1 => Self::info(),
            Role::Warning | Role::Category3 => Self::warn(),
            Role::Danger | Role::Category4 => Self::danger(),
            Role::Category2 => rgb(0x00c9_b2ff),
            Role::Category5 => rgb(0x0099_d9c4),
            Role::Category6 => rgb(0x00e8_b4d8),
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
