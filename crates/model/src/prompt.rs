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
    /// The model or effort the session switches to before this turn runs.
    pub switch: Switch,
}

impl Prompt {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
            switch: Switch::default(),
        }
    }
}

/// A model and effort picked for the turns a person sends next.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Switch {
    pub model: Option<String>,
    /// One of [`crate::agents::EFFORTS`].
    pub effort: Option<String>,
}

impl Switch {
    /// What a turn has to ask for: only the picks that differ from what the session runs, so an
    /// unchanged pick never makes rh switch again.
    #[must_use]
    pub fn against(&self, model: Option<&str>, effort: Option<&str>) -> Self {
        Self {
            model: self.model.clone().filter(|picked| Some(picked.as_str()) != model),
            effort: self
                .effort
                .clone()
                .filter(|picked| Some(picked.as_str()) != effort),
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

#[cfg(test)]
mod tests {
    use super::Switch;

    #[test]
    fn a_turn_asks_only_for_what_the_session_does_not_run_yet() {
        let picked = Switch {
            model: Some("opus".into()),
            effort: Some("high".into()),
        };
        let asked = picked.against(Some("opus"), Some("low"));
        assert_eq!(asked.model, None);
        assert_eq!(asked.effort.as_deref(), Some("high"));
    }
}
