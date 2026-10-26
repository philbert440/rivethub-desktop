use gpui::{Rgba, rgb};

pub struct Theme;

impl Theme {
    pub fn window() -> Rgba {
        rgb(0x000f_1512)
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
    pub fn on_accent() -> Rgba {
        rgb(0x0000_3825)
    }
}
