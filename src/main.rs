//! xiao-generate — project generator for Seeed XIAO modules.
//!
//! M1: headless CLI for the four P0 ESP variants (base template, S3 Sense
//! component templates, µFerris mode). The TUI (M2) will layer over the same
//! `model::Config`.

mod backend;
mod cli;
mod model;
mod patch;
mod prereq;
mod registry;
mod templates;
mod tui;

use std::io::IsTerminal;

use clap::Parser;

use crate::cli::Cli;
use crate::model::{Config, RawConfig};

fn main() {
    std::process::exit(run());
}

/// Which generation path a run takes, after `--list-*` short-circuits. See
/// [`resolve_mode`] for the precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Non-interactive: resolve `Config` from flags ∪ registry defaults, generate.
    Headless,
    /// Interactive Ratatui TUI (which itself resolves a `Config` and hands it back
    /// on the same generation path). Requires `--name`.
    Tui,
}

/// Resolve the run mode from the parsed flags + whether stdout is a terminal,
/// implementing the M2-CLI-MODES precedence exactly (top wins). Pure so it is
/// unit-tested without a real terminal. `--list-*` flags are handled by the caller
/// before this is reached.
///
///  1. `--headless` → headless (never the TUI, even on a TTY).
///  2. `--tui` → TUI.
///  3. `--chip` present → headless.
///  4. interactive TTY (stdout is a terminal) → TUI.
///  5. otherwise (non-interactive, nothing given) → headless (which then requires
///     `--chip`, so a bare non-interactive run is a clean exit-2 usage error).
fn resolve_mode(args: &Cli, stdout_is_terminal: bool) -> Mode {
    if args.headless {
        Mode::Headless
    } else if args.tui {
        Mode::Tui
    } else if args.chip.is_some() {
        Mode::Headless
    } else if stdout_is_terminal {
        Mode::Tui
    } else {
        Mode::Headless
    }
}

/// Assemble the pre-validation [`RawConfig`] for headless generation from the CLI
/// flags. `--chip` is required (there is no default device); the only other thing
/// resolved here is the `--std` alias for `--stack esp-idf`. Every other unspecified
/// parameter is left `None` here and filled from registry defaults by
/// [`Config::resolve`], so once `--chip` is given, headless needs no other flag to
/// resolve a complete, buildable project.
fn build_raw(args: &Cli) -> Result<RawConfig, String> {
    // --chip is required — no default device.
    let Some(chip) = args.chip.clone() else {
        return Err("--chip <chip> is required (see --list-chips)".to_string());
    };

    // Resolve the stack flag: `--std` is a shorthand for `--stack esp-idf`.
    let stack = match (args.stack.clone(), args.std) {
        (Some(s), true) if s != "esp-idf" => {
            return Err(format!(
                "--std conflicts with --stack {s} (--std means --stack esp-idf)"
            ));
        }
        (Some(s), _) => Some(s),
        (None, true) => Some("esp-idf".to_string()),
        (None, false) => None,
    };

    Ok(RawConfig {
        chip,
        stack,
        template: args.template.clone(),
        name: args.name.clone(),
        out_dir: args.out.clone(),
        uferris: args.uferris,
        power_board: args.power_board,
        no_async: args.no_async,
        flashing: args.flashing.clone(),
        logging: args.logging.clone(),
        framework: args.framework.clone(),
        extra_crates: args.add_crate.clone(),
        editor: args.editor.clone(),
    })
}

fn run() -> i32 {
    let args = Cli::parse();

    if args.list_chips {
        print_chips();
        return 0;
    }
    if args.list_templates {
        print_templates(args.chip.as_deref());
        return 0;
    }
    if args.list_stacks {
        print_stacks(args.chip.as_deref());
        return 0;
    }
    if args.check {
        return run_check(&args);
    }

    match resolve_mode(&args, std::io::stdout().is_terminal()) {
        Mode::Tui => {
            // --name is REQUIRED before the TUI launches — enforce it here, BEFORE
            // any terminal init/draw, so a missing name is a clean exit-2 usage
            // error rather than a half-initialised terminal.
            let Some(name) = args.name.clone() else {
                eprintln!("error: --name <name> is required to launch the interactive TUI");
                return 2;
            };
            run_tui(name)
        }
        Mode::Headless => run_headless(&args),
    }
}

/// Headless path: build the `RawConfig` from flags ∪ defaults, resolve + validate,
/// then generate on the one shared generation path.
fn run_headless(args: &Cli) -> i32 {
    let raw = match build_raw(args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let cfg = match Config::resolve(raw) {
        Ok(c) => c,
        // config-time validation failures (R2): actionable message, exit 2.
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    generate(&cfg)
}

/// `--check`: resolve the config exactly as headless generation would, then print
/// only the prerequisite report and stop. Generates nothing and writes nothing.
///
/// Shares `Config::resolve` and `print_prerequisites` with the generation path,
/// so what it reports is by construction what a real run would report. Exit code:
/// 0 = everything needed is installed, 1 = something is missing, 2 = the config
/// itself is invalid (same as headless).
fn run_check(args: &Cli) -> i32 {
    let cfg = match build_raw(args).and_then(Config::resolve) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    println!(
        "Host prerequisites for a {} project on the {} stack:",
        cfg.variant.display, cfg.stack.id
    );
    let missing = print_prerequisites(&cfg);
    if missing > 0 {
        println!("\n{missing} missing — run the install command(s) above.");
        return 1;
    }
    println!("\nAll prerequisites are installed.");
    0
}

/// Interactive path: the TUI builds a resolved `Config` from the same
/// `Config::resolve_for_tui` validation and the passed-in `name`, then hands it
/// back for generation on the SAME code path as the headless flow.
fn run_tui(name: String) -> i32 {
    match tui::launch(name) {
        Ok(tui::Outcome::Generate(cfg)) => generate(&cfg),
        Ok(tui::Outcome::Gated(msg)) => {
            // Non-ESP (M4/M6) selection: nothing generated, but not an error.
            println!("{msg}");
            0
        }
        Ok(tui::Outcome::Cancelled) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

/// The single generation path shared by both modes: scaffold + patch + summary.
fn generate(cfg: &Config) -> i32 {
    if let Err(e) = backend::scaffold(cfg) {
        eprintln!("error: {e:#}");
        return 1;
    }
    if let Err(e) = patch::apply(cfg) {
        eprintln!("error: {e:#}");
        return 1;
    }
    print_summary(cfg);
    0
}

fn print_chips() {
    println!("Supported XIAO module variants:\n");
    for v in registry::VARIANTS {
        let stacks = v
            .stacks
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>()
            .join("/");
        let uf = if v.uferris_gap.is_some() {
            "µFerris: gap"
        } else if v.uferris_eligible() {
            "µFerris: yes"
        } else {
            "µFerris: no"
        };
        let family = if v.is_esp() { "ESP" } else { "Cortex-M" };
        println!("  {:<22} P{}  {:<9} {:<26} {}", v.id, v.priority, family, stacks, uf);
    }
    println!(
        "\nESP variants implement the esp-hal (no_std) and esp-idf (std) stacks; the Cortex-M \
         variants\nimplement a blocking-HAL stack and, where one exists, an embassy-HAL stack \
         (see --list-stacks)."
    );
}

fn print_stacks(chip: Option<&str>) {
    match chip {
        Some(id) => match registry::variant(id) {
            Some(v) => {
                println!("Stacks for {} ({}):\n", v.display, v.id);
                for (i, s) in v.stacks.iter().enumerate() {
                    let default = if i == 0 { "  [default]" } else { "" };
                    println!("  {:<12} {}{}", s.id, s.display, default);
                    println!("               target: {}", s.target);
                    // Frameworks are a live axis on the non-ESP stacks (M4) and on
                    // esp-hal; show which ones this stack can actually generate,
                    // and why the rest can't.
                    if !s.frameworks.is_empty() {
                        let available = s
                            .frameworks
                            .iter()
                            .filter(|f| s.framework_gap(f).is_none())
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ");
                        println!("               frameworks: {available}");
                        for f in s.frameworks {
                            if let Some(reason) = s.framework_gap(f) {
                                println!("                 {f}: [gap] {reason}");
                            }
                        }
                    }
                }
            }
            None => eprintln!("unknown chip '{id}' (see --list-chips)"),
        },
        None => {
            println!("Stacks by variant:\n");
            for v in registry::VARIANTS {
                let stacks = v
                    .stacks
                    .iter()
                    .map(|s| s.id)
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("  {:<22} {}", v.id, stacks);
            }
        }
    }
}

fn print_templates(chip: Option<&str>) {
    match chip {
        Some(id) => match registry::variant(id) {
            Some(v) => {
                println!("Templates for {} ({}):\n", v.display, v.id);
                for s in v.stacks {
                    println!("  [{} stack]", s.id);
                    for t in s.templates {
                        let meta = registry::template(t);
                        // The std stack's synthetic `base` id has no TEMPLATES entry.
                        let disp = meta.map(|m| m.display).unwrap_or(if s.is_std() {
                            "Base — LED blink / log-hello (esp-idf std)"
                        } else {
                            ""
                        });
                        match meta.and_then(|m| m.gap) {
                            Some(_) => println!("    {:<16} {}  [gap — unavailable]", t, disp),
                            None => println!("    {:<16} {}", t, disp),
                        }
                    }
                }
                if v.uferris_eligible() {
                    let note = if v.uferris_gap.is_some() {
                        " [gap — unavailable]"
                    } else {
                        ""
                    };
                    println!(
                        "\n  --uferris        µFerris BSP mode (templates: {}){}",
                        registry::UFERRIS_TEMPLATES.join(", "),
                        note
                    );
                }
            }
            None => {
                eprintln!("unknown chip '{id}' (see --list-chips)");
            }
        },
        None => {
            println!("All templates:\n");
            for t in registry::TEMPLATES {
                let gap = if t.gap.is_some() { "  [gap]" } else { "" };
                println!("  {:<16} {}{}", t.id, t.display, gap);
            }
            println!(
                "  {:<16} µFerris mode (via --uferris): {}",
                "blinky",
                registry::UFERRIS_TEMPLATES.join(", ")
            );
        }
    }
}

/// Report the host prerequisites **this** project needs, each one actually
/// probed on this machine, grouped build-vs-flash.
///
/// The selection is [`model::prerequisites`] (registry-derived, R4) and the
/// detection is [`prereq::Host`] — this function only renders, so it contains no
/// chip, stack or tool knowledge and cannot drift from the registry. Nothing is
/// claimed without a probe: a `✔` means the probe found it, a `✗` means it did
/// not, and the install command is shown only where it is actually needed.
///
/// Returns the number of missing prerequisites so callers (`--check`) can turn
/// it into an exit code.
fn print_prerequisites(cfg: &Config) -> usize {
    let all = model::prerequisites(cfg);
    let mut host = prereq::Host::new();
    let detected: Vec<(&registry::Prereq, prereq::Status)> =
        all.into_iter().map(|p| { let s = host.detect(p); (p, s) }).collect();

    let mut missing = 0;
    for (need, heading) in [
        (registry::Need::Build, "prerequisites to build:"),
        (registry::Need::Flash, "prerequisites to flash:"),
    ] {
        let group: Vec<_> = detected.iter().filter(|(p, _)| p.need == need).collect();
        if group.is_empty() {
            continue;
        }
        println!("\n  {heading}");
        for (p, status) in group {
            println!("    {} {}", status.marker(), p.display);
            if status.is_missing() {
                missing += 1;
                println!("        install: {}", p.install);
            }
            if let Some(note) = p.note {
                println!("        · {note}");
            }
        }
    }
    missing
}

fn print_summary(cfg: &Config) {
    // Universal, device-agnostic facts only (v3 UNIFY): no esp-generate vocabulary
    // and no Add-on/Crates/Output lines. The generated path still appears once, in
    // the actionable `cd` next-step below.
    let uferris_line = match &cfg.uferris {
        Some(u) if u.power_board => {
            if u.async_on { "yes (async · + Megalops power board)" } else { "yes (+ Megalops power board)" }
        }
        Some(u) if u.async_on => "yes (async)",
        Some(_) => "yes",
        None => "no",
    };

    println!("\n✔ Generated {} project", cfg.variant.display);
    println!("  crate:        {}", cfg.project_name);
    println!("  stack:        {} ({})", cfg.stack.id, cfg.stack.display);
    println!("  target:       {}", cfg.stack.target);
    if model::summary_shows_toolchain(cfg) {
        // esp-hal base + every non-ESP project. Flashing carries the external-probe
        // note where it applies (probe-rs off ESP); logging carries the RTT-view
        // note for defmt off ESP — both derived from the same builtin_debug fact.
        match model::flashing_probe_note(cfg.stack, cfg.flashing) {
            Some(note) => println!("  flashing:     {} ({note})", cfg.flashing.display),
            None => println!("  flashing:     {}", cfg.flashing.display),
        }
        match model::defmt_view_note(cfg.stack, cfg.logging) {
            Some(note) => println!("  logging:      {} ({note})", cfg.logging.display),
            None => println!("  logging:      {}", cfg.logging.display),
        }
        println!("  framework:    {}", cfg.framework.display);
    } else if model::category_live(cfg.stack, registry::ToolCategory::Framework) {
        // esp-hal component add-on: the option groups are locked to the template,
        // but framework is still the live axis — shown alone, as before.
        println!("  framework:    {}", cfg.framework.display);
    }
    println!("  µFerris BSP:  {}", uferris_line);
    // Editor is a universal axis (every stack emits the same file), so it is
    // always shown. Any caveat travels with it — a config the user still has to
    // opt into must never read as "done".
    println!("  editor:       {}", cfg.editor.display);
    if let Some(caveat) = cfg.editor.caveat {
        println!("                · {}", caveat);
    }
    println!("\n  equivalent command:");
    println!("    {}", model::equivalent_command(cfg));
    print_prerequisites(cfg);
    // The unconditional `rustup target add` and "S3 is Xtensa" lines that used to
    // live here are gone: the prerequisite block above says the same things, but
    // only when they are actually outstanding on this host, and with the exact
    // install command. Telling a user to install a target the probe just found is
    // the noise this feature exists to remove.
    println!("\nNext steps:");
    println!("  cd {}", cfg.out_dir.display());
    if !cfg.stack.backend.is_esp() {
        // The non-ESP scaffold pins the target in .cargo/config.toml but cannot
        // install it for the user.
        println!("  rustup target add {}", cfg.stack.target);
    }
    println!("  cargo build");
    if cfg.variant.arch == registry::Arch::Xtensa {
        println!("  (ESP32-S3 is Xtensa — needs the `esp` toolchain via espup)");
    }
    if cfg.stack.is_std() {
        // Deliberately does NOT say "via espup": esp-idf-sys is what fetches and
        // builds the SDK, on RISC-V as well as Xtensa, and espup is only in the
        // picture for Xtensa (where the prerequisite block names it).
        println!(
            "  (esp-idf std stack: the first build pulls & builds the ESP-IDF {} SDK via esp-idf-sys — expect a long first build)",
            registry::ESP_IDF_VERSION
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Parse a `Cli` from an arg list (no leading binary name needed).
    fn cli(args: &[&str]) -> Cli {
        let mut v = vec!["xiao-generate"];
        v.extend_from_slice(args);
        Cli::try_parse_from(v).expect("cli should parse")
    }

    // ---- mode resolution precedence (M2-CLI-MODES) ----

    #[test]
    fn headless_flag_forces_headless_even_on_a_tty() {
        // Precedence 1: --headless wins over an interactive terminal.
        assert_eq!(resolve_mode(&cli(&["--headless"]), true), Mode::Headless);
        assert_eq!(resolve_mode(&cli(&["--headless"]), false), Mode::Headless);
    }

    #[test]
    fn headless_beats_tui_when_both_present() {
        // Precedence 1 beats 2: --headless wins even against an explicit --tui.
        assert_eq!(
            resolve_mode(&cli(&["--headless", "--tui", "--name", "x"]), true),
            Mode::Headless
        );
    }

    #[test]
    fn tui_flag_forces_tui_off_a_tty() {
        // Precedence 2: --tui forces the TUI even when stdout is not a terminal.
        assert_eq!(resolve_mode(&cli(&["--tui", "--name", "x"]), false), Mode::Tui);
    }

    #[test]
    fn chip_present_implies_headless_even_on_a_tty() {
        // Precedence 3: --chip present → headless, ahead of the interactive-TTY rule.
        assert_eq!(resolve_mode(&cli(&["--chip", "xiao-esp32c3"]), true), Mode::Headless);
    }

    #[test]
    fn interactive_tty_with_nothing_given_is_tui() {
        // Precedence 4.
        assert_eq!(resolve_mode(&cli(&[]), true), Mode::Tui);
    }

    #[test]
    fn non_interactive_with_nothing_given_is_headless() {
        // Precedence 5: non-TTY + nothing given → headless with all defaults.
        assert_eq!(resolve_mode(&cli(&[]), false), Mode::Headless);
    }

    // ---- (a) --chip is required (no default device); everything else defaults ----

    #[test]
    fn headless_without_chip_is_a_usage_error() {
        // No default device: headless with no --chip is a clean usage error.
        let err = build_raw(&cli(&["--headless"])).unwrap_err();
        assert!(err.contains("--chip"), "got: {err}");
    }

    #[test]
    fn headless_with_only_chip_resolves_all_other_defaults() {
        // Backend-free: with just --chip given, every other axis fills from defaults.
        let raw = build_raw(&cli(&["--headless", "--chip", "xiao-esp32c3"])).unwrap();
        let cfg = Config::resolve(raw).unwrap();
        assert_eq!(cfg.variant.id, "xiao-esp32c3");
        assert_eq!(cfg.stack.id, "esp-hal"); // variant default stack
        assert_eq!(cfg.template_id, "base-hello"); // C3 esp-hal base (no LED → hello)
        assert_eq!(cfg.project_name, "xiao-esp32c3-base-hello"); // derived name
        assert_eq!(cfg.out_dir, PathBuf::from("xiao-esp32c3-base-hello")); // ./<name>
        assert_eq!(cfg.flashing.id, "espflash"); // stack default flashing
        assert_eq!(cfg.logging.id, "none");
        assert_eq!(cfg.framework.id, "none");
        assert!(cfg.uferris.is_none());
        assert!(cfg.extra_crates.is_empty());
    }

    // ---- (c) explicit flags override the defaults ----

    #[test]
    fn explicit_chip_and_framework_override_defaults() {
        let raw = build_raw(&cli(&[
            "--headless",
            "--chip",
            "xiao-esp32s3",
            "--framework",
            "embassy",
        ]))
        .unwrap();
        let cfg = Config::resolve(raw).unwrap();
        assert_eq!(cfg.variant.id, "xiao-esp32s3");
        assert_eq!(cfg.stack.id, "esp-hal");
        assert_eq!(cfg.framework.id, "embassy");
        // untouched axes keep their defaults
        assert_eq!(cfg.flashing.id, "espflash");
        assert_eq!(cfg.logging.id, "none");
    }

    #[test]
    fn explicit_name_and_out_override_derived_defaults() {
        let cfg = Config::resolve(
            build_raw(&cli(&["--headless", "--chip", "xiao-esp32c3", "--name", "myproj"])).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.project_name, "myproj");
        assert_eq!(cfg.out_dir, PathBuf::from("myproj"));
    }

    #[test]
    fn std_alias_maps_to_the_esp_idf_stack() {
        let raw = build_raw(&cli(&["--headless", "--chip", "xiao-esp32c3", "--std"])).unwrap();
        assert_eq!(raw.stack.as_deref(), Some("esp-idf"));
        let cfg = Config::resolve(raw).unwrap();
        assert_eq!(cfg.stack.id, "esp-idf");
        assert_eq!(cfg.template_id, "base");
    }

    #[test]
    fn std_conflicts_with_an_explicit_non_idf_stack() {
        let err = build_raw(&cli(&[
            "--headless", "--chip", "xiao-esp32c3", "--std", "--stack", "esp-hal",
        ]))
        .unwrap_err();
        assert!(err.contains("--std conflicts"), "got: {err}");
    }
}
