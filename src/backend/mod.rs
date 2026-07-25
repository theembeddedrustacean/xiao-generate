//! Backend scaffolding. Backends are external binaries with no stable library
//! API, so we shell out (D1). M1 implements the esp-generate backend; the
//! battery-pack backend arrives at M4 (config-time validation already rejects
//! non-ESP variants before we get here).

use anyhow::{bail, Result};

use crate::model::Config;
use crate::registry::Backend;

pub mod cargogen;
pub mod espgen;

/// Produce the vanilla backend scaffold for `cfg` at `cfg.out_dir`. The backend
/// is a function of the resolved `(variant, stack)` (D6), not the variant alone.
pub fn scaffold(cfg: &Config) -> Result<()> {
    match cfg.stack.backend {
        Backend::EspGenerate => espgen::scaffold(cfg),
        Backend::CargoGenerate => cargogen::scaffold(cfg),
        Backend::BatteryPack => bail!(
            "the embedded-battery-pack backend is not implemented until M4 (variant '{}')",
            cfg.variant.id
        ),
    }
}
