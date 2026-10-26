use gpui::{Rgba, rgb};

pub struct Theme;

impl Theme {
    pub const MONO: &str = "JetBrainsMono Nerd Font";

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
    pub fn on_accent() -> Rgba {
        rgb(0x0000_3825)
    }
}
