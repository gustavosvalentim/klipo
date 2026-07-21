use std::{collections::HashSet, fs, path::Path};

use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::Shortcut;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutSettings {
    pub version: u8,
    pub open_klipo: String,
    pub move_selection_up: String,
    pub move_selection_down: String,
    pub paste_selected_item: String,
    pub delete_selected_item: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            version: 1,
            open_klipo: "SUPER+SHIFT+KeyV".into(),
            move_selection_up: "ArrowUp".into(),
            move_selection_down: "ArrowDown".into(),
            paste_selected_item: "Enter".into(),
            delete_selected_item: "Delete".into(),
        }
    }
}

impl ShortcutSettings {
    pub fn entries(&self) -> [(&str, &str); 5] {
        [
            ("Open Klipo", &self.open_klipo),
            ("Move selection up", &self.move_selection_up),
            ("Move selection down", &self.move_selection_down),
            ("Paste selected item", &self.paste_selected_item),
            ("Delete selected item", &self.delete_selected_item),
        ]
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported settings version".into());
        }

        let mut seen = HashSet::new();
        for (action, value) in self.entries() {
            if value.eq_ignore_ascii_case("Escape") || value.is_empty() {
                return Err(format!("{action}: Escape is reserved for closing Klipo"));
            }

            value
                .parse::<Shortcut>()
                .map_err(|_| format!("{action}: unsupported shortcut"))?;

            if !seen.insert(value.to_ascii_uppercase()) {
                return Err(format!("{action}: this shortcut is already assigned"));
            }
        }

        Ok(())
    }
}

pub fn load(path: &Path) -> ShortcutSettings {
    let Ok(contents) = fs::read_to_string(path) else {
        return ShortcutSettings::default();
    };

    serde_json::from_str(&contents).unwrap_or_default()
}

pub fn save(path: &Path, settings: &ShortcutSettings) -> Result<(), String> {
    let parent = path.parent().ok_or("Settings path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let temporary = path.with_extension("json.tmp");
    let contents = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;

    fs::write(&temporary, contents).map_err(|error| error.to_string())?;
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(ShortcutSettings::default().validate().is_ok());
    }

    #[test]
    fn rejects_duplicates_and_escape() {
        let mut settings = ShortcutSettings::default();
        settings.move_selection_up = settings.open_klipo.clone();
        assert!(settings
            .validate()
            .unwrap_err()
            .contains("already assigned"));
        settings.move_selection_up = "Escape".into();
        assert!(settings.validate().unwrap_err().contains("reserved"));
    }

    #[test]
    fn persists_settings() {
        let directory = std::env::temp_dir().join(format!("klipo-settings-{}", std::process::id()));
        let path = directory.join("settings.json");
        let settings = ShortcutSettings::default();
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), settings);
        let _ = fs::remove_dir_all(directory);
    }
}
