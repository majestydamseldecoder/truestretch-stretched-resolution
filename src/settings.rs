//! Persistent settings, stored as JSON in `%APPDATA%\TrueStretch\settings.json`.

use crate::games::Game;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A width x height pair in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Width divided by height.
    pub fn aspect(self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\u{00D7}{}", self.width, self.height)
    }
}

/// The stretched presets offered in the profile editor.
pub const PRESETS: [Resolution; 4] =
    [Resolution::new(1440, 1080), Resolution::new(1280, 960), Resolution::new(1280, 1024), Resolution::new(1600, 1200)];

/// Per-game stretch profile.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub game: Game,
    pub enabled: bool,
    pub resolution: Resolution,
    /// Patch the game's own settings file so it renders full-screen without black bars.
    pub patch_config: bool,
    /// Mark the patched file read-only so the game cannot reset it.
    pub lock_config: bool,
    /// Ask the driver to stretch (`DMDFO_STRETCH`) instead of centering or keeping aspect.
    pub gpu_stretch: bool,
}

impl Profile {
    pub fn new(game: Game, resolution: Resolution, enabled: bool) -> Self {
        Self { game, enabled, resolution, patch_config: true, lock_config: false, gpu_stretch: true }
    }
}

/// Global hotkey, expressed as Win32 `MOD_*` flags and a virtual-key code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
}

impl Hotkey {
    /// Human readable label, e.g. `Ctrl + Alt + F9`.
    pub fn label(self) -> String {
        let mods = [(0x2, "Ctrl + "), (0x1, "Alt + "), (0x4, "Shift + "), (0x8, "Win + ")];
        let prefix: String = mods.iter().filter(|(f, _)| self.modifiers & f != 0).map(|(_, n)| *n).collect();
        match self.vk {
            0x70..=0x87 => format!("{prefix}F{}", self.vk - 0x6F),
            vk => format!("{prefix}{}", char::from_u32(vk).unwrap_or('?')),
        }
    }
}

/// Everything TrueStretch remembers between runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub profiles: Vec<Profile>,
    /// GDI device name such as `\\.\DISPLAY1`; `None` means the primary monitor.
    pub monitor: Option<String>,
    pub hotkey: Hotkey,
    /// Seconds before an unconfirmed manual change is reverted.
    pub confirm_seconds: u32,
    /// Process watcher poll interval in milliseconds.
    pub poll_ms: u64,
    /// Watch for games automatically.
    pub auto_apply: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profiles: vec![
                Profile::new(Game::Valorant, PRESETS[0], true),
                Profile::new(Game::Fortnite, Resolution::new(1750, 1080), true),
                Profile::new(Game::Cs2, PRESETS[1], true),
                Profile::new(Game::ApexLegends, Resolution::new(1680, 1050), false),
            ],
            monitor: None,
            hotkey: Hotkey { modifiers: 0x2 | 0x1, vk: 0x78 },
            confirm_seconds: 15,
            poll_ms: 750,
            auto_apply: true,
        }
    }
}

impl Settings {
    /// `%APPDATA%\TrueStretch`, created on demand.
    pub fn dir() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("TrueStretch")
    }

    fn path() -> PathBuf {
        Self::dir().join("settings.json")
    }

    /// Loads settings, falling back to defaults when the file is missing or invalid.
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Writes settings atomically (temp file + rename).
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(Self::dir())?;
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = Self::path().with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, Self::path())
    }
}
