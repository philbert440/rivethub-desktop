use std::path::PathBuf;

/// The Omarchy theme this desktop has switched to: its name and its `colors.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmarchyTheme {
    pub name: Option<String>,
    pub colors: String,
}

/// Where Omarchy keeps the active theme: `current/theme` under its state folder, or under its
/// config folder on older installs, as the old desktop app looked.
fn candidates() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let var = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
    };
    [
        var("XDG_STATE_HOME"),
        home.as_ref().map(|home| home.join(".local/state")),
        var("XDG_CONFIG_HOME"),
        home.as_ref().map(|home| home.join(".config")),
    ]
    .into_iter()
    .flatten()
    .map(|dir| dir.join("omarchy/current"))
    .collect()
}

/// The active Omarchy theme, or `None` on a desktop without Omarchy. Omarchy replaces the whole
/// `current/theme` folder on a switch, so each read finds the theme in place now.
pub fn current_theme() -> Option<OmarchyTheme> {
    candidates().into_iter().find_map(|current| {
        let colors = std::fs::read_to_string(current.join("theme/colors.toml")).ok()?;
        let name = std::fs::read_to_string(current.join("theme.name"))
            .ok()
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        Some(OmarchyTheme { name, colors })
    })
}
