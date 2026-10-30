use crate::prefs::Appearance;

/// Every colour the app draws with, as `0xRRGGBB` (the scrim is `0xRRGGBBAA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub window: u32,
    pub panel: u32,
    pub surface: u32,
    pub ink: u32,
    pub ink_dim: u32,
    pub accent: u32,
    pub card: u32,
    pub raised: u32,
    pub warn: u32,
    pub warn_container: u32,
    pub warn_line: u32,
    pub code: u32,
    pub line: u32,
    pub selected: u32,
    pub on_selected: u32,
    pub removed: u32,
    pub added_bg: u32,
    pub removed_bg: u32,
    pub on_accent: u32,
    pub scrim: u32,
    pub outline: u32,
    pub info: u32,
    pub info_container: u32,
    pub danger: u32,
    pub danger_container: u32,
    pub secondary_container: u32,
    /// The plug-in category colours with no tone of their own: violet, teal and pink.
    pub categories: [u32; 3],
}

/// The app's own Material dark theme.
pub const DARK: Palette = Palette {
    window: 0x000f_1512,
    panel: 0x0014_1a17,
    surface: 0x0018_1e1b,
    ink: 0x00de_e4df,
    ink_dim: 0x00a7_b2ab,
    accent: 0x006c_dbac,
    card: 0x0023_2a26,
    raised: 0x002b_332e,
    warn: 0x00f5_c56b,
    warn_container: 0x002a_2618,
    warn_line: 0x005c_4d24,
    code: 0x0011_1714,
    line: 0x0035_3d39,
    selected: 0x001f_4535,
    on_selected: 0x00a6_f2cd,
    removed: 0x00ff_8a80,
    added_bg: 0x0016_3326,
    removed_bg: 0x003a_1f1f,
    on_accent: 0x0000_3825,
    scrim: 0x0509_07b3,
    outline: 0x0085_908a,
    info: 0x008e_cbff,
    info_container: 0x0016_2a38,
    danger: 0x00ff_b4ab,
    danger_container: 0x003a_1f1f,
    secondary_container: 0x0034_4b41,
    categories: [0x00c9_b2ff, 0x0099_d9c4, 0x00e8_b4d8],
};

/// The light Material theme from the same emerald seed.
pub const LIGHT: Palette = Palette {
    window: 0x00e9_f0eb,
    panel: 0x00f2_f7f3,
    surface: 0x00fb_fdfb,
    ink: 0x0017_1d1a,
    ink_dim: 0x0056_615b,
    accent: 0x0000_6c4c,
    card: 0x00e4_ebe6,
    raised: 0x00db_e4de,
    warn: 0x008a_5100,
    warn_container: 0x00fb_efd6,
    warn_line: 0x00e3_c78c,
    code: 0x00ed_f2ee,
    line: 0x00c6_d0c9,
    selected: 0x00c4_ecd8,
    on_selected: 0x0000_2114,
    removed: 0x00ba_1a1a,
    added_bg: 0x00d3_efe0,
    removed_bg: 0x00fa_dcd8,
    on_accent: 0x00ff_ffff,
    scrim: 0x0f28_1c59,
    outline: 0x0070_7a74,
    info: 0x000b_57d0,
    info_container: 0x00d8_e6fb,
    danger: 0x00ba_1a1a,
    danger_container: 0x00fa_dcd8,
    secondary_container: 0x00cf_e8da,
    categories: [0x0067_50a4, 0x0000_6b5e, 0x008e_4585],
};

/// The palette `appearance` shows on a light or dark desktop, with the Omarchy theme's palette
/// when one was read; Omarchy without a theme to follow stays dark.
pub fn chosen(appearance: Appearance, desktop_light: bool, omarchy: Option<&Palette>) -> Palette {
    match appearance {
        Appearance::Light => LIGHT,
        Appearance::System if desktop_light => LIGHT,
        Appearance::Dark | Appearance::System => DARK,
        Appearance::Omarchy => omarchy.copied().unwrap_or(DARK),
    }
}

/// An Omarchy theme's `colors.toml`, in either of its two layouts: named colours with `mode`,
/// `dark_background` and the like, or `color0` to `color15`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Omarchy {
    light: bool,
    background: u32,
    foreground: u32,
    accent: u32,
    red: u32,
    green: u32,
    yellow: u32,
    orange: Option<u32>,
    blue: Option<u32>,
    magenta: Option<u32>,
    cyan: Option<u32>,
    backgrounds: Vec<u32>,
    dim_foreground: Option<u32>,
}

impl Omarchy {
    fn parse(toml: &str) -> Option<Self> {
        let colors: Vec<(&str, u32)> = toml.lines().filter_map(entry).collect();
        let get = |key: &str| {
            colors
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, color)| *color)
        };
        let background = get("background")?;
        let foreground = get("foreground")?;
        let accent = get("accent")?;
        let named = get("red").is_some();
        let pick = |name: &str, slot: &str| if named { get(name) } else { get(slot) };
        let mode = toml.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "mode").then(|| value.trim().trim_matches('"').to_owned())
        });
        Some(Self {
            light: mode.map_or_else(|| luminance(background) > 0.5, |mode| mode == "light"),
            background,
            foreground,
            accent,
            red: pick("red", "color1")?,
            green: pick("green", "color2")?,
            yellow: pick("yellow", "color3")?,
            orange: get("orange"),
            blue: pick("blue", "color4"),
            magenta: pick("magenta", "color5"),
            cyan: pick("cyan", "color6"),
            backgrounds: ["dark_background", "darker_background", "lighter_background"]
                .into_iter()
                .filter_map(get)
                .collect(),
            dim_foreground: get("dark_foreground"),
        })
    }

    /// The theme's own background where content sits, a frame a step away from it, and cards
    /// and inks kept readable on them, as the old app mapped Omarchy themes.
    fn palette(&self) -> Palette {
        let (bg, fg) = (self.background, self.foreground);
        let lum = luminance(bg);
        let lighter = self
            .backgrounds
            .iter()
            .copied()
            .filter(|color| luminance(*color) > lum)
            .max_by(|a, b| luminance(*a).total_cmp(&luminance(*b)));
        let darker = self
            .backgrounds
            .iter()
            .copied()
            .filter(|color| luminance(*color) < lum)
            .min_by(|a, b| luminance(*a).total_cmp(&luminance(*b)));
        let (window, card) = if self.light {
            (mix(bg, fg, 0.06), darker.unwrap_or_else(|| mix(bg, fg, 0.08)))
        } else {
            (
                darker.unwrap_or_else(|| mix(bg, 0x0000_0000, 0.25)),
                lighter.unwrap_or_else(|| mix(bg, fg, 0.06)),
            )
        };
        let toward_ink = if self.light { 0x0000_0000 } else { 0x00ff_ffff };
        let ink = readable(fg, bg, 4.5, toward_ink);
        // Secondary text is small, so it keeps the 4.5 contrast body text needs.
        let ink_dim = readable(
            readable(
                self.dim_foreground.unwrap_or_else(|| mix(fg, bg, 0.45)),
                bg,
                4.5,
                fg,
            ),
            bg,
            4.5,
            toward_ink,
        );
        let selected = mix(bg, self.accent, 0.25);
        let warn = [
            self.orange,
            Some(self.yellow),
            Some(0x00f5_9e0b),
            Some(0x00b4_5309),
        ]
        .into_iter()
        .flatten()
        .find(|color| contrast(*color, bg) >= 3.0)
        .unwrap_or(0x00b4_5309);
        let removed = readable(self.red, bg, 3.0, fg);
        let removed_bg = mix(bg, self.red, 0.15);
        let info = readable(self.blue.unwrap_or(self.accent), bg, 3.0, fg);
        let fallback = if self.light { LIGHT } else { DARK }.categories;
        let hue = |color: Option<u32>, fallback: u32| readable(color.unwrap_or(fallback), bg, 3.0, fg);
        let on_accent = if contrast(0x0000_0000, self.accent) >= contrast(0x00ff_ffff, self.accent) {
            0x0000_0000
        } else {
            0x00ff_ffff
        };
        Palette {
            window,
            panel: mix(window, bg, 0.5),
            surface: bg,
            ink,
            ink_dim,
            accent: self.accent,
            card,
            raised: mix(card, fg, 0.08),
            warn,
            warn_container: mix(bg, warn, 0.12),
            warn_line: mix(bg, warn, 0.35),
            code: mix(window, bg, 0.3),
            line: mix(card, fg, 0.15),
            selected,
            on_selected: readable(mix(self.accent, ink, 0.3), selected, 4.5, ink),
            removed,
            added_bg: mix(bg, self.green, 0.15),
            removed_bg,
            on_accent,
            scrim: (if self.light { fg } else { window }) << 8 | if self.light { 0x59 } else { 0xb3 },
            outline: mix(ink_dim, bg, 0.3),
            info,
            info_container: mix(bg, info, 0.15),
            danger: removed,
            danger_container: removed_bg,
            secondary_container: mix(card, self.accent, 0.15),
            categories: [
                hue(self.magenta, fallback[0]),
                hue(self.cyan, fallback[1]),
                hue(
                    self.magenta.map(|magenta| mix(magenta, self.red, 0.5)),
                    fallback[2],
                ),
            ],
        }
    }
}

/// The palette an Omarchy theme's `colors.toml` asks for, when it reads as one.
pub fn from_omarchy(colors_toml: &str) -> Option<Palette> {
    Omarchy::parse(colors_toml).map(|theme| theme.palette())
}

/// `key = "#rrggbb"` (or `#rgb`) on one line, comments and other values left out.
fn entry(line: &str) -> Option<(&str, u32)> {
    let (key, value) = line.split_once('=')?;
    let value = value
        .split('#')
        .nth(1)
        .map(|hex| hex.trim().trim_end_matches('"'))?;
    let value = value.split(|c: char| !c.is_ascii_hexdigit()).next()?;
    let rgb = match value.len() {
        6 => u32::from_str_radix(value, 16).ok()?,
        3 => {
            let short = u32::from_str_radix(value, 16).ok()?;
            let (r, g, b) = (short >> 8 & 0xf, short >> 4 & 0xf, short & 0xf);
            ((r * 0x11) << 16) | ((g * 0x11) << 8) | (b * 0x11)
        }
        _ => return None,
    };
    Some((key.trim(), rgb))
}

fn channels(color: u32) -> [f64; 3] {
    [color >> 16 & 0xff, color >> 8 & 0xff, color & 0xff].map(f64::from)
}

fn luminance(color: u32) -> f64 {
    let [r, g, b] = channels(color).map(|channel| {
        let s = channel / 255.;
        if s <= 0.040_45 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `a` moved the fraction `t` of the way to `b`.
fn mix(a: u32, b: u32, t: f64) -> u32 {
    let (a, b) = (channels(a), channels(b));
    let channel = |i: usize| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 0..=255"
        )]
        let value = (a[i] * (1. - t) + b[i] * t).round().clamp(0., 255.) as u32;
        value
    };
    channel(0) << 16 | channel(1) << 8 | channel(2)
}

/// `color` moved toward `toward` until it reads against `bg` at `ratio`.
fn readable(color: u32, bg: u32, ratio: f64, toward: u32) -> u32 {
    let mut color = color;
    for _ in 0..25 {
        if contrast(color, bg) >= ratio {
            return color;
        }
        color = mix(color, toward, 0.1);
    }
    if contrast(color, bg) >= ratio {
        color
    } else {
        toward
    }
}

#[cfg(test)]
mod tests {
    use super::{contrast, from_omarchy};

    #[test]
    fn both_omarchy_layouts_give_a_readable_palette() {
        let named = "mode = \"dark\"\naccent = \"#7fbbb3\"\nbackground = \"#2d353b\"\n\
                     dark_background = \"#21272c\"\nlighter_background = \"#343f44\"\n\
                     foreground = \"#d3c6aa\"\nred = \"#e67e80\"\ngreen = \"#a7c080\"\nyellow = \"#dbbc7f\"\n";
        let slots = "accent = \"#1e66f5\"\nbackground = \"#eff1f5\"\nforeground = \"#4c4f69\"\n\
                     color1 = \"#d20f39\"\ncolor2 = \"#40a02b\"\ncolor3 = \"#df8e1d\"\n";
        for (toml, light) in [(named, false), (slots, true)] {
            let palette = from_omarchy(toml).unwrap_or(super::DARK);
            assert_eq!(palette.surface > 0x0080_8080, light);
            assert!(contrast(palette.ink, palette.surface) >= 4.5);
            assert!(contrast(palette.ink_dim, palette.surface) >= 4.5);
        }
        assert_eq!(from_omarchy("background = \"#000000\""), None);
    }
}
