//! Display mode enumeration and switching through GDI (`EnumDisplayDevicesW`,
//! `EnumDisplaySettingsW`, `ChangeDisplaySettingsExW`). Stretched modes are applied with
//! `CDS_FULLSCREEN`, so they are temporary: the registry keeps the native desktop mode and
//! reverting simply restores it.

use crate::settings::Resolution;
use std::fmt;
use windows::Win32::Graphics::Gdi::*;
use windows::core::PCWSTR;

/// One display mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
}

impl Mode {
    pub fn resolution(self) -> Resolution {
        Resolution::new(self.width, self.height)
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\u{00D7}{} @ {} Hz", self.width, self.height, self.hz)
    }
}

/// GPU vendor, derived from the PCI vendor id (`VEN_xxxx`) in the adapter's device id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GpuVendor {
    fn from_device_id(id: &str) -> Self {
        let id = id.to_ascii_uppercase();
        let ids = [("VEN_10DE", Self::Nvidia), ("VEN_1002", Self::Amd), ("VEN_8086", Self::Intel)];
        ids.iter().find(|(v, _)| id.contains(v)).map_or(Self::Other, |(_, vendor)| *vendor)
    }

    /// Where the full-screen scaling switch lives in this vendor's control panel.
    pub fn scaling_steps(self) -> &'static [&'static str] {
        match self {
            Self::Nvidia => &[
                "NVIDIA Control Panel > Display > Adjust desktop size and position",
                "Scaling mode: Full-screen, Perform scaling on: GPU",
                "Tick 'Override the scaling mode set by games and programs'",
            ],
            Self::Amd => {
                &["AMD Software: Adrenalin Edition > Gaming > Display", "GPU Scaling: On, Scaling Mode: Full panel"]
            }
            Self::Intel => {
                &["Intel Graphics Command Center > Display > General", "Scale: Stretch, Override application settings"]
            }
            Self::Other => &["Set display scaling to 'Stretch' or 'Full screen' in your driver panel"],
        }
    }
}

/// An attached monitor and the adapter driving it.
#[derive(Clone, Debug)]
pub struct Monitor {
    /// GDI device name, e.g. `\\.\DISPLAY1`.
    pub device: String,
    pub adapter: String,
    pub monitor_name: String,
    pub vendor: GpuVendor,
    pub primary: bool,
    pub current: Option<Mode>,
    /// The mode stored in the registry: the desktop resolution TrueStretch always returns to.
    pub native: Option<Mode>,
}

/// A failed mode change, carrying the `DISP_CHANGE_*` code.
#[derive(Debug, Clone)]
pub struct DisplayError(pub i32);

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.0 {
            1 => "a restart is required for this mode",
            -1 => "the driver failed to set the mode",
            -2 => "mode not supported; create it as a custom resolution in your GPU panel",
            _ => "invalid flags, parameters or DualView setup",
        };
        write!(f, "display change failed ({}): {what}", self.0)
    }
}

impl std::error::Error for DisplayError {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

fn query(device: &str, which: ENUM_DISPLAY_SETTINGS_MODE) -> Option<DEVMODEW> {
    let name = wide(device);
    let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    // SAFETY: `name` is NUL terminated and `dm` has `dmSize` set.
    unsafe { EnumDisplaySettingsW(PCWSTR(name.as_ptr()), which, &mut dm) }.as_bool().then_some(dm)
}

fn to_mode(dm: DEVMODEW) -> Mode {
    Mode { width: dm.dmPelsWidth, height: dm.dmPelsHeight, hz: dm.dmDisplayFrequency }
}

/// Fills `out` with display device `index` of `parent` (adapters when `parent` is `None`).
fn device_at(parent: Option<&str>, index: u32, out: &mut DISPLAY_DEVICEW) -> bool {
    *out = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
    let name = parent.map(wide);
    let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    // SAFETY: `out.cb` is set and `name` is either null or a NUL terminated string.
    unsafe { EnumDisplayDevicesW(name, index, out, 0) }.as_bool()
}

/// Lists every monitor attached to the desktop.
pub fn monitors() -> Vec<Monitor> {
    let (mut adapter, mut mon, mut out) = (DISPLAY_DEVICEW::default(), DISPLAY_DEVICEW::default(), Vec::new());
    for index in 0.. {
        if !device_at(None, index, &mut adapter) {
            break;
        }
        if (adapter.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP).0 == 0 {
            continue;
        }
        let device = from_wide(&adapter.DeviceName);
        let has_monitor = device_at(Some(&device), 0, &mut mon);
        out.push(Monitor {
            current: query(&device, ENUM_CURRENT_SETTINGS).map(to_mode),
            native: query(&device, ENUM_REGISTRY_SETTINGS).map(to_mode),
            adapter: from_wide(&adapter.DeviceString),
            monitor_name: if has_monitor { from_wide(&mon.DeviceString) } else { "Unknown monitor".into() },
            vendor: GpuVendor::from_device_id(&from_wide(&adapter.DeviceID)),
            primary: (adapter.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE).0 != 0,
            device,
        });
    }
    out
}

/// Every distinct 24/32-bit mode the driver exposes for `device`, sorted ascending.
pub fn modes(device: &str) -> Vec<Mode> {
    let mut list: Vec<Mode> = (0u32..)
        .map_while(|i| query(device, ENUM_DISPLAY_SETTINGS_MODE(i)))
        .filter(|dm| dm.dmBitsPerPel >= 24)
        .map(to_mode)
        .collect();
    list.sort();
    list.dedup();
    list
}

/// The highest-refresh mode matching `res`, if the driver exposes it.
pub fn pick(modes: &[Mode], res: Resolution) -> Option<Mode> {
    modes.iter().copied().filter(|m| m.resolution() == res).max_by_key(|m| m.hz)
}

fn change(device: &str, dm: Option<&DEVMODEW>, flags: CDS_TYPE) -> Result<(), DisplayError> {
    let name = wide(device);
    let dm = dm.map(|d| d as *const DEVMODEW);
    // SAFETY: `name` is NUL terminated; `dm` (if any) points to a fully initialised DEVMODEW.
    let code = unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), dm, None, flags, None) };
    if code == DISP_CHANGE_SUCCESSFUL { Ok(()) } else { Err(DisplayError(code.0)) }
}

/// Validates `mode` with `CDS_TEST`, then applies it temporarily with `CDS_FULLSCREEN`.
/// With `gpu_stretch` the driver is asked to fill the panel (`DMDFO_STRETCH`).
pub fn apply(device: &str, mode: Mode, gpu_stretch: bool) -> Result<(), DisplayError> {
    let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    (dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency) = (mode.width, mode.height, mode.hz);
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    if gpu_stretch {
        let fixed = DEVMODEW_0_1 { dmDisplayFixedOutput: DMDFO_STRETCH, ..Default::default() };
        dm.Anonymous1 = DEVMODEW_0 { Anonymous2: fixed };
        dm.dmFields |= DM_DISPLAYFIXEDOUTPUT;
        // Some drivers reject the fixed-output hint; retry without it rather than fail.
        if change(device, Some(&dm), CDS_TEST).is_err() {
            dm.dmFields &= !DM_DISPLAYFIXEDOUTPUT;
        }
    }
    change(device, Some(&dm), CDS_TEST)?;
    change(device, Some(&dm), CDS_FULLSCREEN)
}

/// Restores the registry (desktop) mode for `device`.
pub fn revert(device: &str) -> Result<(), DisplayError> {
    change(device, None, CDS_TYPE(0))
}
