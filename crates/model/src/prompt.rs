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

/// A file that went out with a user turn, as the transcript records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentFile {
    pub media_type: String,
    pub name: Option<String>,
    /// The upload rh still holds for it, which a turn rh didn't send itself never has.
    pub upload: Option<String>,
}

impl SentFile {
    /// The name rh recorded, or the media type when it recorded none.
    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.media_type)
    }

    /// The upload to show in place of the name: only an image rh still holds.
    pub fn viewable_upload(&self) -> Option<&str> {
        self.upload
            .as_deref()
            .filter(|_| self.media_type.starts_with("image/"))
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
