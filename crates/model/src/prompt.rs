/// A file uploaded to rh and ready to go out with a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub name: String,
    pub media_type: String,
    pub uri: String,
}

/// What the user sends as one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub text: String,
    pub attachments: Vec<Attachment>,
}

impl Prompt {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
        }
    }
}

/// rh only offers image attachments, so a file is attachable when its name says it is an image.
pub fn image_media_type(file_name: &str) -> Option<&'static str> {
    let (_, extension) = file_name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}
