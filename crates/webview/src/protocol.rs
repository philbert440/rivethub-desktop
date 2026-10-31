//! What the app and the engine library say to each other: JSON both ways, through the functions
//! the library exports, with frames and answers' bodies passed as bytes beside it.

use serde::{Deserialize, Serialize};

use crate::{Cursor, Menu, Modifiers, Pointer, Rect, Size};

/// The library's file name.
pub const LIBRARY: &str = "librivethub_webview.so";

/// Bumped whenever a message changes, so an app never drives a library from another build.
pub const VERSION: u32 = 2;

pub const START: &[u8] = b"rivethub_webview_start\0";
pub const SEND: &[u8] = b"rivethub_webview_send\0";
pub const RESPOND: &[u8] = b"rivethub_webview_respond\0";

/// `rivethub_webview_start(version, message, frame)`: starts the engine's thread, or returns why
/// not as a NUL-terminated string the library keeps.
pub type StartFn = unsafe extern "C" fn(u32, MessageSink, FrameSink) -> *const std::ffi::c_char;
/// `rivethub_webview_send(json, len)`: a `ToEngine`.
pub type SendFn = unsafe extern "C" fn(*const u8, usize);
/// `rivethub_webview_respond(request, status, media_type, media_type_len, body, body_len)`.
pub type RespondFn = unsafe extern "C" fn(u64, u16, *const u8, usize, *const u8, usize);
/// The app's sink for a `FromEngine`, called on the engine's thread.
pub type MessageSink = unsafe extern "C" fn(*const u8, usize);
/// The app's sink for a frame: panel, width, height, scale, BGRA pixels and their length.
pub type FrameSink = unsafe extern "C" fn(u64, u32, u32, f32, *const u8, usize);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToEngine {
    Open {
        panel: u64,
        origin: String,
        entry: String,
        style: String,
        background: u32,
        size: Size,
    },
    To {
        panel: u64,
        command: Command,
    },
    Close {
        panel: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Resize(Size),
    Pointer(Pointer, Modifiers),
    Key {
        press: bool,
        keyval: u32,
        modifiers: Modifiers,
    },
    Focus(bool),
    Enter {
        forward: bool,
    },
    Post(String),
    Restyle(String),
    Choose(Option<usize>),
    Compose {
        text: String,
        cursor: usize,
    },
    Commit(String),
    Paste(String),
    Reload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FromEngine {
    Event {
        panel: u64,
        event: Event,
    },
    Fetch {
        panel: u64,
        request: u64,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Warning(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Loaded,
    Message(String),
    Cursor(Cursor),
    Menu(Option<Menu>),
    Copied(String),
    Editing(bool),
    Caret(Rect),
    FocusLeft { forward: bool },
    Stopped,
}
