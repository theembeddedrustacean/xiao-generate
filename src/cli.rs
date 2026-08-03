//! clap CLI definition. Every choice here is a plain flag so generation is
//! fully scriptable/CI-able (R2). The future TUI (M2) will populate the same
//! `model::RawConfig` from interactive screens.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "xiao-generate",
    version,
    about = "Generate ready-to-build Rust projects for Seeed XIAO modules.",
    long_about = "Spawns a buildable project for a XIAO module with the recommended crate stack, \
                  onboard-component driver templates, and optional uFerris BSP mode.\n\n\
                  Two modes:\n  \
                  - Interactive TUI: the default when stdout is a terminal, or forced with --tui. \
                  Pick everything on one screen. REQUIRES --name <name> (the crate name is a CLI \
                  concern, not a TUI field).\n  \
                  - Headless: forced with --headless, or implied whenever --chip is given. \
                  Generates straight from flags with NO terminal UI. --chip is required (there is \
                  no default device); every other unspecified parameter falls back to a registry \
                  default (variant's default stack, base template, espflash, no logging/framework, \
                  name derived from the variant), so `--headless --chip <chip>` alone produces a \
                  working project.\n\n\
                  ESP variants support a no_std esp-hal stack (default) and a std esp-idf stack \
                  (--stack esp-idf / --std).\n\n\
                  Every generated project ships a project-level rust-analyzer.toml. Editor-specific \
                  client config is opt-in via --editor (vscode/helix/neovim/zed); the default is \
                  none, which writes no editor-specific files."
)]
pub struct Cli {
    /// XIAO module variant, e.g. `xiao-esp32s3-sense` (see --list-chips).
    #[arg(long)]
    pub chip: Option<String>,

    /// Stack to target, e.g. `esp-hal` (no_std, default) or `esp-idf` (std).
    /// See --list-stacks. Defaults to the variant's default stack.
    #[arg(long)]
    pub stack: Option<String>,

    /// Shorthand for `--stack esp-idf` (the ESP std / ESP-IDF stack).
    #[arg(long)]
    pub std: bool,

    /// Template to generate (see --list-templates). Defaults to the variant's base template.
    #[arg(long, short = 't')]
    pub template: Option<String>,

    /// Project (crate) name. Defaults to `<variant>-<template>`.
    #[arg(long)]
    pub name: Option<String>,

    /// Output directory (defaults to ./<name>). Its final component is the crate name.
    #[arg(long, short = 'o')]
    pub out: Option<PathBuf>,

    /// Add uFerris BSP support to the project (ESP no_std / esp-hal stack only).
    #[arg(long)]
    pub uferris: bool,

    /// Enable the uFerris Megalops power board (implies --uferris).
    #[arg(long)]
    pub power_board: bool,

    /// Flashing tool. Each device defaults to its no-probe USB route — espflash
    /// (ESP), picotool (RP BOOTSEL), or the UF2 bootloader (nRF/SAMD21) — with
    /// `probe-rs` the external-SWD-debug-probe alternative everywhere. Not
    /// user-selectable on the esp-idf stack (the template fixes it).
    #[arg(long, value_name = "TOOL")]
    pub flashing: Option<String>,

    /// Logging frontend: `none` (default) or `defmt` on every no_std stack; the
    /// esp-hal stack additionally offers `log` (esp-println, ESP-only, and
    /// unavailable with `--flashing probe-rs`). On the Cortex-M boards defmt is
    /// wired for RTT — viewing its output needs a debug probe.
    #[arg(long, value_name = "FRONTEND")]
    pub logging: Option<String>,

    /// Framework for the esp-hal (no_std) base project: `none` (default,
    /// bare-metal) or `embassy`. (RTIC is a Cortex-M framework — not on ESP.)
    #[arg(long, value_name = "FRAMEWORK")]
    pub framework: Option<String>,

    /// Keep µFerris async (embassy) support on. This is the default and the only
    /// verified µFerris path; the flag is accepted for explicitness.
    #[arg(long = "async")]
    pub async_on: bool,

    /// Disable µFerris async support. Documented gap — refused (no verified
    /// non-async µFerris body); rejected at config time.
    #[arg(long = "no-async", conflicts_with = "async_on")]
    pub no_async: bool,

    /// Bundle an extra crate into the generated project's [dependencies].
    /// Repeatable; each value is `name` or `name@version`
    /// (e.g. `--add-crate heapless --add-crate serde@1.0`). Applies to all stacks.
    #[arg(long = "add-crate", value_name = "SPEC")]
    pub add_crate: Vec<String>,

    /// Editor to emit rust-analyzer client config for: `none` (default, no
    /// editor-specific files), `vscode`, `helix`, `neovim`, or `zed`. Every
    /// project ships a project-level rust-analyzer.toml regardless. Applies to
    /// all stacks.
    #[arg(long, value_name = "EDITOR")]
    pub editor: Option<String>,

    /// Force the interactive Ratatui TUI (otherwise it launches automatically
    /// when no --chip/--headless/--list-* flag is given and stdout is a
    /// terminal). Requires --name.
    #[arg(long)]
    pub tui: bool,

    /// Force headless (non-interactive) generation straight from flags — no
    /// terminal UI. --chip is required (no default device); every other
    /// unspecified parameter falls back to a registry default, so
    /// `--headless --chip <chip>` alone generates a working project. Passing
    /// --chip also implies headless.
    #[arg(long)]
    pub headless: bool,

    /// Report the host prerequisites a project for this configuration needs —
    /// which are installed, and the exact command for any that are not — then
    /// exit WITHOUT generating anything. Same report the summary prints after a
    /// real generation. Requires --chip; honours every other flag, so
    /// `--check --chip xiao-esp32c6 --flashing probe-rs` reports what THAT
    /// project would need. Exits 1 if anything is missing.
    #[arg(long)]
    pub check: bool,

    /// List supported module variants and exit.
    #[arg(long)]
    pub list_chips: bool,

    /// List available templates (optionally filtered by --chip) and exit.
    #[arg(long)]
    pub list_templates: bool,

    /// List supported stacks (optionally filtered by --chip) and exit.
    #[arg(long)]
    pub list_stacks: bool,

    /// Install a missing ESP backend generator (esp-generate / cargo-generate)
    /// without prompting. By default xiao-generate asks first; this answers yes.
    /// Only the ESP stacks need a backend — non-ESP boards ignore it.
    #[arg(long = "yes", short = 'y', conflicts_with = "no_install")]
    pub yes: bool,

    /// Never auto-install a missing ESP backend generator — print the manual
    /// `cargo install` command and stop instead.
    #[arg(long = "no-install")]
    pub no_install: bool,
}
