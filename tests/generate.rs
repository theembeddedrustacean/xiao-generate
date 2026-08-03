//! Black-box tests over the built binary: CLI behaviour (R2 rejections, listing)
//! and end-to-end generation snapshots (R6). The ESP generation tests require the
//! pinned `esp-generate` / `cargo-generate` on PATH; they self-skip if it is
//! absent so the suite still passes on a machine/CI lane without it.
//!
//! The non-ESP (M4) tests need no external generator at all — the scaffold is
//! written directly — so their *generation* half always runs. Their *compile*
//! half additionally needs the ARM target installed, and self-skips the same way
//! (`rustup target add thumbv6m-none-eabi thumbv7em-none-eabihf
//! thumbv8m.main-none-eabihf`). Compiling is opt-in via `XIAO_COMPILE_GATE=1`
//! because it downloads and builds whole HAL trees.

use std::path::Path;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xiao-generate"))
}

/// The compile gate is the load-bearing M4 acceptance check, but it pulls and
/// builds every HAL, so it runs only when asked for.
fn compile_gate_enabled() -> bool {
    std::env::var("XIAO_COMPILE_GATE").map(|v| v == "1").unwrap_or(false)
}

fn target_installed(triple: &str) -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == triple))
        .unwrap_or(false)
}

fn esp_generate_available() -> bool {
    Command::new("esp-generate")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn cargo_generate_available() -> bool {
    Command::new("cargo")
        .args(["generate", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn list_chips_lists_all_variants() {
    let out = bin().arg("--list-chips").output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("xiao-esp32s3-sense"));
    assert!(s.contains("xiao-esp32c3"));
    assert!(s.contains("xiao-esp32c5"));
}

#[test]
fn invalid_combo_exits_nonzero_with_message() {
    // camera on a variant with no camera
    let out = bin()
        .args(["--chip", "xiao-esp32c3", "--template", "camera-snapshot"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not available"), "stderr: {err}");
}

#[test]
fn uferris_on_s3_reports_gap() {
    let out = bin()
        .args(["--chip", "xiao-esp32s3", "--uferris"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not available"), "stderr: {err}");
}

#[test]
fn tui_without_name_exits_2_before_init() {
    // (b) The TUI requires --name. `--tui` forces TUI mode regardless of the
    // (non-TTY) test stdout, so the name gate is hit BEFORE any terminal init —
    // a clean exit 2 with the documented message, never a half-drawn TUI.
    let out = bin().arg("--tui").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        err.trim(),
        "error: --name <name> is required to launch the interactive TUI",
        "stderr: {err}"
    );
}

#[test]
fn headless_under_non_tty_resolves_from_flags_without_backend() {
    // (d) Under the harness's non-TTY stdout, --headless takes the headless
    // resolve path — not the TUI. With --chip supplied (required), a config-time
    // rejection (unknown --framework) proves resolution ran to completion without
    // invoking any backend.
    let out = bin()
        .args(["--headless", "--chip", "xiao-esp32c3", "--framework", "tokio"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown --framework"), "stderr: {err}");
    assert!(!err.contains("interactive TUI"), "must not hit the TUI name gate:\n{err}");
}

#[test]
fn headless_without_chip_is_a_usage_error() {
    // No default device: `--headless` with no --chip is a clean
    // exit-2 usage error, reported before any backend — so no esp-generate needed.
    let out = bin().arg("--headless").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--chip"), "stderr: {err}");
    assert!(!err.contains("interactive TUI"), "headless path, not the TUI name gate:\n{err}");
}

#[test]
fn no_flags_non_tty_is_headless_and_requires_chip() {
    // M2-CLI-MODES precedence 5: nothing given + non-TTY stdout → headless. With no
    // default device, that headless run then fails on the required --chip (exit 2),
    // rather than launching a TUI or picking a device for the user.
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--chip"), "stderr: {err}");
    assert!(!err.contains("interactive TUI"), "non-TTY is headless, not the TUI gate:\n{err}");
}

// ---- generation snapshots (need esp-generate) ----

fn generate(args: &[&str], out_dir: &Path) {
    let mut c = bin();
    c.args(args).args(["-o", out_dir.to_str().unwrap()]);
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "generate {args:?} failed:\nstdout {}\nstderr {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn snapshot_c3_base_leaves_esp_generate_main_intact() {
    // KEY DECISION ("esp-generate owns the base"): on the esp-hal stack a BASE
    // project no longer injects our own main — esp-generate's generated main
    // stands untouched. We assert the upstream main is intact (its generator
    // marker is present, our template body is NOT) and that the scaffold deps are
    // there. extra_crates are still applied (covered by the next test).
    if !esp_generate_available() {
        eprintln!("skipping: esp-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("c3-base");
    // default base (no --template): esp-generate owns the main.
    generate(&["--chip", "xiao-esp32c3"], &out);

    let main = std::fs::read_to_string(out.join("src/bin/main.rs")).unwrap();
    // esp-generate's own main carries the generator provenance marker.
    assert!(main.contains("// generator version:"), "esp-generate main should be intact:\n{main}");
    assert!(main.contains("-o esp-backtrace"), "default espflash flashing → esp-backtrace:\n{main}");
    // Our old injected base-hello body must NOT be present (we no longer inject).
    assert!(!main.contains("Hello from xiao-generate"), "base main must not be our template:\n{main}");

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(toml.contains("esp-hal"));
    // -o esp-backtrace pulls esp-println into the scaffold
    assert!(toml.contains("esp-println"), "Cargo.toml:\n{toml}");

    // esp-generate emits editor config only under its per-editor `-o` options
    // (which we do not pass), so we supply the editor-agnostic file ourselves.
    assert_rust_analyzer_toml(&out, "esp-hal no_std");
}

#[test]
fn snapshot_c6_base_embassy_defmt_passes_group_options() {
    // A non-default base combo: the flashing/logging/framework selections flow
    // into esp-generate's -o flags. esp-generate owns the (async) main; we only
    // assert the scaffold reflects the requested options. (This combo was
    // additionally `cargo check`ed by hand — see NOTES.md.)
    if !esp_generate_available() {
        eprintln!("skipping: esp-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("c6-emb");
    generate(
        &["--chip", "xiao-esp32c6", "--framework", "embassy", "--logging", "defmt"],
        &out,
    );

    let main = std::fs::read_to_string(out.join("src/bin/main.rs")).unwrap();
    assert!(main.contains("// generator version:"), "esp-generate owns the main:\n{main}");
    // -o embassy → esp-generate emits an async main.
    assert!(main.contains("async fn main"), "embassy base main should be async:\n{main}");

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(toml.contains("embassy-executor"), "embassy stack pulled in:\n{toml}");
    assert!(toml.contains("defmt"), "defmt logging pulled in:\n{toml}");
}

#[test]
fn snapshot_c3_base_probe_rs_uses_rtt_panic_handler() {
    if !esp_generate_available() {
        eprintln!("skipping: esp-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("c3-probe");
    generate(&["--chip", "xiao-esp32c3", "--flashing", "probe-rs"], &out);

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    // probe-rs implies panic-rtt-target (its panic handler); esp-backtrace absent.
    assert!(toml.contains("panic-rtt-target"), "probe-rs → panic-rtt-target:\n{toml}");

    let main = std::fs::read_to_string(out.join("src/bin/main.rs")).unwrap();
    assert!(main.contains("// generator version:"), "esp-generate owns the main:\n{main}");
}

#[test]
fn snapshot_s3_sense_sd_adds_driver_deps() {
    if !esp_generate_available() {
        eprintln!("skipping: esp-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("sd");
    generate(&["--chip", "xiao-esp32s3-sense", "--template", "sd-list"], &out);

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(toml.contains("embedded-sdmmc"), "Cargo.toml:\n{toml}");
    assert!(toml.contains("default-features = false"));
    assert!(toml.contains("embedded-hal-bus"));

    let main = std::fs::read_to_string(out.join("src/bin/main.rs")).unwrap();
    assert!(main.contains("GPIO21")); // CS
    assert!(main.contains("embedded_sdmmc"));
}

#[test]
fn snapshot_c3_uferris_adds_bsp_dep() {
    if !esp_generate_available() {
        eprintln!("skipping: esp-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("uf");
    generate(&["--chip", "xiao-esp32c3", "--uferris"], &out);

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(toml.contains("uferris-bsp"), "Cargo.toml:\n{toml}");
    assert!(toml.contains("xiao-esp32c3"));
    assert!(toml.contains("embassy"));

    let main = std::fs::read_to_string(out.join("src/bin/main.rs")).unwrap();
    assert!(main.contains("uferris_init"));
}

#[test]
fn snapshot_c3_add_crate_injects_extra_dependencies() {
    if !esp_generate_available() {
        eprintln!("skipping: esp-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("extra");
    generate(
        &[
            "--chip",
            "xiao-esp32c3",
            "--template",
            "base-hello",
            "--add-crate",
            "heapless@0.8",
            "--add-crate",
            "fugit",
        ],
        &out,
    );

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    // versioned spec → pinned; bare spec → "*". Both land in [dependencies].
    assert!(toml.contains("heapless = \"0.8\""), "Cargo.toml:\n{toml}");
    assert!(toml.contains("fugit = \"*\""), "Cargo.toml:\n{toml}");
    // the scaffold's own deps are untouched
    assert!(toml.contains("esp-hal"), "Cargo.toml:\n{toml}");

    // base = esp-generate owns the main even while our extra_crates are applied.
    let main = std::fs::read_to_string(out.join("src/bin/main.rs")).unwrap();
    assert!(main.contains("// generator version:"), "base main must stay esp-generate's:\n{main}");
    assert!(!main.contains("Hello from xiao-generate"), "no base body injection:\n{main}");
}

#[test]
fn add_crate_bad_spec_is_config_error() {
    // Invalid crate name → rejected at config time (exit 2), before any backend.
    let out = bin()
        .args([
            "--chip",
            "xiao-esp32c3",
            "--add-crate",
            "not a crate",
            "-o",
            "/tmp/should-not-be-created",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("add-crate"), "stderr: {err}");
}

// ---- M5: ESP `std` (esp-idf) stack snapshots (need cargo-generate) ----
// Generation-only verification (R9): we assert the tree/Cargo.toml/main.rs, we do
// NOT `cargo check` (that pulls & builds the full ESP-IDF SDK).

#[test]
fn snapshot_s3_esp_idf_std_blink() {
    if !cargo_generate_available() {
        eprintln!("skipping: cargo-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("s3-idf");
    generate(&["--chip", "xiao-esp32s3", "--stack", "esp-idf"], &out);

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    // template-pinned std crate + our added esp-idf-hal
    assert!(toml.contains("esp-idf-svc"), "Cargo.toml:\n{toml}");
    assert!(toml.contains("esp-idf-hal"), "esp-idf-hal not added:\n{toml}");

    // .cargo/config.toml carries the espidf target + baked mcu
    let cfg = std::fs::read_to_string(out.join(".cargo/config.toml")).unwrap();
    assert!(cfg.contains("xtensa-esp32s3-espidf"), ".cargo/config.toml:\n{cfg}");
    assert!(cfg.contains("esp32s3"));

    // std layout uses src/main.rs (NOT src/bin/main.rs); S3 has an LED → blink on GPIO21
    assert!(!out.join("src/bin/main.rs").exists(), "std layout must not have src/bin/main.rs");
    let main = std::fs::read_to_string(out.join("src/main.rs")).unwrap();
    assert!(main.contains("esp_idf_hal"), "main.rs:\n{main}");
    assert!(main.contains("gpio21"), "S3 blink should target GPIO21; main.rs:\n{main}");
    assert!(main.contains("PinDriver::output"));
}

#[test]
fn snapshot_c3_esp_idf_std_hello() {
    if !cargo_generate_available() {
        eprintln!("skipping: cargo-generate not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("c3-idf");
    // exercise the --std alias here (== --stack esp-idf)
    generate(&["--chip", "xiao-esp32c3", "--std"], &out);

    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(toml.contains("esp-idf-svc"), "Cargo.toml:\n{toml}");
    assert!(toml.contains("esp-idf-hal"));

    let cfg = std::fs::read_to_string(out.join(".cargo/config.toml")).unwrap();
    assert!(cfg.contains("riscv32imc-esp-espidf"), ".cargo/config.toml:\n{cfg}");
    assert!(cfg.contains("esp32c3"));

    // C3 has no LED → std hello (log), not a blink
    let main = std::fs::read_to_string(out.join("src/main.rs")).unwrap();
    assert!(main.contains("Hello from xiao-generate"), "main.rs:\n{main}");
    assert!(main.contains("EspLogger::initialize_default"));
    assert!(!main.contains("PinDriver"), "C3 hello must not blink an LED");

    // The esp-idf stack looks hosted, but its `.cargo/config.toml` sets
    // `build-std = ["std", "panic_abort"]` — no `test` — so it needs the config
    // exactly like the bare-metal stacks do.
    assert!(
        cfg.contains("build-std"),
        "expectation is premised on build-std; .cargo/config.toml:\n{cfg}"
    );
    assert!(!cfg.contains("\"test\""), "if build-std gained `test`, revisit:\n{cfg}");
    assert_rust_analyzer_toml(&out, "esp-idf std");
}

// ---------------------------------------------------------------------------
// M4: non-ESP direct scaffold. No external generator needed, so the tree/
// manifest assertions always run; the compile gate is opt-in (see the module
// docs) and additionally self-skips when the ARM target is not installed.
// ---------------------------------------------------------------------------

/// Every non-ESP (variant, stack, framework) cell xiao-generate claims to
/// support. Each of these was generated and **built to a real ARM firmware
/// image** (non-zero entry point + `Reset` + `__vector_table`) while M4 was
/// implemented; this table is what the compile gate re-checks.
const NON_ESP_CELLS: &[(&str, &str, &str, &str)] = &[
    // (chip, stack, framework, target)
    ("xiao-rp2040", "rp2040-hal", "none", "thumbv6m-none-eabi"),
    ("xiao-rp2040", "embassy-rp", "none", "thumbv6m-none-eabi"),
    ("xiao-rp2040", "embassy-rp", "embassy", "thumbv6m-none-eabi"),
    ("xiao-rp2350", "rp235x-hal", "none", "thumbv8m.main-none-eabihf"),
    ("xiao-rp2350", "embassy-rp", "none", "thumbv8m.main-none-eabihf"),
    ("xiao-rp2350", "embassy-rp", "embassy", "thumbv8m.main-none-eabihf"),
    ("xiao-nrf52840", "nrf-hal", "none", "thumbv7em-none-eabihf"),
    ("xiao-nrf52840", "embassy-nrf", "none", "thumbv7em-none-eabihf"),
    ("xiao-nrf52840", "embassy-nrf", "embassy", "thumbv7em-none-eabihf"),
    ("xiao-nrf52840-sense", "nrf-hal", "none", "thumbv7em-none-eabihf"),
    ("xiao-nrf52840-sense", "embassy-nrf", "none", "thumbv7em-none-eabihf"),
    ("xiao-nrf52840-sense", "embassy-nrf", "embassy", "thumbv7em-none-eabihf"),
    ("xiao-samd21", "atsamd-hal", "none", "thumbv6m-none-eabi"),
];

fn generate_cell(chip: &str, stack: &str, framework: &str, out: &Path) {
    let mut args = vec!["--chip", chip, "--stack", stack];
    if framework != "none" {
        args.extend_from_slice(&["--framework", framework]);
    }
    generate(&args, out);
}

#[test]
fn non_esp_scaffold_writes_the_full_project_tree() {
    let tmp = tempfile::tempdir().unwrap();
    for (chip, stack, framework, target) in NON_ESP_CELLS {
        let out = tmp.path().join(format!("{chip}-{stack}-{framework}"));
        generate_cell(chip, stack, framework, &out);

        // The four always-present files. Non-ESP uses the single-bin layout.
        assert!(out.join("Cargo.toml").exists(), "{chip}/{stack}: Cargo.toml");
        assert!(out.join("src/main.rs").exists(), "{chip}/{stack}: src/main.rs");
        assert!(out.join(".cargo/config.toml").exists(), "{chip}/{stack}: .cargo/config.toml");
        assert!(!out.join("src/bin/main.rs").exists(), "{chip}/{stack}: not the esp-hal layout");

        // memory.x + build.rs travel together; only the SAMD21 BSP supplies its
        // own linker script.
        let has_memory = out.join("memory.x").exists();
        assert_eq!(has_memory, out.join("build.rs").exists(), "{chip}/{stack}: memory.x/build.rs");
        assert_eq!(has_memory, *stack != "atsamd-hal", "{chip}/{stack}: memory.x expectation");

        let cfg = std::fs::read_to_string(out.join(".cargo/config.toml")).unwrap();
        assert!(cfg.contains(&format!("target = \"{target}\"")), "{chip}/{stack}:\n{cfg}");

        let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        // Profiles must be real tables, not a `profile = {}` inline value.
        assert!(toml.contains("[profile.dev]"), "{chip}/{stack}:\n{toml}");
        assert!(toml.contains("[profile.release]"), "{chip}/{stack}:\n{toml}");
        assert!(!toml.contains("profile = {"), "{chip}/{stack}: inline profile:\n{toml}");
    }
}

/// Assert a generated project carries the editor config that suppresses the
/// spurious `can't find crate for `test`` diagnostic. Shared by the non-ESP and
/// ESP checks below because the requirement is backend-independent.
fn assert_rust_analyzer_config(out: &Path, what: &str) {
    assert_rust_analyzer_toml(out, what);
    assert_vscode_client_config(out, what);
}

/// The editor-agnostic `rust-analyzer.toml`, which EVERY generated project gets
/// regardless of `--editor` (including `--editor none`, the default).
fn assert_rust_analyzer_toml(out: &Path, what: &str) {
    // In rust-analyzer.toml's un-prefixed, nested form. Keys are the ones
    // `rust-analyzer --print-config-schema` documents.
    let path = out.join("rust-analyzer.toml");
    assert!(path.exists(), "{what}: missing rust-analyzer.toml");
    let s = std::fs::read_to_string(&path).unwrap();
    assert!(s.contains("[cargo]"), "{what}: no [cargo] table:\n{s}");
    assert!(s.contains("[check]"), "{what}: no [check] table:\n{s}");
    assert_eq!(
        s.matches("allTargets = false").count(),
        2,
        "{what}: expected both cargo.allTargets and check.allTargets to be false:\n{s}"
    );

}

/// The VS Code client-config file — the delivery path actually verified to
/// suppress the diagnostic (rust-analyzer.toml alone does not, as of 1.97.1).
/// Only written when the user asks for it with `--editor vscode`.
fn assert_vscode_client_config(out: &Path, what: &str) {
    let vs = out.join(".vscode/settings.json");
    assert!(vs.exists(), "{what}: missing .vscode/settings.json");
    let v = std::fs::read_to_string(&vs).unwrap();
    assert!(
        v.contains("\"rust-analyzer.cargo.allTargets\": false"),
        "{what}: settings.json must disable cargo.allTargets:\n{v}"
    );
    assert!(
        v.contains("\"rust-analyzer.check.allTargets\": false"),
        "{what}: settings.json must disable check.allTargets:\n{v}"
    );
    // Client config is JSON, and a trailing comma (which esp-generate's own
    // settings.json happens to carry) is not valid strict JSON.
    assert!(!v.contains(",\n}"), "{what}: trailing comma in settings.json:\n{v}");
}

/// EVERY generated bare-metal project must ship the rust-analyzer config —
/// rust-analyzer's default `--all-targets` builds a test target, `test` does not
/// exist for these targets, and the user sees `E0463` on a project that builds
/// fine. This covers the whole non-ESP matrix; the ESP stacks are covered in
/// their own snapshots (which need their generators on PATH).
#[test]
fn non_esp_projects_ship_rust_analyzer_config() {
    let tmp = tempfile::tempdir().unwrap();
    for (chip, stack, framework, _) in NON_ESP_CELLS {
        let out = tmp.path().join(format!("{chip}-{stack}-{framework}"));
        generate_cell(chip, stack, framework, &out);
        assert_rust_analyzer_toml(&out, &format!("{chip}/{stack}/{framework}"));
    }
}

// ---------------------------------------------------------------------------
// Universal editor support (`--editor`). The files are written by the backend
// DISPATCHER, so this behaviour is backend-independent — these tests use the
// non-ESP scaffold because it needs no external generator, and
// `editor_files_are_backend_independent` re-checks one on each ESP stack too.
// ---------------------------------------------------------------------------

/// Every editor's file, and the exact key spelling that must appear in it. A
/// mis-spelled key is silently ignored by the editor, so the whole feature fails
/// invisibly — which is precisely what these literals pin.
const EDITOR_FILES: &[(&str, &str, &[&str])] = &[
    (
        "vscode",
        ".vscode/settings.json",
        &[
            "\"rust-analyzer.cargo.allTargets\": false",
            "\"rust-analyzer.check.allTargets\": false",
        ],
    ),
    (
        "helix",
        ".helix/languages.toml",
        &[
            "[language-server.rust-analyzer.config]",
            "cargo.allTargets = false",
            "check.allTargets = false",
        ],
    ),
    (
        "zed",
        ".zed/settings.json",
        &["\"initialization_options\"", "\"allTargets\": false"],
    ),
    (
        "neovim",
        ".nvim.lua",
        &["vim.lsp.config(\"rust_analyzer\"", "allTargets = false"],
    ),
];

#[test]
fn each_editor_emits_its_own_file_with_the_right_keys() {
    let tmp = tempfile::tempdir().unwrap();
    for (id, path, needles) in EDITOR_FILES {
        let out = tmp.path().join(format!("ed-{id}"));
        let mut c = bin();
        c.args(["--chip", "xiao-rp2040", "--editor", id, "-o", out.to_str().unwrap()]);
        assert!(c.output().unwrap().status.success(), "--editor {id} should generate");

        let f = out.join(path);
        assert!(f.exists(), "--editor {id} must write {path}");
        let s = std::fs::read_to_string(&f).unwrap();
        for n in *needles {
            assert!(s.contains(n), "--editor {id}: {path} missing `{n}`:\n{s}");
        }
        // The editor-agnostic file rides along whenever any editor is selected.
        assert!(out.join("rust-analyzer.toml").exists(), "--editor {id}: rust-analyzer.toml");

        // …and no OTHER editor's file is written.
        for (other, other_path, _) in EDITOR_FILES {
            if other != id {
                assert!(
                    !out.join(other_path).exists(),
                    "--editor {id} must not write {other}'s {other_path}"
                );
            }
        }
    }
}

#[test]
fn summary_reports_the_editor_and_the_neovim_caveat() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("nvim");
    let res = bin()
        .args(["--chip", "xiao-rp2040", "--editor", "neovim", "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(res.status.success());
    let s = String::from_utf8_lossy(&res.stdout);
    assert!(s.contains("editor:"), "summary must report the editor:\n{s}");
    assert!(s.contains("Neovim"), "summary must name the editor:\n{s}");
    assert!(s.contains("exrc"), "summary must surface the Neovim caveat:\n{s}");
    // The equivalent command round-trips the choice.
    assert!(s.contains("--editor neovim"), "command must carry the editor:\n{s}");
}

// ---------------------------------------------------------------------------
// Host prerequisite reporting (post-generation summary + `--check`).
//
// These assert the *end-to-end* behaviour over the real binary on the real host:
// the report appears, it is scoped to the project, and each marker reflects an
// actual probe. They deliberately do not assert that any particular tool IS
// installed (that varies by machine) — only that what is reported is consistent
// with probing the same thing here.
// ---------------------------------------------------------------------------

/// Run a probe the same way the tool does: is this executable on PATH at all
/// (exit status ignored — `picotool --version` exits 255 yet is installed).
fn tool_on_path(argv: &[&str]) -> bool {
    let (exe, args) = argv.split_first().unwrap();
    Command::new(exe).args(args).output().is_ok()
}

/// Both groups appear, and the marker next to a tool matches what an independent
/// probe of that same tool says right now. This is what makes the report
/// trustworthy: it is not a static list dressed up with ticks.
#[test]
fn summary_reports_prerequisites_and_the_markers_are_real() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("prereq-rp");
    let res = bin()
        .args(["--chip", "xiao-rp2040", "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(res.status.success());
    let s = String::from_utf8_lossy(&res.stdout);

    assert!(s.contains("prerequisites to build:"), "missing build group:\n{s}");
    assert!(s.contains("prerequisites to flash:"), "missing flash group:\n{s}");
    assert!(s.contains("thumbv6m-none-eabi"), "must name the RP2040 target:\n{s}");
    assert!(s.contains("picotool"), "must name the RP2040 flash tool:\n{s}");

    // The picotool PREREQUISITE line's marker must agree with an independent
    // probe. (The summary also carries a `flashing: picotool` line now that
    // FLASHING is a live axis on the non-ESP scaffold, so match the marker line.)
    let line = s
        .lines()
        .find(|l| l.contains("picotool") && (l.contains('✔') || l.contains('✗')))
        .unwrap_or_else(|| panic!("no picotool prerequisite line:\n{s}"));
    if tool_on_path(&["picotool", "version"]) {
        assert!(line.contains('✔'), "picotool is on PATH but reported missing: {line}");
        assert!(
            !s.contains("install: brew install picotool"),
            "an installed tool must not be given an install command:\n{s}"
        );
    } else {
        assert!(line.contains('✗'), "picotool is not on PATH but reported present: {line}");
        assert!(
            s.contains("install: brew install picotool"),
            "a missing tool must carry its install command:\n{s}"
        );
    }
}

/// The scoping promise, over the real binary: an RP2040 project never mentions
/// the ESP toolchain, and an ESP project never mentions the RP/UF2 tools.
#[test]
fn prerequisite_report_never_mentions_another_board_s_tools() {
    let tmp = tempfile::tempdir().unwrap();

    let rp = tmp.path().join("scope-rp");
    let out = bin()
        .args(["--chip", "xiao-rp2040", "-o", rp.to_str().unwrap()])
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    for foreign in ["espup", "espflash", "ldproxy", "hf2", "uf2conv"] {
        assert!(!s.contains(foreign), "RP2040 summary mentions {foreign}:\n{s}");
    }

    // `--check` needs no generator, so the ESP half runs everywhere.
    let out = bin().args(["--check", "--chip", "xiao-esp32c3"]).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    for foreign in ["picotool", "uf2conv", "hf2", "thumbv"] {
        assert!(!s.contains(foreign), "ESP32-C3 report mentions {foreign}:\n{s}");
    }
    assert!(s.contains("riscv32imc-unknown-none-elf"), "C3 must name its target:\n{s}");
}

/// Xtensa must be reported as needing espup, and RISC-V must not — the
/// distinction the ESP prerequisite story turns on. Uses `--check`, so no
/// generator is required.
#[test]
fn check_reports_espup_for_xtensa_only() {
    let s3 = bin().args(["--check", "--chip", "xiao-esp32s3"]).output().unwrap();
    let s3 = String::from_utf8_lossy(&s3.stdout);
    assert!(s3.contains("Xtensa"), "S3 must report the Xtensa toolchain:\n{s3}");
    assert!(s3.contains("espup"), "S3 must point at espup:\n{s3}");

    let c6 = bin().args(["--check", "--chip", "xiao-esp32c6"]).output().unwrap();
    let c6 = String::from_utf8_lossy(&c6.stdout);
    assert!(c6.contains("riscv32imac-unknown-none-elf"), "C6 target:\n{c6}");
    assert!(!c6.contains("espup"), "C6 is RISC-V — espup must not appear:\n{c6}");
}

/// The ESP `std` stack's own build tools, and the fact that RISC-V std needs
/// nightly + rust-src rather than espup.
#[test]
fn check_reports_the_esp_idf_std_build_tools() {
    let out = bin().args(["--check", "--chip", "xiao-esp32c3", "--std"]).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("ldproxy"), "esp-idf links through ldproxy:\n{s}");
    assert!(s.contains("python3") && s.contains("git"), "esp-idf host tools:\n{s}");
    assert!(s.contains("rust-src"), "RISC-V std is a build-std target:\n{s}");
    assert!(!s.contains("espup"), "RISC-V std must not ask for espup:\n{s}");
    // cmake/ninja are installed by esp-idf-sys itself — not host prerequisites.
    assert!(!s.contains("cmake") && !s.contains("ninja"), "bootstrapped tools leaked:\n{s}");
}

/// `--check` reports without generating, and its exit code carries the answer.
#[test]
fn check_generates_nothing_and_exit_code_tracks_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("should-not-exist");
    let res = bin()
        .args(["--check", "--chip", "xiao-rp2040", "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.exists(), "--check must not write a project");

    let s = String::from_utf8_lossy(&res.stdout);
    let missing = s.matches('✗').count();
    if missing == 0 {
        assert!(res.status.success(), "nothing missing → exit 0:\n{s}");
        assert!(s.contains("All prerequisites are installed"), "{s}");
    } else {
        assert_eq!(res.status.code(), Some(1), "something missing → exit 1:\n{s}");
        assert!(s.contains("missing"), "{s}");
    }
}

/// `--check` runs the same validation as generation, so a bad config is the same
/// clean exit-2 usage error — not a report about a project that cannot exist.
#[test]
fn check_rejects_an_invalid_config_before_probing() {
    let res = bin().args(["--check", "--chip", "xiao-nope"]).output().unwrap();
    assert_eq!(res.status.code(), Some(2));
    let e = String::from_utf8_lossy(&res.stderr);
    assert!(e.contains("unknown chip"), "{e}");

    // …and --check still requires a chip.
    let res = bin().arg("--check").output().unwrap();
    assert_eq!(res.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&res.stderr).contains("--chip"));
}

/// `--editor` takes exactly ONE value: a second occurrence is a clap usage error,
/// not a silently-merged selection.
#[test]
fn editor_flag_is_not_repeatable() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("dup");
    let res = bin()
        .args([
            "--chip", "xiao-rp2040", "--editor", "helix", "--editor", "zed",
            "-o", out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!res.status.success(), "a repeated --editor must be rejected");
    assert!(!out.exists(), "nothing must be written for a rejected invocation");
}

/// The opposite of `editor_none_writes_no_editor_specific_files`: asking for an
/// editor must deliver BOTH halves — the project-level `rust-analyzer.toml` and
/// that editor's client config. The client config is the half actually verified
/// to suppress `E0463` (the .toml alone does not, as of rust-analyzer 1.97.1), so
/// a regression that dropped it would leave the diagnostic in place while looking
/// like the feature still worked.
#[test]
fn editor_vscode_writes_both_the_project_file_and_the_client_config() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("vscode");
    let mut c = bin();
    c.args(["--chip", "xiao-rp2040", "--editor", "vscode", "-o", out.to_str().unwrap()]);
    assert!(c.output().unwrap().status.success());
    assert_rust_analyzer_config(&out, "--editor vscode");
}

#[test]
fn editor_none_writes_no_editor_specific_files() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("none");
    let mut c = bin();
    c.args(["--chip", "xiao-rp2040", "--editor", "none", "-o", out.to_str().unwrap()]);
    assert!(c.output().unwrap().status.success());

    // `rust-analyzer.toml` is NOT an editor file — it is a project file that
    // rust-analyzer reads itself, so it ships even here. `none` is the default,
    // so gating it on the editor would leave the typical project with no
    // rust-analyzer config at all.
    assert_rust_analyzer_toml(&out, "--editor none");

    // What `none` does suppress: every editor-SPECIFIC client config.
    for (id, path, _) in EDITOR_FILES {
        assert!(!out.join(path).exists(), "--editor none must not write {id}'s {path}");
    }
    // The project itself is still complete.
    assert!(out.join("Cargo.toml").exists(), "the project is still generated");
    assert!(out.join("src/main.rs").exists());
}

/// The no-regression guarantee: with NO `--editor` flag, a generated project must
/// still carry the VS Code client config that clears the spurious `test`-crate
/// error. This is the behaviour every project has always had.
#[test]
fn the_default_writes_no_editor_files_but_still_ships_rust_analyzer_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("default");
    generate_cell("xiao-rp2040", "rp2040-hal", "none", &out);

    // The editor-agnostic file is unconditional — it is what keeps a plain
    // `cargo build` project from tripping E0463 in whatever editor the user runs.
    assert_rust_analyzer_toml(&out, "default (no --editor)");

    // Writing into someone's editor configuration is opt-in, and `none` is the
    // default, so NO editor's file may appear — VS Code's included.
    for (id, path, _) in EDITOR_FILES {
        assert!(!out.join(path).exists(), "default must not write {id}'s {path}");
    }
}

/// The dispatcher writes these for every backend, so the same `--editor helix`
/// must land identically on the ESP stacks too (they self-skip without their
/// generators, exactly like the other ESP snapshots).
#[test]
fn editor_files_are_backend_independent() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: &[(&str, &[&str], bool)] = &[
        ("esp-hal", &["--chip", "xiao-esp32c3"], esp_generate_available()),
        ("esp-idf", &["--chip", "xiao-esp32c3", "--std"], cargo_generate_available()),
        ("non-esp", &["--chip", "xiao-rp2040"], true),
    ];
    for (label, args, available) in cases {
        if !available {
            eprintln!("skipping {label}: generator not installed");
            continue;
        }
        let out = tmp.path().join(format!("be-{label}"));
        let mut c = bin();
        c.args(*args).args(["--editor", "helix", "-o", out.to_str().unwrap()]);
        assert!(c.output().unwrap().status.success(), "{label} should generate");

        let f = out.join(".helix/languages.toml");
        assert!(f.exists(), "{label}: missing .helix/languages.toml");
        let s = std::fs::read_to_string(&f).unwrap();
        assert!(s.contains("[language-server.rust-analyzer.config]"), "{label}:\n{s}");
        assert_eq!(
            s.matches("allTargets = false").count(),
            2,
            "{label}: both cargo and check must be disabled:\n{s}"
        );
        // VS Code's file is NOT written when it was not asked for, on any backend.
        assert!(!out.join(".vscode/settings.json").exists(), "{label}: vscode not selected");
    }
}

#[test]
fn unknown_editor_is_a_config_error_before_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("bad");
    let res = bin()
        .args(["--chip", "xiao-rp2040", "--editor", "emacs", "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(res.status.code(), Some(2));
    let err = String::from_utf8_lossy(&res.stderr);
    assert!(err.contains("unknown --editor"), "stderr: {err}");
    // The message lists the whole catalog, `none` included.
    assert!(err.contains("vscode") && err.contains("none"), "stderr: {err}");
    assert!(!out.exists(), "nothing must be written for a rejected selection");
}

/// Cargo looks config keys up by splitting on `.`, so a target triple with a dot
/// must appear UNQUOTED in the `[target.…]` header. Quoting it is valid TOML but
/// cargo silently never matches the section, the rustflags (including
/// `-Tlink.x`) are dropped, and the RP2350 still "builds" — into an image with
/// no vector table and a 0x0 entry point.
#[test]
fn rp2350_config_target_header_is_unquoted() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("rp2350");
    generate_cell("xiao-rp2350", "rp235x-hal", "none", &out);
    let cfg = std::fs::read_to_string(out.join(".cargo/config.toml")).unwrap();
    assert!(cfg.contains("[target.thumbv8m.main-none-eabihf]"), "{cfg}");
    assert!(!cfg.contains("[target.\"thumbv8m.main-none-eabihf\"]"), "{cfg}");
    assert!(cfg.contains("-Tlink.x"), "{cfg}");
}

#[test]
fn non_esp_framework_axis_changes_the_generated_project() {
    let tmp = tempfile::tempdir().unwrap();

    // framework = none on an embassy HAL: the HAL is there, the executor and the
    // time driver are NOT (enabling the driver without an embassy-time user
    // drags the whole time stack in for nothing).
    let bare = tmp.path().join("bare");
    generate_cell("xiao-rp2040", "embassy-rp", "none", &bare);
    let toml = std::fs::read_to_string(bare.join("Cargo.toml")).unwrap();
    assert!(toml.contains("embassy-rp"), "{toml}");
    assert!(!toml.contains("embassy-executor"), "no executor without the framework:\n{toml}");
    assert!(!toml.contains("embassy-time"), "no embassy-time without the framework:\n{toml}");
    assert!(!toml.contains("time-driver"), "no time-driver feature without the framework:\n{toml}");
    let main = std::fs::read_to_string(bare.join("src/main.rs")).unwrap();
    assert!(main.contains("cortex_m_rt::entry"), "bare body:\n{main}");
    assert!(!main.contains("async fn main"), "bare body must not be async:\n{main}");

    // framework = embassy on the same HAL: executor + time + the time driver.
    let emb = tmp.path().join("emb");
    generate_cell("xiao-rp2040", "embassy-rp", "embassy", &emb);
    let toml = std::fs::read_to_string(emb.join("Cargo.toml")).unwrap();
    assert!(toml.contains("embassy-executor"), "{toml}");
    assert!(toml.contains("platform-cortex-m"), "executor 0.10 feature name:\n{toml}");
    assert!(toml.contains("embassy-time"), "{toml}");
    assert!(toml.contains("time-driver"), "{toml}");
    let main = std::fs::read_to_string(emb.join("src/main.rs")).unwrap();
    assert!(main.contains("async fn main"), "embassy body:\n{main}");
    assert!(main.contains("Timer::after_millis"), "embassy body:\n{main}");
}

/// `--logging defmt` wires the defmt logging stack into the non-ESP scaffold:
/// the deps, the `-Tdefmt.x` link arg (in rustflags, or in build.rs for the
/// embassy-rp/RP2040 stack), the global logger, and one `defmt::info!`. `--logging
/// none` (the default) touches none of it.
#[test]
fn non_esp_defmt_logging_wires_the_scaffold() {
    let tmp = tempfile::tempdir().unwrap();

    // A rustflags stack (rp2040-hal): defmt.x lands in .cargo/config.toml.
    let out = tmp.path().join("rp-defmt");
    let res = bin()
        .args(["--chip", "xiao-rp2040", "--logging", "defmt", "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(res.status.success());
    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(toml.contains("defmt = "), "defmt dep present:\n{toml}");
    assert!(toml.contains("defmt-rtt = "), "defmt-rtt dep present:\n{toml}");
    let cfg = std::fs::read_to_string(out.join(".cargo/config.toml")).unwrap();
    assert!(cfg.contains("-Tdefmt.x"), "defmt linker script in rustflags:\n{cfg}");
    let main = std::fs::read_to_string(out.join("src/main.rs")).unwrap();
    assert!(main.contains("use defmt_rtt as _;"), "global logger pulled in:\n{main}");
    assert!(main.contains("defmt::info!"), "one real defmt log emitted:\n{main}");

    // A build.rs-link-arg stack (embassy-rp on RP2040): defmt.x lands in build.rs,
    // after link.x / link-rp.x — NOT in .cargo/config.toml (that stack has no
    // rustflags block).
    let erp = tmp.path().join("erp-defmt");
    let res = bin()
        .args([
            "--chip", "xiao-rp2040", "--stack", "embassy-rp",
            "--logging", "defmt", "-o", erp.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(res.status.success());
    let build = std::fs::read_to_string(erp.join("build.rs")).unwrap();
    assert!(build.contains("-Tdefmt.x"), "defmt linker script in build.rs:\n{build}");
    let order = build.find("-Tlink.x").zip(build.find("-Tdefmt.x"));
    let (link_pos, defmt_pos) = order.expect("both link scripts must be emitted");
    assert!(link_pos < defmt_pos, "defmt.x must follow link.x:\n{build}");

    // logging = none (the default) wires none of it — byte-for-byte the old body.
    let none = tmp.path().join("rp-none");
    generate_cell("xiao-rp2040", "rp2040-hal", "none", &none);
    let toml = std::fs::read_to_string(none.join("Cargo.toml")).unwrap();
    assert!(!toml.contains("defmt"), "no defmt without --logging defmt:\n{toml}");
    let cfg = std::fs::read_to_string(none.join(".cargo/config.toml")).unwrap();
    assert!(!cfg.contains("defmt.x"), "no defmt.x without --logging defmt:\n{cfg}");
    let main = std::fs::read_to_string(none.join("src/main.rs")).unwrap();
    assert!(!main.contains("defmt"), "no defmt in the body without --logging defmt:\n{main}");
}

#[test]
fn non_esp_led_pin_is_substituted_from_the_registry() {
    let tmp = tempfile::tempdir().unwrap();

    let rp = tmp.path().join("rp");
    generate_cell("xiao-rp2040", "rp2040-hal", "none", &rp);
    assert!(std::fs::read_to_string(rp.join("src/main.rs")).unwrap().contains("gpio25"));

    let nrf = tmp.path().join("nrf");
    generate_cell("xiao-nrf52840", "nrf-hal", "none", &nrf);
    let main = std::fs::read_to_string(nrf.join("src/main.rs")).unwrap();
    assert!(main.contains("p0_26"), "nRF LED is P0.26:\n{main}");
    // Active-low LED must be initialised to its OFF level.
    assert!(main.contains("Level::High"), "active-low LED starts off:\n{main}");

    let emb = tmp.path().join("embnrf");
    generate_cell("xiao-nrf52840", "embassy-nrf", "embassy", &emb);
    assert!(std::fs::read_to_string(emb.join("src/main.rs")).unwrap().contains("P0_26"));

    // SAMD21's LED is owned by the xiao_m0 BSP (`Led0` == PA17), so its body
    // carries no pin placeholder at all.
    let samd = tmp.path().join("samd");
    generate_cell("xiao-samd21", "atsamd-hal", "none", &samd);
    let main = std::fs::read_to_string(samd.join("src/main.rs")).unwrap();
    assert!(main.contains("pins.led0"), "{main}");
    assert!(!main.contains("{{LED"), "no unsubstituted placeholder:\n{main}");
}

#[test]
fn non_esp_add_crate_and_extra_deps_layer_onto_the_scaffold() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("extra");
    let mut c = bin();
    c.args([
        "--chip", "xiao-rp2040",
        "--add-crate", "heapless@0.8",
        "--add-crate", "fugit",
        "-o", out.to_str().unwrap(),
    ]);
    assert!(c.output().unwrap().status.success());
    let toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(toml.contains("heapless = \"0.8\""), "{toml}");
    assert!(toml.contains("fugit = \"*\""), "{toml}");
    // the scaffold's own deps survive
    assert!(toml.contains("rp2040-hal"), "{toml}");
    assert!(toml.contains("rp2040-boot2"), "{toml}");
}

/// The M4 gates, at config time, with a real reason and exit 2 — and nothing
/// written to disk.
#[test]
fn non_esp_gated_combos_are_refused_before_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: &[(&[&str], &str)] = &[
        (
            &["--chip", "xiao-rp2040", "--stack", "rp2040-hal", "--framework", "embassy"],
            "embassy HAL",
        ),
        (
            &["--chip", "xiao-rp2350", "--stack", "rp235x-hal", "--framework", "embassy"],
            "embassy HAL",
        ),
        (
            &["--chip", "xiao-nrf52840", "--stack", "nrf-hal", "--framework", "embassy"],
            "embassy HAL",
        ),
        (
            &["--chip", "xiao-nrf52840-sense", "--stack", "nrf-hal", "--framework", "embassy"],
            "embassy HAL",
        ),
        (&["--chip", "xiao-rp2040", "--framework", "rtic"], "RTIC"),
        (&["--chip", "xiao-samd21", "--framework", "rtic"], "RTIC"),
        // SAMD21 has no embassy stack at all -> a support-list rejection.
        (&["--chip", "xiao-samd21", "--framework", "embassy"], "not supported"),
        // `log` is the ESP-only (esp-println) frontend — refused on Cortex-M.
        (&["--chip", "xiao-rp2040", "--logging", "log"], "ESP-only"),
        // component templates stay gated in M4.
        (&["--chip", "xiao-rp2040", "--template", "rgb-led"], "compile-verified"),
        (&["--chip", "xiao-nrf52840-sense", "--template", "imu-read"], "not documented by Seeed"),
    ];
    for (i, (args, needle)) in cases.iter().enumerate() {
        let out = tmp.path().join(format!("gated{i}"));
        let res = bin().args(*args).args(["-o", out.to_str().unwrap()]).output().unwrap();
        assert_eq!(res.status.code(), Some(2), "{args:?} should exit 2");
        let err = String::from_utf8_lossy(&res.stderr);
        assert!(err.contains(needle), "{args:?} stderr should mention '{needle}':\n{err}");
        assert!(!out.exists(), "{args:?} must not create {}", out.display());
    }
}

#[test]
fn list_stacks_reports_non_esp_frameworks_and_gaps() {
    let out = bin().args(["--list-stacks", "--chip", "xiao-rp2040"]).output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(!s.contains("not implemented until M4"), "M4 landed:\n{s}");
    assert!(s.contains("rp2040-hal") && s.contains("embassy-rp"), "{s}");
    assert!(s.contains("thumbv6m-none-eabi"), "{s}");
    // embassy is a gap on the blocking HAL and available on the embassy HAL.
    assert!(s.contains("[gap]"), "the blocking stack should show its embassy gap:\n{s}");
    // The per-stack flashing/logging support subset is reported (so the matrix
    // harness enumerates each stack's axes from here, not the universal catalog).
    assert!(s.contains("flashing: picotool, probe-rs"), "rp2040 flashing subset:\n{s}");
    assert!(s.contains("logging: none, defmt"), "rp2040 logging subset:\n{s}");
    assert!(!s.contains("espflash"), "espflash is not an rp2040 flash tool:\n{s}");
}

/// THE M4 COMPILE GATE. Generates each supported cell and builds it for its ARM
/// target, then asserts the result is a real firmware image — not merely "an ELF
/// was produced". A dropped linker script still links successfully; it just
/// yields an image with a 0x0 entry point and no vector table, which is exactly
/// the failure this test exists to catch.
#[test]
#[ignore = "opt-in: set XIAO_COMPILE_GATE=1 (downloads and builds every HAL)"]
fn compile_gate_every_non_esp_cell_builds_a_real_image() {
    if !compile_gate_enabled() {
        eprintln!("skipping: set XIAO_COMPILE_GATE=1 to run the compile gate");
        return;
    }
    let nm = llvm_nm().expect("llvm-nm from the rustup sysroot");
    let tmp = tempfile::tempdir().unwrap();
    for (chip, stack, framework, target) in NON_ESP_CELLS {
        if !target_installed(target) {
            eprintln!("skipping {chip}/{stack}/{framework}: target {target} not installed");
            continue;
        }
        let name = format!("{chip}-{stack}-{framework}");
        let out = tmp.path().join(&name);
        generate_cell(chip, stack, framework, &out);

        let build = Command::new("cargo")
            .arg("build")
            .current_dir(&out)
            .output()
            .expect("cargo build");
        assert!(
            build.status.success(),
            "{name} failed to build:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let elf = out.join("target").join(target).join("debug").join(&name);
        assert!(elf.exists(), "{name}: no ELF at {}", elf.display());
        let syms = Command::new(&nm).arg(&elf).output().expect("llvm-nm");
        let syms = String::from_utf8_lossy(&syms.stdout);
        assert!(
            syms.lines().any(|l| l.ends_with(" Reset")),
            "{name}: no Reset handler — the linker script was not applied"
        );
        assert!(
            syms.lines().any(|l| l.ends_with(" __vector_table")),
            "{name}: no vector table — the linker script was not applied"
        );
    }
}

/// The defmt half of the compile gate: every non-ESP cell must ALSO build with
/// `--logging defmt`. This is the load-bearing proof that the defmt wiring (the
/// deps, the `-Tdefmt.x` link arg, the injected global logger + `defmt::info!`)
/// links on every non-ESP HAL — the whole non-ESP logging axis rests on it. Same
/// opt-in gate as the base-image test; builds `--release`.
#[test]
#[ignore = "opt-in: set XIAO_COMPILE_GATE=1 (downloads and builds every HAL)"]
fn compile_gate_every_non_esp_cell_builds_with_defmt() {
    if !compile_gate_enabled() {
        eprintln!("skipping: set XIAO_COMPILE_GATE=1 to run the compile gate");
        return;
    }
    let nm = llvm_nm().expect("llvm-nm from the rustup sysroot");
    let tmp = tempfile::tempdir().unwrap();
    for (chip, stack, framework, target) in NON_ESP_CELLS {
        if !target_installed(target) {
            eprintln!("skipping {chip}/{stack}/{framework}: target {target} not installed");
            continue;
        }
        let name = format!("{chip}-{stack}-{framework}-defmt");
        let out = tmp.path().join(&name);
        let mut args = vec!["--chip", *chip, "--stack", *stack, "--logging", "defmt"];
        if *framework != "none" {
            args.extend_from_slice(&["--framework", framework]);
        }
        generate(&args, &out);

        // The wiring must be present in the generated project.
        assert!(
            std::fs::read_to_string(out.join("Cargo.toml")).unwrap().contains("defmt-rtt"),
            "{name}: no defmt-rtt dep"
        );
        assert!(
            std::fs::read_to_string(out.join("src/main.rs")).unwrap().contains("defmt::info!"),
            "{name}: no defmt::info! in the body"
        );

        let build = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&out)
            .output()
            .expect("cargo build --release");
        assert!(
            build.status.success(),
            "{name} failed to build with defmt:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let elf = out.join("target").join(target).join("release").join(&name);
        assert!(elf.exists(), "{name}: no ELF at {}", elf.display());
        let syms = Command::new(&nm).arg(&elf).output().expect("llvm-nm");
        let syms = String::from_utf8_lossy(&syms.stdout);
        // Both linker scripts applied: link.x's vector table AND defmt.x's own
        // linker-defined symbols (`_defmt_*`).
        assert!(
            syms.lines().any(|l| l.ends_with(" __vector_table")),
            "{name}: no vector table — link.x was not applied"
        );
        assert!(syms.contains("_defmt_"), "{name}: no _defmt_* symbols — defmt.x was not applied");
    }
}

fn llvm_nm() -> Option<std::path::PathBuf> {
    let sysroot = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;
    let sysroot = String::from_utf8_lossy(&sysroot.stdout).trim().to_string();
    let base = Path::new(&sysroot).join("lib/rustlib");
    for entry in std::fs::read_dir(base).ok()? {
        let p = entry.ok()?.path().join("bin/llvm-nm");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// THE nRF52840 BRICK GUARD.
//
// A stock XIAO nRF52840 / nRF52840 Sense ships the Adafruit nRF52 UF2
// bootloader with SoftDevice S140 v7. That leaves the application exactly one
// window of flash, and the regions on either side of it are not recoverable
// over USB — putting them back needs an SWD probe. So a generated project whose
// `memory.x` drifts back to the flat `ORIGIN = 0x0` layout produces firmware
// that destroys the board's bootloader the moment it is UF2-flashed.
//
// The layout below is sourced in `src/templates/files/memory_nrf52840.x`
// (Zephyr's `xiao_ble` partition map + Adafruit's own bootloader linker
// script); these two tests are what keep the generator honest about it.
// ---------------------------------------------------------------------------

/// Where the application must start. The MBR + SoftDevice S140 7.3.0 own
/// everything below it, and the bootloader jumps to exactly this address
/// (`SD_SIZE_GET(MBR_SIZE)`), so it is not merely a floor.
const NRF_APP_FLASH_ORIGIN: u64 = 0x0002_7000;
/// One past the last byte the UF2 bootloader will write: `USER_FLASH_END =
/// BOOTLOADER_REGION_START (0xF4000) - DFU_APP_DATA_RESERVED (40K)`. Blocks
/// outside it are dropped, so an image that runs past here is flashed truncated.
const NRF_UF2_USER_FLASH_END: u64 = 0x000E_A000;
/// The MBR forwards every interrupt through a pointer it keeps in the lowest
/// bytes of RAM, so the application's RAM may not start at 0x20000000.
const NRF_APP_RAM_ORIGIN: u64 = 0x2000_0008;
/// One past the last byte of the nRF52840's 256 KiB of RAM.
const NRF_RAM_END: u64 = 0x2004_0000;

/// Strip `/* … */` comments, so prose that mentions FLASH, RAM or ORIGIN — and
/// `memory_nrf52840.x`'s header comment mentions all three, including the
/// bootloader's own `ORIGIN 0xF4000` — can never be parsed as the real MEMORY
/// block.
fn strip_block_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        match rest[open..].find("*/") {
            Some(close) => rest = &rest[open + close + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Evaluate one linker-script size expression: `+`/`-` over hex or decimal
/// literals with an optional `K`/`M` suffix. That is the whole grammar our
/// `memory.x` bodies use (`0x00027000`, `788K`, `256K - 8`), and evaluating it
/// rather than string-matching is the point — the test asserts where the image
/// lands, not how the script happens to spell it.
fn eval_linker_expr(expr: &str) -> u64 {
    fn term(t: &str) -> u64 {
        let t = t.trim();
        let (digits, scale) = match t.chars().last() {
            Some('K' | 'k') => (&t[..t.len() - 1], 1024),
            Some('M' | 'm') => (&t[..t.len() - 1], 1024 * 1024),
            _ => (t, 1),
        };
        let digits = digits.trim();
        let parsed = match digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
            Some(hex) => u64::from_str_radix(hex, 16),
            None => digits.parse::<u64>(),
        };
        parsed.unwrap_or_else(|_| panic!("memory.x: cannot evaluate term `{t}`")) * scale
    }

    let mut total: i64 = 0;
    let mut sign: i64 = 1;
    let mut current = String::new();
    for ch in expr.chars() {
        if ch == '+' || ch == '-' {
            total += sign * term(&current) as i64;
            sign = if ch == '+' { 1 } else { -1 };
            current.clear();
        } else {
            current.push(ch);
        }
    }
    total += sign * term(&current) as i64;
    u64::try_from(total).unwrap_or_else(|_| panic!("memory.x: `{expr}` evaluated negative"))
}

/// `(origin, length)` of one region of a `memory.x` MEMORY block.
fn memory_region(script: &str, region: &str) -> (u64, u64) {
    let script = strip_block_comments(script);
    for line in script.lines() {
        let line = line.trim().trim_end_matches(',');
        let Some(rest) = line.strip_prefix(region) else { continue };
        let Some(rest) = rest.trim_start().strip_prefix(':') else { continue };
        let (origin, length) = rest
            .split_once("LENGTH")
            .unwrap_or_else(|| panic!("memory.x: no LENGTH in `{line}`"));
        let origin = origin
            .split_once('=')
            .unwrap_or_else(|| panic!("memory.x: no ORIGIN `=` in `{line}`"))
            .1
            .trim_end()
            .trim_end_matches(',');
        let length = length
            .split_once('=')
            .unwrap_or_else(|| panic!("memory.x: no LENGTH `=` in `{line}`"))
            .1;
        return (eval_linker_expr(origin), eval_linker_expr(length));
    }
    panic!("memory.x: no `{region}` region in:\n{script}");
}

/// BRICK GUARD, generation half — runs unconditionally, no toolchain needed.
///
/// Every generated nRF52840 project must place FLASH inside the one window the
/// stock bootloader leaves free, and must keep its hands off the bottom of RAM
/// (the MBR forwards interrupts through a pointer it keeps there, so an
/// application that allocates over it loses every interrupt on the board).
#[test]
fn nrf52840_memory_x_stays_clear_of_the_bootloader_and_softdevice() {
    let tmp = tempfile::tempdir().unwrap();
    let mut checked = 0;
    for (chip, stack, framework, _) in NON_ESP_CELLS {
        if !chip.starts_with("xiao-nrf52840") {
            continue;
        }
        let cell = format!("{chip}/{stack}/{framework}");
        let out = tmp.path().join(format!("{chip}-{stack}-{framework}"));
        generate_cell(chip, stack, framework, &out);
        let script = std::fs::read_to_string(out.join("memory.x"))
            .unwrap_or_else(|e| panic!("{cell}: no memory.x ({e})"));

        let (flash_origin, flash_len) = memory_region(&script, "FLASH");
        assert!(
            flash_origin >= NRF_APP_FLASH_ORIGIN,
            "{cell}: FLASH ORIGIN {flash_origin:#x} is inside the MBR + SoftDevice S140 v7 \
             region (below {NRF_APP_FLASH_ORIGIN:#x}). Firmware linked there, UF2-flashed to a \
             stock XIAO, overwrites the bootloader — recovery needs an SWD probe.\n{script}"
        );
        assert_eq!(
            flash_origin, NRF_APP_FLASH_ORIGIN,
            "{cell}: FLASH ORIGIN {flash_origin:#x} is not where the bootloader jumps. It \
             branches to SD_SIZE_GET(MBR_SIZE), which S140 7.3.0 reports as \
             {NRF_APP_FLASH_ORIGIN:#x} — an image linked anywhere else simply never starts.\
             \n{script}"
        );
        assert!(
            flash_origin + flash_len <= NRF_UF2_USER_FLASH_END,
            "{cell}: FLASH runs to {:#x}, past USER_FLASH_END ({NRF_UF2_USER_FLASH_END:#x}). \
             The UF2 bootloader drops every block at or above that, so an image this size is \
             flashed with its tail silently missing.\n{script}",
            flash_origin + flash_len
        );
        // A vector table needs 0x100 alignment on this part (16 exceptions + 48
        // IRQs = 256 bytes), and it sits at ORIGIN(FLASH).
        assert_eq!(flash_origin % 0x100, 0, "{cell}: FLASH ORIGIN {flash_origin:#x} misaligned");

        let (ram_origin, ram_len) = memory_region(&script, "RAM");
        assert!(
            ram_origin >= NRF_APP_RAM_ORIGIN,
            "{cell}: RAM ORIGIN {ram_origin:#x} overlaps the MBR's interrupt-forwarding \
             pointer (must be at least {NRF_APP_RAM_ORIGIN:#x})\n{script}"
        );
        assert!(
            ram_origin + ram_len <= NRF_RAM_END,
            "{cell}: RAM runs past the end of the nRF52840's 256 KiB\n{script}"
        );
        checked += 1;
    }
    assert!(checked > 0, "no nRF52840 cells in NON_ESP_CELLS — did the matrix lose them?");
}

/// ELF32 `e_entry`: the little-endian word at file offset 24. Deliberately the
/// same read `scripts/matrix.sh`'s flashing safety gate performs before it will
/// write an image to a board, so the test and the gate cannot disagree.
fn elf_entry(elf: &Path) -> u64 {
    let bytes = std::fs::read(elf).unwrap_or_else(|e| panic!("read {}: {e}", elf.display()));
    assert!(bytes.len() > 28, "{}: too short to be an ELF", elf.display());
    assert_eq!(&bytes[..4], b"\x7fELF", "{}: not an ELF", elf.display());
    assert_eq!(bytes[4], 1, "{}: expected a 32-bit ELF", elf.display());
    u64::from(u32::from_le_bytes(bytes[24..28].try_into().unwrap()))
}

/// BRICK GUARD, linked-image half. The generation test above proves the
/// *script* is right; this proves the *linker honoured it*. A `memory.x` that is
/// never picked up (dropped `build.rs`, dropped `-Tlink.x`) links happily to a
/// low entry point, which is the exact image that destroys the bootloader.
#[test]
#[ignore = "opt-in: set XIAO_COMPILE_GATE=1 (downloads and builds every HAL)"]
fn compile_gate_nrf52840_firmware_links_above_the_bootloader_region() {
    if !compile_gate_enabled() {
        eprintln!("skipping: set XIAO_COMPILE_GATE=1 to run the compile gate");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    for (chip, stack, framework, target) in NON_ESP_CELLS {
        if !chip.starts_with("xiao-nrf52840") {
            continue;
        }
        if !target_installed(target) {
            eprintln!("skipping {chip}/{stack}/{framework}: target {target} not installed");
            continue;
        }
        let name = format!("{chip}-{stack}-{framework}");
        let out = tmp.path().join(&name);
        generate_cell(chip, stack, framework, &out);

        let build = Command::new("cargo").arg("build").current_dir(&out).output().expect("cargo");
        assert!(
            build.status.success(),
            "{name} failed to build:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );

        let elf = out.join("target").join(target).join("debug").join(&name);
        let entry = elf_entry(&elf);
        assert!(
            entry >= NRF_APP_FLASH_ORIGIN,
            "{name}: entry point {entry:#010x} is below {NRF_APP_FLASH_ORIGIN:#x}. This image \
             cannot be UF2-flashed to a stock XIAO nRF52840: written where it is linked it \
             overwrites the MBR + SoftDevice + bootloader (recovery needs an SWD probe), and \
             relocated to {NRF_APP_FLASH_ORIGIN:#x} every absolute address in it is wrong."
        );
        assert!(
            entry < NRF_UF2_USER_FLASH_END,
            "{name}: entry point {entry:#010x} is past USER_FLASH_END \
             ({NRF_UF2_USER_FLASH_END:#x}) — outside the window the UF2 bootloader will write"
        );
    }
}
