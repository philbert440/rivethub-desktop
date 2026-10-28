mod brand;
mod input;
mod launcher;
mod markdown_view;
mod session_pane;
mod side_panel;
mod terminal_view;
mod text;
mod theme;
mod tool_view;
mod topic_picker;
mod transcript;
mod turn_rail;
mod workspace;

use std::path::PathBuf;
use std::process::ExitCode;

use app::Hub;
use gateway::{Endpoint, Gateway, Identity};
use gpui::{
    App, AppContext, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use session_pane::ToggleTerminal;
use workspace::{NewSession, NextNeedingYou, ToggleSidePanel, Workspace, ZoomOut};

fn endpoint() -> Endpoint {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let base_url = std::env::var("RIVETHUB_URL").unwrap_or_else(|_| "https://127.0.0.1:5174".into());
    let ca_pem =
        std::env::var_os("RIVETHUB_CA").map_or_else(|| home.join(".rivethub/step-root.crt"), PathBuf::from);
    Endpoint {
        base_url,
        ca_pem,
        identity: None,
    }
}

fn device_identity() -> Option<Identity> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let dir = std::env::var_os("RIVETHUB_DEVICE_DIR")
        .map_or_else(|| home.join(".config/RivetHub/mtls"), PathBuf::from);
    let identity = Identity {
        cert_pem: dir.join("device.crt"),
        key_pem: dir.join("device.key"),
    };
    (identity.cert_pem.is_file() && identity.key_pem.is_file()).then_some(identity)
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-up", ZoomOut, Some("Workspace")),
        KeyBinding::new("ctrl-t", NewSession, Some("Workspace")),
        KeyBinding::new("ctrl-b", ToggleSidePanel, Some("Workspace")),
        KeyBinding::new("ctrl-j", NextNeedingYou, Some("Workspace")),
        KeyBinding::new("ctrl-j", gpui::NoAction, Some("Terminal")),
        KeyBinding::new("escape", topic_picker::Dismiss, Some("TopicPicker")),
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
        KeyBinding::new("ctrl-insert", input::Copy, Some("TextInput")),
        KeyBinding::new("shift-insert", input::Paste, Some("TextInput")),
        KeyBinding::new("ctrl-x", input::Cut, Some("TextInput")),
        KeyBinding::new("home", input::Home, Some("TextInput")),
        KeyBinding::new("end", input::End, Some("TextInput")),
    ]);
}

fn log_file() -> Option<std::fs::File> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    let dir = state.join("rivethub-desktop");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("rivethub-desktop.log"))
        .ok()
}

fn init_logging() {
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    match log_file() {
        Some(file) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::io::stderr.and(std::sync::Mutex::new(file)))
            .init(),
        None => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        tracing::error!(
            thread = thread.name().unwrap_or("unnamed"),
            location = info.location().map(ToString::to_string).unwrap_or_default(),
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "panic: {}",
            info.payload_as_str().unwrap_or("non-string panic payload"),
        );
    }));
}

fn main() -> ExitCode {
    init_logging();
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
    let hub = Hub::new(
        runtime.handle().clone(),
        gateway,
        endpoint.ca_pem.clone(),
        device_identity(),
    );
    gpui_platform::application()
        .with_assets(brand::Icons)
        .run(move |cx: &mut App| {
            bind_keys(cx);
            if let Err(err) = cx.text_system().add_fonts(brand::fonts()) {
                tracing::warn!(%err, "loading the bundled fonts failed");
            }
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
