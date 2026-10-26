mod brand;
mod theme;
mod workspace;

use gpui::{
    App, AppContext, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use workspace::{Workspace, ZoomOut};

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        cx.bind_keys([KeyBinding::new("ctrl-up", ZoomOut, Some("Workspace"))]);
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("RivetHub".into()),
                ..Default::default()
            }),
            app_id: Some("rivethub-desktop".into()),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let workspace = cx.new(Workspace::new);
            window.focus(&workspace.focus_handle(cx), cx);
            workspace
        });
        if opened.is_err() {
            cx.quit();
        }
        cx.activate(true);
    });
}
