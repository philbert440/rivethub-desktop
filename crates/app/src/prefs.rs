use std::path::PathBuf;

use model::HarnessId;
use model::prefs::{Appearance, LastLaunch, Preferences, SessionView};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::Hub;

/// The file's shape: every field optional, so a file from an older or newer build still loads.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[expect(clippy::struct_excessive_bools, reason = "each switch in Settings is a bool")]
struct Stored {
    notifications: bool,
    notify_finished: bool,
    notify_needs_you: bool,
    notify_sound: bool,
    jump_to_newest: bool,
    #[serde(deserialize_with = "or_none")]
    last_launch: Option<StoredLaunch>,
    #[serde(deserialize_with = "or_default")]
    appearance: StoredAppearance,
    #[serde(deserialize_with = "or_default")]
    opens_in: StoredView,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum StoredAppearance {
    #[default]
    Dark,
    Light,
    System,
    Omarchy,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum StoredView {
    #[default]
    Chat,
    Terminal,
}

/// A value this build doesn't know, such as an appearance from a newer one, keeps the default.
fn or_default<'de, D: Deserializer<'de>, T: Default + DeserializeOwned>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum StoredLaunch {
    Agent {
        id: String,
        name: String,
    },
    Harness {
        node: Option<String>,
        harness: String,
        model: Option<String>,
    },
}

/// A launch this build can't read, such as a kind from a newer one, is forgotten rather than
/// costing every other preference.
fn or_none<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<StoredLaunch>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

impl Default for Stored {
    fn default() -> Self {
        Self::from(&Preferences::default())
    }
}

impl From<&Preferences> for Stored {
    fn from(prefs: &Preferences) -> Self {
        Self {
            notifications: prefs.notifications,
            notify_finished: prefs.notify_finished,
            notify_needs_you: prefs.notify_needs_you,
            notify_sound: prefs.notify_sound,
            jump_to_newest: prefs.jump_to_newest,
            last_launch: prefs.last_launch.as_ref().map(|launch| match launch {
                LastLaunch::Agent { id, name } => StoredLaunch::Agent {
                    id: id.clone(),
                    name: name.clone(),
                },
                LastLaunch::Harness { node, harness, model } => StoredLaunch::Harness {
                    node: node.clone(),
                    harness: harness.as_str().to_owned(),
                    model: model.clone(),
                },
            }),
            appearance: match prefs.appearance {
                Appearance::Dark => StoredAppearance::Dark,
                Appearance::Light => StoredAppearance::Light,
                Appearance::System => StoredAppearance::System,
                Appearance::Omarchy => StoredAppearance::Omarchy,
            },
            opens_in: match prefs.opens_in {
                SessionView::Chat => StoredView::Chat,
                SessionView::Terminal => StoredView::Terminal,
            },
        }
    }
}

impl From<Stored> for Preferences {
    fn from(stored: Stored) -> Self {
        Self {
            notifications: stored.notifications,
            notify_finished: stored.notify_finished,
            notify_needs_you: stored.notify_needs_you,
            notify_sound: stored.notify_sound,
            jump_to_newest: stored.jump_to_newest,
            last_launch: stored.last_launch.map(|launch| match launch {
                StoredLaunch::Agent { id, name } => LastLaunch::Agent { id, name },
                StoredLaunch::Harness { node, harness, model } => LastLaunch::Harness {
                    node,
                    harness: HarnessId::new(harness),
                    model,
                },
            }),
            appearance: match stored.appearance {
                StoredAppearance::Dark => Appearance::Dark,
                StoredAppearance::Light => Appearance::Light,
                StoredAppearance::System => Appearance::System,
                StoredAppearance::Omarchy => Appearance::Omarchy,
            },
            opens_in: match stored.opens_in {
                StoredView::Chat => SessionView::Chat,
                StoredView::Terminal => SessionView::Terminal,
            },
        }
    }
}

fn path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("rivethub-desktop").join("preferences.json"))
}

/// The saved preferences, or the defaults when there are none or they can't be read.
pub(crate) fn load() -> Preferences {
    let Some(path) = path() else {
        return Preferences::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Stored>(&bytes).map_or_else(
            |err| {
                tracing::warn!(%err, path = %path.display(), "the preferences file is unreadable");
                Preferences::default()
            },
            Preferences::from,
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Preferences::default(),
        Err(err) => {
            tracing::warn!(%err, path = %path.display(), "reading the preferences failed");
            Preferences::default()
        }
    }
}

/// Writes beside the file and renames over it, so a crash mid-write never leaves half a file.
fn save(prefs: &Preferences) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("neither XDG_CONFIG_HOME nor HOME is set"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(&Stored::from(prefs)).map_err(std::io::Error::other)?;
    let staged = path.with_extension("json.tmp");
    std::fs::write(&staged, json)?;
    std::fs::rename(staged, path)
}

impl Hub {
    /// Where the preferences are kept.
    pub fn preferences_path(&self) -> Option<PathBuf> {
        path()
    }

    pub fn preferences(&self) -> Preferences {
        self.inner.prefs.read().clone()
    }

    /// Takes effect at once and saves in the background. Saves run one at a time and each writes
    /// whatever is current, so the file ends on the latest choice however fast they come.
    pub fn set_preferences(&self, prefs: Preferences) {
        *self.inner.prefs.write() = prefs;
        let hub = self.clone();
        self.inner.runtime.spawn_blocking(move || {
            let _one_at_a_time = hub.inner.saving.lock();
            if let Err(err) = save(&hub.preferences()) {
                tracing::warn!(%err, "saving the preferences failed");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use model::prefs::{LastLaunch, Preferences, SessionView};

    use super::Stored;

    #[test]
    fn an_older_file_or_an_unknown_launch_still_loads() {
        let older: Stored = serde_json::from_str(r#"{"notifyFinished":false}"#).unwrap_or_default();
        let older = Preferences::from(older);
        assert!(!older.notify_finished);
        assert!(older.notifications && !older.notify_sound);
        assert!(older.jump_to_newest);
        assert_eq!(older.last_launch, None);

        let newer: Stored = serde_json::from_str(
            r#"{"jumpToNewest":false,"lastLaunch":{"kind":"workflow","id":"w1"},"appearance":"sepia"}"#,
        )
        .unwrap_or_default();
        let newer = Preferences::from(newer);
        assert!(!newer.jump_to_newest);
        assert_eq!(newer.last_launch, None);
        assert_eq!(newer.appearance, model::prefs::Appearance::Dark);

        let saved = Preferences {
            last_launch: Some(LastLaunch::Harness {
                node: Some("ct115".into()),
                harness: model::HarnessId::new("codex"),
                model: None,
            }),
            appearance: model::prefs::Appearance::Omarchy,
            opens_in: SessionView::Terminal,
            notifications: false,
            notify_sound: true,
            ..Preferences::default()
        };
        let json = serde_json::to_string(&Stored::from(&saved)).unwrap_or_default();
        let read: Stored = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(Preferences::from(read), saved);
    }
}
