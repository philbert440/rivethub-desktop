//! The main window's size, position and maximized state, kept between runs as one line of text.

use std::path::PathBuf;

use gpui::{App, Bounds, Pixels, WindowBounds, point, px, size};

const FIRST_SIZE: (f32, f32) = (1440., 900.);
const SMALLEST: f32 = 320.;

fn file() -> Option<PathBuf> {
    crate::state_dir().map(|dir| dir.join("window"))
}

/// Where the window opens: as it was last closed, moved back to the centre if that spot is no
/// longer on any display.
pub fn restore(cx: &App) -> WindowBounds {
    let saved = file()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|line| parse(&line));
    let Some(saved) = saved else {
        return WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(FIRST_SIZE.0), px(FIRST_SIZE.1)),
            cx,
        ));
    };
    let on_screen = |bounds: &Bounds<Pixels>| {
        cx.displays()
            .iter()
            .any(|display| display.bounds().intersects(bounds))
    };
    let placed = |bounds: Bounds<Pixels>| {
        if on_screen(&bounds) {
            bounds
        } else {
            Bounds::centered(None, bounds.size, cx)
        }
    };
    match saved {
        WindowBounds::Windowed(bounds) => WindowBounds::Windowed(placed(bounds)),
        WindowBounds::Maximized(bounds) => WindowBounds::Maximized(placed(bounds)),
        WindowBounds::Fullscreen(bounds) => WindowBounds::Fullscreen(placed(bounds)),
    }
}

pub fn save(bounds: WindowBounds) {
    let Some(file) = file() else {
        return;
    };
    let (mode, rect) = match bounds {
        WindowBounds::Windowed(rect) => ("windowed", rect),
        WindowBounds::Maximized(rect) => ("maximized", rect),
        WindowBounds::Fullscreen(rect) => ("fullscreen", rect),
    };
    let line = format!(
        "{mode} {} {} {} {}\n",
        f32::from(rect.origin.x),
        f32::from(rect.origin.y),
        f32::from(rect.size.width),
        f32::from(rect.size.height),
    );
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&file, line));
    if let Err(err) = written {
        tracing::warn!(%err, "saving the window's size and position failed");
    }
}

fn parse(line: &str) -> Option<WindowBounds> {
    let mut words = line.split_whitespace();
    let mode = words.next()?;
    let mut number = || {
        words
            .next()?
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
    };
    let (x, y, width, height) = (number()?, number()?, number()?, number()?);
    if width < SMALLEST || height < SMALLEST {
        return None;
    }
    let rect = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
    match mode {
        "windowed" => Some(WindowBounds::Windowed(rect)),
        "maximized" => Some(WindowBounds::Maximized(rect)),
        "fullscreen" => Some(WindowBounds::Fullscreen(rect)),
        _ => None,
    }
}
