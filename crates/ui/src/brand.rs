use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use gpui::{Image, ImageFormat, SharedString};

/// The Material Symbols plug-ins name their icons with, as `name path-data` lines.
const SYMBOLS: &str = include_str!("../assets/icons/material.txt");
/// The generic plug-in glyph, drawn for a symbol this app doesn't carry.
const PLUGIN_GLYPH: &str = "extension";

static RH: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/brand/rh.png").to_vec(),
    ))
});

static GHOST: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/brand/ghost.png").to_vec(),
    ))
});

pub fn rh() -> Arc<Image> {
    Arc::clone(&RH)
}

pub fn ghost() -> Arc<Image> {
    Arc::clone(&GHOST)
}

pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-400.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-500.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-600.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/RobotoFlex-700.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf").as_slice()),
    ]
}

pub struct Icons;

impl gpui::AssetSource for Icons {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path
            .strip_prefix("icons/symbols/")
            .and_then(|file| file.strip_suffix(".svg"))
        {
            return Ok(symbol_data(name).map(|data| {
                Cow::Owned(
                    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960"><path d="{data}"/></svg>"#)
                        .into_bytes(),
                )
            }));
        }
        let bytes: &'static [u8] = match path {
            "icons/search.svg" => include_bytes!("../assets/icons/search.svg"),
            "icons/history.svg" => include_bytes!("../assets/icons/history.svg"),
            "icons/side-panel.svg" => include_bytes!("../assets/icons/side-panel.svg"),
            "icons/workspace.svg" => include_bytes!("../assets/icons/workspace.svg"),
            "icons/rename.svg" => include_bytes!("../assets/icons/rename.svg"),
            "icons/arrow-up.svg" => include_bytes!("../assets/icons/arrow-up.svg"),
            "icons/square.svg" => include_bytes!("../assets/icons/square.svg"),
            "icons/plus.svg" => include_bytes!("../assets/icons/plus.svg"),
            "icons/message-square.svg" => include_bytes!("../assets/icons/message-square.svg"),
            "icons/library.svg" => include_bytes!("../assets/icons/library.svg"),
            "icons/book-open.svg" => include_bytes!("../assets/icons/book-open.svg"),
            "icons/tags.svg" => include_bytes!("../assets/icons/tags.svg"),
            "icons/settings.svg" => include_bytes!("../assets/icons/settings.svg"),
            "icons/copy.svg" => include_bytes!("../assets/icons/copy.svg"),
            "icons/check.svg" => include_bytes!("../assets/icons/check.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(Vec::new())
    }
}

fn symbol_data(name: &str) -> Option<&'static str> {
    SYMBOLS
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
}

/// The asset path of the Material Symbol `name`, or of the generic plug-in glyph when there is no
/// name or this app doesn't carry it.
pub fn symbol(name: Option<&str>) -> SharedString {
    let name = name
        .filter(|name| symbol_data(name).is_some())
        .unwrap_or(PLUGIN_GLYPH);
    format!("icons/symbols/{name}.svg").into()
}

pub fn icon(name: impl Into<SharedString>, size: f32, color: gpui::Rgba) -> gpui::Svg {
    use gpui::Styled;
    gpui::svg()
        .path(name)
        .flex_none()
        .size(gpui::px(size))
        .text_color(color)
}
