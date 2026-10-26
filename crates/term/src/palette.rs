use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    const fn hex(value: u32) -> Self {
        let [_, r, g, b] = value.to_be_bytes();
        Self { r, g, b }
    }

    #[must_use]
    pub fn dimmed(self) -> Self {
        let scale = |channel: u8| u8::try_from(u16::from(channel) * 2 / 3).unwrap_or(channel);
        Self {
            r: scale(self.r),
            g: scale(self.g),
            b: scale(self.b),
        }
    }

    pub fn to_u32(self) -> u32 {
        (u32::from(self.r) << 16) | (u32::from(self.g) << 8) | u32::from(self.b)
    }
}

const FOREGROUND: Rgb = Rgb::hex(0x00de_e4df);
const BACKGROUND: Rgb = Rgb::hex(0x000b_100d);
const BASE: [Rgb; 16] = [
    Rgb::hex(0x001c_221f),
    Rgb::hex(0x00ff_8a80),
    Rgb::hex(0x006c_dbac),
    Rgb::hex(0x00f5_c56b),
    Rgb::hex(0x008e_cbff),
    Rgb::hex(0x00c7_a6ff),
    Rgb::hex(0x0080_d8d0),
    Rgb::hex(0x00c9_d1cb),
    Rgb::hex(0x005d_6862),
    Rgb::hex(0x00ff_b4ab),
    Rgb::hex(0x00a6_f2cd),
    Rgb::hex(0x00ff_dd99),
    Rgb::hex(0x00b8_dcff),
    Rgb::hex(0x00e0_ccff),
    Rgb::hex(0x00b0_f0e8),
    Rgb::hex(0x00f4_f8f5),
];

fn indexed(index: u8) -> Rgb {
    match index {
        0..=15 => BASE[usize::from(index)],
        16..=231 => {
            let cube = index - 16;
            let level = |step: u8| if step == 0 { 0 } else { step * 40 + 55 };
            Rgb {
                r: level(cube / 36),
                g: level((cube / 6) % 6),
                b: level(cube % 6),
            }
        }
        _ => {
            let gray = (index - 232) * 10 + 8;
            Rgb {
                r: gray,
                g: gray,
                b: gray,
            }
        }
    }
}

fn named(color: NamedColor, bold: bool) -> Rgb {
    let index = color as usize;
    if index < 8 && bold {
        return BASE[index + 8];
    }
    if index < 16 {
        return BASE[index];
    }
    match color {
        NamedColor::Background => BACKGROUND,
        NamedColor::DimForeground => FOREGROUND.dimmed(),
        NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => BASE[index - NamedColor::DimBlack as usize].dimmed(),
        _ => FOREGROUND,
    }
}

pub fn resolve(color: Color, overrides: &Colors, bold: bool) -> Rgb {
    let from = |rgb: alacritty_terminal::vte::ansi::Rgb| Rgb {
        r: rgb.r,
        g: rgb.g,
        b: rgb.b,
    };
    match color {
        Color::Spec(rgb) => from(rgb),
        Color::Indexed(index) => overrides[usize::from(index)].map_or_else(|| indexed(index), from),
        Color::Named(name) => overrides[name as usize].map_or_else(|| named(name, bold), from),
    }
}

pub fn background(overrides: &Colors) -> Rgb {
    overrides[NamedColor::Background as usize].map_or(BACKGROUND, |rgb| Rgb {
        r: rgb.r,
        g: rgb.g,
        b: rgb.b,
    })
}
