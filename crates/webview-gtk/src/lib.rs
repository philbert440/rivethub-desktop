//! The app's web panel engine, a library the app loads only when a web panel opens: `WebKitGTK`
//! pages painted offscreen on a thread of their own that runs GTK's main loop. The app drives it
//! through the three functions exported here and hears from it through the two it hands `start`;
//! the messages are `webview::protocol`'s.
#![allow(unsafe_code)]

mod engine;
mod events;
mod ime;
mod page;
mod scheme;

use std::ffi::c_char;
use std::sync::OnceLock;

use webview::protocol::{self, Event, FromEngine};

/// The app's two functions, set once by `start`.
struct Sinks {
    message: protocol::MessageSink,
    frame: protocol::FrameSink,
}

static SINKS: OnceLock<Sinks> = OnceLock::new();

/// Tells the app something.
pub(crate) fn emit(message: &FromEngine) {
    let (Some(sinks), Ok(json)) = (SINKS.get(), serde_json::to_vec(message)) else {
        return;
    };
    // SAFETY: the app's sink reads `json.len()` bytes from a buffer that lives for the call.
    unsafe { (sinks.message)(json.as_ptr(), json.len()) };
}

pub(crate) fn event(panel: u64, event: Event) {
    emit(&FromEngine::Event { panel, event });
}

pub(crate) fn warn(warning: String) {
    emit(&FromEngine::Warning(warning));
}

/// Hands the app a frame: BGRA with premultiplied alpha, `width * 4` bytes a row.
pub(crate) fn frame(panel: u64, width: u32, height: u32, scale: f32, pixels: &[u8]) {
    if let Some(sinks) = SINKS.get() {
        // SAFETY: the app's sink copies `pixels.len()` bytes from a buffer that lives for the call.
        unsafe { (sinks.frame)(panel, width, height, scale, pixels.as_ptr(), pixels.len()) };
    }
}

/// Starts the engine for an app speaking `version` of the protocol, handing it the app's sinks.
/// Returns null, or why it can't start as a string that lives as long as the library.
///
/// # Safety
/// `message` and `frame` must stay callable, from any thread, for as long as the library is loaded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rivethub_webview_start(
    version: u32,
    message: protocol::MessageSink,
    frame: protocol::FrameSink,
) -> *const c_char {
    static REFUSAL: OnceLock<std::ffi::CString> = OnceLock::new();
    if version != protocol::VERSION {
        return c"the app and its web panel engine are from different builds".as_ptr();
    }
    if SINKS.set(Sinks { message, frame }).is_err() {
        return std::ptr::null();
    }
    match engine::start() {
        Ok(()) => std::ptr::null(),
        Err(why) => REFUSAL
            .get_or_init(|| std::ffi::CString::new(why).unwrap_or_default())
            .as_ptr(),
    }
}

/// Takes one `ToEngine`, as JSON.
///
/// # Safety
/// `json` must point to `len` readable bytes for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rivethub_webview_send(json: *const u8, len: usize) {
    // SAFETY: per the contract above.
    let bytes = unsafe { std::slice::from_raw_parts(json, len) };
    match serde_json::from_slice(bytes) {
        Ok(message) => engine::send(engine::Command::App(message)),
        Err(err) => warn(format!("a message the engine can't read: {err}")),
    }
}

/// Answers one of a page's requests.
///
/// # Safety
/// `media_type` and `body` must point to `media_type_len` and `body_len` readable bytes for the
/// duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rivethub_webview_respond(
    request: u64,
    status: u16,
    media_type: *const u8,
    media_type_len: usize,
    body: *const u8,
    body_len: usize,
) {
    // SAFETY: per the contract above.
    let (media_type, body) = unsafe {
        (
            std::slice::from_raw_parts(media_type, media_type_len),
            std::slice::from_raw_parts(body, body_len),
        )
    };
    let media_type = std::str::from_utf8(media_type).unwrap_or_default();
    engine::send(engine::Command::Respond {
        request,
        status,
        media_type: (!media_type.is_empty()).then(|| media_type.to_owned()),
        body: body.to_vec(),
    });
}
