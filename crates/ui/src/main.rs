mod brand;
mod input;
mod markdown_view;
mod session_pane;
mod side_panel;
mod terminal_view;
mod text;
mod theme;
mod transcript;
mod turn_rail;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

use app::Hub;
use gateway::{Endpoint, Gateway};
use gpui::{
    App, AppContext, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use session_pane::ToggleTerminal;
use workspace::{NewSession, ToggleSidePanel, Workspace, ZoomOut};

fn endpoint() -> Endpoint {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let base_url = std::env::var("RIVETHUB_URL").unwrap_or_else(|_| "https://127.0.0.1:5174".into());
    let ca_pem =
        std::env::var_os("RIVETHUB_CA").map_or_else(|| home.join(".rivethub/step-root.crt"), PathBuf::from);
    Endpoint { base_url, ca_pem }
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-up", ZoomOut, Some("Workspace")),
        KeyBinding::new("ctrl-t", NewSession, Some("Workspace")),
        KeyBinding::new("ctrl-b", ToggleSidePanel, Some("Workspace")),
        KeyBinding::new("ctrl-shift-t", ToggleTerminal, Some("Session")),
        KeyBinding::new("enter", input::Submit, Some("TextInput")),
        KeyBinding::new("shift-enter", input::Newline, Some("TextInput")),
        KeyBinding::new("up", input::Up, Some("TextInput")),
        KeyBinding::new("down", input::Down, Some("TextInput")),
        KeyBinding::new("shift-up", input::SelectUp, Some("TextInput")),
        KeyBinding::new("shift-down", input::SelectDown, Some("TextInput")),
        KeyBinding::new("backspace", input::Backspace, Some("TextInput")),
        KeyBinding::new("delete", input::Delete, Some("TextInput")),
        KeyBinding::new("left", input::Left, Some("TextInput")),
        KeyBinding::new("right", input::Right, Some("TextInput")),
        KeyBinding::new("shift-left", input::SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", input::SelectRight, Some("TextInput")),
        KeyBinding::new("ctrl-a", input::SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-v", input::Paste, Some("TextInput")),
        KeyBinding::new("ctrl-c", input::Copy, Some("TextInput")),
        KeyBinding::new("ctrl-x", input::Cut, Some("TextInput")),
        KeyBinding::new("home", input::Home, Some("TextInput")),
        KeyBinding::new("end", input::End, Some("TextInput")),
    ]);
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(%err, "starting the async runtime failed");
            return ExitCode::FAILURE;
        }
    };
    let endpoint = endpoint();
    let gateway = match Gateway::connect(&endpoint) {
        Ok(gateway) => gateway,
        Err(err) => {
            tracing::error!(%err, url = %endpoint.base_url, "connecting to the gateway failed");
            return ExitCode::FAILURE;
        }
    };
    let hub = Hub::new(runtime.handle().clone(), gateway);
    gpui_platform::application().run(move |cx: &mut App| {
        bind_keys(cx);
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
            let workspace = cx.new(|cx| Workspace::new(hub, window, cx));
            window.focus(&workspace.focus_handle(cx), cx);
            workspace
        });
        if opened.is_err() {
            cx.quit();
        }
        cx.activate(true);
    });
    drop(runtime);
    ExitCode::SUCCESS
}
