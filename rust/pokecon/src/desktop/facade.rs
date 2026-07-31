//! Private facade for the canonical desktop shell compiled by the compatibility crate.

pub(crate) use pokecon_desktop::{CloseBehavior, DesktopError, DesktopRuntimeSettings};
#[cfg(feature = "tauri-shell")]
pub(crate) use pokecon_desktop::{DesktopLifecycle, DesktopShellConfig, run_tauri_shell};
