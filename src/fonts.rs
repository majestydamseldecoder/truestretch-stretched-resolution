//! Runtime font discovery. SF Pro is used when the user has it installed; otherwise Segoe UI
//! Variable, otherwise egui's built-in fonts. No font file is ever bundled with the binary.

use eframe::egui::{FontData, FontDefinitions, FontFamily};
use std::path::PathBuf;
use std::sync::Arc;

/// Wide, heavy family used for the big resolution numerals and headings.
pub const DISPLAY: &str = "display";

const UI_CANDIDATES: &[&str] = &["SF-Pro-Text-Regular.otf", "SF-Pro.ttf", "SegUIVar.ttf", "segoeui.ttf"];
const DISPLAY_CANDIDATES: &[&str] =
    &["SF-Pro-Display-Heavy.otf", "SF-Pro-Display-Bold.otf", "SF-Pro.ttf", "SegUIVar.ttf", "segoeuib.ttf"];
const MONO_CANDIDATES: &[&str] = &["SF-Mono-Medium.otf", "SF-Mono-Regular.otf", "CascadiaMono.ttf", "consola.ttf"];

/// System-wide and per-user font folders, in lookup order.
fn font_dirs() -> Vec<PathBuf> {
    let system = std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts"));
    let user = std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join(r"Microsoft\Windows\Fonts"));
    system.into_iter().chain(user).collect()
}

/// Reads the first candidate that exists in any font folder.
fn load_first(dirs: &[PathBuf], candidates: &[&str]) -> Option<(String, Vec<u8>)> {
    candidates.iter().find_map(|name| {
        dirs.iter().find_map(|dir| std::fs::read(dir.join(name)).ok()).map(|bytes| ((*name).to_owned(), bytes))
    })
}

/// Builds font definitions with `Proportional`, `Monospace` and the named [`DISPLAY`] family.
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let dirs = font_dirs();
    let mut display: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let insert = |defs: &mut FontDefinitions, candidates: &[&str], family: Option<FontFamily>| {
        let (name, bytes) = load_first(&dirs, candidates)?;
        defs.font_data.entry(name.clone()).or_insert_with(|| Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = family {
            defs.families.entry(family).or_default().insert(0, name.clone());
        }
        Some(name)
    };
    insert(&mut defs, UI_CANDIDATES, Some(FontFamily::Proportional));
    insert(&mut defs, MONO_CANDIDATES, Some(FontFamily::Monospace));
    if let Some(name) = insert(&mut defs, DISPLAY_CANDIDATES, None) {
        display.insert(0, name);
    }
    defs.families.insert(FontFamily::Name(DISPLAY.into()), display);
    defs
}
