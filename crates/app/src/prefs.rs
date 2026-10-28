use std::path::PathBuf;

use model::prefs::Preferences;
use serde::{Deserialize, Serialize};

use crate::Hub;

/// The file's shape: every field optional, so a file from an older or newer build still loads.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Stored {
    notify_finished: bool,
    notify_needs_you: bool,
}

impl Default for Stored {
    fn default() -> Self {
        Self::from(&Preferences::default())
    }
}

impl From<&Preferences> for Stored {
    fn from(prefs: &Preferences) -> Self {
        Self {
            notify_finished: prefs.notify_finished,
            notify_needs_you: prefs.notify_needs_you,
        }
    }
}

impl From<Stored> for Preferences {
    fn from(stored: Stored) -> Self {
        Self {
            notify_finished: stored.notify_finished,
            notify_needs_you: stored.notify_needs_you,
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
