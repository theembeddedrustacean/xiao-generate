//! Backend scaffolding. The two ESP backends are external binaries with no
//! stable library API, so we shell out (D1); the non-ESP backend (M4) has no
//! external generator at all and writes the (small, fully-specified) bare-metal
//! Cortex-M file set directly from registry data.

use anyhow::Result;

use crate::model::Config;
use crate::registry::Backend;

pub mod cargogen;
pub mod espgen;
pub mod nonesp;

/// Produce the vanilla backend scaffold for `cfg` at `cfg.out_dir`. The backend
/// is a function of the resolved `(variant, stack)` (D6), not the variant alone.
pub fn scaffold(cfg: &Config) -> Result<()> {
    match cfg.stack.backend {
        Backend::EspGenerate => espgen::scaffold(cfg),
        Backend::CargoGenerate => cargogen::scaffold(cfg),
        Backend::NonEspScaffold => nonesp::scaffold(cfg),
    }
}
