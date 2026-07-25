//! esp-generate backend (P0). Drives a **pinned** esp-generate fully headlessly
//! (the user never sees its TUI). Option sets come from the registry and are
//! pre-validated, so esp-generate's own constraint checks should never trip —
//! but if they do, we surface its stderr verbatim.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::model::Config;
use crate::registry::{self, ESP_GENERATE_VERSION};

/// Check esp-generate is installed and report its version. Missing tool → a
/// clear, actionable error (headless: we fail with instructions per the plan).
pub fn preflight() -> Result<String> {
    let out = Command::new("esp-generate")
        .arg("--version")
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run `esp-generate` ({e}). Install the pinned version with:\n    \
                 cargo install esp-generate --version {ESP_GENERATE_VERSION} --locked"
            )
        })?;
    if !out.status.success() {
        bail!("`esp-generate --version` failed");
    }
    // "esp-generate 1.3.0"
    let ver = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();
    Ok(ver)
}

pub fn scaffold(cfg: &Config) -> Result<()> {
    let installed = preflight()?;
    if installed != ESP_GENERATE_VERSION {
        // Not fatal, but the snapshots/pins are validated against the pin.
        eprintln!(
            "warning: esp-generate {installed} is installed but xiao-generate is pinned to \
             {ESP_GENERATE_VERSION}; generated output may differ from the validated pin.\n         \
             install the pin with: cargo install esp-generate --version {ESP_GENERATE_VERSION} --locked"
        );
    }

    // Defense in depth: our registry option sets are pre-validated, but re-check
    // against the 1.3.0 constraint graph so we never shell out a combo esp-generate
    // would reject (D2). A failure here is an internal registry bug, not user error.
    let opts = cfg.esp_options();
    registry::validate_esp_options(&opts)
        .map_err(|e| anyhow::anyhow!("internal error: invalid esp-generate option set {opts:?}: {e}"))?;

    let out_dir = &cfg.out_dir;
    let name = out_dir
        .file_name()
        .and_then(|s| s.to_str())
        .context("output directory has no final component to use as the project name")?;
    let parent: &Path = out_dir.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating parent directory {}", parent.display()))?;

    let mut cmd = Command::new("esp-generate");
    cmd.arg("--headless")
        .arg("--skip-update-check")
        .arg("--chip")
        .arg(cfg.variant.chip);
    for opt in cfg.esp_options() {
        cmd.arg("-o").arg(opt);
    }
    cmd.arg("-O").arg(parent).arg(name);

    let output = cmd
        .output()
        .context("failed to spawn esp-generate")?;
    if !output.status.success() {
        bail!(
            "esp-generate failed (chip {}, options {:?}):\n{}",
            cfg.variant.chip,
            cfg.esp_options(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
