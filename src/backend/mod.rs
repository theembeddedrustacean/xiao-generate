//! Backend scaffolding. The two ESP backends are external binaries with no
//! stable library API, so we shell out (D1); the non-ESP backend (M4) has no
//! external generator at all and writes the (small, fully-specified) bare-metal
//! Cortex-M file set directly from registry data.

use anyhow::{Context, Result};

use crate::model::Config;
use crate::registry::Backend;
use crate::templates;

pub mod cargogen;
pub mod espgen;
pub mod nonesp;

/// Produce the vanilla backend scaffold for `cfg` at `cfg.out_dir`. The backend
/// is a function of the resolved `(variant, stack)` (D6), not the variant alone.
pub fn scaffold(cfg: &Config) -> Result<()> {
    match cfg.stack.backend {
        Backend::EspGenerate => espgen::scaffold(cfg)?,
        Backend::CargoGenerate => cargogen::scaffold(cfg)?,
        Backend::NonEspScaffold => nonesp::scaffold(cfg)?,
    }
    write_editor_config(cfg)?;
    Ok(())
}

/// Emit the rust-analyzer settings that keep a spurious `E0463: can't find crate
/// for test` out of the user's editor, into a freshly scaffolded project.
///
/// This lives in the dispatcher rather than in any one backend because the
/// problem it solves is universal: rust-analyzer's default `--all-targets` makes
/// it build the test target, which needs the `test` crate, which **none** of the
/// three stacks we emit has.
///
/// - non-ESP Cortex-M and ESP `no_std`: bare-metal targets have no `test` crate.
/// - ESP `std` (esp-idf): looks hosted, but the template's `.cargo/config.toml`
///   sets `build-std = ["std", "panic_abort"]`, and `test` is not in that list —
///   so it fails the same way. Verified against a generated project.
///
/// Neither generator does this for us: `esp-idf-template` writes no editor config
/// at all, and while `esp-generate` has its own `-o vscode|helix|neovim|zed`
/// options we deliberately do **not** pass them through (see below).
///
/// ## What gets written
///
/// With `--editor none` — nothing at all.
///
/// Otherwise `rust-analyzer.toml` (the editor-neutral file) plus the selected
/// editor's own *client*-config file. Both halves are needed: `rust-analyzer.toml`
/// is the correct long-term home and is parsed by rust-analyzer, but as of 1.97.1
/// it does not by itself suppress this diagnostic — the same keys delivered as
/// **client** configuration do. So the per-editor file is what actually fixes the
/// error, and the neutral file documents the intent for tooling that reads it.
///
/// ## Why our own files, not esp-generate's `-o vscode|helix|neovim|zed`
///
/// Passing through would fix the diagnostic on exactly one of the three backends:
/// `cargo-generate`/`esp-idf-template` and the non-ESP direct scaffold have no such
/// option, so they would still need our files, and the same editor would then be
/// configured by two different mechanisms depending on the chip the user picked.
/// Emitting our own everywhere keeps one code path, one file format per editor, and
/// one set of tests. The `-o` editor flags therefore never enter `Config::esp_options`
/// and esp-generate never writes editor files, which is also why the guard below can
/// never fire in practice — it is kept only so a backend that *does* start writing
/// its own config wins over ours rather than being silently overwritten.
fn write_editor_config(cfg: &Config) -> Result<()> {
    let out = &cfg.out_dir;

    // `rust-analyzer.toml` is written UNCONDITIONALLY, `--editor none` included.
    //
    // It is not an editor's config file — it is a project file that rust-analyzer
    // reads itself, so it belongs to the generated project rather than to whoever
    // is editing it. Since `none` is the default, gating this on the editor choice
    // would mean the typical project ships no rust-analyzer config at all, which
    // is the E0463-on-every-project problem this exists to solve.
    let ratoml = out.join("rust-analyzer.toml");
    std::fs::write(&ratoml, templates::rust_analyzer_toml())
        .with_context(|| format!("writing {}", ratoml.display()))?;

    // `--editor none`: no editor-SPECIFIC client config beyond that.
    let Some(editor_path) = cfg.editor.path else {
        return Ok(());
    };

    let path = out.join(editor_path);
    // Never clobber a file a backend already produced.
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let body = templates::editor_config(cfg.editor.id).ok_or_else(|| {
        // Unreachable: the catalog and the template table are pinned together by
        // `every_file_emitting_editor_has_a_config_body` in `templates`.
        anyhow::anyhow!("internal error: no editor config body for '{}'", cfg.editor.id)
    })?;
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
