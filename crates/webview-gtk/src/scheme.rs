//! The pages' scheme: every request a page makes of its own origin goes to the app, and every
//! answer carries the page's security policy, whatever the server sent.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::{gio, glib};
use webkit2gtk::{URISchemeRequestExt, URISchemeResponseExt};
use webview::protocol::FromEngine;

use crate::engine::Origins;

pub(crate) const SCHEME: &str = "rhpanel";

/// Scripts, styles, fonts and requests from the page's own origin only, inline styles, images
/// also from data and blob URLs; and through the sandbox, no pop-ups, downloads, dialogs or
/// navigation of other windows.
const POLICY: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
                      sandbox allow-scripts allow-same-origin allow-forms";

/// Requests waiting on the app's answer, by the id the app was given.
#[derive(Clone, Default)]
pub(crate) struct Pending {
    next: Rc<Cell<u64>>,
    requests: Rc<RefCell<HashMap<u64, webkit2gtk::URISchemeRequest>>>,
}

impl Pending {
    pub(crate) fn answer(&self, id: u64, status: u16, media_type: Option<&str>, body: Vec<u8>) {
        let request = self.requests.borrow_mut().remove(&id);
        if let Some(request) = request {
            finish(&request, status, media_type, body);
        }
    }
}

/// Hands a page's request to the app, through a page open at its origin.
pub(crate) fn ask(request: &webkit2gtk::URISchemeRequest, origins: &Origins, pending: &Pending) {
    let uri = request.uri().map(|uri| uri.to_string()).unwrap_or_default();
    let Some((host, path)) = split(&uri) else {
        finish(request, 400, None, b"not a panel address".to_vec());
        return;
    };
    let Some(panel) = origins.page(host) else {
        finish(request, 404, None, b"no panel at this address".to_vec());
        return;
    };
    let id = pending.next.get() + 1;
    pending.next.set(id);
    pending.requests.borrow_mut().insert(id, request.clone());
    crate::emit(&FromEngine::Fetch {
        panel,
        request: id,
        method: request
            .http_method()
            .map_or_else(|| "GET".to_owned(), |method| method.to_string()),
        path: path.to_owned(),
        headers: headers(request),
        body: body(request),
    });
}

/// The host and the path with its query, of `rhpanel://host/path?query#fragment`.
fn split(uri: &str) -> Option<(&str, &str)> {
    let rest = uri.strip_prefix(SCHEME)?.strip_prefix("://")?;
    let rest = rest.split('#').next().unwrap_or(rest);
    let slash = rest.find('/')?;
    Some((&rest[..slash], &rest[slash..]))
}

fn headers(request: &webkit2gtk::URISchemeRequest) -> Vec<(String, String)> {
    let mut found = Vec::new();
    if let Some(headers) = request.http_headers() {
        headers.foreach(|name, value| found.push((name.to_owned(), value.to_owned())));
    }
    found
}

fn body(request: &webkit2gtk::URISchemeRequest) -> Vec<u8> {
    use gio::prelude::InputStreamExtManual;
    let Some(stream) = request.http_body() else {
        return Vec::new();
    };
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    while let Ok(read) = stream.read(&mut chunk, gio::Cancellable::NONE) {
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body
}

fn finish(request: &webkit2gtk::URISchemeRequest, status: u16, media_type: Option<&str>, body: Vec<u8>) {
    let length = i64::try_from(body.len()).unwrap_or(-1);
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(body));
    let response = webkit2gtk::URISchemeResponse::new(&stream, length);
    response.set_status(u32::from(status), None);
    let media_type = media_type.unwrap_or("application/octet-stream");
    // WebKit takes the content type as a bare media type; its parameters (the charset) go in the
    // header, where `nosniff` checks the type.
    response.set_content_type(media_type.split(';').next().unwrap_or(media_type).trim());
    let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
    headers.append("Content-Type", media_type);
    headers.append("Content-Security-Policy", POLICY);
    headers.append("X-Content-Type-Options", "nosniff");
    headers.append("Cache-Control", "no-store");
    response.set_http_headers(headers);
    request.finish_with_response(&response);
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn a_panel_address_splits_into_its_origin_and_path() {
        assert_eq!(
            split("rhpanel://o1/index.html?a=1#top"),
            Some(("o1", "/index.html?a=1"))
        );
        assert_eq!(split("rhpanel://o2/"), Some(("o2", "/")));
        assert_eq!(split("rhpanel://o2"), None);
        assert_eq!(split("https://o1/index.html"), None);
    }
}
