//! Supported games: process names, config locations and the patchers that make the game render
//! a true stretched image instead of a letterboxed or pillarboxed one.

use crate::settings::Resolution;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::core::w;

/// A game TrueStretch knows how to detect and configure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Game {
    Valorant,
    Fortnite,
    Cs2,
    ApexLegends,
}

/// `(key, old value, new value)` for one changed or added setting.
pub type Change = (String, String, String);

impl Game {
    /// Title, watched executable, sidebar tile initials and tile color (`0xRRGGBB`).
    fn info(self) -> (&'static str, &'static str, &'static str, u32) {
        match self {
            Game::Valorant => ("VALORANT", "VALORANT-Win64-Shipping.exe", "V", 0xFF4655),
            Game::Fortnite => ("Fortnite", "FortniteClient-Win64-Shipping.exe", "F", 0x2A7FFF),
            Game::Cs2 => ("Counter-Strike 2", "cs2.exe", "CS", 0xDE9B35),
            Game::ApexLegends => ("Apex Legends", "r5apex.exe", "A", 0xDA292A),
        }
    }

    /// Initials and color for the game tile in the sidebar.
    pub fn tile(self) -> (&'static str, u32) {
        (self.info().2, self.info().3)
    }

    pub fn title(self) -> &'static str {
        self.info().0
    }

    pub fn exe(self) -> &'static str {
        self.info().1
    }

    /// The patcher for this game's own settings file.
    pub fn patcher(self) -> Box<dyn ConfigPatcher> {
        match self {
            Game::Valorant | Game::Fortnite => Box::new(UnrealIni(self)),
            Game::Cs2 | Game::ApexLegends => Box::new(SourceVideo(self)),
        }
    }

    /// Launch options worth adding in Steam / the EA app, shown on the config screen.
    pub fn launch_options(self, r: Resolution) -> Option<String> {
        matches!(self, Game::Cs2 | Game::ApexLegends).then(|| format!("-fullscreen -w {} -h {}", r.width, r.height))
    }
}

/// Result of patching one settings file.
#[derive(Clone, Debug)]
pub struct PatchReport {
    pub file: PathBuf,
    /// Backup written by this patch (only the first patch backs up, so the original is kept).
    pub backup: Option<PathBuf>,
    pub changes: Vec<Change>,
}

/// Extension point: implement this to teach TrueStretch another game's config format.
pub trait ConfigPatcher {
    /// Every settings file that should be patched (VALORANT keeps one per Riot account).
    fn locate(&self) -> Vec<PathBuf>;
    /// Key/value pairs that force a stretched, non-letterboxed image at `r`.
    fn keys(&self, r: Resolution) -> Vec<(&'static str, String)>;
    /// Splits one line into `(key, value)` if it is a setting line.
    fn parse(&self, line: &str) -> Option<(String, String)>;
    /// Renders one setting line.
    fn render(&self, key: &str, value: &str) -> String;

    /// Replaces known keys in place and inserts missing ones after the first section header
    /// (`[...]`) or opening brace, preserving every other line.
    fn rewrite(&self, text: &str, keys: &[(&'static str, String)]) -> (String, Vec<Change>) {
        let mut seen = vec![false; keys.len()];
        let mut changes = Vec::new();
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        for line in &mut lines {
            let Some((k, old)) = self.parse(line) else { continue };
            let Some(i) = keys.iter().position(|(key, _)| key.eq_ignore_ascii_case(&k)) else { continue };
            seen[i] = true;
            if old != keys[i].1 {
                *line = self.render(keys[i].0, &keys[i].1);
                changes.push((keys[i].0.to_owned(), old, keys[i].1.clone()));
            }
        }
        let anchor = lines.iter().position(|l| l.trim_start().starts_with('[') || l.trim() == "{");
        let anchor = anchor.map_or(lines.len(), |i| i + 1);
        let missing = keys.iter().zip(&seen).filter(|(_, seen)| !**seen).map(|((k, v), _)| {
            changes.push(((*k).to_owned(), String::new(), v.clone()));
            self.render(k, v)
        });
        lines.splice(anchor..anchor, missing.collect::<Vec<_>>());
        (lines.join("\r\n") + "\r\n", changes)
    }

    /// Backs the file up once, clears read-only, writes the patch and optionally re-locks it.
    fn patch(&self, file: &Path, r: Resolution, lock_read_only: bool) -> std::io::Result<PatchReport> {
        let text = std::fs::read_to_string(file)?;
        let (patched, changes) = self.rewrite(&text, &self.keys(r));
        let mut backup = file.as_os_str().to_owned();
        backup.push(".truestretch.bak");
        let backup = PathBuf::from(backup);
        let made_backup = if backup.exists() { None } else { std::fs::copy(file, &backup).map(|_| Some(backup))? };
        set_read_only(file, false)?;
        if !changes.is_empty() {
            std::fs::write(file, patched)?;
        }
        if lock_read_only {
            set_read_only(file, true)?;
        }
        Ok(PatchReport { file: file.to_path_buf(), backup: made_backup, changes })
    }
}

fn set_read_only(file: &Path, read_only: bool) -> std::io::Result<()> {
    let mut perms = std::fs::metadata(file)?.permissions();
    if perms.readonly() == read_only {
        return Ok(());
    }
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(read_only);
    std::fs::set_permissions(file, perms)
}

/// Collects `<root>\<each subfolder>\<tail>` files, e.g. one config per account folder.
fn per_account(root: PathBuf, tail: &str) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(root) else { return Vec::new() };
    dir.flatten().map(|e| e.path().join(tail)).filter(|p| p.is_file()).collect()
}

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
}

/// Unreal Engine `GameUserSettings.ini` (VALORANT, Fortnite).
pub struct UnrealIni(pub Game);

impl ConfigPatcher for UnrealIni {
    fn locate(&self) -> Vec<PathBuf> {
        let local = env_dir("LOCALAPPDATA");
        match self.0 {
            // %LOCALAPPDATA%\VALORANT\Saved\Config\<account-id>\Windows\GameUserSettings.ini
            Game::Valorant => per_account(local.join(r"VALORANT\Saved\Config"), r"Windows\GameUserSettings.ini"),
            _ => {
                let p = local.join(r"FortniteGame\Saved\Config\WindowsClient\GameUserSettings.ini");
                if p.is_file() { vec![p] } else { Vec::new() }
            }
        }
    }

    fn keys(&self, r: Resolution) -> Vec<(&'static str, String)> {
        let (w, h) = (r.width.to_string(), r.height.to_string());
        let mut keys = vec![
            ("ResolutionSizeX", w.clone()),
            ("ResolutionSizeY", h.clone()),
            ("LastUserConfirmedResolutionSizeX", w.clone()),
            ("LastUserConfirmedResolutionSizeY", h.clone()),
            ("DesiredScreenWidth", w.clone()),
            ("DesiredScreenHeight", h.clone()),
            ("LastUserConfirmedDesiredScreenWidth", w),
            ("LastUserConfirmedDesiredScreenHeight", h),
            ("FullscreenMode", "0".into()),
            ("LastConfirmedFullscreenMode", "0".into()),
            ("PreferredFullscreenMode", "0".into()),
        ];
        if self.0 == Game::Valorant {
            keys.extend([("bShouldLetterbox", "False".into()), ("bLastConfirmedShouldLetterbox", "False".into())]);
        }
        keys
    }

    fn parse(&self, line: &str) -> Option<(String, String)> {
        line.split_once('=').map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
    }

    fn render(&self, key: &str, value: &str) -> String {
        format!("{key}={value}")
    }
}

/// Source / Source 2 style `"key" "value"` video config (CS2 `cs2_video.txt`, Apex `videoconfig.txt`).
pub struct SourceVideo(pub Game);

impl ConfigPatcher for SourceVideo {
    fn locate(&self) -> Vec<PathBuf> {
        if self.0 == Game::ApexLegends {
            let p = env_dir("USERPROFILE").join(r"Saved Games\Respawn\Apex\local\videoconfig.txt");
            return if p.is_file() { vec![p] } else { Vec::new() };
        }
        // <Steam>\userdata\<account>\730\local\cfg\cs2_video.txt
        steam_path().map_or_else(Vec::new, |s| per_account(s.join("userdata"), r"730\local\cfg\cs2_video.txt"))
    }

    fn keys(&self, r: Resolution) -> Vec<(&'static str, String)> {
        let mut keys = vec![
            ("setting.defaultres", r.width.to_string()),
            ("setting.defaultresheight", r.height.to_string()),
            ("setting.fullscreen", "1".into()),
            ("setting.nowindowborder", "0".into()),
        ];
        if self.0 == Game::Cs2 {
            // 0 = 4:3, 1 = 16:9, 2 = 16:10
            keys.push(("setting.aspectratiomode", if r.aspect() < 1.5 { "0" } else { "2" }.into()));
        }
        keys
    }

    fn parse(&self, line: &str) -> Option<(String, String)> {
        let mut parts = line.split('"').filter(|s| !s.trim().is_empty());
        Some((parts.next()?.trim().to_owned(), parts.next()?.trim().to_owned()))
    }

    fn render(&self, key: &str, value: &str) -> String {
        format!("\t\"{key}\"\t\t\"{value}\"")
    }
}

/// Steam install folder from `HKCU\Software\Valve\Steam\SteamPath`.
pub fn steam_path() -> Option<PathBuf> {
    let mut buf = [0u16; 520];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: buffer and size describe a valid, writable UTF-16 buffer.
    let status = unsafe {
        let data = Some(buf.as_mut_ptr().cast());
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Valve\Steam"),
            w!("SteamPath"),
            RRF_RT_REG_SZ,
            None,
            data,
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len]).replace('/', "\\")))
}
