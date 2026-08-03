//! Resolved configuration shared by the CLI (M1) and the future TUI (M2), plus
//! all config-time validation (R2: invalid combos rejected with clear errors
//! *before* any backend is invoked, so we never emit a broken project).

use std::path::PathBuf;

use crate::registry::{
    self, Backend, Editor, Need, Prereq, Stack, ToolCategory, ToolChoice, Variant,
};

/// Whether µFerris BSP support can be toggled on for a `(variant, stack)` right
/// now. Additive-support UX (M2 redesign, point 6): the toggle is greyed out
/// unless the variant is µFerris-eligible, the stack offers µFerris (esp-hal
/// only — never `esp-idf`/std, never non-ESP), AND there is no current µFerris
/// gap for the variant (e.g. the S3 upstream gap). Pure — derived entirely from
/// registry eligibility (R4), so the TUI grey-out logic fabricates no new data.
pub fn uferris_available(variant: &Variant, stack: &Stack) -> bool {
    variant.uferris_eligible() && stack.uferris && variant.uferris_gap.is_none()
}

#[derive(Debug, Clone)]
pub struct UferrisOpts {
    /// Megalops power board (INA219 + SD). `embassy` is always on in µFerris mode.
    pub power_board: bool,
    /// Whether the µFerris project is async (embassy). Default ON — the only
    /// verified µFerris path (uferris-bsp 0.1.3's examples are embassy-based).
    /// Async-off is a documented gap: there is no verified non-async µFerris body,
    /// so `Config::resolve` refuses `async_on = false` (see NOTES.md). Kept here so
    /// the summary can state it explicitly.
    pub async_on: bool,
}

/// Whether the esp-generate option groups (flashing/logging/framework) are a live,
/// editable axis for this config: the esp-hal (`EspGenerate`) backend, on the
/// implicit base project (no component add-on), with µFerris off. Component add-ons
/// pin the option set to their own `esp_options` (groups locked); the std/non-ESP
/// stacks and µFerris don't use the groups at all. Pure over the registry (R4).
pub fn option_groups_apply(cfg: &Config) -> bool {
    cfg.uferris.is_none()
        && matches!(cfg.stack.backend, Backend::EspGenerate)
        && registry::is_base_template(&cfg.template_id)
}

/// Whether one universal toolchain category is a *live*, user-selectable axis for
/// a stack — i.e. picking a value has a real effect. Derived purely from the
/// backend (R4):
///  - **esp-hal**: all three (they become esp-generate `-o` flags).
///  - **esp-idf**: none (the template fixes the toolchain).
///  - **non-ESP scaffold (M4)**: all three.
///    - FRAMEWORK: `none` emits the bare-metal body, `embassy` the executor body
///      + the HAL's time driver.
///    - LOGGING: `defmt` wires the defmt global logger + `defmt::info!` into the
///      scaffold (deps, the `-Tdefmt.x` link arg, the base body); `none` does not.
///    - FLASHING: does not change the emitted code, but it is still a real choice —
///      it selects the flash route reported in the prerequisites/summary (the
///      no-probe USB/UF2 tool vs `probe-rs`).
pub fn category_live(stack: &Stack, cat: ToolCategory) -> bool {
    let _ = cat;
    match stack.backend {
        Backend::EspGenerate => true,
        Backend::CargoGenerate => false,
        Backend::NonEspScaffold => true,
    }
}

/// The reason one universal toolchain category is not editable for a stack, or
/// `None` when it is live. Counterpart of [`category_live`]. Only esp-idf reaches
/// the non-`None` case now — its whole toolchain is fixed by the template.
pub fn category_grey_reason(stack: &Stack, cat: ToolCategory) -> Option<&'static str> {
    if category_live(stack, cat) {
        return None;
    }
    match stack.backend {
        Backend::CargoGenerate => Some("configured by the esp-idf template"),
        Backend::EspGenerate | Backend::NonEspScaffold => None,
    }
}

/// The note shown next to a flashing choice when it drives the board over an
/// **external** SWD debug probe on this stack. Only `probe-rs` needs a probe, and
/// only where the chip has no built-in debug interface — so it fires on the
/// Cortex-M boards but NOT on ESP (built-in USB-Serial-JTAG). `None` for every
/// no-probe route and on the ESP stacks. Surfaced beside the flasher in both the
/// TUI flashing section and the generation summary.
pub fn flashing_probe_note(stack: &Stack, choice: &ToolChoice) -> Option<&'static str> {
    (choice.needs_probe && !stack.builtin_debug)
        .then_some("requires an external SWD debug probe")
}

/// The note shown next to `--logging defmt` when its output can only be VIEWED
/// with an external debug probe (RTT) — the Cortex-M boards with no built-in
/// debug. defmt still compiles and links without a probe; this only flags how to
/// read it. `None` on the ESP stacks (RTT over the built-in USB-Serial-JTAG) and
/// for any non-defmt logging choice. Mirrors [`flashing_probe_note`].
pub fn defmt_view_note(stack: &Stack, choice: &ToolChoice) -> Option<&'static str> {
    (choice.id == "defmt" && !stack.builtin_debug)
        .then_some("viewed over RTT — needs a debug probe attached")
}

/// Whether the flashing/logging/framework triple should be shown in the summary
/// for this config. True on the esp-hal base project (all three are the live
/// esp-generate axes) and on every non-ESP scaffold project (flashing selects the
/// flash route, logging wires defmt, framework picks the body). False on esp-idf
/// (template-owned) and on esp-hal component add-ons (locked to the template's
/// own option set — those keep the existing framework-only summary line).
pub fn summary_shows_toolchain(cfg: &Config) -> bool {
    option_groups_apply(cfg) || matches!(cfg.stack.backend, Backend::NonEspScaffold)
}

/// The reason a specific catalog choice is unavailable for a `(variant, stack,
/// category)` and the current sibling selections, or `None` if it is a live,
/// selectable choice. Two registry-derived sources:
///  1. the choice is not in this stack's support list → a device-support reason
///     (`rtic` on ESP → "not supported on ESP"; otherwise "not supported on
///     <device>");
///  2. on the live esp-hal axis, adding this choice's INTERNAL esp flags to the
///     siblings' would violate the esp-generate constraint graph (this is what
///     greys `log` when the flashing tool is probe-rs).
/// `sibling_esp_opts` is the union of the other two categories' currently-selected
/// internal esp flags (only meaningful on esp-hal; pass `&[]` elsewhere).
///
/// A third source (M4): the stack lists the choice as real for the device but
/// declares it a *gap* it cannot generate — `embassy` on a blocking HAL, `rtic`
/// anywhere. That reason is more useful than a flat "not supported".
pub fn choice_grey_reason(
    variant: &Variant,
    stack: &Stack,
    cat: ToolCategory,
    choice: &ToolChoice,
    sibling_esp_opts: &[&str],
) -> Option<String> {
    if !stack.support(cat).contains(&choice.id) {
        return Some(unsupported_reason(variant, stack, cat, choice));
    }
    if matches!(cat, ToolCategory::Framework) {
        if let Some(reason) = stack.framework_gap(choice.id) {
            return Some(reason.to_string());
        }
    }
    if matches!(stack.backend, Backend::EspGenerate) {
        let mut candidate: Vec<&str> = sibling_esp_opts.to_vec();
        for o in choice.esp_options {
            if !candidate.contains(o) {
                candidate.push(o);
            }
        }
        if let Err(e) = registry::validate_esp_options(&candidate) {
            return Some(e);
        }
    }
    None
}

/// A short, device-agnostic reason a catalog entry isn't supported by a stack.
/// Curated for the known universal case (RTIC on ESP), generic otherwise. Never
/// mentions a backend generator.
pub fn unsupported_reason(
    variant: &Variant,
    stack: &Stack,
    cat: ToolCategory,
    choice: &ToolChoice,
) -> String {
    let esp = stack.backend.is_esp();
    match (cat, choice.id) {
        (ToolCategory::Framework, "rtic") if esp => "not supported on ESP".to_string(),
        // `log` is the esp-println frontend; there is no generic `log` backend for
        // the Cortex-M boards, so name it as ESP-only rather than a flat
        // "not supported on <device>".
        (ToolCategory::Logging, "log") if !esp => {
            "log is an ESP-only logging frontend (esp-println); use defmt".to_string()
        }
        _ => format!("not supported on {}", variant.display),
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub variant: &'static Variant,
    /// Resolved stack (`esp-hal`, `esp-idf`, …). Selects the backend, target,
    /// and template set for this project (D6).
    pub stack: &'static Stack,
    /// Template id. For µFerris mode this is a µFerris template (e.g. `blinky`);
    /// for the ESP `std` stack the synthetic `base`; otherwise a registry
    /// template (`base-blink`, `camera-snapshot`, …).
    pub template_id: String,
    pub project_name: String,
    pub out_dir: PathBuf,
    pub uferris: Option<UferrisOpts>,
    /// Universal toolchain selections (flashing / logging / framework), as
    /// resolved catalog choices. Always resolved (to the stack/universal default
    /// if unspecified) so the summary can show them, but they only contribute to
    /// the build on the esp-hal base project — see [`option_groups_apply`] and
    /// [`Config::esp_options`], which maps the chosen ids to esp-generate flags
    /// internally.
    pub flashing: &'static ToolChoice,
    pub logging: &'static ToolChoice,
    pub framework: &'static ToolChoice,
    /// Arbitrary extra crates the user asked to bundle in (`--add-crate`, or the
    /// TUI's free-form field). Each entry is a validated `name[@version]` spec;
    /// the patch layer inserts them into `[dependencies]` for ALL stacks (M2
    /// redesign, point 5).
    pub extra_crates: Vec<String>,
    /// Editor to emit rust-analyzer client configuration for. Defaults to
    /// [`registry::default_editor`]; the `none` entry emits no files at all.
    /// Backend-agnostic: the dispatcher writes this for every stack (see
    /// `backend::write_editor_config`).
    pub editor: &'static Editor,
}

/// Whether a structurally-valid selection can actually be generated right now.
///
/// The headless CLI treats a not-yet-implemented backend as a hard error (exit
/// 2). The TUI, by contrast, wants to surface the *entire* registry — including
/// the non-ESP variants whose battery-pack backend is not wired until M4/M6 — so
/// the owner can visually confirm the model. `resolve_for_tui` therefore resolves
/// such selections successfully with `Readiness::Gated(reason)` instead of
/// erroring, so the TUI can still show the summary + equivalent command, then the
/// "not implemented" message in place of generating.
#[derive(Debug, Clone)]
pub enum Readiness {
    /// Backend implemented — generation will run.
    Ready,
    /// Structurally valid, but generation is gated because the backend itself is
    /// not wired yet. Nothing is in this state since M4 landed the non-ESP
    /// scaffold (all three backends are implemented); the mechanism is kept for
    /// the next backend that ships behind its registry data, and the TUI still
    /// renders it.
    #[allow(dead_code)]
    Gated(String),
}

/// Raw, pre-validation inputs (populated by clap or, later, the TUI).
#[derive(Debug)]
pub struct RawConfig {
    pub chip: String,
    /// Stack id (`--stack`). `None` = the variant's default stack.
    pub stack: Option<String>,
    pub template: Option<String>,
    pub name: Option<String>,
    pub out_dir: Option<PathBuf>,
    pub uferris: bool,
    pub power_board: bool,
    /// Disable µFerris async (embassy). Default `false` (async on). `true` is a
    /// documented gap — resolution refuses it (no verified non-async µFerris body).
    pub no_async: bool,
    /// Universal toolchain ids (`--flashing` / `--logging` / `--framework`).
    /// `None` = the stack's default (or the universal default). Validated against
    /// the catalog + the stack's support list.
    pub flashing: Option<String>,
    pub logging: Option<String>,
    pub framework: Option<String>,
    /// Raw `--add-crate` specs (`name[@version]`), repeatable. Validated during
    /// resolution; empty when none were requested.
    pub extra_crates: Vec<String>,
    /// Raw `--editor` id. `None` = not given → [`registry::default_editor`].
    /// Validated against the catalog, in which `none` is a real entry.
    pub editor: Option<String>,
}

impl Config {
    /// Resolve + validate raw inputs into a Config, or return an actionable error.
    /// Headless path (CLI): a not-yet-implemented backend is a hard error.
    pub fn resolve(raw: RawConfig) -> Result<Config, String> {
        match Self::resolve_inner(raw, false)? {
            (cfg, Readiness::Ready) => Ok(cfg),
            // In headless mode the inner resolver never returns `Gated` (it errors
            // instead), but be explicit rather than rely on that invariant.
            (_, Readiness::Gated(msg)) => Err(msg),
        }
    }

    /// TUI-facing resolution. Identical validation to [`Config::resolve`], except
    /// a structurally-valid selection whose backend is not yet implemented (the
    /// non-ESP battery-pack backend) resolves successfully with
    /// [`Readiness::Gated`] instead of erroring — so the TUI can render the
    /// summary + equivalent command for the whole registry and only refuse to
    /// *generate*. Genuine validation errors (unknown chip/stack/template, bad
    /// name, gap template, µFerris ineligibility/gap) are still `Err`.
    pub fn resolve_for_tui(raw: RawConfig) -> Result<(Config, Readiness), String> {
        Self::resolve_inner(raw, true)
    }

    fn resolve_inner(raw: RawConfig, for_tui: bool) -> Result<(Config, Readiness), String> {
        // --- variant ---
        let variant = registry::variant(&raw.chip).ok_or_else(|| {
            let known = registry::VARIANTS
                .iter()
                .map(|v| v.id)
                .collect::<Vec<_>>()
                .join(", ");
            format!("unknown chip '{}'. Known variants: {known}", raw.chip)
        })?;

        // --- stack selection (D6) ---
        let stack = match raw.stack.as_deref() {
            Some(id) => variant.stack(id).ok_or_else(|| {
                format!(
                    "stack '{id}' is not supported by '{}'. Supported stacks: {}",
                    variant.id,
                    variant
                        .stacks
                        .iter()
                        .map(|s| s.id)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
            None => variant.default_stack(),
        };

        // --- backend availability ---
        // All three backends are implemented (esp-generate = M1, cargo-generate
        // = M5, the non-ESP direct scaffold = M4), so nothing is gated at the
        // backend level any more. The `Gated` path is kept for the next backend
        // that lands ahead of its stacks; narrower gaps (templates, frameworks,
        // µFerris) are hard errors in both modes, exactly as before.
        let _ = for_tui;
        let readiness = Readiness::Ready;

        // --- µFerris eligibility + power_board implies uferris ---
        let uferris_requested = raw.uferris || raw.power_board;
        // µFerris is a no_std/esp-hal BSP — never available on the `esp-idf` std
        // stack. Check this first so `--uferris --stack esp-idf` on an otherwise
        // eligible ESP variant reports the stack incompatibility, not a gap.
        if uferris_requested && !stack.uferris {
            return Err(format!(
                "µFerris BSP support is not available on the '{}' stack. µFerris is a no_std/esp-hal BSP; \
                 use the default esp-hal stack (drop --stack esp-idf/--std).",
                stack.id
            ));
        }
        let uferris = if uferris_requested {
            if !variant.uferris_eligible() {
                return Err(format!(
                    "µFerris BSP support is not available for '{}'. Eligible variants: {}",
                    variant.id,
                    registry::VARIANTS
                        .iter()
                        .filter(|v| v.uferris_eligible())
                        .map(|v| v.id)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Some(reason) = variant.uferris_gap {
                return Err(format!(
                    "µFerris BSP support is not available for '{}': {reason}",
                    variant.id
                ));
            }
            // Async-off µFerris is a documented gap: uferris-bsp 0.1.3's examples
            // are embassy-based and that is the only verified body. Refuse rather
            // than fabricate a non-async µFerris path (see NOTES.md).
            if raw.no_async {
                return Err(
                    "µFerris async (embassy) support cannot be disabled: there is no verified \
                     non-async µFerris path in uferris-bsp 0.1.3 (documented gap). Leave async on."
                        .to_string(),
                );
            }
            Some(UferrisOpts {
                power_board: raw.power_board,
                async_on: true,
            })
        } else {
            None
        };

        // --- template selection + applicability (stack-aware) ---
        let template_id = if uferris.is_some() {
            let t = raw.template.as_deref().unwrap_or("blinky");
            if !registry::UFERRIS_TEMPLATES.contains(&t) {
                return Err(format!(
                    "unknown µFerris template '{t}'. Available: {}",
                    registry::UFERRIS_TEMPLATES.join(", ")
                ));
            }
            t.to_string()
        } else if matches!(stack.backend, Backend::CargoGenerate) {
            // ESP `std` stack: base-template only for now (R9/D6). Component
            // templates (camera/mic/sd) are not offered on esp-idf yet.
            let t = raw.template.as_deref().unwrap_or("base");
            if !stack.templates.contains(&t) {
                return Err(format!(
                    "template '{t}' is not available on the '{}' stack \
                     (the esp-idf std stack currently ships the base template only). Available: {}",
                    stack.id,
                    stack.templates.join(", ")
                ));
            }
            t.to_string()
        } else {
            let t = match raw.template.as_deref() {
                Some(t) => t,
                None => {
                    // default = first template listed for this variant+stack
                    stack.templates.first().copied().ok_or_else(|| {
                        format!("variant '{}' has no templates on stack '{}'", variant.id, stack.id)
                    })?
                }
            };
            if registry::template(t).is_none() {
                return Err(format!(
                    "unknown template '{t}'. Known templates: {}",
                    registry::TEMPLATES
                        .iter()
                        .map(|x| x.id)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !stack.templates.contains(&t) {
                let comp = registry::template(t).map(|m| m.component).unwrap_or("");
                return Err(format!(
                    "template '{t}' (onboard component: {comp}) is not available for '{}' \
                     (this variant has no matching onboard hardware). Available for '{}': {}",
                    variant.id,
                    variant.id,
                    stack.templates.join(", ")
                ));
            }
            if let Some(reason) = registry::template(t).and_then(|tt| tt.gap) {
                return Err(format!("template '{t}' is currently unavailable: {reason}"));
            }
            t.to_string()
        };

        // --- project name / out dir ---
        // The crate name is the final path component of the output dir (that is
        // what esp-generate uses as the NAME), so derive it from out_dir to keep
        // the two consistent.
        let name =
            raw.name
                .clone()
                .unwrap_or_else(|| default_name(variant, &template_id, uferris.is_some()));
        let out_dir = raw.out_dir.clone().unwrap_or_else(|| PathBuf::from(&name));
        let project_name = out_dir
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("output path '{}' has no final component", out_dir.display()))?;
        validate_crate_name(&project_name)?;

        // --- extra crates (--add-crate / TUI free-form) ---
        // Validate each spec now (before any backend runs) so a bad crate name or
        // empty version fails at config time, and store the normalised specs so
        // `equivalent_command` reproduces them exactly. Applies to ALL stacks.
        let mut extra_crates = Vec::new();
        for spec in &raw.extra_crates {
            let (_name, normalised) = parse_crate_spec(spec)?;
            extra_crates.push(normalised);
        }

        // --- editor support (--editor) ---
        // Backend-agnostic (the spurious `test`-crate diagnostic is identical on
        // every stack), so this is resolved without consulting variant/stack at all
        // — there is no stack on which `--editor` is refused.
        let editor = match raw.editor.as_deref().map(str::trim) {
            None => registry::default_editor(),
            Some(id) => registry::editor(id).ok_or_else(|| {
                format!(
                    "unknown --editor value '{id}'. Available: {}",
                    registry::EDITORS.iter().map(|e| e.id).collect::<Vec<_>>().join(", ")
                )
            })?,
        };

        // --- universal toolchain catalogs (flashing / logging / framework) ---
        // The three catalogs are device-agnostic; a stack's support list decides
        // which entries apply. They are a live, selectable axis only on the esp-hal
        // (EspGenerate) backend — on any other stack the toolchain is owned by the
        // template/backend, so an explicit flag there is rejected. Each chosen id
        // is mapped to esp-generate `-o` flags INTERNALLY by `esp_options()`; those
        // flag names never leave this module. A choice not in the stack's support
        // list (e.g. RTIC on esp-hal) is rejected with a support reason.
        let resolve_cat =
            |cat: ToolCategory, raw_val: Option<&str>| -> Result<&'static ToolChoice, String> {
                match raw_val {
                    None => Ok(stack.default_tool(cat).unwrap_or_else(|| cat.default_choice())),
                    Some(id) => {
                        let choice = registry::tool_choice(cat, id).ok_or_else(|| {
                            format!(
                                "unknown --{} value '{id}'. Available: {}",
                                cat.flag(),
                                cat.catalog().iter().map(|c| c.id).collect::<Vec<_>>().join(", ")
                            )
                        })?;
                        if !stack.support(cat).contains(&choice.id) {
                            return Err(format!(
                                "--{} '{id}' is not supported on the '{}' stack ({}). Supported: {}",
                                cat.flag(),
                                stack.id,
                                unsupported_reason(variant, stack, cat, choice),
                                if stack.support(cat).is_empty() {
                                    "none (configured by the template)".to_string()
                                } else {
                                    stack.support(cat).join(", ")
                                }
                            ));
                        }
                        Ok(choice)
                    }
                }
            };
        for (cat, val) in [
            (ToolCategory::Flashing, &raw.flashing),
            (ToolCategory::Logging, &raw.logging),
            (ToolCategory::Framework, &raw.framework),
        ] {
            if val.is_some() && !category_live(stack, cat) {
                return Err(format!(
                    "--{} does not apply to the '{}' stack ({}); it is not user-selectable here",
                    cat.flag(),
                    stack.id,
                    category_grey_reason(stack, cat).unwrap_or("not a live axis")
                ));
            }
        }
        let flashing = resolve_cat(ToolCategory::Flashing, raw.flashing.as_deref())?;
        let logging = resolve_cat(ToolCategory::Logging, raw.logging.as_deref())?;
        let framework = resolve_cat(ToolCategory::Framework, raw.framework.as_deref())?;

        // A framework the stack lists for the device but cannot generate is a
        // declared gap (registry data, compile-verified) — refuse rather than
        // silently emit a project that does not use it. This is what rejects
        // `embassy` on a blocking HAL and `rtic` everywhere.
        if let Some(reason) = stack.framework_gap(framework.id) {
            return Err(format!(
                "framework '{}' is not available on the '{}' stack of '{}': {reason}",
                framework.id, stack.id, variant.id
            ));
        }

        let cfg = Config {
            variant,
            stack,
            template_id,
            project_name,
            out_dir,
            uferris,
            flashing,
            logging,
            framework,
            extra_crates,
            editor,
        };

        // Assembled base option set must satisfy the constraint graph before any
        // backend runs (e.g. `--flashing probe-rs --logging log` is rejected here,
        // exit 2). Only meaningful on the esp-hal base project.
        if option_groups_apply(&cfg) {
            registry::validate_esp_options(&cfg.esp_options()).map_err(|e| {
                format!(
                    "the selected esp-generate options conflict: {e}. \
                     (flashing={}, logging={}, framework={})",
                    cfg.flashing.id, cfg.logging.id, cfg.framework.id
                )
            })?;
        }

        Ok((cfg, readiness))
    }

    /// The esp-generate `-o` option set for this config.
    ///
    /// - **µFerris** → its own fixed set (`UFERRIS_ESP_OPTIONS`).
    /// - **esp-hal base project** → the flashing/logging/framework group
    ///   selections (esp-generate owns the base main; the groups fully determine
    ///   the flags — the "esp-generate owns the base" decision).
    /// - **esp-hal component add-on** → the template's own `esp_options` (the
    ///   component is compile-verified against exactly that set; the groups are
    ///   locked/greyed, so their union reduces to the template set).
    /// - **std stack** → unused (no `-o`; the cargo-generate backend ignores this).
    pub fn esp_options(&self) -> Vec<&'static str> {
        if self.uferris.is_some() {
            registry::UFERRIS_ESP_OPTIONS.to_vec()
        } else if matches!(self.stack.backend, Backend::EspGenerate)
            && registry::is_base_template(&self.template_id)
        {
            let mut v: Vec<&'static str> = Vec::new();
            for o in self
                .flashing
                .esp_options
                .iter()
                .chain(self.logging.esp_options)
                .chain(self.framework.esp_options)
            {
                if !v.contains(o) {
                    v.push(o);
                }
            }
            v
        } else {
            registry::template(&self.template_id)
                .map(|t| t.esp_options.to_vec())
                .unwrap_or_default()
        }
    }
}

/// The host prerequisites a resolved config actually needs, in display order
/// (build group first, then flash), de-duplicated.
///
/// Pure over the registry (R4) — no chip/stack/tool knowledge lives here, only
/// the three places a prerequisite can come from:
///
///  1. **the target/toolchain**, derived from `stack.target`, whose triple is
///     itself a [`registry::PREREQS`] id. This is what makes an Xtensa project ask
///     for espup while a RISC-V one asks for `rustup target add`, with no branch
///     on `arch` anywhere;
///  2. **the stack's own build tools** (`Stack::build_prereqs`) — e.g. esp-idf's
///     `ldproxy`/`python3`/`git`;
///  3. **the flash route**. On a stack where FLASHING is a live user choice
///     (esp-hal AND the non-ESP scaffolds) it follows the chosen
///     [`registry::ToolChoice`], so `--flashing probe-rs` swaps the board's
///     default no-probe route out for probe-rs. Only esp-idf fixes the route in
///     `Stack::flash_prereqs` (its template's cargo runner is espflash).
///
/// µFerris adds nothing: `uferris-bsp` is an ordinary cargo dependency on the
/// esp-hal stack, so a µFerris project needs exactly what the same chip's plain
/// esp-hal project needs (pinned by `uferris_adds_no_host_prerequisite`).
pub fn prerequisites(cfg: &Config) -> Vec<&'static Prereq> {
    let mut out: Vec<&'static Prereq> = Vec::new();
    let push = |out: &mut Vec<&'static Prereq>, id: &str| {
        if let Some(p) = registry::prereq(id) {
            if !out.iter().any(|q| q.id == p.id) {
                out.push(p);
            }
        }
    };

    // (1) target / toolchain, then (2) the stack's extra build tools.
    push(&mut out, cfg.stack.target);
    for id in cfg.stack.build_prereqs {
        push(&mut out, id);
    }

    // (3) the flash route.
    if category_live(cfg.stack, ToolCategory::Flashing) {
        for id in cfg.flashing.prereqs {
            push(&mut out, id);
        }
    }
    for id in cfg.stack.flash_prereqs {
        push(&mut out, id);
    }

    // Build before flash, stable within each group (the order above).
    out.sort_by_key(|p| match p.need {
        Need::Build => 0,
        Need::Flash => 1,
    });
    out
}

/// The default template id for a resolved config's `(variant, stack, mode)` —
/// i.e. what `Config::resolve` would pick if `--template` were omitted. Used by
/// [`equivalent_command`] to decide whether `--template` needs to appear.
fn default_template_id(cfg: &Config) -> &'static str {
    if cfg.uferris.is_some() {
        // µFerris default template (the only one) — never needs `--template`.
        "blinky"
    } else if matches!(cfg.stack.backend, Backend::CargoGenerate) {
        // ESP `std` stack: the synthetic `base` id.
        "base"
    } else {
        cfg.stack.templates.first().copied().unwrap_or("")
    }
}

/// Render the headless `xiao-generate` invocation that reproduces `cfg`'s
/// toolchain selection (the M2 parity acceptance feature). Pure function — no I/O,
/// no side effects — so it is trivially unit-testable and shown on the TUI summary
/// screen. Uses the universal flags only and omits the output location (default
/// `<name>` dir), so no esp-generate `-o` appears:
///
/// `xiao-generate --chip <id> [--stack <id> if not default] [--template <id> if
/// not the default] [--flashing/--logging/--framework if not default] [--uferris]
/// [--power-board]`
pub fn equivalent_command(cfg: &Config) -> String {
    let mut parts = vec![
        "xiao-generate".to_string(),
        format!("--chip {}", cfg.variant.id),
    ];

    // --stack only when it differs from the variant's default stack.
    if cfg.stack.id != cfg.variant.default_stack().id {
        parts.push(format!("--stack {}", cfg.stack.id));
    }

    if cfg.uferris.is_some() {
        parts.push("--uferris".to_string());
        if cfg.uferris.as_ref().map(|u| u.power_board).unwrap_or(false) {
            parts.push("--power-board".to_string());
        }
    } else if cfg.template_id != default_template_id(cfg) {
        // --template only when it differs from the variant+stack default.
        parts.push(format!("--template {}", cfg.template_id));
    }

    // Universal toolchain flags — only for categories that are a live axis on
    // this stack (all three on the esp-hal base project AND the non-ESP scaffold),
    // and only when a choice differs from the STACK's default (keep the command
    // terse but faithful/runnable). The default to compare against is the stack's
    // own — on non-ESP the flashing default is the board's no-probe route
    // (picotool/uf2/hf2), not the universal espflash — so an unmodified selection
    // never appears. No esp-generate `-o` ever appears.
    let esp_groups = option_groups_apply(cfg);
    for (cat, chosen) in [
        (ToolCategory::Flashing, cfg.flashing),
        (ToolCategory::Logging, cfg.logging),
        (ToolCategory::Framework, cfg.framework),
    ] {
        // On esp-hal a component add-on locks the groups, so they must not be
        // re-emitted; elsewhere liveness alone decides.
        let live = if matches!(cfg.stack.backend, Backend::EspGenerate) {
            esp_groups
        } else {
            category_live(cfg.stack, cat)
        };
        let default_id = cfg
            .stack
            .default_tool(cat)
            .map(|c| c.id)
            .unwrap_or_else(|| cat.default_choice().id);
        if live && chosen.id != default_id {
            parts.push(format!("--{} {}", cat.flag(), chosen.id));
        }
    }

    // Extra crates: one --add-crate per bundled crate, in the order given
    // (headless-only in v3 — the TUI no longer surfaces extra crates).
    for spec in &cfg.extra_crates {
        parts.push(format!("--add-crate {spec}"));
    }

    // Editor: omitted while it is the catalog default (VS Code), exactly like the
    // toolchain flags above. `--editor none` is a normal catalog value, so it
    // round-trips through the same branch.
    if cfg.editor.id != registry::default_editor().id {
        parts.push(format!("--editor {}", cfg.editor.id));
    }

    // Output location is omitted: generation uses the default `<name>` dir in the
    // current directory, so the command carries no `-o` (v3 UNIFY).
    parts.join(" ")
}

fn default_name(variant: &Variant, template: &str, uferris: bool) -> String {
    let suffix = if uferris { "uferris" } else { template };
    // e.g. "xiao-esp32s3-sense-camera-snapshot"
    format!("{}-{}", variant.id, suffix)
}

/// crates/cargo package-name rules (relaxed): non-empty, ascii alnum + `-`/`_`,
/// not starting with a digit. Keeps generated projects `cargo`-valid.
fn validate_crate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("project name is empty".to_string());
    }
    if name.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return Err(format!("project name '{name}' must not start with a digit"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "project name '{name}' may only contain ASCII letters, digits, '-' and '_'"
        ));
    }
    Ok(())
}

/// Parse + validate a `--add-crate` spec of the form `name[@version]` (M2
/// redesign, point 5). Returns the crate name and a normalised, re-emittable
/// spec string (`name` or `name@version`) — the latter is stored on the resolved
/// `Config` so [`equivalent_command`] can reproduce the flags exactly. The crate
/// name must be a valid cargo dependency name (ASCII alnum + `-`/`_`); an `@`
/// with nothing after it is rejected.
pub fn parse_crate_spec(spec: &str) -> Result<(String, String), String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("--add-crate was given an empty crate spec".to_string());
    }
    let (name, version) = match spec.split_once('@') {
        Some((n, v)) => (n.trim(), Some(v.trim())),
        None => (spec, None),
    };
    if name.is_empty() {
        return Err(format!("--add-crate spec '{spec}' has no crate name"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "--add-crate crate name '{name}' may only contain ASCII letters, digits, '-' and '_'"
        ));
    }
    match version {
        Some(v) if v.is_empty() => Err(format!(
            "--add-crate spec '{spec}' has an empty version after '@'"
        )),
        Some(v) => Ok((name.to_string(), format!("{name}@{v}"))),
        None => Ok((name.to_string(), name.to_string())),
    }
}

/// The `(name, TOML-value-literal)` an already-validated `--add-crate` spec adds
/// to `[dependencies]`. A bare `name` pins to `"*"` (any version); `name@version`
/// pins to that exact requirement. Called by the patch layer.
pub fn crate_dependency(spec: &str) -> (String, String) {
    match spec.split_once('@') {
        Some((name, version)) => (name.to_string(), format!("\"{version}\"")),
        None => (spec.to_string(), "\"*\"".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(chip: &str) -> RawConfig {
        RawConfig {
            chip: chip.to_string(),
            stack: None,
            template: None,
            name: None,
            out_dir: None,
            uferris: false,
            power_board: false,
            no_async: false,
            flashing: None,
            logging: None,
            framework: None,
            extra_crates: Vec::new(),
            editor: None,
        }
    }

    #[test]
    fn unknown_chip_rejected() {
        assert!(Config::resolve(raw("xiao-nope")).is_err());
    }

    // ---- M4: non-ESP resolution ----

    #[test]
    fn non_esp_variants_resolve_headlessly() {
        // The direct scaffold is implemented, so every non-ESP variant resolves
        // from `--chip` alone, on its default (blocking-HAL) stack, bare-metal.
        for (chip, stack, target) in [
            ("xiao-rp2040", "rp2040-hal", "thumbv6m-none-eabi"),
            ("xiao-rp2350", "rp235x-hal", "thumbv8m.main-none-eabihf"),
            ("xiao-nrf52840", "nrf-hal", "thumbv7em-none-eabihf"),
            ("xiao-nrf52840-sense", "nrf-hal", "thumbv7em-none-eabihf"),
            ("xiao-samd21", "atsamd-hal", "thumbv6m-none-eabi"),
        ] {
            let c = Config::resolve(raw(chip)).unwrap_or_else(|e| panic!("{chip}: {e}"));
            assert_eq!(c.stack.id, stack);
            assert_eq!(c.stack.target, target);
            assert_eq!(c.template_id, "base-blink");
            assert_eq!(c.framework.id, "none");
            assert!(c.stack.scaffold.is_some(), "{chip} must carry scaffold data");
        }
    }

    #[test]
    fn embassy_framework_accepted_on_an_embassy_hal() {
        for (chip, stack) in [
            ("xiao-rp2040", "embassy-rp"),
            ("xiao-rp2350", "embassy-rp"),
            ("xiao-nrf52840", "embassy-nrf"),
            ("xiao-nrf52840-sense", "embassy-nrf"),
        ] {
            let mut r = raw(chip);
            r.stack = Some(stack.to_string());
            r.framework = Some("embassy".to_string());
            let c = Config::resolve(r).unwrap_or_else(|e| panic!("{chip}/{stack}: {e}"));
            assert_eq!(c.framework.id, "embassy");
        }
    }

    #[test]
    fn embassy_framework_refused_on_a_blocking_hal() {
        // The M4 rule: framework = embassy REQUIRES an embassy HAL. Compile-
        // verified — a blocking HAL cannot get a working embassy-time driver.
        for (chip, stack, pointer) in [
            ("xiao-rp2040", "rp2040-hal", "embassy-rp"),
            ("xiao-rp2350", "rp235x-hal", "embassy-rp"),
            ("xiao-nrf52840", "nrf-hal", "embassy-nrf"),
        ] {
            let mut r = raw(chip);
            r.stack = Some(stack.to_string());
            r.framework = Some("embassy".to_string());
            let e = Config::resolve(r).unwrap_err();
            assert!(e.contains("embassy HAL"), "{chip}/{stack} reason: {e}");
            assert!(e.contains(pointer), "{chip}/{stack} should point at {pointer}: {e}");
        }
    }

    #[test]
    fn rtic_framework_refused_on_non_esp_with_a_gap_reason() {
        let mut r = raw("xiao-rp2040");
        r.framework = Some("rtic".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("RTIC"), "got: {e}");
        assert!(e.contains("not implemented"), "got: {e}");
    }

    #[test]
    fn flashing_and_logging_are_a_live_axis_on_non_esp() {
        // All three toolchain axes are live on the non-ESP scaffold now.
        let rp = registry::variant("xiao-rp2040").unwrap().default_stack();
        assert!(category_live(rp, ToolCategory::Flashing));
        assert!(category_live(rp, ToolCategory::Logging));
        assert!(category_live(rp, ToolCategory::Framework));

        // `--flashing probe-rs` is ACCEPTED on a Cortex-M board — the external-probe
        // alternative to the default USB/UF2 route.
        let mut r = raw("xiao-nrf52840");
        r.flashing = Some("probe-rs".to_string());
        assert_eq!(Config::resolve(r).unwrap().flashing.id, "probe-rs");

        // The DEFAULT flashing route is the board's no-probe tool, never espflash.
        assert_eq!(Config::resolve(raw("xiao-rp2040")).unwrap().flashing.id, "picotool");
        assert_eq!(Config::resolve(raw("xiao-nrf52840")).unwrap().flashing.id, "uf2");
        assert_eq!(Config::resolve(raw("xiao-samd21")).unwrap().flashing.id, "hf2");

        // `--logging defmt` is accepted; `--logging log` is refused as ESP-only.
        let mut r = raw("xiao-rp2040");
        r.logging = Some("defmt".to_string());
        assert_eq!(Config::resolve(r).unwrap().logging.id, "defmt");
        let mut r = raw("xiao-rp2040");
        r.logging = Some("log".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("--logging") && e.contains("log"), "got: {e}");
        assert!(e.contains("ESP-only"), "the `log` refusal must name it ESP-only: {e}");
        // …and the default logging is `none` (no defmt unless asked).
        assert_eq!(Config::resolve(raw("xiao-rp2040")).unwrap().logging.id, "none");
    }

    #[test]
    fn probe_and_defmt_notes_fire_only_off_esp() {
        let rp = registry::variant("xiao-rp2040").unwrap().default_stack();
        let esp = registry::variant("xiao-esp32c6").unwrap().stack("esp-hal").unwrap();
        let probe_rs = registry::tool_choice(ToolCategory::Flashing, "probe-rs").unwrap();
        let picotool = registry::tool_choice(ToolCategory::Flashing, "picotool").unwrap();
        let defmt = registry::tool_choice(ToolCategory::Logging, "defmt").unwrap();
        let none = registry::tool_choice(ToolCategory::Logging, "none").unwrap();

        // Cortex-M: probe-rs needs an external probe; no-probe routes and non-defmt
        // logging carry no note.
        assert!(flashing_probe_note(rp, probe_rs).is_some());
        assert!(flashing_probe_note(rp, picotool).is_none());
        assert!(defmt_view_note(rp, defmt).is_some());
        assert!(defmt_view_note(rp, none).is_none());
        // ESP: built-in USB-Serial-JTAG, so even probe-rs / defmt need no external
        // probe — no note.
        assert!(flashing_probe_note(esp, probe_rs).is_none());
        assert!(defmt_view_note(esp, defmt).is_none());
    }

    #[test]
    fn non_esp_flashing_choice_drives_the_flash_prerequisite() {
        // Default nRF route is the UF2 pipeline (rust-objcopy + uf2conv), no probe.
        let default = prereq_ids("xiao-nrf52840", |_| {});
        assert!(default.contains(&"rust-objcopy") && default.contains(&"uf2conv"), "{default:?}");
        assert!(!default.contains(&"probe-rs"), "{default:?}");
        // --flashing probe-rs swaps the whole route for the probe.
        let probe = prereq_ids("xiao-nrf52840", |r| r.flashing = Some("probe-rs".into()));
        assert!(probe.contains(&"probe-rs"), "{probe:?}");
        assert!(!probe.contains(&"rust-objcopy") && !probe.contains(&"uf2conv"), "{probe:?}");
        // RP default is picotool; probe-rs swaps it there too.
        assert!(prereq_ids("xiao-rp2040", |_| {}).contains(&"picotool"));
        let rp_probe = prereq_ids("xiao-rp2040", |r| r.flashing = Some("probe-rs".into()));
        assert!(rp_probe.contains(&"probe-rs") && !rp_probe.contains(&"picotool"), "{rp_probe:?}");
    }

    #[test]
    fn non_esp_choice_grey_reason_reports_the_framework_gap() {
        let rp = registry::variant("xiao-rp2040").unwrap();
        let blocking = rp.stack("rp2040-hal").unwrap();
        let embassy_stack = rp.stack("embassy-rp").unwrap();
        let embassy = registry::tool_choice(ToolCategory::Framework, "embassy").unwrap();
        let none = registry::tool_choice(ToolCategory::Framework, "none").unwrap();
        assert!(
            choice_grey_reason(rp, blocking, ToolCategory::Framework, embassy, &[])
                .unwrap()
                .contains("embassy HAL")
        );
        assert!(choice_grey_reason(rp, embassy_stack, ToolCategory::Framework, embassy, &[]).is_none());
        assert!(choice_grey_reason(rp, blocking, ToolCategory::Framework, none, &[]).is_none());
    }

    #[test]
    fn camera_template_rejected_on_c3() {
        let mut r = raw("xiao-esp32c3");
        r.template = Some("camera-snapshot".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("not available"), "got: {e}");
    }

    #[test]
    fn mic_template_is_a_documented_gap() {
        let mut r = raw("xiao-esp32s3-sense");
        r.template = Some("mic-capture".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("PDM"), "gap reason should mention PDM; got: {e}");
    }

    #[test]
    fn uferris_on_s3_is_a_documented_gap() {
        let mut r = raw("xiao-esp32s3");
        r.uferris = true;
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("S3") || e.contains("uferris-bsp"), "got: {e}");
    }

    #[test]
    fn uferris_on_c3_resolves_with_expected_options() {
        let mut r = raw("xiao-esp32c3");
        r.uferris = true;
        let c = Config::resolve(r).unwrap();
        assert!(c.uferris.is_some());
        assert_eq!(c.template_id, "blinky");
        let opts = c.esp_options();
        assert!(opts.contains(&"embassy") && opts.contains(&"esp-backtrace"), "got: {opts:?}");
    }

    #[test]
    fn c3_default_template_is_hello() {
        let c = Config::resolve(raw("xiao-esp32c3")).unwrap();
        assert_eq!(c.template_id, "base-hello");
    }

    #[test]
    fn s3_sense_camera_and_sd_resolve() {
        for t in ["camera-snapshot", "sd-list", "base-blink"] {
            let mut r = raw("xiao-esp32s3-sense");
            r.template = Some(t.to_string());
            assert!(Config::resolve(r).is_ok(), "{t} should resolve on S3 Sense");
        }
    }

    #[test]
    fn power_board_implies_uferris() {
        let mut r = raw("xiao-esp32c3");
        r.power_board = true;
        let c = Config::resolve(r).unwrap();
        assert!(c.uferris.as_ref().unwrap().power_board);
    }

    // ---- M5: stack axis ----

    #[test]
    fn esp_default_stack_is_esp_hal() {
        let c = Config::resolve(raw("xiao-esp32c3")).unwrap();
        assert_eq!(c.stack.id, "esp-hal");
        assert_eq!(c.stack.target, "riscv32imc-unknown-none-elf");
    }

    #[test]
    fn esp_idf_stack_selectable_on_esp_variants() {
        let mut r = raw("xiao-esp32s3");
        r.stack = Some("esp-idf".to_string());
        let c = Config::resolve(r).unwrap();
        assert_eq!(c.stack.id, "esp-idf");
        assert_eq!(c.stack.target, "xtensa-esp32s3-espidf");
        assert_eq!(c.template_id, "base");
    }

    #[test]
    fn esp_idf_default_template_is_base() {
        for chip in ["xiao-esp32c3", "xiao-esp32c6", "xiao-esp32s3", "xiao-esp32s3-sense"] {
            let mut r = raw(chip);
            r.stack = Some("esp-idf".to_string());
            let c = Config::resolve(r).unwrap();
            assert_eq!(c.template_id, "base", "{chip} std default template");
        }
    }

    #[test]
    fn std_stack_rejected_on_non_esp() {
        // rp2040 has no `esp-idf` stack → stack resolution rejects it.
        let mut r = raw("xiao-rp2040");
        r.stack = Some("esp-idf".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("not supported"), "got: {e}");
    }

    #[test]
    fn uferris_rejected_on_esp_idf_stack() {
        let mut r = raw("xiao-esp32c3");
        r.stack = Some("esp-idf".to_string());
        r.uferris = true;
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("esp-idf") || e.contains("no_std"), "got: {e}");
    }

    #[test]
    fn component_template_rejected_on_esp_idf_stack() {
        // camera exists on S3-Sense's esp-hal stack but not on esp-idf.
        let mut r = raw("xiao-esp32s3-sense");
        r.stack = Some("esp-idf".to_string());
        r.template = Some("camera-snapshot".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("esp-idf") && e.contains("not available"), "got: {e}");
    }

    #[test]
    fn unknown_stack_rejected_with_supported_list() {
        let mut r = raw("xiao-esp32c3");
        r.stack = Some("bogus".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("esp-hal") && e.contains("esp-idf"), "got: {e}");
    }

    // ---- M2: parity — equivalent_command (the testable core of the acceptance) ----

    /// Build a config with a fixed out-dir so the `-o` component is deterministic.
    fn cmd_for(build: impl FnOnce(&mut RawConfig)) -> String {
        let mut r = raw("xiao-esp32c3");
        r.out_dir = Some(PathBuf::from("proj"));
        build(&mut r);
        let c = Config::resolve(r).unwrap();
        equivalent_command(&c)
    }

    #[test]
    fn parity_esp_no_std_base_omits_defaults() {
        // C6 base-blink is the default stack + default template → neither flag,
        // and no `-o` (default output).
        let cmd = cmd_for(|r| r.chip = "xiao-esp32c6".to_string());
        assert_eq!(cmd, "xiao-generate --chip xiao-esp32c6");
    }

    #[test]
    fn parity_esp_std_shows_stack_but_not_base_template() {
        let cmd = cmd_for(|r| {
            r.chip = "xiao-esp32s3".to_string();
            r.stack = Some("esp-idf".to_string());
        });
        assert_eq!(cmd, "xiao-generate --chip xiao-esp32s3 --stack esp-idf");
    }

    #[test]
    fn parity_s3_sense_camera_shows_template() {
        let cmd = cmd_for(|r| {
            r.chip = "xiao-esp32s3-sense".to_string();
            r.template = Some("camera-snapshot".to_string());
        });
        assert_eq!(
            cmd,
            "xiao-generate --chip xiao-esp32s3-sense --template camera-snapshot"
        );
    }

    #[test]
    fn parity_c3_uferris_shows_uferris_flag() {
        let cmd = cmd_for(|r| r.uferris = true);
        assert_eq!(cmd, "xiao-generate --chip xiao-esp32c3 --uferris");
    }

    #[test]
    fn parity_uferris_power_board_shows_both_flags() {
        let cmd = cmd_for(|r| r.power_board = true);
        assert_eq!(cmd, "xiao-generate --chip xiao-esp32c3 --uferris --power-board");
    }

    #[test]
    fn parity_non_esp_variant_via_tui_resolution() {
        // Non-ESP is Ready since M4, and the equivalent command is exact —
        // rp2040-hal is the default stack, base-blink the default template and
        // `none` the default framework, so none of them appears.
        let mut r = raw("xiao-rp2040");
        r.out_dir = Some(PathBuf::from("proj"));
        let (c, readiness) = Config::resolve_for_tui(r).unwrap();
        assert!(matches!(readiness, Readiness::Ready), "non-ESP generates since M4");
        assert_eq!(equivalent_command(&c), "xiao-generate --chip xiao-rp2040");
    }

    #[test]
    fn parity_non_esp_embassy_stack_shows_stack() {
        let mut r = raw("xiao-rp2040");
        r.stack = Some("embassy-rp".to_string());
        r.out_dir = Some(PathBuf::from("proj"));
        let (c, _) = Config::resolve_for_tui(r).unwrap();
        assert_eq!(
            equivalent_command(&c),
            "xiao-generate --chip xiao-rp2040 --stack embassy-rp"
        );
    }

    #[test]
    fn parity_non_esp_framework_flag_round_trips() {
        // FRAMEWORK is the live toolchain axis on the non-ESP scaffold, so a
        // non-default choice must appear in the reproducible command.
        let mut r = raw("xiao-nrf52840");
        r.stack = Some("embassy-nrf".to_string());
        r.framework = Some("embassy".to_string());
        r.out_dir = Some(PathBuf::from("proj"));
        let c = Config::resolve(r).unwrap();
        assert_eq!(
            equivalent_command(&c),
            "xiao-generate --chip xiao-nrf52840 --stack embassy-nrf --framework embassy"
        );
    }

    #[test]
    fn parity_non_esp_flashing_and_logging_round_trip() {
        // The stack default (picotool / none) is omitted — a bare non-ESP command
        // stays terse even though flashing/logging are now live.
        let c = cmd_for(|r| r.chip = "xiao-rp2040".to_string());
        assert_eq!(c, "xiao-generate --chip xiao-rp2040");
        // probe-rs (non-default flashing) and defmt (non-default logging) DO appear.
        let c = cmd_for(|r| {
            r.chip = "xiao-rp2040".to_string();
            r.flashing = Some("probe-rs".to_string());
            r.logging = Some("defmt".to_string());
        });
        assert_eq!(
            c,
            "xiao-generate --chip xiao-rp2040 --flashing probe-rs --logging defmt"
        );
        // Passing the stack default flashing explicitly still round-trips to terse.
        let c = cmd_for(|r| {
            r.chip = "xiao-nrf52840".to_string();
            r.flashing = Some("uf2".to_string());
        });
        assert_eq!(c, "xiao-generate --chip xiao-nrf52840");
    }

    #[test]
    fn tui_resolution_of_esp_variant_is_ready() {
        let (_, readiness) = Config::resolve_for_tui(raw("xiao-esp32c3")).unwrap();
        assert!(matches!(readiness, Readiness::Ready), "ESP variant should be Ready");
    }

    // ---- M2 redesign: extra crates (--add-crate) ----

    #[test]
    fn parse_crate_spec_accepts_name_and_versioned() {
        assert_eq!(parse_crate_spec("heapless").unwrap(), ("heapless".into(), "heapless".into()));
        assert_eq!(
            parse_crate_spec("serde@1.0").unwrap(),
            ("serde".into(), "serde@1.0".into())
        );
        // whitespace around the spec is tolerated + normalised away.
        assert_eq!(parse_crate_spec("  fugit @ 0.3 ").unwrap().1, "fugit@0.3");
    }

    #[test]
    fn parse_crate_spec_rejects_bad_specs() {
        assert!(parse_crate_spec("").is_err(), "empty");
        assert!(parse_crate_spec("bad name").is_err(), "space in name");
        assert!(parse_crate_spec("serde@").is_err(), "empty version");
        assert!(parse_crate_spec("@1.0").is_err(), "no name");
    }

    #[test]
    fn crate_dependency_builds_toml_literals() {
        assert_eq!(crate_dependency("serde@1.0"), ("serde".into(), "\"1.0\"".into()));
        assert_eq!(crate_dependency("heapless"), ("heapless".into(), "\"*\"".into()));
    }

    #[test]
    fn extra_crates_validated_at_resolve_time() {
        let mut r = raw("xiao-esp32c3");
        r.extra_crates = vec!["not valid".to_string()];
        assert!(Config::resolve(r).is_err(), "invalid crate spec must fail at config time");

        let mut r = raw("xiao-esp32c3");
        r.extra_crates = vec!["heapless".to_string(), "serde@1.0".to_string()];
        let c = Config::resolve(r).unwrap();
        assert_eq!(c.extra_crates, vec!["heapless".to_string(), "serde@1.0".to_string()]);
    }

    #[test]
    fn parity_extra_crates_emit_add_crate_flags() {
        // --add-crate applies to every stack, in order, after the template flags.
        let cmd = cmd_for(|r| {
            r.extra_crates = vec!["heapless".to_string(), "serde@1.0".to_string()];
        });
        assert_eq!(
            cmd,
            "xiao-generate --chip xiao-esp32c3 --add-crate heapless --add-crate serde@1.0"
        );
    }

    #[test]
    fn parity_extra_crates_combine_with_uferris() {
        let cmd = cmd_for(|r| {
            r.uferris = true;
            r.extra_crates = vec!["fugit@0.3".to_string()];
        });
        assert_eq!(
            cmd,
            "xiao-generate --chip xiao-esp32c3 --uferris --add-crate fugit@0.3"
        );
    }

    // ---- M2 redesign: grey-out eligibility (derived from registry) ----

    #[test]
    fn uferris_available_only_on_esp_hal_eligible_no_gap() {
        let c3 = registry::variant("xiao-esp32c3").unwrap();
        let c6 = registry::variant("xiao-esp32c6").unwrap();
        let s3 = registry::variant("xiao-esp32s3").unwrap();
        let rp = registry::variant("xiao-rp2040").unwrap();
        // C3/C6 on esp-hal → available (no gap).
        assert!(uferris_available(c3, c3.stack("esp-hal").unwrap()));
        assert!(uferris_available(c6, c6.stack("esp-hal").unwrap()));
        // esp-idf stack never offers µFerris.
        assert!(!uferris_available(c3, c3.stack("esp-idf").unwrap()));
        // S3 is eligible but has the documented upstream gap → greyed.
        assert!(!uferris_available(s3, s3.stack("esp-hal").unwrap()));
        // non-ESP is not eligible at all.
        assert!(!uferris_available(rp, rp.default_stack()));
    }

    // ---- M2 redesign v2: universal toolchain resolution ----

    #[test]
    fn base_esp_hal_option_set_comes_from_groups() {
        // Default base = espflash/none/none → just esp-backtrace (esp-generate
        // owns the main; the groups fully determine the flags).
        let c = Config::resolve(raw("xiao-esp32c6")).unwrap();
        assert_eq!(c.esp_options(), vec!["esp-backtrace"]);
        assert_eq!(c.flashing.id, "espflash");
        assert_eq!(c.logging.id, "none");
        assert_eq!(c.framework.id, "none");
    }

    #[test]
    fn base_esp_hal_groups_assemble_and_validate() {
        let mut r = raw("xiao-esp32c6");
        r.framework = Some("embassy".to_string());
        r.logging = Some("defmt".to_string());
        let c = Config::resolve(r).unwrap();
        let opts = c.esp_options();
        assert!(opts.contains(&"embassy") && opts.contains(&"unstable-hal"));
        assert!(opts.contains(&"defmt"));
        assert!(opts.contains(&"esp-backtrace")); // espflash default
        registry::validate_esp_options(&opts).unwrap();
    }

    #[test]
    fn probe_rs_flashing_pulls_its_panic_handler() {
        let mut r = raw("xiao-esp32c6");
        r.flashing = Some("probe-rs".to_string());
        let c = Config::resolve(r).unwrap();
        let opts = c.esp_options();
        assert!(opts.contains(&"probe-rs") && opts.contains(&"panic-rtt-target"));
        assert!(!opts.contains(&"esp-backtrace"), "probe-rs replaces the espflash panic handler");
    }

    #[test]
    fn probe_rs_plus_log_is_rejected_at_config_time() {
        let mut r = raw("xiao-esp32c6");
        r.flashing = Some("probe-rs".to_string());
        r.logging = Some("log".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("conflict"), "probe-rs + log must be rejected: {e}");
    }

    #[test]
    fn unknown_group_value_rejected() {
        let mut r = raw("xiao-esp32c6");
        r.framework = Some("tokio".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("unknown --framework"), "got: {e}");
    }

    #[test]
    fn rtic_framework_is_refused_on_esp_hal() {
        // RTIC is a real universal framework, but no ESP stack supports it → the
        // esp-hal stack rejects it with a support reason (not an "invented gap").
        let mut r = raw("xiao-esp32c6");
        r.framework = Some("rtic".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("not supported") && e.contains("rtic"), "got: {e}");
        assert!(e.contains("not supported on ESP"), "should carry the ESP reason: {e}");
    }

    #[test]
    fn group_flags_rejected_on_non_esp_hal_stacks() {
        // esp-idf (std) stack.
        let mut r = raw("xiao-esp32c6");
        r.stack = Some("esp-idf".to_string());
        r.framework = Some("embassy".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("--framework does not apply"), "got: {e}");
        assert!(e.contains("esp-idf template"), "should carry the reason: {e}");
    }

    #[test]
    fn component_addon_option_set_is_the_template_not_the_groups() {
        // camera add-on pins the option set to its own esp_options; a stray group
        // flag doesn't leak in (groups are locked for add-ons). The group flag is
        // still accepted (esp-hal stack) but has no effect on esp_options.
        let mut r = raw("xiao-esp32s3-sense");
        r.template = Some("camera-snapshot".to_string());
        r.logging = Some("defmt".to_string());
        let c = Config::resolve(r).unwrap();
        assert!(!option_groups_apply(&c), "add-on locks the groups");
        assert_eq!(c.esp_options(), vec!["unstable-hal", "esp-backtrace"]);
    }

    #[test]
    fn category_liveness_is_backend_derived() {
        let c6 = registry::variant("xiao-esp32c6").unwrap();
        let esp_hal = c6.stack("esp-hal").unwrap();
        let esp_idf = c6.stack("esp-idf").unwrap();
        let rp = registry::variant("xiao-rp2040").unwrap().default_stack();
        for cat in ToolCategory::ALL {
            // esp-hal → every category is a live, editable axis.
            assert!(category_live(esp_hal, cat));
            assert!(category_grey_reason(esp_hal, cat).is_none());
            // esp-idf → template-owned, every category greyed.
            assert!(!category_live(esp_idf, cat));
            assert!(category_grey_reason(esp_idf, cat).is_some());
        }
        // non-ESP → all three axes are live now (flashing selects the flash route,
        // logging wires defmt, framework picks the body); nothing greys at the
        // section level.
        for cat in ToolCategory::ALL {
            assert!(category_live(rp, cat), "non-ESP: {cat:?} is live");
            assert!(category_grey_reason(rp, cat).is_none(), "non-ESP: {cat:?} header is bare");
        }
    }

    #[test]
    fn choice_grey_reason_covers_support_and_the_constraint_graph() {
        let c6 = registry::variant("xiao-esp32c6").unwrap();
        let esp_hal = c6.stack("esp-hal").unwrap();
        let log = registry::tool_choice(ToolCategory::Logging, "log").unwrap();
        let probe_rs = registry::tool_choice(ToolCategory::Flashing, "probe-rs").unwrap();
        let espflash = registry::tool_choice(ToolCategory::Flashing, "espflash").unwrap();
        let rtic = registry::tool_choice(ToolCategory::Framework, "rtic").unwrap();

        // With probe-rs as the sibling flashing tool, `log` greys (graph conflict).
        assert!(choice_grey_reason(c6, esp_hal, ToolCategory::Logging, log, probe_rs.esp_options).is_some());
        // With espflash, `log` is fine.
        assert!(choice_grey_reason(c6, esp_hal, ToolCategory::Logging, log, espflash.esp_options).is_none());
        // Unsupported-on-stack reasons (no sibling opts needed).
        assert_eq!(
            choice_grey_reason(c6, esp_hal, ToolCategory::Framework, rtic, &[]).as_deref(),
            Some("not supported on ESP")
        );
    }

    #[test]
    fn uferris_no_async_is_a_documented_gap() {
        let mut r = raw("xiao-esp32c3");
        r.uferris = true;
        r.no_async = true;
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("non-async") || e.contains("async"), "got: {e}");
    }

    #[test]
    fn parity_group_flags_emitted_when_non_default() {
        let cmd = cmd_for(|r| {
            r.chip = "xiao-esp32c6".to_string();
            r.flashing = Some("probe-rs".to_string());
            r.framework = Some("embassy".to_string());
        });
        assert_eq!(
            cmd,
            "xiao-generate --chip xiao-esp32c6 --flashing probe-rs --framework embassy"
        );
    }

    #[test]
    fn parity_default_groups_are_omitted() {
        // All-default groups → no group flags (keeps the command terse).
        let cmd = cmd_for(|r| r.chip = "xiao-esp32c6".to_string());
        assert_eq!(cmd, "xiao-generate --chip xiao-esp32c6");
    }

    // ---- universal editor support (--editor) ----

    #[test]
    fn editor_defaults_to_none_when_the_flag_is_absent() {
        // Writing into someone's editor config is opt-in: passing nothing writes
        // no editor files at all. `none` also leads the catalog, so this axis reads
        // like every other universal option list — do-nothing first and selected.
        let c = Config::resolve(raw("xiao-esp32c3")).unwrap();
        assert_eq!(c.editor.id, "none");
        assert_eq!(c.editor.path, None, "the default must not emit an editor file");
        // …and the default is catalog index 0, the convention the other catalogs use.
        assert_eq!(registry::default_editor().id, registry::EDITORS[0].id);
    }

    #[test]
    fn editor_default_is_backend_independent() {
        // Editor choice has nothing to do with the backend, so the default must
        // not vary by stack — every one of them gets the same editor.
        for (chip, stack) in [
            ("xiao-esp32c3", None),
            ("xiao-esp32c3", Some("esp-idf")),
            ("xiao-rp2040", None),
            ("xiao-rp2040", Some("embassy-rp")),
            ("xiao-samd21", None),
        ] {
            let mut r = raw(chip);
            r.stack = stack.map(|s| s.to_string());
            let c = Config::resolve(r).unwrap_or_else(|e| panic!("{chip}/{stack:?}: {e}"));
            assert_eq!(c.editor.id, "none", "{chip}/{stack:?} must get the same default");
        }
    }

    #[test]
    fn editor_none_is_a_catalog_entry_that_emits_nothing() {
        let mut r = raw("xiao-esp32c3");
        r.editor = Some("none".to_string());
        let c = Config::resolve(r).unwrap();
        assert_eq!(c.editor.id, "none");
        assert!(c.editor.path.is_none(), "`none` must write no files");
        assert!(c.editor.path.is_none());
    }

    #[test]
    fn editor_accepts_every_catalog_id_and_rejects_unknown_ones() {
        for e in registry::EDITORS {
            let mut r = raw("xiao-esp32c3");
            r.editor = Some(e.id.to_string());
            let c = Config::resolve(r).unwrap_or_else(|err| panic!("{}: {err}", e.id));
            assert_eq!(c.editor.id, e.id);
        }
        let mut r = raw("xiao-esp32c3");
        r.editor = Some("emacs".to_string());
        let e = Config::resolve(r).unwrap_err();
        assert!(e.contains("unknown --editor"), "got: {e}");
        // The error lists what IS available, including `none`.
        assert!(e.contains("vscode") && e.contains("none"), "got: {e}");
    }

    #[test]
    fn editor_is_accepted_on_every_stack_including_uferris() {
        // Unlike flashing/logging/framework there is no stack on which `--editor`
        // is refused — the axis is universal.
        let mut r = raw("xiao-esp32c3");
        r.stack = Some("esp-idf".to_string());
        r.editor = Some("helix".to_string());
        assert_eq!(Config::resolve(r).unwrap().editor.id, "helix");

        let mut r = raw("xiao-esp32c3");
        r.uferris = true;
        r.editor = Some("zed".to_string());
        assert_eq!(Config::resolve(r).unwrap().editor.id, "zed");
    }

    #[test]
    fn parity_editor_default_is_omitted_from_the_command() {
        // Default choice → no --editor flag (keeps the command terse).
        let cmd = cmd_for(|r| r.chip = "xiao-esp32c6".to_string());
        assert_eq!(cmd, "xiao-generate --chip xiao-esp32c6");
        // …and passing the default explicitly round-trips to the same terse form.
        let cmd = cmd_for(|r| {
            r.chip = "xiao-esp32c6".to_string();
            r.editor = Some("none".to_string());
        });
        assert_eq!(cmd, "xiao-generate --chip xiao-esp32c6");
    }

    /// Every non-default choice — `none` included — must appear in the command,
    /// and the command must resolve back to the SAME choice, or the "parity"
    /// promise is broken for this axis.
    #[test]
    fn parity_editor_choice_round_trips_through_resolution() {
        for e in registry::EDITORS {
            let mut r = raw("xiao-esp32c6");
            r.out_dir = Some(PathBuf::from("proj"));
            r.editor = Some(e.id.to_string());
            let first = Config::resolve(r).unwrap();
            let cmd = equivalent_command(&first);

            if e.id == registry::default_editor().id {
                assert!(!cmd.contains("--editor"), "the default must be omitted: {cmd}");
            } else {
                assert!(
                    cmd.ends_with(&format!("--editor {}", e.id)),
                    "`{}` must appear in the command: {cmd}",
                    e.id
                );
            }

            // Re-parse the `--editor` value out of the rendered command and resolve
            // again; the resulting choice must be identical.
            let toks: Vec<&str> = cmd.split_whitespace().collect();
            let reparsed = toks
                .windows(2)
                .find(|w| w[0] == "--editor")
                .map(|w| w[1].to_string());
            let mut r2 = raw("xiao-esp32c6");
            r2.out_dir = Some(PathBuf::from("proj"));
            r2.editor = reparsed;
            let second = Config::resolve(r2).unwrap();
            assert_eq!(first.editor.id, second.editor.id, "`{}` did not round-trip via `{cmd}`", e.id);
        }
    }

    // ---- host prerequisite SELECTION (the right prereqs for the right config) ----

    /// Resolve `chip` (+ tweaks) and return the ids `prerequisites` selected.
    fn prereq_ids(chip: &str, build: impl FnOnce(&mut RawConfig)) -> Vec<&'static str> {
        let mut r = raw(chip);
        build(&mut r);
        let cfg = Config::resolve(r).unwrap_or_else(|e| panic!("{chip}: {e}"));
        prerequisites(&cfg).into_iter().map(|p| p.id).collect()
    }

    /// THE acceptance property: a project is only ever told about tools it can
    /// actually use. An RP2040 project must not mention espup or espflash; an
    /// esp-idf project must not mention picotool.
    #[test]
    fn prerequisites_are_scoped_to_the_resolved_config() {
        let rp = prereq_ids("xiao-rp2040", |_| {});
        assert_eq!(rp, vec!["thumbv6m-none-eabi", "picotool"]);
        for foreign in ["xtensa-esp32s3-none-elf", "espflash", "ldproxy", "hf2", "uf2conv"] {
            assert!(!rp.contains(&foreign), "RP2040 must not mention {foreign}: {rp:?}");
        }

        let idf = prereq_ids("xiao-esp32c3", |r| r.stack = Some("esp-idf".into()));
        assert_eq!(
            idf,
            vec!["riscv32imc-esp-espidf", "ldproxy", "python3", "git", "espflash"]
        );
        for foreign in ["picotool", "hf2", "uf2conv", "probe-rs"] {
            assert!(!idf.contains(&foreign), "esp-idf must not mention {foreign}: {idf:?}");
        }
    }

    /// Xtensa asks for espup; RISC-V no_std asks for a plain rustup target. The
    /// distinction the whole ESP story turns on.
    #[test]
    fn xtensa_asks_for_espup_and_riscv_does_not() {
        let s3 = prereq_ids("xiao-esp32s3", |_| {});
        assert_eq!(s3, vec!["xtensa-esp32s3-none-elf", "espflash"]);
        assert!(registry::prereq("xtensa-esp32s3-none-elf").unwrap().install.contains("espup"));

        for (chip, target) in [
            ("xiao-esp32c3", "riscv32imc-unknown-none-elf"),
            ("xiao-esp32c6", "riscv32imac-unknown-none-elf"),
        ] {
            let ids = prereq_ids(chip, |_| {});
            assert_eq!(ids, vec![target, "espflash"], "{chip}");
            assert!(
                !registry::prereq(target).unwrap().install.contains("espup"),
                "{chip} must not need espup"
            );
        }
    }

    /// On esp-hal the flashing tool is a live user choice, so the flash
    /// prerequisite must follow `--flashing` rather than being fixed.
    #[test]
    fn esp_hal_flash_prerequisite_follows_the_flashing_choice() {
        let default = prereq_ids("xiao-esp32c6", |_| {});
        assert!(default.contains(&"espflash") && !default.contains(&"probe-rs"), "{default:?}");

        let probe = prereq_ids("xiao-esp32c6", |r| r.flashing = Some("probe-rs".into()));
        assert!(probe.contains(&"probe-rs") && !probe.contains(&"espflash"), "{probe:?}");
        // …and the build side is unchanged by a flashing choice.
        assert_eq!(probe[0], "riscv32imac-unknown-none-elf");
    }

    /// µFerris is an ordinary cargo dependency on the esp-hal stack, so it must
    /// add nothing to the host prerequisites. (Stated as a claim in the summary's
    /// design; pinned here so it stays true.)
    #[test]
    fn uferris_adds_no_host_prerequisite() {
        for chip in ["xiao-esp32c3", "xiao-esp32c6"] {
            let plain = prereq_ids(chip, |_| {});
            let uferris = prereq_ids(chip, |r| r.uferris = true);
            let power = prereq_ids(chip, |r| r.power_board = true);
            assert_eq!(plain, uferris, "{chip}: µFerris changed the prerequisites");
            assert_eq!(plain, power, "{chip}: the power board changed the prerequisites");
        }
    }

    /// Each non-ESP board's USB/UF2 flash route, end to end through resolution.
    #[test]
    fn non_esp_boards_get_their_own_usb_flash_route() {
        for (chip, want) in [
            ("xiao-rp2040", vec!["picotool"]),
            ("xiao-rp2350", vec!["picotool"]),
            ("xiao-nrf52840", vec!["rust-objcopy", "uf2conv"]),
            ("xiao-nrf52840-sense", vec!["rust-objcopy", "uf2conv"]),
            ("xiao-samd21", vec!["hf2"]),
        ] {
            let ids = prereq_ids(chip, |_| {});
            let flash: Vec<&str> = ids
                .iter()
                .copied()
                .filter(|id| registry::prereq(id).unwrap().need == Need::Flash)
                .collect();
            assert_eq!(flash, want, "{chip}");
        }
    }

    /// Every generatable configuration must select at least one build
    /// prerequisite (the target/toolchain), grouped build-before-flash, with no
    /// duplicates. Sweeps the whole registry so a new variant/stack cannot ship
    /// with an empty or mis-ordered report.
    #[test]
    fn every_configuration_reports_a_well_formed_prerequisite_list() {
        for v in registry::VARIANTS {
            for s in v.stacks {
                let mut r = raw(v.id);
                r.stack = Some(s.id.to_string());
                let cfg = Config::resolve(r).unwrap_or_else(|e| panic!("{}/{}: {e}", v.id, s.id));
                let list = prerequisites(&cfg);

                assert!(!list.is_empty(), "{}/{} selected nothing", v.id, s.id);
                // The first entry is always the target/toolchain for this stack.
                assert_eq!(list[0].id, s.target, "{}/{} must lead with its target", v.id, s.id);
                // No duplicates.
                let mut ids: Vec<&str> = list.iter().map(|p| p.id).collect();
                let before = ids.len();
                ids.sort_unstable();
                ids.dedup();
                assert_eq!(ids.len(), before, "{}/{} has duplicate prerequisites", v.id, s.id);
                // Build group strictly before flash group.
                let first_flash = list.iter().position(|p| p.need == Need::Flash);
                if let Some(i) = first_flash {
                    assert!(
                        list[i..].iter().all(|p| p.need == Need::Flash),
                        "{}/{}: build prerequisites must all precede flash ones",
                        v.id, s.id
                    );
                }
                // Every generatable project must be flashable somehow.
                assert!(first_flash.is_some(), "{}/{} has no flash route", v.id, s.id);
            }
        }
    }

    #[test]
    fn tui_resolution_still_rejects_gap_template() {
        // Picking a gap template on a non-ESP variant is a hard error even in the
        // TUI (softening applies only to the not-yet-implemented backend).
        let mut r = raw("xiao-rp2040");
        r.template = Some("rgb-led".to_string());
        let e = Config::resolve_for_tui(r).unwrap_err();
        assert!(e.contains("not yet implemented") || e.contains("unavailable"), "got: {e}");
    }
}
