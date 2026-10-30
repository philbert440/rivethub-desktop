//! A plug-in's web panel: its own page, shown in a block of one of its views, which the app tells
//! its theme and the page asks to run actions as a button would.

use crate::plugin::canvas::Role;
use crate::plugin::view::Action;
use crate::plugin::{Context, Params, PluginRef};

/// The app's theme as a page gets it.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub light: bool,
    /// Each colour role, as `0xRRGGBB`.
    pub colors: Vec<(Role, u32)>,
    pub font_family: String,
    pub font_mono: String,
    /// The body text size in logical pixels.
    pub font_size: f32,
    /// The corner radius of a card in logical pixels.
    pub radius: f32,
}

/// What the app tells a page.
#[derive(Debug, Clone, PartialEq)]
pub enum HostMessage {
    /// Once the page has loaded: the theme, the block's params, and what the view is about.
    Init {
        theme: Theme,
        params: Option<Params>,
        context: Context,
        plugin: PluginRef,
    },
    /// The theme changed.
    Theme(Theme),
}

/// What a page asks of the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Ready,
    /// The height the page wants, in logical pixels.
    Resize(u32),
    Action(Action),
}

/// A request a page makes of its own origin, passed to the plug-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The path at the page's origin, with its query: `/http/…` goes to the plug-in's `http/`
    /// route and any other path to its `panel/` route.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The plug-in's answer to a page's request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub media_type: Option<String>,
    pub body: Vec<u8>,
}
