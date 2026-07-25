//! cargo-generate backend (M5, ESP `std`/`esp-idf` stack). Drives a **pinned**
//! `esp-rs/esp-idf-template` (the `cargo` sub-template) fully non-interactively
//! via `cargo generate -d …` (D1/D7). The template ref is pinned by commit SHA
//! (the repo has no releases/tags — see research/esp-idf-template.md); the
//! `mcu`/`espidfver` placeholders come from the registry (D2).
//!
//! Verification is generation-only (R9): we never `cargo check` the generated
//! std project — that would pull and build the full ESP-IDF SDK. We rely on
//! esp-idf-template to emit a buildable base and the user's ESP-IDF toolchain to
//! build it.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::model::Config;
use crate::registry::{
    CARGO_GENERATE_MIN_VERSION, ESP_IDF_TEMPLATE_GIT, ESP_IDF_TEMPLATE_REV, ESP_IDF_VERSION,
};

/// Check `cargo generate` is installed and report its version. Missing tool → a
/// clear, actionable error (headless: we fail with the install command).
pub fn preflight() -> Result<String> {
    let out = Command::new("cargo")
        .args(["generate", "--version"])
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run `cargo generate` ({e}). Install it with:\n    \
                 cargo install cargo-generate"
            )
        })?;
    if !out.status.success() {
        bail!(
            "`cargo generate --version` failed — install cargo-generate (>= {CARGO_GENERATE_MIN_VERSION}) with:\n    \
             cargo install cargo-generate"
        );
    }
    // "cargo generate-generate 0.22.0" → last whitespace token is the version.
    let ver = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_string();
    Ok(ver)
}

pub fn scaffold(cfg: &Config) -> Result<()> {
    let _installed = preflight()?;

    let out_dir = &cfg.out_dir;
    let name = out_dir
        .file_name()
        .and_then(|s| s.to_str())
        .context("output directory has no final component to use as the project name")?;
    // cargo-generate creates <destination>/<name>, so the destination is the
    // parent of out_dir (mirrors espgen's -O parent handling).
    let parent: &Path = out_dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating parent directory {}", parent.display()))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("generate")
        .arg("--git")
        .arg(ESP_IDF_TEMPLATE_GIT)
        .arg("--rev")
        .arg(ESP_IDF_TEMPLATE_REV)
        .arg("cargo")
        .arg("--name")
        .arg(name)
        .arg("--define")
        .arg(format!("mcu={}", cfg.variant.chip))
        // `advanced=true` unlocks espidfver; we pin it explicitly (D2/R5).
        .arg("--define")
        .arg("advanced=true")
        .arg("--define")
        .arg(format!("espidfver={ESP_IDF_VERSION}"))
        .arg("--silent")
        .arg("--vcs")
        .arg("none")
        .arg("--destination")
        .arg(parent);

    let output = cmd.output().context("failed to spawn `cargo generate`")?;
    if !output.status.success() {
        bail!(
            "cargo generate failed (mcu {}, template {}@{}):\n{}",
            cfg.variant.chip,
            ESP_IDF_TEMPLATE_GIT,
            ESP_IDF_TEMPLATE_REV,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
