//! Static module registry — the single source of truth for everything
//! variant-specific (R4). No variant logic lives in the patch/backend code;
//! it all reads from here. Kept deliberately declarative so that "add a XIAO
//! variant" or "bump a driver crate" is a data-only, PR-shaped change (see
//! CONTRIBUTING.md at M3).
//!
//! Provenance for every value in this file is recorded in `research/*.md`
//! (Seeed wiki pin maps, driver-crate versions, esp-generate 1.3.0 option
//! audit, uferris-bsp 0.1.3 manifest). Unverified values are NOT invented —
//! they are left `None` and the affected template is gated or omitted.

/// Pinned backend/tool versions for this xiao-generate release (R5).
pub const ESP_GENERATE_VERSION: &str = "1.3.0";
pub const UFERRIS_BSP_VERSION: &str = "0.1.3";

// --- ESP `std` stack (M5): cargo-generate + esp-idf-template pins (R5) ---
// The esp-idf-template repo has no releases/tags (re-verified 2026-07-29:
// `git ls-remote --tags` is empty), so the pin is a commit SHA
// (research/esp-idf-template.md). cargo-generate itself is pinned only by a
// minimum (the template's `cargo_generate_version` requirement); the
// reproducibility-critical pin is this revision.
//
// NOTE: this SHA is deliberately NOT required to be the branch tip, and must
// never be "refreshed" to chase upstream. `cargo generate --git … --rev` cannot
// resolve a non-tip revision (it shallow-clones), so `backend::cargogen` fetches
// this exact commit itself and expands it with `--path`. See the rationale on
// `cargogen::fetch_revision`.
pub const ESP_IDF_TEMPLATE_GIT: &str = "https://github.com/esp-rs/esp-idf-template";
pub const ESP_IDF_TEMPLATE_REV: &str = "6f092f06c8cef93a41b82b4d5c7373446e9fcf1b";
pub const CARGO_GENERATE_MIN_VERSION: &str = "0.17.4";
/// ESP-IDF SDK version baked into the generated project (`espidfver` placeholder).
pub const ESP_IDF_VERSION: &str = "v5.5.3";
/// XIAO's esp-idf-hal added on top of the template's esp-idf-svc (std base).
pub const ESP_IDF_HAL_VERSION: &str = "0.46.2";

// --- Non-ESP HAL / runtime pins (M4) ---------------------------------------
// Every version below was COMPILE-verified for the (variant, stack, framework)
// cells it is used by — see research/hal-framework-matrix.md (the HAL×framework
// matrix) and research/m4-impl-plan.md. They are deliberately NOT
// "crates.io-latest": e.g. rp235x-hal is pinned to the verified 0.3.0, not
// 0.4.0, and SAMD21 goes through the `xiao_m0` BSP (which itself resolves
// atsamd-hal 0.17) because that is the path with a verified XIAO LED body.
pub const RP2040_HAL_VERSION: &str = "0.12";
pub const RP235X_HAL_VERSION: &str = "0.3.0";
pub const NRF52840_HAL_VERSION: &str = "0.19";
pub const XIAO_M0_BSP_VERSION: &str = "0.13.0";
pub const EMBASSY_RP_VERSION: &str = "0.10";
pub const EMBASSY_NRF_VERSION: &str = "0.11";
// The two framework pins live inside the literal TOML specs of
// `EMBASSY_FRAMEWORK_DEPS` (a `Dep.spec` is a `&'static str`, so it cannot be
// assembled from consts). They are declared here as the documented
// single-source-of-truth and tied to those specs by
// `embassy_framework_deps_use_the_pinned_versions`.
/// embassy-executor 0.10 renamed the Cortex-M arch feature to
/// `platform-cortex-m` (it was `arch-cortex-m` on older releases).
#[allow(dead_code)]
pub const EMBASSY_EXECUTOR_VERSION: &str = "0.10";
#[allow(dead_code)]
pub const EMBASSY_TIME_VERSION: &str = "0.5.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    RiscV,
    Xtensa,
    Arm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// ESP `no_std`/`esp-hal` stack — shell out to `esp-generate` (P0).
    EspGenerate,
    /// ESP `std`/`esp-idf` stack — shell out to `cargo generate` +
    /// `esp-idf-template` (M5).
    CargoGenerate,
    /// Non-ESP (Cortex-M) variants — **direct file scaffold** (M4). A bare-metal
    /// Cortex-M project has no irreducible bootstrap: it is `Cargo.toml` +
    /// `.cargo/config.toml` + (`memory.x` + `build.rs`) + `src/main.rs`, so we
    /// write it ourselves from [`Scaffold`] data instead of shelling out to a
    /// generator. Keeps the output XIAO-accurate (no wrong-board BSP) and adds
    /// no external tool to the non-ESP path.
    NonEspScaffold,
}

impl Backend {
    /// Both ESP backends target ESP chips; only the direct scaffold is non-ESP.
    pub fn is_esp(&self) -> bool {
        matches!(self, Backend::EspGenerate | Backend::CargoGenerate)
    }
}

/// A framework this stack *lists* (it is real for the device) but cannot
/// generate, with the compile-verified reason. Selecting it fails at config time
/// — the same "ship the gap, never a broken project" rule as [`Template::gap`].
#[derive(Debug, Clone, Copy)]
pub struct FrameworkGap {
    pub framework: &'static str,
    pub reason: &'static str,
}

/// Everything [`Backend::NonEspScaffold`] needs to write a base project for a
/// `(variant, stack)`. `None` on ESP stacks, whose base files come from the
/// backend generator. Every field is compile-verified data (R4) — the backend
/// contains no chip logic, it just renders this.
#[derive(Debug, Clone, Copy)]
pub struct Scaffold {
    /// The HAL crate the project is built on (`rp2040-hal`, `xiao_m0`, …).
    pub hal_crate: &'static str,
    /// Pinned version requirement for `hal_crate`.
    pub hal_version: &'static str,
    /// Cargo features always enabled on `hal_crate`. Empty → the dependency is
    /// emitted as a bare version string (default features).
    pub hal_features: &'static [&'static str],
    /// Extra `hal_crate` features enabled **only** when `framework = embassy` —
    /// the embassy-time driver (and friends). Verified fact: an embassy HAL
    /// without an executor must NOT enable its time-driver feature, or it drags
    /// `embassy-time` in for nothing. Empty on stacks that cannot host embassy.
    pub embassy_hal_features: &'static [&'static str],
    /// Fixed extra dependencies (core/runtime/panic handler).
    pub base_deps: &'static [Dep],
    /// `.cargo/config.toml` rustflags, already split into rustc argv items.
    pub rustflags: &'static [&'static str],
    /// Id of the `memory.x` body to ship (see `templates::memory_x`), or `None`
    /// when the HAL/BSP supplies its own (the `xiao_m0` BSP does).
    pub memory_x: Option<&'static str>,
    /// Extra `cargo:rustc-link-arg-bins=` values the generated `build.rs` emits.
    /// embassy-rp on RP2040 needs `-Tlink-rp.x` (its own boot2 script) this way.
    pub link_args: &'static [&'static str],
    /// Frameworks listed in [`Stack::frameworks`] that this stack cannot
    /// generate, with the reason.
    pub framework_gaps: &'static [FrameworkGap],
}

/// A **stack** — the (backend, target triple, HAL/runtime family) tuple a project
/// targets on a given variant (D6). The backend is a function of `(variant,
/// stack)`, not of the variant alone: the same XIAO ESP32-C3 has an `esp-hal`
/// (`no_std`, esp-generate) stack and an `esp-idf` (`std`, cargo-generate) stack,
/// with different targets and different template bodies. First in a variant's
/// `stacks` list is the default.
#[derive(Debug, Clone, Copy)]
pub struct Stack {
    pub id: &'static str,
    pub display: &'static str,
    pub backend: Backend,
    /// Rust target triple for this `(variant, stack)`.
    pub target: &'static str,
    /// Template ids available on this stack for the owning variant. For the ESP
    /// `std` stack this is the single synthetic `base` id (blink on variants with
    /// an LED, hello otherwise); component templates are follow-on work.
    pub templates: &'static [&'static str],
    /// Whether µFerris mode is offered on this stack. µFerris is a no_std/esp-hal
    /// BSP, so it is `true` only on the `esp-hal` stack (never on `esp-idf`).
    pub uferris: bool,
    /// Which entries of the universal [`FLASHING_TOOLS`] catalog this stack
    /// supports. First id = the stack's default. Empty = the whole flashing
    /// section is greyed for this stack (the template/backend owns it). This is
    /// device-agnostic support data — it never names a backend generator (R4).
    pub flashing: &'static [&'static str],
    /// Which entries of the universal [`LOGGING`] catalog this stack supports.
    pub logging: &'static [&'static str],
    /// Which entries of the universal [`FRAMEWORKS`] catalog this stack supports.
    pub frameworks: &'static [&'static str],
    /// Direct-scaffold data for the non-ESP backend (M4). `None` on ESP stacks.
    pub scaffold: Option<&'static Scaffold>,
    /// [`PREREQS`] ids needed to BUILD a project on this stack, **beyond** the
    /// target/toolchain — that one is always derived from [`Stack::target`],
    /// whose triple is itself a `PREREQS` id, so it is never repeated here.
    pub build_prereqs: &'static [&'static str],
    /// [`PREREQS`] ids needed to FLASH a project from this stack. Empty on the
    /// stacks where FLASHING is a live user choice (esp-hal): there the
    /// prerequisites come from the chosen [`ToolChoice`] instead, so the summary
    /// follows `--flashing`. Elsewhere the flash route is fixed by the board's
    /// bootloader (or by the template's cargo runner) and lives here.
    pub flash_prereqs: &'static [&'static str],
}

impl Stack {
    /// The ESP `std` stack (cargo-generate + esp-idf-template).
    pub fn is_std(&self) -> bool {
        matches!(self.backend, Backend::CargoGenerate)
    }

    /// Why `framework` cannot be generated on this stack, or `None` if it can.
    /// Data-driven from [`Scaffold::framework_gaps`]; ESP stacks have no gaps.
    pub fn framework_gap(&self, framework: &str) -> Option<&'static str> {
        self.scaffold?
            .framework_gaps
            .iter()
            .find(|g| g.framework == framework)
            .map(|g| g.reason)
    }

    /// The support list this stack declares for a universal toolchain category.
    /// First id (if any) is the stack's default; an empty list means the whole
    /// section is greyed (configured elsewhere).
    pub fn support(&self, cat: ToolCategory) -> &'static [&'static str] {
        match cat {
            ToolCategory::Flashing => self.flashing,
            ToolCategory::Logging => self.logging,
            ToolCategory::Framework => self.frameworks,
        }
    }

    /// The default (first-listed) supported choice for a category, or `None` when
    /// this stack lists no support of its own (section is template/backend-owned).
    pub fn default_tool(&self, cat: ToolCategory) -> Option<&'static ToolChoice> {
        self.support(cat).first().and_then(|id| tool_choice(cat, id))
    }
}

/// An onboard user LED (from Seeed docs; polarity `None` = undocumented).
#[derive(Debug, Clone, Copy)]
pub struct Led {
    pub gpio: u8,
    /// `Some(true)` = active-low, `Some(false)` = active-high, `None` = polarity
    /// not documented by Seeed (a blink still works — we never assert on/off).
    pub active_low: Option<bool>,
}

/// A Cargo dependency our patch layer adds on top of the backend scaffold.
/// `spec` is a valid TOML value literal (string or inline table).
#[derive(Debug, Clone, Copy)]
pub struct Dep {
    pub name: &'static str,
    pub spec: &'static str,
}

/// A generatable template (non-µFerris). µFerris templates are handled by the
/// µFerris mode path (see `patch::uferris`).
#[derive(Debug, Clone, Copy)]
pub struct Template {
    pub id: &'static str,
    pub display: &'static str,
    /// esp-generate `-o` options this template's scaffold requires. These sets
    /// are pre-validated against the 1.3.0 constraint graph (see `ESP_OPTIONS`)
    /// and compile-verified (see tests + research/upstream).
    pub esp_options: &'static [&'static str],
    /// Extra crates added to `Cargo.toml` after scaffolding.
    pub extra_deps: &'static [Dep],
    /// If true, the template body contains `{{LED_GPIO}}` to substitute from the
    /// variant's `led`.
    pub needs_led: bool,
    /// Component this template targets, for error messages ("no mic on …").
    pub component: &'static str,
    /// `Some(reason)` if this template is a known, documented gap for the pinned
    /// stack (D4 + non-goals: ship without the template, not a broken project).
    /// Selecting a gapped template fails at config time with `reason`.
    pub gap: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct Variant {
    pub id: &'static str,
    pub display: &'static str,
    pub priority: u8,
    /// esp-generate `--chip` / esp-idf-template `mcu` value (ESP), or the
    /// non-ESP chip family id (`rp2040`, `rp235x`, `nrf52840`, `atsamd21`) that
    /// selects the scaffold's base body. Shared across a variant's stacks.
    pub chip: &'static str,
    /// Instruction-set family. Registry truth about the part, kept as data even
    /// though no *rendering* path branches on it any more: prerequisites are
    /// derived from the target triple (which is a [`PREREQS`] id) rather than
    /// from the arch, so the print path needs no arch knowledge. The invariant
    /// tying the two together — Xtensa targets need espup, RISC-V/ARM ones are
    /// plain rustup targets — is asserted by
    /// `xtensa_targets_need_espup_and_others_do_not`.
    #[allow(dead_code)]
    pub arch: Arch,
    pub led: Option<Led>,
    /// Supported stacks (D6). First entry is the default. The backend + target +
    /// per-stack template list live here, not on the variant.
    pub stacks: &'static [Stack],
    /// `Some(feature)` if µFerris-eligible — the uferris-bsp chip feature to set.
    pub uferris_chip_feature: Option<&'static str>,
    /// `Some(reason)` if µFerris is eligible in principle but currently a
    /// documented gap for this variant against the pinned uferris-bsp (flips to
    /// `None` when the BSP fixes it). Selecting µFerris fails with `reason`.
    pub uferris_gap: Option<&'static str>,
    /// Driver/recommended crates this variant always seeds into `[dependencies]`
    /// (the "some XIAOs ship driver crates enabled by default" hook — M2 redesign
    /// v2). Threaded into the resolved crate list by the patch layer on top of the
    /// stack/template deps. `&[]` for every current variant (a data-only knob for
    /// later; no crates are invented here).
    pub default_crates: &'static [Dep],
}

impl Variant {
    /// Default stack (first listed).
    pub fn default_stack(&self) -> &'static Stack {
        let stacks: &'static [Stack] = self.stacks;
        &stacks[0]
    }
    /// Look up one of this variant's supported stacks by id.
    pub fn stack(&self, id: &str) -> Option<&'static Stack> {
        let stacks: &'static [Stack] = self.stacks;
        stacks.iter().find(|s| s.id == id)
    }
    /// A variant is ESP if its default stack uses an ESP backend.
    pub fn is_esp(&self) -> bool {
        self.default_stack().backend.is_esp()
    }
    pub fn uferris_eligible(&self) -> bool {
        self.uferris_chip_feature.is_some()
    }
}

// ---------------------------------------------------------------------------
// esp-generate 1.3.0 option constraint graph (research/esp-generate-options.md).
// This is an INTERNAL backend detail of the esp-hal stack — the esp-generate
// `-o` flag names below never appear in any user-facing UI or summary text (the
// universal catalogs above are what the user sees). Used to validate any option
// set before shell-out (D2) and is unit-tested.
// ---------------------------------------------------------------------------

pub struct EspOption {
    pub name: &'static str,
    /// options that must ALSO be present.
    pub requires: &'static [&'static str],
    /// options that must NOT be present (negative `!x` requires).
    pub conflicts: &'static [&'static str],
    /// mutual-exclusion group id (only one option per group).
    pub group: Option<&'static str>,
}

pub const ESP_OPTIONS: &[EspOption] = &[
    EspOption { name: "unstable-hal", requires: &[], conflicts: &[], group: None },
    EspOption { name: "alloc", requires: &[], conflicts: &[], group: None },
    EspOption { name: "wifi", requires: &["alloc", "unstable-hal"], conflicts: &[], group: None },
    EspOption { name: "ble-bleps", requires: &["alloc", "unstable-hal"], conflicts: &[], group: Some("ble-lib") },
    EspOption { name: "ble-trouble", requires: &["alloc", "unstable-hal", "embassy"], conflicts: &[], group: Some("ble-lib") },
    EspOption { name: "embassy", requires: &["unstable-hal"], conflicts: &[], group: None },
    EspOption { name: "stack-smashing-protection", requires: &[], conflicts: &[], group: None },
    EspOption { name: "probe-rs", requires: &[], conflicts: &["log", "esp-backtrace"], group: None },
    EspOption { name: "defmt", requires: &[], conflicts: &[], group: Some("log-frontend") },
    EspOption { name: "panic-rtt-target", requires: &["probe-rs"], conflicts: &[], group: Some("panic-handler") },
    EspOption { name: "embedded-test", requires: &["probe-rs"], conflicts: &[], group: None },
    EspOption { name: "log", requires: &[], conflicts: &["probe-rs"], group: Some("log-frontend") },
    EspOption { name: "esp-backtrace", requires: &[], conflicts: &["probe-rs"], group: Some("panic-handler") },
    EspOption { name: "wokwi", requires: &[], conflicts: &[], group: None },
    EspOption { name: "ci", requires: &[], conflicts: &[], group: None },
    EspOption { name: "helix", requires: &[], conflicts: &[], group: None },
    EspOption { name: "neovim", requires: &[], conflicts: &[], group: None },
    EspOption { name: "vscode", requires: &[], conflicts: &[], group: None },
    EspOption { name: "zed", requires: &[], conflicts: &[], group: None },
];

/// Validate an esp-generate option set against the constraint graph. Returns an
/// error string describing the first violation, mirroring esp-generate's own
/// rejection semantics so our pre-flight check matches the backend.
pub fn validate_esp_options(opts: &[&str]) -> Result<(), String> {
    let known = |o: &str| ESP_OPTIONS.iter().find(|e| e.name == o);
    for o in opts {
        let Some(e) = known(o) else {
            return Err(format!("unknown esp-generate option '{o}'"));
        };
        for r in e.requires {
            if !opts.contains(r) {
                return Err(format!("option '{}' requires '{}'", e.name, r));
            }
        }
        for c in e.conflicts {
            if opts.contains(c) {
                return Err(format!("options '{}' and '{}' cannot be enabled together", e.name, c));
            }
        }
    }
    // mutual exclusion groups
    for e in ESP_OPTIONS {
        let Some(g) = e.group else { continue };
        if opts.contains(&e.name) {
            for other in ESP_OPTIONS {
                if other.name != e.name && other.group == Some(g) && opts.contains(&other.name) {
                    return Err(format!(
                        "options '{}' and '{}' cannot be enabled together (group '{}')",
                        e.name, other.name, g
                    ));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Universal toolchain catalogs (M2 redesign v3 — UNIFY).
//
// xiao-generate is a *unifying* platform: Flashing / Logging / Framework are
// device-agnostic toolchain categories shown for EVERY device. The three
// canonical catalogs below are the whole universe of choices; each `Stack`
// declares which entries it supports (see `Stack::flashing/logging/frameworks`),
// and everything greys out from that support data — no per-variant UI logic.
//
// `esp_options` on each choice is the INTERNAL mapping to esp-generate `-o`
// flags for the esp-hal backend only. Those flag names are a backend detail and
// are NEVER shown in the UI/summary; they exist solely so `Config::esp_options`
// can drive the esp-generate shell-out. Choices with no esp mapping (rtic and
// the "none" defaults) carry `&[]`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct ToolChoice {
    pub id: &'static str,
    pub display: &'static str,
    /// esp-generate `-o` flags this choice maps to on the esp-hal backend
    /// (internal; never surfaced). `&[]` for "none"/unmapped choices.
    pub esp_options: &'static [&'static str],
    /// [`PREREQS`] ids this choice needs on the host. Populated for the flashing
    /// tools (picking `probe-rs` over `espflash` genuinely changes what must be
    /// installed); `&[]` for the logging/framework catalogs, whose choices are
    /// plain cargo dependencies and add no host tool.
    pub prereqs: &'static [&'static str],
}

/// Flashing tools (universal). espflash + probe-rs are the two flashing tools;
/// espflash is the esp-hal default, probe-rs the debug-probe path.
pub const FLASHING_TOOLS: &[ToolChoice] = &[
    ToolChoice { id: "espflash", display: "espflash", esp_options: &["esp-backtrace"], prereqs: &["espflash"] },
    ToolChoice { id: "probe-rs", display: "probe-rs", esp_options: &["probe-rs", "panic-rtt-target"], prereqs: &["probe-rs"] },
];

/// Logging frontends (universal). On esp-hal, `log` is mutually exclusive with a
/// probe-rs flashing tool (constraint graph) — that greying is derived, not
/// hand-coded.
pub const LOGGING: &[ToolChoice] = &[
    ToolChoice { id: "none", display: "none", esp_options: &[], prereqs: &[] },
    ToolChoice { id: "log", display: "log", esp_options: &["log"], prereqs: &[] },
    ToolChoice { id: "defmt", display: "defmt", esp_options: &["defmt"], prereqs: &[] },
];

/// Application frameworks (universal). embassy is the async framework (wired on
/// esp-hal, pulls `unstable-hal`); RTIC is the Cortex-M framework (no ESP support
/// — it greys on ESP; on the ARM parts it is a declared framework gap until an
/// RTIC body is compile-verified, see `Scaffold::framework_gaps`).
pub const FRAMEWORKS: &[ToolChoice] = &[
    ToolChoice { id: "none", display: "none", esp_options: &[], prereqs: &[] },
    ToolChoice { id: "embassy", display: "embassy", esp_options: &["embassy", "unstable-hal"], prereqs: &[] },
    ToolChoice { id: "rtic", display: "RTIC", esp_options: &[], prereqs: &[] },
];

/// The three universal toolchain categories, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Flashing,
    Logging,
    Framework,
}

impl ToolCategory {
    /// In display order.
    pub const ALL: [ToolCategory; 3] =
        [ToolCategory::Flashing, ToolCategory::Logging, ToolCategory::Framework];

    pub fn catalog(self) -> &'static [ToolChoice] {
        match self {
            ToolCategory::Flashing => FLASHING_TOOLS,
            ToolCategory::Logging => LOGGING,
            ToolCategory::Framework => FRAMEWORKS,
        }
    }

    /// Section heading shown in the Configure page.
    pub fn heading(self) -> &'static str {
        match self {
            ToolCategory::Flashing => "FLASHING TOOL",
            ToolCategory::Logging => "LOGGING",
            ToolCategory::Framework => "FRAMEWORK",
        }
    }

    /// CLI flag stem (`--flashing` / `--logging` / `--framework`) and group id.
    pub fn flag(self) -> &'static str {
        match self {
            ToolCategory::Flashing => "flashing",
            ToolCategory::Logging => "logging",
            ToolCategory::Framework => "framework",
        }
    }

    /// The universal default choice (index 0 of the catalog): espflash / none /
    /// none. Used when a stack lists no support of its own, and to decide which
    /// flags `equivalent_command` may omit.
    pub fn default_choice(self) -> &'static ToolChoice {
        &self.catalog()[0]
    }
}

/// Resolve a choice id within a universal catalog.
pub fn tool_choice(cat: ToolCategory, id: &str) -> Option<&'static ToolChoice> {
    cat.catalog().iter().find(|c| c.id == id)
}

// ---------------------------------------------------------------------------
// Universal EDITOR SUPPORT catalog.
//
// Every project xiao-generate emits is a bare-metal (or `build-std`) crate with
// no `test` crate, so rust-analyzer's default `--all-targets` makes the user's
// editor report a spurious `E0463: can't find crate for test` against a project
// that `cargo build` compiles perfectly. The fix is two settings —
// `cargo.allTargets = false` and `check.allTargets = false` — but the delivery
// path is EDITOR-SPECIFIC: a project-local `rust-analyzer.toml` is parsed by
// rust-analyzer yet does not drive flycheck's `--all-targets` (verified against
// 1.97.1), so each editor needs its own *client* configuration file.
//
// Unlike the flashing/logging/framework catalogs, this axis is backend-agnostic:
// the diagnostic, and its fix, are identical on esp-hal, esp-idf and the non-ESP
// scaffold, so EDITOR is live on EVERY stack and nothing here greys out.
//
// Single-select, exactly like the other universal catalogs: one editor per
// generated project, with `none` a real catalog entry (emit nothing) rather than
// a magic flag value. Index 0 is the default, matching `FLASHING_TOOLS`.
// ---------------------------------------------------------------------------

/// One entry of the universal editor catalog. `path` is where that editor reads
/// project-local config from — `None` for the `none` entry, which emits nothing.
/// The file body lives in `templates::editor_config`.
#[derive(Debug, Clone, Copy)]
pub struct Editor {
    pub id: &'static str,
    pub display: &'static str,
    /// Project-relative path of the file emitted for this editor, or `None` when
    /// the entry emits no file at all (`none`).
    pub path: Option<&'static str>,
    /// `Some(note)` when the emitted file is NOT picked up automatically and the
    /// user must opt in. Surfaced in the TUI hint + the summary rather than
    /// hidden, so we never imply a config is active when it needs a manual step.
    pub caveat: Option<&'static str>,
}

/// The universal editor catalog, in display order. **Index 0 is the default**
/// (VS Code — the delivery path empirically verified to clear the diagnostic in a
/// real editor, and what every generated project has always carried, so the
/// default must not regress it). Each `path`/schema was taken from that editor's
/// own documentation and is pinned by a unit test in `templates`.
pub const EDITORS: &[Editor] = &[
    Editor {
        id: "none",
        display: "none",
        path: None,
        caveat: None,
    },
    Editor {
        id: "vscode",
        display: "VS Code",
        path: Some(".vscode/settings.json"),
        caveat: None,
    },
    Editor {
        id: "helix",
        display: "Helix",
        path: Some(".helix/languages.toml"),
        caveat: None,
    },
    Editor {
        id: "zed",
        display: "Zed",
        path: Some(".zed/settings.json"),
        caveat: None,
    },
    Editor {
        id: "neovim",
        display: "Neovim",
        path: Some(".nvim.lua"),
        // Neovim deliberately does NOT source project-local config by default:
        // `.nvim.lua` is only read when the user has `vim.o.exrc = true`, and
        // even then the file must be trusted once (`:trust`). There is no
        // editor-native mechanism that works on a fresh clone, so we state the
        // opt-in instead of pretending the file is self-executing.
        caveat: Some("needs `vim.o.exrc = true` + `:trust` (Neovim has no automatic project-local config)"),
    },
];

/// Resolve an editor id.
pub fn editor(id: &str) -> Option<&'static Editor> {
    EDITORS.iter().find(|e| e.id == id)
}

/// The editor used when `--editor` is not given: catalog index 0.
///
/// That is `none` — writing editor files is opt-in, and `none` leads the catalog
/// so it reads the same way as every other universal option list, where the
/// do-nothing choice is first and selected until the user picks otherwise.
pub fn default_editor() -> &'static Editor {
    &EDITORS[0]
}

// ---------------------------------------------------------------------------
// HOST PREREQUISITES — what the *user* must already have installed for a
// generated project to BUILD and to FLASH.
//
// Registry data (R4), exactly like the toolchain catalogs above: the print path
// in `main::print_prerequisites` contains no chip/stack knowledge, it renders
// whatever `model::prerequisites` selects out of this catalog. Selection is
// derived from the resolved `Config` (target triple → toolchain, stack → build
// tools, flashing tool / stack → flash tools), so an `xiao-rp2040` project never
// mentions espup and an esp-idf project never mentions picotool.
//
// Scope note: these are the *generated project's* prerequisites, not
// xiao-generate's own. The backend generators (esp-generate, cargo-generate) are
// necessarily already installed by the time a summary prints — they are
// documented in the README instead.
// ---------------------------------------------------------------------------

/// How a prerequisite's presence is detected on the host. Every variant is cheap
/// (one short-lived process at most) and is only run for prerequisites the
/// resolved config actually needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Present iff this argv can be **spawned** — i.e. the executable resolves on
    /// `PATH`. The exit status is deliberately ignored: several of these tools
    /// exit non-zero for a version query yet are perfectly installed (`picotool
    /// --version` exits 255, and `ldproxy` exits 101 for *every* invocation — it
    /// is a linker shim, not a CLI). This is the same question `command -v`
    /// answers, which is what `scripts/matrix.sh` uses. The argv is still chosen
    /// to be a harmless, instant, side-effect-free query, never a bare
    /// invocation that might go looking for a board.
    Binary(&'static [&'static str]),
    /// Present iff `rustup target list --installed` lists this triple.
    RustupTarget(&'static str),
    /// Present iff rustup has an `esp` toolchain whose rustc can build this
    /// *marker* triple (always the no_std `xtensa-esp32s3-none-elf` — building it
    /// is what proves the toolchain is Espressif's Xtensa-enabled rustc, which is
    /// equally the requirement for the `xtensa-*-espidf` std target).
    ///
    /// Xtensa is **not** a rustup target — `rustup target add xtensa-…` does not
    /// work, and custom (espup-linked) toolchains are not listable with `rustup
    /// target list --toolchain esp` either. The only honest probe is to ask the
    /// toolchain itself what it can build, which is what `scripts/matrix.sh`
    /// does.
    XtensaToolchain(&'static str),
    /// Present iff `component` is installed on rustup toolchain `toolchain`.
    /// Used for the RISC-V ESP `std` targets, which are `build-std` targets
    /// rather than rustup targets: they are built from source by a nightly
    /// toolchain carrying `rust-src`.
    RustupComponent {
        toolchain: &'static str,
        component: &'static str,
    },
}

/// When a prerequisite is needed. Drives the two groups the summary prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Required before `cargo build` can succeed.
    Build,
    /// Required to get the built firmware onto the board.
    Flash,
}

/// One host prerequisite. `id` is what stacks/flashing tools reference; for a
/// Rust target the id **is** the target triple, so `Stack::target` resolves its
/// own toolchain prerequisite with no extra mapping (pinned by
/// `every_stack_target_has_a_prerequisite`).
#[derive(Debug, Clone, Copy)]
pub struct Prereq {
    pub id: &'static str,
    /// User-facing name. Never names a backend generator (v3 UNIFY).
    pub display: &'static str,
    pub probe: Probe,
    /// The exact command that installs it. Shown only when it is missing.
    pub install: &'static str,
    pub need: Need,
    /// Something true about this prerequisite that the install command does not
    /// convey (e.g. that the first ESP-IDF build is very long). Shown whether or
    /// not the tool is present.
    pub note: Option<&'static str>,
}

/// The prerequisite catalog. Every entry's `install` command and `probe` argv was
/// checked against the tool itself on a real host (see the report accompanying
/// this change); nothing here is guessed.
pub const PREREQS: &[Prereq] = &[
    // ---- Rust targets / toolchains (build) ----
    // The RISC-V and Cortex-M triples are ordinary rustup targets on stable.
    Prereq {
        id: "riscv32imc-unknown-none-elf",
        display: "Rust target riscv32imc-unknown-none-elf",
        probe: Probe::RustupTarget("riscv32imc-unknown-none-elf"),
        install: "rustup target add riscv32imc-unknown-none-elf",
        need: Need::Build,
        note: None,
    },
    Prereq {
        id: "riscv32imac-unknown-none-elf",
        display: "Rust target riscv32imac-unknown-none-elf",
        probe: Probe::RustupTarget("riscv32imac-unknown-none-elf"),
        install: "rustup target add riscv32imac-unknown-none-elf",
        need: Need::Build,
        note: None,
    },
    Prereq {
        id: "thumbv6m-none-eabi",
        display: "Rust target thumbv6m-none-eabi",
        probe: Probe::RustupTarget("thumbv6m-none-eabi"),
        install: "rustup target add thumbv6m-none-eabi",
        need: Need::Build,
        note: None,
    },
    Prereq {
        id: "thumbv7em-none-eabihf",
        display: "Rust target thumbv7em-none-eabihf",
        probe: Probe::RustupTarget("thumbv7em-none-eabihf"),
        install: "rustup target add thumbv7em-none-eabihf",
        need: Need::Build,
        note: None,
    },
    Prereq {
        id: "thumbv8m.main-none-eabihf",
        display: "Rust target thumbv8m.main-none-eabihf",
        probe: Probe::RustupTarget("thumbv8m.main-none-eabihf"),
        install: "rustup target add thumbv8m.main-none-eabihf",
        need: Need::Build,
        note: None,
    },
    // Xtensa: NOT a rustup target. `espup install` links an `esp` toolchain
    // carrying Espressif's Xtensa-enabled rustc; that toolchain is the only way
    // to build `xtensa-*`.
    Prereq {
        id: "xtensa-esp32s3-none-elf",
        display: "Xtensa `esp` toolchain (for xtensa-esp32s3-none-elf)",
        probe: Probe::XtensaToolchain("xtensa-esp32s3-none-elf"),
        install: "cargo install espup --locked && espup install",
        need: Need::Build,
        note: Some(
            "Xtensa is not a rustup target — `rustup target add` cannot provide it; \
             the `esp` toolchain from espup can",
        ),
    },
    Prereq {
        id: "xtensa-esp32s3-espidf",
        display: "Xtensa `esp` toolchain (for xtensa-esp32s3-espidf)",
        probe: Probe::XtensaToolchain("xtensa-esp32s3-none-elf"),
        install: "cargo install espup --locked && espup install",
        need: Need::Build,
        note: Some(
            "Xtensa is not a rustup target — `rustup target add` cannot provide it; \
             the `esp` toolchain from espup can",
        ),
    },
    // The RISC-V ESP `std` targets are NOT rustup targets: esp-idf-template's
    // `.cargo/config.toml` sets `build-std = ["std", "panic_abort"]` and its
    // `rust-toolchain.toml` pins `channel = "nightly"` + `components =
    // ["rust-src"]` (verified by generating one). So the requirement is a
    // nightly toolchain with rust-src — and notably NOT espup, which the Xtensa
    // std target does need.
    Prereq {
        id: "riscv32imc-esp-espidf",
        display: "nightly toolchain + rust-src (for riscv32imc-esp-espidf)",
        probe: Probe::RustupComponent { toolchain: "nightly", component: "rust-src" },
        install: "rustup toolchain install nightly --component rust-src",
        need: Need::Build,
        note: Some(
            "a build-std target, not a rustup target; the project's rust-toolchain.toml pins \
             nightly + rust-src, so rustup installs it on the first build too",
        ),
    },
    Prereq {
        id: "riscv32imac-esp-espidf",
        display: "nightly toolchain + rust-src (for riscv32imac-esp-espidf)",
        probe: Probe::RustupComponent { toolchain: "nightly", component: "rust-src" },
        install: "rustup toolchain install nightly --component rust-src",
        need: Need::Build,
        note: Some(
            "a build-std target, not a rustup target; the project's rust-toolchain.toml pins \
             nightly + rust-src, so rustup installs it on the first build too",
        ),
    },
    // ---- ESP `std` build tools ----
    // Directly verified: the generated project's `.cargo/config.toml` sets
    // `linker = "ldproxy"` for `cfg(target_os = "espidf")`, so EVERY esp-idf
    // build (Xtensa and RISC-V alike) links through it.
    Prereq {
        id: "ldproxy",
        display: "ldproxy",
        // `ldproxy` is a linker shim, not a CLI: it exits 101 for every
        // invocation, `--version` included. Spawn-success is the real signal.
        probe: Probe::Binary(&["ldproxy", "--version"]),
        install: "cargo install ldproxy",
        need: Need::Build,
        note: None,
    },
    // esp-idf-sys bootstraps the ESP-IDF SDK itself — and its GCC/clang
    // toolchains, cmake and ninja — through `idf_tools.py`, so those are NOT
    // host prerequisites. python3 and git are: embuild shells out to the `git`
    // binary (clone/checkout/apply/rev-parse) and runs `python3` to create the
    // SDK's venv, before any of the bootstrapped tools exist.
    Prereq {
        id: "python3",
        display: "python3",
        probe: Probe::Binary(&["python3", "--version"]),
        install: "brew install python3",
        need: Need::Build,
        note: None,
    },
    Prereq {
        id: "git",
        display: "git",
        probe: Probe::Binary(&["git", "--version"]),
        install: "xcode-select --install",
        need: Need::Build,
        note: None,
    },
    // ---- Flashing tools ----
    Prereq {
        id: "espflash",
        display: "espflash",
        probe: Probe::Binary(&["espflash", "--version"]),
        install: "cargo install espflash --locked",
        need: Need::Flash,
        note: None,
    },
    Prereq {
        id: "probe-rs",
        display: "probe-rs",
        probe: Probe::Binary(&["probe-rs", "--version"]),
        // The CLI ships in the `probe-rs-tools` crate; `probe-rs` itself is the
        // library.
        install: "cargo install probe-rs-tools --locked",
        need: Need::Flash,
        note: Some("needs an SWD debug probe attached — espflash/UF2 need only the USB-C cable"),
    },
    Prereq {
        id: "picotool",
        display: "picotool",
        // `picotool --version` exits 255; the subcommand is `picotool version`.
        probe: Probe::Binary(&["picotool", "version"]),
        install: "brew install picotool",
        need: Need::Flash,
        note: Some("flashes over USB in BOOTSEL mode (hold B while plugging in) — no probe needed"),
    },
    Prereq {
        id: "hf2",
        display: "hf2-cli",
        probe: Probe::Binary(&["hf2", "--version"]),
        install: "cargo install hf2-cli",
        need: Need::Flash,
        note: Some(
            "talks to the UF2 bootloader over HID (short the RST pads twice) — no probe needed",
        ),
    },
    Prereq {
        id: "rust-objcopy",
        display: "rust-objcopy (cargo-binutils)",
        probe: Probe::Binary(&["rust-objcopy", "--version"]),
        install: "cargo install cargo-binutils && rustup component add llvm-tools",
        need: Need::Flash,
        note: None,
    },
    Prereq {
        id: "uf2conv",
        display: "uf2conv.py (+ uf2families.json)",
        probe: Probe::Binary(&["uf2conv.py", "-h"]),
        install: "curl -LO https://raw.githubusercontent.com/microsoft/uf2/master/utils/uf2conv.py \
                  && curl -LO https://raw.githubusercontent.com/microsoft/uf2/master/utils/uf2families.json \
                  && chmod +x uf2conv.py   # then put BOTH on PATH",
        need: Need::Flash,
        note: Some("uf2conv.py reads uf2families.json from beside itself — both files must be installed"),
    },
];

/// Resolve a prerequisite by id. For Rust targets the id is the target triple.
pub fn prereq(id: &str) -> Option<&'static Prereq> {
    PREREQS.iter().find(|p| p.id == id)
}

// --- Per-stack / per-tool prerequisite lists ---
// These name only the EXTRA prerequisites: the target/toolchain one is always
// derived from `Stack::target` (whose triple is a `PREREQS` id), so it is never
// repeated here.

/// esp-hal (ESP no_std): nothing beyond the target/toolchain. The flashing tool
/// is a live user choice on this stack, so its prerequisites come from the
/// chosen [`ToolChoice`] rather than from the stack.
const NO_PREREQS: &[&str] = &[];
/// ESP `std`: the linker shim every esp-idf build routes through, plus the host
/// tools esp-idf-sys needs to bootstrap the SDK.
const ESP_IDF_BUILD_PREREQS: &[&str] = &["ldproxy", "python3", "git"];
/// The esp-idf template's cargo runner is espflash, and FLASHING is not a live
/// axis on that stack (the template owns it), so it is stack data here.
const ESP_FLASH_PREREQS: &[&str] = &["espflash"];
/// RP2040 / RP2350 over USB (BOOTSEL mass storage). picotool reads the ELF
/// directly, so no objcopy/UF2 conversion is needed.
const RP_FLASH_PREREQS: &[&str] = &["picotool"];
/// nRF52840 over USB (Adafruit nRF52 UF2 bootloader, double-tap RESET): the ELF
/// must be objcopy'd to a raw binary and then converted to UF2.
const NRF_FLASH_PREREQS: &[&str] = &["rust-objcopy", "uf2conv"];
/// SAMD21 over USB (Seeed uf2-samdx1 bootloader): hf2 takes the ELF directly.
const SAMD_FLASH_PREREQS: &[&str] = &["hf2"];

// --- Per-stack support lists (which universal entries a stack offers) ---
// esp-hal wires all three categories; the non-ESP scaffold wires the FRAMEWORK
// category only (bare-metal vs embassy) — its flashing/logging entries are
// grounded display data that drive greying but never generation (M4).

/// esp-hal (ESP no_std) — the wired axis.
const ESP_HAL_FLASHING: &[&str] = &["espflash", "probe-rs"];
const ESP_HAL_LOGGING: &[&str] = &["none", "log", "defmt"];
const ESP_HAL_FRAMEWORKS: &[&str] = &["none", "embassy"];

/// esp-idf (ESP std) — the toolchain is fixed by the esp-idf template, so the
/// whole section is greyed (empty support).
const IDF_NONE: &[&str] = &[];

/// Non-ESP support lists. `frameworks` is a live axis (M4): `none` generates
/// the blocking/no-executor body, `embassy` the executor body — but only on an
/// embassy HAL, see `Scaffold::framework_gaps`. Flashing/logging are display-only.
const RP_FLASHING: &[&str] = &["probe-rs"];
const NRF_FLASHING: &[&str] = &["probe-rs"];
const SAMD_FLASHING: &[&str] = &["probe-rs"];
const ARM_LOGGING: &[&str] = &["none", "log", "defmt"];
const RP_FRAMEWORKS: &[&str] = &["none", "embassy", "rtic"];
const NRF_FRAMEWORKS: &[&str] = &["none", "embassy", "rtic"];
const SAMD_FRAMEWORKS: &[&str] = &["none", "rtic"];

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

// Note: `-o esp-backtrace` makes esp-generate add `esp-println` as a direct
// dependency, so templates that only `println!` need no extra dep (verified).

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "base-blink",
        display: "Base — blink the onboard user LED",
        esp_options: &[],
        extra_deps: &[],
        needs_led: true,
        component: "led",
        gap: None,
    },
    Template {
        id: "base-hello",
        display: "Base — hello over serial (for variants without a user LED)",
        esp_options: &["esp-backtrace"],
        extra_deps: &[],
        needs_led: false,
        component: "serial",
        gap: None,
    },
    Template {
        id: "camera-snapshot",
        display: "Onboard OV2640 camera — capture a JPEG frame (DVP, blocking)",
        esp_options: &["unstable-hal", "esp-backtrace"],
        extra_deps: &[],
        needs_led: false,
        component: "camera",
        gap: None,
    },
    Template {
        id: "mic-capture",
        display: "Onboard PDM microphone — capture PCM samples (I2S PDM, embassy)",
        esp_options: &["unstable-hal", "alloc", "embassy", "esp-backtrace"],
        extra_deps: &[],
        needs_led: false,
        component: "mic",
        // Documented gap: the esp-hal PDM RX master API (I2s::new_pdm, PdmConfig,
        // PdmRxConfig, PdmSlotMode) exists on esp-hal git main/qa-test but is NOT
        // in the published esp-hal 1.1.x that `~1.1` resolves to. Reaching PDM
        // would require raw PAC register code (forbidden) or a git pin. Re-enable
        // when esp-hal releases the PDM RX API. (research/driver-crates.md)
        gap: Some(
            "the esp-hal PDM RX API (I2s::new_pdm/PdmConfig) is not yet in a published \
             esp-hal 1.1.x release; the onboard PDM mic template is a tracked gap until it ships",
        ),
    },
    Template {
        id: "sd-list",
        display: "Onboard microSD — mount FAT and list the root directory (SPI, blocking)",
        esp_options: &["unstable-hal", "esp-backtrace"],
        extra_deps: &[
            // no_std: embedded-sdmmc's default features pull in std/log — must disable.
            Dep { name: "embedded-sdmmc", spec: r#"{ version = "0.9.0", default-features = false }"# },
            Dep { name: "embedded-hal-bus", spec: r#""0.3""# },
        ],
        needs_led: false,
        component: "sd",
        gap: None,
    },
    // ---- Non-ESP onboard-component templates ----
    // Declared so the non-ESP variants' template lists resolve and DISPLAY, but
    // they carry no body: selecting (or generating) one fails at config time with
    // the `gap` reason. M4 deliberately ships BASE project generation only — an
    // onboard-component template only lands with a compile-verified body, and
    // neither of these has one yet (the reasons below are the blockers, recorded
    // in research/nonesp-registry-draft.md §B/§E).
    Template {
        id: "rgb-led",
        display: "Onboard RGB LED — cycle colours",
        esp_options: &[],
        extra_deps: &[],
        needs_led: false,
        component: "rgb-led",
        // RP2040: the WS2812 driver `ws2812-pio 0.9.0` pins `rp2040-hal ^0.11`
        // and `embedded-hal 0.2`, which does not compose with our verified
        // rp2040-hal 0.12 base (breaking PIO API bump). RP2350: only the pre-1.0
        // `ws2812-rp235x 0.1.1` exists. nRF52840(/Sense): the board has NO
        // WS2812 at all — it has three discrete common-anode LEDs (P0.26 red,
        // P0.30 green, P0.06 blue), so its body is three GPIO outputs and no
        // driver crate; written and compile-verified, it can un-gate for nRF
        // independently of RP.
        gap: Some(
            "no compile-verified body yet: on RP the WS2812 driver crates do not compose with \
             the pinned HALs (ws2812-pio 0.9 needs rp2040-hal ^0.11 + embedded-hal 0.2; RP2350 \
             only has the pre-1.0 ws2812-rp235x), and on nRF52840 there is no onboard WS2812 at \
             all — its RGB LED is three discrete GPIOs and needs its own body",
        ),
    },
    Template {
        id: "imu-read",
        display: "Onboard IMU — stream accelerometer / gyroscope readings",
        esp_options: &[],
        extra_deps: &[],
        needs_led: false,
        component: "imu",
        // The part is confirmed (LSM6DS3TR-C, driver crate `lsm6ds3tr` 0.2.2,
        // embedded-hal 1.0, no_std) and the power-enable (P1.08) / INT1 (P0.11)
        // pins are documented — but the internal I2C SCL/SDA GPIOs and the bus
        // address are NOT documented by Seeed, and we do not guess pin maps.
        gap: Some(
            "the onboard LSM6DS3TR-C's internal I2C SCL/SDA GPIOs and bus address are not \
             documented by Seeed; xiao-generate does not guess pin maps, so this template is a \
             tracked gap until the wiring is confirmed or exposed as configuration",
        ),
    },
];

pub fn template(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

/// Whether a template id is a *base* template — the implicit working project that
/// is always produced and is never offered as a user choice (M2 redesign, point
/// 4). Covers the no_std `base-blink`/`base-hello` and the ESP `std` synthetic
/// `base` id. Everything else is an optional onboard-component add-on (headless
/// `--template` only; the TUI does not surface component add-ons in v3).
pub fn is_base_template(id: &str) -> bool {
    matches!(id, "base" | "base-blink" | "base-hello")
}

// ---------------------------------------------------------------------------
// µFerris (uferris-bsp 0.1.3) — ESP only. Chip feature comes from the variant.
// Templates are sourced from the uferris-bsp examples. `blinky` is the M1
// compile-verified default; `power-board` gates full-board features.
// ---------------------------------------------------------------------------

pub const UFERRIS_TEMPLATES: &[&str] = &["blinky"];
/// esp-generate options µFerris mode always needs (matches uferris example deps
/// + `esp-backtrace` so the scaffold provides the panic handler the uferris
/// blinky expects via `use esp_backtrace as _`). Compile-verified on C3.
pub const UFERRIS_ESP_OPTIONS: &[&str] = &["unstable-hal", "alloc", "embassy", "esp-backtrace"];

/// Documented gap: uferris-bsp 0.1.3's `xiao_esp32s3` board module calls the old
/// 1-arg `esp_rtos::start(timer)` while its own esp-rtos 0.3.0 dep requires the
/// 2-arg `start(timer, sw_int)`, so the crate fails to build with the
/// `xiao-esp32s3` feature. C3/C6 build fine. Flips to `None` when upstream fixes
/// it. (research/uferris-bsp.md; verified via cargo check.)
pub const UFERRIS_S3_GAP: &str = "uferris-bsp 0.1.3's ESP32-S3 board module does not build against \
    its own esp-rtos 0.3.0 dependency (stale 1-arg esp_rtos::start call); µFerris on S3/S3-Sense is a \
    tracked upstream gap until a fixed uferris-bsp is released. µFerris works on XIAO ESP32-C3 and C6.";

// ---------------------------------------------------------------------------
// Variants
// ---------------------------------------------------------------------------

const ESP_BASE_ONLY: &[&str] = &["base-blink", "base-hello"];
const ESP_S3_SENSE: &[&str] = &["base-blink", "camera-snapshot", "mic-capture", "sd-list"];

/// ESP `std` stack template ids. Single synthetic `base` id: resolves to a std
/// blink on variants with an onboard LED, a std log-hello otherwise (M5:
/// base-only; component templates for std are follow-on, D6/R9).
pub const STD_TEMPLATES: &[&str] = &["base"];

// Stack display strings (shared). Universal HAL/runtime-family names only — the
// backend generator (esp-generate / cargo-generate / direct scaffold) is an internal
// detail and is NOT named in these user-facing strings (v3 UNIFY).
const STD_DISPLAY: &str = "esp-idf · std";
const ESP_HAL_DISPLAY: &str = "esp-hal · no_std";

// Non-ESP HAL-profile stack displays (M4). These are distinct HAL crate
// families, not an async toggle (D8): the HAL axis and the FRAMEWORK axis are
// ORTHOGONAL — an embassy HAL builds fine with `framework = none` (verified),
// and `framework = embassy` requires an embassy HAL (verified; see the
// `framework_gaps` on the blocking stacks).
// Just the HAL crate name — the std/no_std family is only meaningful (and shown)
// on the ESP stacks; these non-ESP HAL crates carry no extra qualifier.
const RP2040_DISPLAY: &str = "rp2040-hal";
const RP235X_DISPLAY: &str = "rp235x-hal";
const NRF_DISPLAY: &str = "nrf-hal";
const ATSAMD_DISPLAY: &str = "atsamd-hal";
const EMBASSY_RP_DISPLAY: &str = "embassy-rp";
const EMBASSY_NRF_DISPLAY: &str = "embassy-nrf";

// Non-ESP per-variant template lists (base always; onboard-component templates
// are declared as gap entries in TEMPLATES so they display but don't generate).
const NON_ESP_RP: &[&str] = &["base-blink", "rgb-led"];
const NON_ESP_NRF_SENSE: &[&str] = &["base-blink", "imu-read", "rgb-led"];
const NON_ESP_BASE_ONLY: &[&str] = &["base-blink"];

// ---------------------------------------------------------------------------
// Non-ESP direct-scaffold data (M4). Every dependency set, feature list,
// rustflag and linker-script choice below produced a real ARM ELF — see
// research/hal-framework-matrix.md §6 (the verified template material) and the
// compile matrix in tests/generate.rs.
// ---------------------------------------------------------------------------

// Shared core deps. `cortex-m` carries the critical-section impl only on nRF /
// SAMD21; on RP the HAL's own `critical-section-impl` feature provides it and a
// second impl would be a duplicate-symbol error (matrix §7).
const CORTEX_M: Dep = Dep { name: "cortex-m", spec: r#""0.7.7""# };
const CORTEX_M_CS: Dep = Dep {
    name: "cortex-m",
    spec: r#"{ version = "0.7.7", features = ["critical-section-single-core"] }"#,
};
const CORTEX_M_RT: Dep = Dep { name: "cortex-m-rt", spec: r#""0.7.5""# };
const EMBEDDED_HAL: Dep = Dep { name: "embedded-hal", spec: r#""1.0""# };
const PANIC_HALT: Dep = Dep { name: "panic-halt", spec: r#""1.0""# };

/// Deps added on top of the stack's own set when `framework = embassy`. The
/// executor's Cortex-M feature is `platform-cortex-m` on embassy-executor 0.10
/// (renamed from `arch-cortex-m`).
pub const EMBASSY_FRAMEWORK_DEPS: &[Dep] = &[
    Dep {
        name: "embassy-executor",
        spec: r#"{ version = "0.10", features = ["platform-cortex-m", "executor-thread"] }"#,
    },
    Dep { name: "embassy-time", spec: r#""0.5.1""# },
];

const RP2040_HAL_DEPS: &[Dep] = &[
    CORTEX_M,
    CORTEX_M_RT,
    EMBEDDED_HAL,
    PANIC_HALT,
    // RP2040 needs a 2nd-stage bootloader blob in the `.boot2` section.
    Dep { name: "rp2040-boot2", spec: r#""0.3.0""# },
];
const RP235X_HAL_DEPS: &[Dep] = &[CORTEX_M, CORTEX_M_RT, EMBEDDED_HAL, PANIC_HALT];
const NRF_HAL_DEPS: &[Dep] = &[CORTEX_M_CS, CORTEX_M_RT, EMBEDDED_HAL, PANIC_HALT];
const EMBASSY_RP_DEPS: &[Dep] = &[CORTEX_M, CORTEX_M_RT, PANIC_HALT];
const EMBASSY_NRF_DEPS: &[Dep] = &[CORTEX_M_CS, CORTEX_M_RT, PANIC_HALT];
// SAMD21 goes through the `xiao_m0` board crate, which owns the LED (`Led0`),
// the memory.x and the `atsamd-hal` (0.17) it pins; the project needs no
// embedded-hal of its own and the BSP's example uses panic-halt 0.2.
const SAMD21_DEPS: &[Dep] = &[
    Dep {
        name: "cortex-m",
        spec: r#"{ version = "0.7", features = ["critical-section-single-core"] }"#,
    },
    Dep { name: "panic-halt", spec: r#""0.2""# },
];

/// `--nmagic` + cortex-m-rt's `link.x`: the standard Cortex-M pair.
const ARM_RUSTFLAGS: &[&str] = &["-C", "link-arg=--nmagic", "-C", "link-arg=-Tlink.x"];
/// RP2350 additionally selects the Cortex-M33 CPU model.
const RP2350_RUSTFLAGS: &[&str] = &[
    "-C",
    "link-arg=--nmagic",
    "-C",
    "link-arg=-Tlink.x",
    "-C",
    "target-cpu=cortex-m33",
];
/// embassy-rp on RP2040 gets its link args from `build.rs` instead, because
/// `-Tlink-rp.x` (embassy-rp's own boot2 script) is emitted into `OUT_DIR` by
/// embassy-rp's build script and must follow `link.x` in bin-only link order.
const NO_RUSTFLAGS: &[&str] = &[];
const EMBASSY_RP2040_LINK_ARGS: &[&str] = &["--nmagic", "-Tlink.x", "-Tlink-rp.x"];
const NO_LINK_ARGS: &[&str] = &[];

/// The verified reason `framework = embassy` is refused on a *blocking* HAL:
/// there is no way to get a working `embassy-time` driver alongside it.
/// (matrix §3/§4: `nrf52840-hal` + `embassy-nrf`-as-driver either collides on
/// `__INTERRUPTS` or silently links `RTC1` to `DefaultHandler`; `rp2040-hal` +
/// `embassy-rp`-as-driver fails outright because the `Driver` impl is behind
/// `rt`. An executor with no timers is not worth shipping.)
const RP2040_EMBASSY_GAP: FrameworkGap = FrameworkGap {
    framework: "embassy",
    reason: "embassy needs an embassy HAL — a blocking HAL has no embassy-time driver \
             (linking one in from embassy-rp fails: its time driver is gated behind the \
             same `rt` feature that owns the vector table). Use `--stack embassy-rp`.",
};
const RP235X_EMBASSY_GAP: FrameworkGap = FrameworkGap {
    framework: "embassy",
    reason: "embassy needs an embassy HAL — a blocking HAL has no embassy-time driver. \
             Use `--stack embassy-rp`.",
};
const NRF_EMBASSY_GAP: FrameworkGap = FrameworkGap {
    framework: "embassy",
    reason: "embassy needs an embassy HAL — a blocking HAL has no embassy-time driver \
             (linking one in from embassy-nrf either collides on `__INTERRUPTS` or leaves \
             RTC1 pointing at DefaultHandler, so timers never fire). Use `--stack embassy-nrf`.",
};
/// RTIC is a real Cortex-M framework and a real catalog entry for these chips,
/// but xiao-generate has no compile-verified RTIC body yet, so it is a declared
/// gap rather than a silently-ignored selection.
const RTIC_GAP: FrameworkGap = FrameworkGap {
    framework: "rtic",
    reason: "RTIC project generation is not implemented yet — xiao-generate M4 ships the \
             bare-metal (`none`) and embassy frameworks. Tracked as a gap, not a broken project.",
};

const RP2040_HAL_SCAFFOLD: Scaffold = Scaffold {
    hal_crate: "rp2040-hal",
    hal_version: RP2040_HAL_VERSION,
    hal_features: &["rt", "critical-section-impl"],
    embassy_hal_features: &[],
    base_deps: RP2040_HAL_DEPS,
    rustflags: ARM_RUSTFLAGS,
    memory_x: Some("rp2040-boot2"),
    link_args: NO_LINK_ARGS,
    framework_gaps: &[RP2040_EMBASSY_GAP, RTIC_GAP],
};
const RP235X_HAL_SCAFFOLD: Scaffold = Scaffold {
    hal_crate: "rp235x-hal",
    hal_version: RP235X_HAL_VERSION,
    hal_features: &["rt", "critical-section-impl", "binary-info"],
    embassy_hal_features: &[],
    base_deps: RP235X_HAL_DEPS,
    rustflags: RP2350_RUSTFLAGS,
    memory_x: Some("rp2350"),
    link_args: NO_LINK_ARGS,
    framework_gaps: &[RP235X_EMBASSY_GAP, RTIC_GAP],
};
const NRF_HAL_SCAFFOLD: Scaffold = Scaffold {
    hal_crate: "nrf52840-hal",
    hal_version: NRF52840_HAL_VERSION,
    hal_features: &[],
    embassy_hal_features: &[],
    base_deps: NRF_HAL_DEPS,
    rustflags: ARM_RUSTFLAGS,
    memory_x: Some("nrf52840"),
    link_args: NO_LINK_ARGS,
    framework_gaps: &[NRF_EMBASSY_GAP, RTIC_GAP],
};
const EMBASSY_RP2040_SCAFFOLD: Scaffold = Scaffold {
    hal_crate: "embassy-rp",
    hal_version: EMBASSY_RP_VERSION,
    hal_features: &["rp2040", "rt", "critical-section-impl"],
    embassy_hal_features: &["time-driver"],
    base_deps: EMBASSY_RP_DEPS,
    rustflags: NO_RUSTFLAGS,
    memory_x: Some("rp2040-embassy"),
    link_args: EMBASSY_RP2040_LINK_ARGS,
    framework_gaps: &[RTIC_GAP],
};
const EMBASSY_RP235X_SCAFFOLD: Scaffold = Scaffold {
    hal_crate: "embassy-rp",
    hal_version: EMBASSY_RP_VERSION,
    hal_features: &["rp235xa", "rt", "critical-section-impl"],
    embassy_hal_features: &["time-driver"],
    base_deps: EMBASSY_RP_DEPS,
    // No `-Tlink-rp.x` here: embassy-rp only emits that script for the `rp2040`
    // feature. On RP2350 the boot block comes from our memory.x `.start_block`
    // section plus the IMAGE_DEF embassy-rp itself places there.
    rustflags: RP2350_RUSTFLAGS,
    memory_x: Some("rp2350"),
    link_args: NO_LINK_ARGS,
    framework_gaps: &[RTIC_GAP],
};
const EMBASSY_NRF_SCAFFOLD: Scaffold = Scaffold {
    hal_crate: "embassy-nrf",
    hal_version: EMBASSY_NRF_VERSION,
    hal_features: &["nrf52840", "rt"],
    embassy_hal_features: &["time-driver-rtc1", "gpiote"],
    base_deps: EMBASSY_NRF_DEPS,
    rustflags: ARM_RUSTFLAGS,
    memory_x: Some("nrf52840"),
    link_args: NO_LINK_ARGS,
    framework_gaps: &[RTIC_GAP],
};
const SAMD21_SCAFFOLD: Scaffold = Scaffold {
    hal_crate: "xiao_m0",
    hal_version: XIAO_M0_BSP_VERSION,
    hal_features: &[],
    embassy_hal_features: &[],
    base_deps: SAMD21_DEPS,
    rustflags: ARM_RUSTFLAGS,
    // The xiao_m0 BSP's own build.rs emits memory.x (feature `rt`).
    memory_x: None,
    link_args: NO_LINK_ARGS,
    framework_gaps: &[RTIC_GAP],
};

pub const VARIANTS: &[Variant] = &[
    // ---- P0 ESP (M1 esp-hal + M5 esp-idf) ----
    Variant {
        id: "xiao-esp32c3",
        display: "XIAO ESP32-C3",
        priority: 0,
        chip: "esp32c3",
        arch: Arch::RiscV,
        led: None, // Seeed: no onboard user LED
        stacks: &[
            Stack {
                id: "esp-hal",
                display: ESP_HAL_DISPLAY,
                backend: Backend::EspGenerate,
                target: "riscv32imc-unknown-none-elf",
                templates: &["base-hello"], // no LED → hello is the sensible base
                uferris: true,
                flashing: ESP_HAL_FLASHING,
                logging: ESP_HAL_LOGGING,
                frameworks: ESP_HAL_FRAMEWORKS,
                scaffold: None,
                build_prereqs: NO_PREREQS,
                flash_prereqs: NO_PREREQS,
            },
            Stack {
                id: "esp-idf",
                display: STD_DISPLAY,
                backend: Backend::CargoGenerate,
                target: "riscv32imc-esp-espidf",
                templates: STD_TEMPLATES,
                uferris: false,
                flashing: IDF_NONE,
                logging: IDF_NONE,
                frameworks: IDF_NONE,
                scaffold: None,
                build_prereqs: ESP_IDF_BUILD_PREREQS,
                flash_prereqs: ESP_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: Some("xiao-esp32c3"),
        uferris_gap: None,
        default_crates: &[],
    },
    Variant {
        id: "xiao-esp32c6",
        display: "XIAO ESP32-C6",
        priority: 0,
        chip: "esp32c6",
        arch: Arch::RiscV,
        led: Some(Led { gpio: 15, active_low: None }), // polarity undocumented; blink is polarity-agnostic
        stacks: &[
            Stack {
                id: "esp-hal",
                display: ESP_HAL_DISPLAY,
                backend: Backend::EspGenerate,
                target: "riscv32imac-unknown-none-elf",
                templates: ESP_BASE_ONLY,
                uferris: true,
                flashing: ESP_HAL_FLASHING,
                logging: ESP_HAL_LOGGING,
                frameworks: ESP_HAL_FRAMEWORKS,
                scaffold: None,
                build_prereqs: NO_PREREQS,
                flash_prereqs: NO_PREREQS,
            },
            Stack {
                id: "esp-idf",
                display: STD_DISPLAY,
                backend: Backend::CargoGenerate,
                target: "riscv32imac-esp-espidf",
                templates: STD_TEMPLATES,
                uferris: false,
                flashing: IDF_NONE,
                logging: IDF_NONE,
                frameworks: IDF_NONE,
                scaffold: None,
                build_prereqs: ESP_IDF_BUILD_PREREQS,
                flash_prereqs: ESP_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: Some("xiao-esp32c6"),
        uferris_gap: None,
        default_crates: &[],
    },
    Variant {
        id: "xiao-esp32s3",
        display: "XIAO ESP32-S3",
        priority: 0,
        chip: "esp32s3",
        arch: Arch::Xtensa,
        led: Some(Led { gpio: 21, active_low: Some(true) }),
        stacks: &[
            Stack {
                id: "esp-hal",
                display: ESP_HAL_DISPLAY,
                backend: Backend::EspGenerate,
                target: "xtensa-esp32s3-none-elf",
                templates: ESP_BASE_ONLY,
                uferris: true,
                flashing: ESP_HAL_FLASHING,
                logging: ESP_HAL_LOGGING,
                frameworks: ESP_HAL_FRAMEWORKS,
                scaffold: None,
                build_prereqs: NO_PREREQS,
                flash_prereqs: NO_PREREQS,
            },
            Stack {
                id: "esp-idf",
                display: STD_DISPLAY,
                backend: Backend::CargoGenerate,
                target: "xtensa-esp32s3-espidf",
                templates: STD_TEMPLATES,
                uferris: false,
                flashing: IDF_NONE,
                logging: IDF_NONE,
                frameworks: IDF_NONE,
                scaffold: None,
                build_prereqs: ESP_IDF_BUILD_PREREQS,
                flash_prereqs: ESP_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: Some("xiao-esp32s3"),
        uferris_gap: Some(UFERRIS_S3_GAP),
        default_crates: &[],
    },
    Variant {
        id: "xiao-esp32s3-sense",
        display: "XIAO ESP32-S3 Sense",
        priority: 0,
        chip: "esp32s3",
        arch: Arch::Xtensa,
        led: Some(Led { gpio: 21, active_low: Some(true) }),
        stacks: &[
            Stack {
                id: "esp-hal",
                display: ESP_HAL_DISPLAY,
                backend: Backend::EspGenerate,
                target: "xtensa-esp32s3-none-elf",
                templates: ESP_S3_SENSE,
                uferris: true,
                flashing: ESP_HAL_FLASHING,
                logging: ESP_HAL_LOGGING,
                frameworks: ESP_HAL_FRAMEWORKS,
                scaffold: None,
                build_prereqs: NO_PREREQS,
                flash_prereqs: NO_PREREQS,
            },
            Stack {
                id: "esp-idf",
                display: STD_DISPLAY,
                backend: Backend::CargoGenerate,
                target: "xtensa-esp32s3-espidf",
                templates: STD_TEMPLATES, // std is base-only; component templates are follow-on
                uferris: false,
                flashing: IDF_NONE,
                logging: IDF_NONE,
                frameworks: IDF_NONE,
                scaffold: None,
                build_prereqs: ESP_IDF_BUILD_PREREQS,
                flash_prereqs: ESP_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: Some("xiao-esp32s3"), // S3 flag; derived from C6 example
        uferris_gap: Some(UFERRIS_S3_GAP),
        default_crates: &[],
    },
    // ---- P1 non-ESP (M4 — direct scaffold). Each variant declares its
    //      blocking-HAL stack (default) and, where one exists, its embassy-HAL
    //      stack (D8). HAL and framework are orthogonal axes. ----
    Variant {
        id: "xiao-rp2040",
        display: "XIAO RP2040",
        priority: 1,
        chip: "rp2040",
        arch: Arch::Arm,
        led: Some(Led { gpio: 25, active_low: Some(true) }),
        stacks: &[
            Stack {
                id: "rp2040-hal",
                display: RP2040_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv6m-none-eabi",
                templates: NON_ESP_RP,
                uferris: false,
                flashing: RP_FLASHING,
                logging: ARM_LOGGING,
                frameworks: RP_FRAMEWORKS,
                scaffold: Some(&RP2040_HAL_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: RP_FLASH_PREREQS,
            },
            Stack {
                id: "embassy-rp",
                display: EMBASSY_RP_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv6m-none-eabi",
                templates: NON_ESP_RP,
                uferris: false,
                flashing: RP_FLASHING,
                logging: ARM_LOGGING,
                frameworks: RP_FRAMEWORKS,
                scaffold: Some(&EMBASSY_RP2040_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: RP_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: None,
        uferris_gap: None,
        default_crates: &[],
    },
    Variant {
        id: "xiao-rp2350",
        display: "XIAO RP2350",
        priority: 1,
        chip: "rp235x",
        arch: Arch::Arm,
        led: Some(Led { gpio: 25, active_low: Some(true) }),
        stacks: &[
            Stack {
                id: "rp235x-hal",
                display: RP235X_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv8m.main-none-eabihf",
                templates: NON_ESP_RP,
                uferris: false,
                flashing: RP_FLASHING,
                logging: ARM_LOGGING,
                frameworks: RP_FRAMEWORKS,
                scaffold: Some(&RP235X_HAL_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: RP_FLASH_PREREQS,
            },
            Stack {
                id: "embassy-rp",
                display: EMBASSY_RP_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv8m.main-none-eabihf",
                templates: NON_ESP_RP,
                uferris: false,
                flashing: RP_FLASHING,
                logging: ARM_LOGGING,
                frameworks: RP_FRAMEWORKS,
                scaffold: Some(&EMBASSY_RP235X_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: RP_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: None,
        uferris_gap: None,
        default_crates: &[],
    },
    Variant {
        id: "xiao-nrf52840",
        display: "XIAO nRF52840",
        priority: 1,
        chip: "nrf52840",
        arch: Arch::Arm,
        led: Some(Led { gpio: 26, active_low: Some(true) }), // P0.26 (red)
        stacks: &[
            Stack {
                id: "nrf-hal",
                display: NRF_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv7em-none-eabihf",
                templates: NON_ESP_BASE_ONLY,
                uferris: false,
                flashing: NRF_FLASHING,
                logging: ARM_LOGGING,
                frameworks: NRF_FRAMEWORKS,
                scaffold: Some(&NRF_HAL_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: NRF_FLASH_PREREQS,
            },
            Stack {
                id: "embassy-nrf",
                display: EMBASSY_NRF_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv7em-none-eabihf",
                templates: NON_ESP_BASE_ONLY,
                uferris: false,
                flashing: NRF_FLASHING,
                logging: ARM_LOGGING,
                frameworks: NRF_FRAMEWORKS,
                scaffold: Some(&EMBASSY_NRF_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: NRF_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: None,
        uferris_gap: None,
        default_crates: &[],
    },
    Variant {
        id: "xiao-nrf52840-sense",
        display: "XIAO nRF52840 Sense",
        priority: 1,
        chip: "nrf52840",
        arch: Arch::Arm,
        led: Some(Led { gpio: 26, active_low: Some(true) }),
        stacks: &[
            Stack {
                id: "nrf-hal",
                display: NRF_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv7em-none-eabihf",
                templates: NON_ESP_NRF_SENSE,
                uferris: false,
                flashing: NRF_FLASHING,
                logging: ARM_LOGGING,
                frameworks: NRF_FRAMEWORKS,
                scaffold: Some(&NRF_HAL_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: NRF_FLASH_PREREQS,
            },
            Stack {
                id: "embassy-nrf",
                display: EMBASSY_NRF_DISPLAY,
                backend: Backend::NonEspScaffold,
                target: "thumbv7em-none-eabihf",
                templates: NON_ESP_NRF_SENSE,
                uferris: false,
                flashing: NRF_FLASHING,
                logging: ARM_LOGGING,
                frameworks: NRF_FRAMEWORKS,
                scaffold: Some(&EMBASSY_NRF_SCAFFOLD),
                build_prereqs: NO_PREREQS,
                flash_prereqs: NRF_FLASH_PREREQS,
            },
        ],
        uferris_chip_feature: None,
        uferris_gap: None,
        default_crates: &[],
    },
    Variant {
        id: "xiao-samd21",
        display: "XIAO SAMD21 (Seeeduino XIAO)",
        priority: 2,
        chip: "atsamd21",
        arch: Arch::Arm,
        led: Some(Led { gpio: 17, active_low: Some(true) }), // PA17
        stacks: &[Stack {
            // SAMD21: one stack. There is no dedicated embassy-samd HAL crate the
            // way embassy-rp/embassy-nrf exist; async on SAMD21 would go through
            // atsamd-hal's own `async` feature + embassy-executor, which has no
            // compile-verified XIAO body here. Hence a single blocking stack.
            id: "atsamd-hal",
            display: ATSAMD_DISPLAY,
            backend: Backend::NonEspScaffold,
            target: "thumbv6m-none-eabi",
            templates: NON_ESP_BASE_ONLY,
            uferris: false,
            flashing: SAMD_FLASHING,
            logging: ARM_LOGGING,
            frameworks: SAMD_FRAMEWORKS,
            scaffold: Some(&SAMD21_SCAFFOLD),
            build_prereqs: NO_PREREQS,
            flash_prereqs: SAMD_FLASH_PREREQS,
        }],
        uferris_chip_feature: None,
        uferris_gap: None,
        default_crates: &[],
    },
];

pub fn variant(id: &str) -> Option<&'static Variant> {
    VARIANTS.iter().find(|v| v.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_template_is_defined() {
        for v in VARIANTS {
            for s in v.stacks {
                for t in s.templates {
                    // The ESP `std` stack uses the synthetic `base` id, which is
                    // resolved at patch time (blink vs hello) and is not a TEMPLATES
                    // entry; every other stack lists real registry templates.
                    if s.is_std() {
                        assert_eq!(*t, "base", "std stack lists only the synthetic `base` id");
                    } else {
                        assert!(
                            template(t).is_some(),
                            "variant {} stack {} lists undefined template {t}",
                            v.id,
                            s.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_variant_has_a_default_stack() {
        for v in VARIANTS {
            assert!(!v.stacks.is_empty(), "variant {} has no stacks", v.id);
            // µFerris is offered only on the esp-hal stack, never on esp-idf.
            for s in v.stacks {
                if s.is_std() {
                    assert!(!s.uferris, "std stack must not offer µFerris ({})", v.id);
                }
            }
        }
    }

    #[test]
    fn esp_variants_offer_both_stacks() {
        for id in ["xiao-esp32c3", "xiao-esp32c6", "xiao-esp32s3", "xiao-esp32s3-sense"] {
            let v = variant(id).unwrap();
            assert!(v.stack("esp-hal").is_some(), "{id} missing esp-hal stack");
            assert!(v.stack("esp-idf").is_some(), "{id} missing esp-idf stack");
            assert_eq!(v.default_stack().id, "esp-hal", "{id} default must be esp-hal");
            // esp-idf targets end in -espidf.
            assert!(v.stack("esp-idf").unwrap().target.ends_with("-espidf"));
        }
    }

    #[test]
    fn all_template_option_sets_satisfy_the_constraint_graph() {
        for t in TEMPLATES {
            validate_esp_options(t.esp_options)
                .unwrap_or_else(|e| panic!("template {} has an invalid option set: {e}", t.id));
        }
        validate_esp_options(UFERRIS_ESP_OPTIONS).expect("uferris option set invalid");
    }

    #[test]
    fn constraint_graph_matches_esp_generate_semantics() {
        // requires
        assert!(validate_esp_options(&["embassy"]).is_err(), "embassy requires unstable-hal");
        assert!(validate_esp_options(&["embassy", "unstable-hal"]).is_ok());
        assert!(validate_esp_options(&["wifi", "unstable-hal"]).is_err(), "wifi requires alloc");
        assert!(validate_esp_options(&["wifi", "unstable-hal", "alloc"]).is_ok());
        // mutual-exclusion groups
        assert!(validate_esp_options(&["defmt", "log"]).is_err(), "log-frontend group");
        assert!(validate_esp_options(&["esp-backtrace", "panic-rtt-target", "probe-rs"]).is_err());
        // negative requires (conflicts)
        assert!(validate_esp_options(&["log", "probe-rs"]).is_err(), "log disabled by probe-rs");
    }

    #[test]
    fn documented_gaps_are_registered() {
        assert!(template("mic-capture").unwrap().gap.is_some(), "mic-capture is a known gap");
        assert!(variant("xiao-esp32s3-sense").unwrap().uferris_gap.is_some());
        assert!(variant("xiao-esp32s3").unwrap().uferris_gap.is_some());
        assert!(variant("xiao-esp32c3").unwrap().uferris_gap.is_none());
        assert!(variant("xiao-esp32c6").unwrap().uferris_gap.is_none());
    }

    #[test]
    fn non_esp_variants_surface_blocking_and_embassy_stacks() {
        // The TUI must be able to display the full non-ESP structure (M2 goal),
        // even though generation is gated. Each RP/nRF variant declares a
        // blocking-HAL stack (default) AND an embassy-HAL stack (D8).
        for id in ["xiao-rp2040", "xiao-rp2350", "xiao-nrf52840", "xiao-nrf52840-sense"] {
            let v = variant(id).unwrap();
            assert_eq!(v.stacks.len(), 2, "{id} should offer blocking + embassy");
            assert!(v.stacks.iter().any(|s| s.id.starts_with("embassy-")), "{id} missing embassy stack");
            assert!(v.stacks.iter().any(|s| !s.id.starts_with("embassy-")), "{id} missing blocking stack");
            // both use the direct non-ESP scaffold backend
            for s in v.stacks {
                assert!(matches!(s.backend, Backend::NonEspScaffold), "{id}/{} backend", s.id);
                assert!(!s.backend.is_esp());
            }
        }
        // SAMD21: one stack (no dedicated embassy-samd HAL crate).
        let samd = variant("xiao-samd21").unwrap();
        assert_eq!(samd.stacks.len(), 1);
        assert_eq!(samd.stacks[0].id, "atsamd-hal");
        // The expected per-variant targets are declared.
        assert_eq!(variant("xiao-rp2040").unwrap().default_stack().target, "thumbv6m-none-eabi");
        assert_eq!(variant("xiao-rp2350").unwrap().default_stack().target, "thumbv8m.main-none-eabihf");
        assert_eq!(variant("xiao-nrf52840").unwrap().default_stack().target, "thumbv7em-none-eabihf");
    }

    // ---- M4: the non-ESP direct scaffold ----

    /// Every non-ESP stack must carry complete, well-formed scaffold data — this
    /// is the input the backend renders, so a bad edit here is a broken project.
    #[test]
    fn non_esp_stacks_carry_scaffold_data() {
        for v in VARIANTS {
            for s in v.stacks {
                let Some(sc) = s.scaffold else {
                    assert!(
                        s.backend.is_esp(),
                        "{}/{} is non-ESP but has no scaffold data",
                        v.id,
                        s.id
                    );
                    continue;
                };
                assert!(matches!(s.backend, Backend::NonEspScaffold));
                assert!(!sc.hal_crate.is_empty(), "{}/{} hal_crate", v.id, s.id);
                assert!(!sc.hal_version.is_empty(), "{}/{} hal_version", v.id, s.id);
                assert!(!sc.base_deps.is_empty(), "{}/{} base_deps", v.id, s.id);
                // Every dependency spec must be a valid TOML value literal or the
                // scaffold's toml_edit insert fails at generation time.
                for d in sc.base_deps {
                    d.spec.parse::<toml_edit::Value>().unwrap_or_else(|e| {
                        panic!("{}/{} dep '{}' spec is not TOML: {e}", v.id, s.id, d.name)
                    });
                }
                // rustflags are passed straight through as an argv array, so an
                // odd count means a `-C`/value pair got split.
                assert_eq!(
                    sc.rustflags.len() % 2,
                    0,
                    "{}/{} rustflags must be -C/value pairs",
                    v.id,
                    s.id
                );
                // A declared memory.x id must resolve to a real body, and a stack
                // that ships one is the only kind that gets a build.rs.
                if let Some(id) = sc.memory_x {
                    assert!(
                        crate::templates::memory_x(id).is_some(),
                        "{}/{} memory.x id '{id}' has no body",
                        v.id,
                        s.id
                    );
                } else {
                    assert!(
                        sc.link_args.is_empty(),
                        "{}/{} has link args but ships no build.rs",
                        v.id,
                        s.id
                    );
                }
            }
        }
    }

    /// The M4 rule, encoded: `framework = embassy` REQUIRES an embassy HAL.
    /// Compile-verified — a blocking HAL has no way to get a working
    /// `embassy-time` driver (research/hal-framework-matrix.md §3/§4).
    #[test]
    fn embassy_framework_requires_an_embassy_hal() {
        for v in VARIANTS {
            for s in v.stacks {
                let Some(sc) = s.scaffold else { continue };
                let is_embassy_hal = s.id.starts_with("embassy-");
                match s.framework_gap("embassy") {
                    Some(reason) => {
                        assert!(!is_embassy_hal, "{}/{} gates embassy on an embassy HAL", v.id, s.id);
                        assert!(
                            reason.contains("embassy HAL"),
                            "{}/{} embassy gap should say why: {reason}",
                            v.id,
                            s.id
                        );
                    }
                    None => assert!(
                        is_embassy_hal || !s.frameworks.contains(&"embassy"),
                        "{}/{} allows embassy on a blocking HAL",
                        v.id,
                        s.id
                    ),
                }
                // The embassy-time driver feature exists only where embassy can
                // actually run: enabling it without an `embassy-time` user drags
                // the whole time stack in for nothing.
                assert_eq!(
                    !sc.embassy_hal_features.is_empty(),
                    is_embassy_hal,
                    "{}/{} embassy_hal_features vs HAL family",
                    v.id,
                    s.id
                );
            }
        }
    }

    /// RTIC is a real Cortex-M framework the registry lists, but no verified body
    /// exists — it must be a declared gap on every stack that lists it, never a
    /// silently-ignored selection.
    #[test]
    fn rtic_is_a_declared_gap_wherever_it_is_listed() {
        for v in VARIANTS {
            for s in v.stacks {
                if s.support(ToolCategory::Framework).contains(&"rtic") {
                    assert!(
                        s.framework_gap("rtic").is_some(),
                        "{}/{} lists rtic without a gap reason",
                        v.id,
                        s.id
                    );
                }
            }
        }
    }

    /// The embassy pins documented at the top of this file are the ones actually
    /// written into generated manifests.
    #[test]
    fn embassy_framework_deps_use_the_pinned_versions() {
        let executor = EMBASSY_FRAMEWORK_DEPS
            .iter()
            .find(|d| d.name == "embassy-executor")
            .unwrap();
        assert!(executor.spec.contains(EMBASSY_EXECUTOR_VERSION), "{}", executor.spec);
        // embassy-executor 0.10 renamed arch-cortex-m -> platform-cortex-m.
        assert!(executor.spec.contains("platform-cortex-m"), "{}", executor.spec);
        let time = EMBASSY_FRAMEWORK_DEPS.iter().find(|d| d.name == "embassy-time").unwrap();
        assert!(time.spec.contains(EMBASSY_TIME_VERSION), "{}", time.spec);
        // The verified rp235x-hal pin is 0.3.0, NOT crates.io-latest 0.4.0.
        assert_eq!(RP235X_HAL_VERSION, "0.3.0");
        assert_eq!(
            variant("xiao-rp2350").unwrap().default_stack().scaffold.unwrap().hal_version,
            "0.3.0"
        );
        // SAMD21 goes through the xiao_m0 BSP, not raw atsamd-hal.
        assert_eq!(
            variant("xiao-samd21").unwrap().default_stack().scaffold.unwrap().hal_crate,
            "xiao_m0"
        );
    }

    /// Every enabled (variant, stack, framework) cell must have a verified body.
    #[test]
    fn every_enabled_non_esp_cell_has_a_base_body() {
        for v in VARIANTS {
            for s in v.stacks {
                if s.scaffold.is_none() {
                    continue;
                }
                for f in s.frameworks {
                    if s.framework_gap(f).is_some() {
                        continue;
                    }
                    assert!(
                        crate::templates::nonesp_base_body(v.chip, s.id, f).is_some(),
                        "no base body for {}/{} framework {f}",
                        v.id,
                        s.id
                    );
                }
            }
        }
    }

    #[test]
    fn non_esp_component_templates_display_but_are_gaps() {
        // rgb-led / imu-read must be in TEMPLATES (so they render) but gated.
        assert!(template("rgb-led").unwrap().gap.is_some(), "rgb-led must be a declared gap");
        assert!(template("imu-read").unwrap().gap.is_some(), "imu-read must be a declared gap");
        // nRF52840 Sense surfaces the IMU + RGB templates.
        let sense = variant("xiao-nrf52840-sense").unwrap();
        for s in sense.stacks {
            assert!(s.templates.contains(&"imu-read"), "sense/{} missing imu-read", s.id);
            assert!(s.templates.contains(&"rgb-led"), "sense/{} missing rgb-led", s.id);
        }
    }

    // ---- M2 redesign v3: universal toolchain catalogs ----

    #[test]
    fn universal_catalogs_are_well_formed() {
        // Three device-agnostic catalogs, in the documented order.
        assert_eq!(
            FLASHING_TOOLS.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec!["espflash", "probe-rs"]
        );
        assert_eq!(
            LOGGING.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec!["none", "log", "defmt"]
        );
        assert_eq!(
            FRAMEWORKS.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec!["none", "embassy", "rtic"]
        );
        // Universal defaults (index 0 of each catalog).
        assert_eq!(ToolCategory::Flashing.default_choice().id, "espflash");
        assert_eq!(ToolCategory::Logging.default_choice().id, "none");
        assert_eq!(ToolCategory::Framework.default_choice().id, "none");
        // Every choice's internal esp mapping (if any) satisfies the graph.
        for cat in ToolCategory::ALL {
            for c in cat.catalog() {
                validate_esp_options(c.esp_options)
                    .unwrap_or_else(|e| panic!("{} {} maps to an invalid esp set: {e}", cat.flag(), c.id));
            }
        }
    }

    #[test]
    fn per_stack_support_lists_are_grounded() {
        let c6 = variant("xiao-esp32c6").unwrap();
        let esp_hal = c6.stack("esp-hal").unwrap();
        assert_eq!(esp_hal.support(ToolCategory::Flashing), &["espflash", "probe-rs"]);
        assert_eq!(esp_hal.support(ToolCategory::Logging), &["none", "log", "defmt"]);
        assert_eq!(esp_hal.support(ToolCategory::Framework), &["none", "embassy"]);
        // esp-hal defaults: espflash / none / none.
        assert_eq!(esp_hal.default_tool(ToolCategory::Flashing).unwrap().id, "espflash");
        assert_eq!(esp_hal.default_tool(ToolCategory::Framework).unwrap().id, "none");
        // RTIC exists in the catalog but is NOT in esp-hal's support.
        assert!(!esp_hal.support(ToolCategory::Framework).contains(&"rtic"));

        // esp-idf: all three empty (configured by the template) → no default.
        let esp_idf = c6.stack("esp-idf").unwrap();
        for cat in ToolCategory::ALL {
            assert!(esp_idf.support(cat).is_empty(), "esp-idf {} must be empty", cat.flag());
            assert!(esp_idf.default_tool(cat).is_none());
        }

        // non-ESP support sets, incl. RTIC on the ARM parts (a declared gap).
        let rp = variant("xiao-rp2040").unwrap().default_stack();
        assert!(rp.support(ToolCategory::Framework).contains(&"rtic"), "RTIC visible on RP");
        let samd = variant("xiao-samd21").unwrap().default_stack();
        assert_eq!(samd.support(ToolCategory::Flashing), &["probe-rs"]);
        assert_eq!(samd.support(ToolCategory::Framework), &["none", "rtic"]);
    }

    #[test]
    fn rtic_is_a_universal_framework_unsupported_on_esp() {
        // RTIC is a legit catalog entry (not an invented gap) — present in the
        // universal FRAMEWORKS catalog, but absent from every ESP stack's support.
        assert!(tool_choice(ToolCategory::Framework, "rtic").is_some());
        for id in ["xiao-esp32c3", "xiao-esp32c6", "xiao-esp32s3", "xiao-esp32s3-sense"] {
            let v = variant(id).unwrap();
            for s in v.stacks {
                assert!(
                    !s.support(ToolCategory::Framework).contains(&"rtic"),
                    "{id}/{} must not support RTIC",
                    s.id
                );
            }
        }
    }

    #[test]
    fn default_base_combo_and_probe_rs_combos_validate() {
        // Assemble the esp option set from a universal (flashing, logging,
        // framework) id triple, mirroring Config::esp_options for the base.
        let assemble = |f: &str, l: &str, fw: &str| -> Vec<&'static str> {
            let mut v = Vec::new();
            v.extend_from_slice(tool_choice(ToolCategory::Flashing, f).unwrap().esp_options);
            v.extend_from_slice(tool_choice(ToolCategory::Logging, l).unwrap().esp_options);
            v.extend_from_slice(tool_choice(ToolCategory::Framework, fw).unwrap().esp_options);
            v
        };
        // Default base = espflash / none / none → esp-backtrace only.
        assert!(validate_esp_options(&assemble("espflash", "none", "none")).is_ok());
        // espflash + defmt + embassy is a valid non-default combo.
        assert!(validate_esp_options(&assemble("espflash", "defmt", "embassy")).is_ok());
        // probe-rs + defmt + embassy is valid; probe-rs + log is NOT (grey-out).
        assert!(validate_esp_options(&assemble("probe-rs", "defmt", "embassy")).is_ok());
        assert!(validate_esp_options(&assemble("probe-rs", "log", "none")).is_err());
    }

    #[test]
    fn default_crates_hook_is_present_and_empty_for_now() {
        // The hook exists on every variant and is intentionally empty (data-only
        // knob for later; no crates invented here).
        for v in VARIANTS {
            assert!(v.default_crates.is_empty(), "{} default_crates must be empty for now", v.id);
        }
    }

    // ---- host prerequisites (registry data) ----

    #[test]
    fn prereq_catalog_is_well_formed() {
        let mut seen = Vec::new();
        for p in PREREQS {
            assert!(!seen.contains(&p.id), "duplicate prerequisite id '{}'", p.id);
            seen.push(p.id);
            assert!(!p.display.is_empty(), "{} has no display name", p.id);
            assert!(!p.install.is_empty(), "{} has no install command", p.id);
            assert_eq!(prereq(p.id).map(|q| q.id), Some(p.id), "{} is not resolvable", p.id);
            if let Probe::Binary(argv) = p.probe {
                assert!(!argv.is_empty(), "{} has an empty probe argv", p.id);
            }
        }
        // A backend generator is xiao-generate's own dependency, not the
        // generated project's — it must never leak into this catalog (v3 UNIFY).
        for p in PREREQS {
            for banned in ["esp-generate", "cargo-generate"] {
                assert_ne!(p.id, banned, "{banned} is the generator's prerequisite, not a project's");
            }
        }
    }

    /// The load-bearing invariant of the whole design: `Stack::target` doubles as
    /// the id of that stack's toolchain prerequisite, so the selection code needs
    /// no target→toolchain mapping. Adding a stack with a new triple and no
    /// catalog entry would silently drop its build prerequisite — this catches it.
    #[test]
    fn every_stack_target_has_a_prerequisite() {
        for v in VARIANTS {
            for s in v.stacks {
                let p = prereq(s.target).unwrap_or_else(|| {
                    panic!("{}/{}: target '{}' has no PREREQS entry", v.id, s.id, s.target)
                });
                assert_eq!(p.need, Need::Build, "{}/{}: a target is a build prereq", v.id, s.id);
            }
        }
    }

    /// Every id referenced from a stack or a tool choice must resolve, and must
    /// sit in the group that references it — a flash tool listed under
    /// `build_prereqs` would print in the wrong section.
    #[test]
    fn referenced_prereq_ids_resolve_into_the_right_group() {
        let check = |ids: &[&str], want: Need, what: &str| {
            for id in ids {
                let p = prereq(id).unwrap_or_else(|| panic!("{what}: unknown prerequisite '{id}'"));
                assert_eq!(p.need, want, "{what}: '{id}' is in the wrong group");
            }
        };
        for v in VARIANTS {
            for s in v.stacks {
                check(s.build_prereqs, Need::Build, &format!("{}/{} build", v.id, s.id));
                check(s.flash_prereqs, Need::Flash, &format!("{}/{} flash", v.id, s.id));
            }
        }
        for c in FLASHING_TOOLS {
            check(c.prereqs, Need::Flash, &format!("flashing '{}'", c.id));
        }
        // Logging/framework choices are plain cargo dependencies — no host tool.
        for cat in [ToolCategory::Logging, ToolCategory::Framework] {
            for c in cat.catalog() {
                assert!(c.prereqs.is_empty(), "{} '{}' should need no host tool", cat.flag(), c.id);
            }
        }
    }

    /// The arch↔toolchain rule, encoded. Xtensa is not a rustup target and must
    /// be probed through the espup-installed `esp` toolchain; RISC-V and Cortex-M
    /// no_std targets are ordinary rustup targets. (The ESP `std` targets are
    /// neither — they are `build-std` targets, checked separately below.)
    #[test]
    fn xtensa_targets_need_espup_and_others_do_not() {
        for v in VARIANTS {
            for s in v.stacks {
                let p = prereq(s.target).unwrap();
                match (v.arch, s.is_std()) {
                    (Arch::Xtensa, _) => {
                        assert!(
                            matches!(p.probe, Probe::XtensaToolchain(_)),
                            "{}/{}: Xtensa must go through the esp toolchain, got {:?}",
                            v.id, s.id, p.probe
                        );
                        assert!(
                            p.install.contains("espup"),
                            "{}/{}: Xtensa install command must use espup: {}",
                            v.id, s.id, p.install
                        );
                    }
                    // ESP `std` on RISC-V: a build-std target (nightly + rust-src),
                    // and specifically NOT espup — espup's RISC-V support is on the
                    // stable channel, which cannot do build-std.
                    (Arch::RiscV, true) => {
                        assert!(
                            matches!(p.probe, Probe::RustupComponent { .. }),
                            "{}/{}: RISC-V std is a build-std target, got {:?}",
                            v.id, s.id, p.probe
                        );
                        assert!(
                            !p.install.contains("espup"),
                            "{}/{}: RISC-V std must not ask for espup: {}",
                            v.id, s.id, p.install
                        );
                    }
                    (Arch::RiscV, false) | (Arch::Arm, _) => {
                        assert!(
                            matches!(p.probe, Probe::RustupTarget(t) if t == s.target),
                            "{}/{}: expected a plain rustup target, got {:?}",
                            v.id, s.id, p.probe
                        );
                        assert_eq!(p.install, format!("rustup target add {}", s.target));
                    }
                }
            }
        }
    }

    /// The flash route is a property of the board's bootloader (or, on esp-hal,
    /// of the user's flashing choice) — pinned here against
    /// `scripts/matrix.sh`, which is the script that actually flashed these
    /// boards over USB with no SWD probe.
    #[test]
    fn flash_routes_match_the_verified_usb_routes() {
        let route = |chip: &str, stack: &str| {
            variant(chip).unwrap().stack(stack).unwrap().flash_prereqs
        };
        // RP2040/RP2350: picotool reads the ELF directly (BOOTSEL mass storage).
        for (chip, stacks) in [
            ("xiao-rp2040", ["rp2040-hal", "embassy-rp"]),
            ("xiao-rp2350", ["rp235x-hal", "embassy-rp"]),
        ] {
            for s in stacks {
                assert_eq!(route(chip, s), &["picotool"], "{chip}/{s}");
            }
        }
        // nRF52840: ELF -> raw bin (rust-objcopy) -> UF2 (uf2conv.py).
        for (chip, stacks) in [
            ("xiao-nrf52840", ["nrf-hal", "embassy-nrf"]),
            ("xiao-nrf52840-sense", ["nrf-hal", "embassy-nrf"]),
        ] {
            for s in stacks {
                assert_eq!(route(chip, s), &["rust-objcopy", "uf2conv"], "{chip}/{s}");
            }
        }
        // SAMD21: hf2 takes the ELF directly over HID.
        assert_eq!(route("xiao-samd21", "atsamd-hal"), &["hf2"]);
        // esp-idf: the template's cargo runner is espflash, and FLASHING is not a
        // live axis there, so it is stack data.
        assert_eq!(route("xiao-esp32c3", "esp-idf"), &["espflash"]);
        // esp-hal: empty — the chosen flashing tool decides (see the model test).
        assert!(route("xiao-esp32c3", "esp-hal").is_empty());
        // Every non-ESP route is USB/UF2: no probe-rs, because there is no SWD probe.
        for v in VARIANTS {
            if v.is_esp() {
                continue;
            }
            for s in v.stacks {
                assert!(
                    !s.flash_prereqs.contains(&"probe-rs"),
                    "{}/{} default flash route must not need a debug probe",
                    v.id, s.id
                );
            }
        }
    }

    /// Only the ESP `std` stack carries extra build tools, and it carries the
    /// three the ESP-IDF build genuinely needs on the host. cmake/ninja are
    /// deliberately absent: esp-idf-sys installs them itself via `idf_tools.py`.
    #[test]
    fn only_esp_idf_declares_extra_build_tools() {
        for v in VARIANTS {
            for s in v.stacks {
                if s.is_std() {
                    assert_eq!(s.build_prereqs, &["ldproxy", "python3", "git"], "{}/{}", v.id, s.id);
                } else {
                    assert!(s.build_prereqs.is_empty(), "{}/{} needs no extra build tool", v.id, s.id);
                }
                for banned in ["cmake", "ninja"] {
                    assert!(
                        !s.build_prereqs.contains(&banned),
                        "{banned} is bootstrapped by esp-idf-sys, not a host prerequisite"
                    );
                }
            }
        }
    }

    #[test]
    fn verified_hardware_facts() {
        assert_eq!(variant("xiao-esp32s3").unwrap().led.unwrap().active_low, Some(true));
        assert!(variant("xiao-esp32c3").unwrap().led.is_none(), "C3 has no user LED");
        assert_eq!(template("camera-snapshot").unwrap().component, "camera");
    }
}
