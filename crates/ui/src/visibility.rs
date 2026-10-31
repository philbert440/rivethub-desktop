use model::visibility::{Glyph, Me, Visibility};

use crate::brand;
use crate::theme::Theme;

pub fn glyph(me: Option<&Me>, known: Option<&Visibility>) -> Option<gpui::Svg> {
    let name = match me?.glyph(known)? {
        Glyph::Private => "lock",
        Glyph::Shared => "group",
    };
    Some(brand::icon(brand::symbol(Some(name)), 14., Theme::ink_dim()))
}
