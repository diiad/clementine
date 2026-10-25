//! Persistent preferences, stored as JSON in the app config directory.

use crate::i18n::Language;
use crate::watcher::Modifier;
use clementine_core::{Category, Options, Recipe, VideoPreset};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputLocation {
    /// Next to the original file.
    Beside,
    Downloads,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HapticStrength {
    Light,
    #[default]
    Medium,
    Strong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub convert_key: Modifier,
    pub tools_key: Modifier,
    pub actions_key: Modifier,
    pub output: OutputLocation,
    pub custom_dir: Option<PathBuf>,
    pub quality: u8,
    pub dpi: u32,
    pub video_preset: VideoPreset,
    /// Size the "Fit size" tool aims under, in MB.
    pub target_mb: f64,
    /// Chains of operations, shown first on the Tools wheel.
    pub recipes: Vec<Recipe>,
    pub reveal_after: bool,
    pub notify: bool,
    /// Global shortcut that opens the wheel for the Finder selection.
    pub shortcut: String,
    pub language: Language,
    /// Tick the trackpad when the pointer moves onto another slice.
    pub haptics: bool,
    pub haptic_strength: HapticStrength,
    /// Hide the menu-bar icon. Opening the app again shows Settings.
    pub hide_menu_bar_icon: bool,
    /// Look for a new version every few hours and offer to install it.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            convert_key: Modifier::Shift,
            // The Windows key opens the Start menu when released: use Alt.
            tools_key: if cfg!(windows) {
                Modifier::Option
            } else {
                Modifier::Command
            },
            actions_key: Modifier::Control,
            output: OutputLocation::Beside,
            custom_dir: None,
            quality: 85,
            dpi: 300,
            video_preset: VideoPreset::Standard,
            target_mb: 10.0,
            recipes: vec![Recipe {
                name: "Web 1080".into(),
                steps: vec!["tool:resize-1080".into(), "convert:jpg".into()],
            }],
            reveal_after: false,
            notify: true,
            // ⌥⌘T on macOS; Win+Alt combinations are partly taken on Windows.
            shortcut: if cfg!(windows) {
                "Control+Alt+KeyT"
            } else {
                "Alt+Super+KeyT"
            }
            .into(),
            language: Language::System,
            haptics: true,
            haptic_strength: HapticStrength::Medium,
            hide_menu_bar_icon: false,
            auto_update: true,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        #[allow(unused_mut)]
        let mut s: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Windows builds before the drag trigger saved the macOS default.
        #[cfg(windows)]
        if s.tools_key == Modifier::Command
            && s.convert_key != Modifier::Option
            && s.actions_key != Modifier::Option
        {
            s.tools_key = Modifier::Option;
        }
        s
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn options(&self) -> Options {
        let out_dir = match self.output {
            OutputLocation::Beside => None,
            OutputLocation::Downloads => {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"))
            }
            OutputLocation::Custom => self.custom_dir.clone(),
        };
        Options {
            out_dir,
            quality: self.quality.clamp(1, 100),
            dpi: self.dpi.clamp(36, 1200),
            video_preset: self.video_preset,
            target_mb: self.target_mb.clamp(0.1, 10_000.0),
        }
    }

    /// Which wheel the currently held modifiers open. Exactly one trigger key
    /// must be held, so ⇧⌘ during a drag does nothing.
    pub fn category_for(&self, mods: &crate::watcher::Mods) -> Option<Category> {
        let held: Vec<Category> = [
            (self.convert_key, Category::Convert),
            (self.tools_key, Category::Tool),
            (self.actions_key, Category::Action),
        ]
        .into_iter()
        .filter(|(k, _)| mods.has(*k))
        .map(|(_, c)| c)
        .collect();
        match held.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }
}
