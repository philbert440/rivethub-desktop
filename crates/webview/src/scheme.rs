//! The pages' scheme: every request a page makes of its own origin goes to that origin's loader,
//! and every answer carries the page's security policy, whatever the server sent.

use gtk::{gio, glib};
use webkit2gtk::{URISchemeRequestExt, URISchemeResponseExt};

use crate::engine::Origins;
use crate::{Fetch, Fetched};

pub(crate) const SCHEME: &str = "rhpanel";

/// Scripts, styles, fonts and requests from the page's own origin only, inline styles, images
/// also from data and blob URLs; and through the sandbox, no pop-ups, downloads, dialogs or
/// navigation of other windows.
const POLICY: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
                      sandbox allow-scripts allow-same-origin allow-forms";

pub(crate) fn answer(request: &webkit2gtk::URISchemeRequest, origins: &Origins) {
    let uri = request.uri().map(|uri| uri.to_string()).unwrap_or_default();
    let Some((host, path)) = split(&uri) else {
        finish(request, &failed(400, "not a panel address"));
        return;
    };
    let Some(loader) = origins.loader(host) else {
        finish(request, &failed(404, "no panel at this address"));
        return;
    };
    let fetch = Fetch {
        method: request
            .http_method()
            .map_or_else(|| "GET".to_owned(), |method| method.to_string()),
        path: path.to_owned(),
        headers: headers(request),
        body: body(request),
    };
    let request = request.clone();
    let answered = loader(fetch);
    glib::MainContext::default().spawn_local(async move {
        finish(&request, &answered.await);
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

fn failed(status: u16, why: &str) -> Fetched {
    Fetched {
        status,
        media_type: Some("text/plain; charset=utf-8".into()),
        body: why.as_bytes().to_vec(),
    }
}

fn finish(request: &webkit2gtk::URISchemeRequest, fetched: &Fetched) {
    let length = i64::try_from(fetched.body.len()).unwrap_or(-1);
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from(&fetched.body));
    let response = webkit2gtk::URISchemeResponse::new(&stream, length);
    response.set_status(u32::from(fetched.status), None);
    let media_type = fetched
        .media_type
        .as_deref()
        .unwrap_or("application/octet-stream");
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
