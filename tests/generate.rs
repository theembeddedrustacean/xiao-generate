//! Black-box tests over the built binary: CLI behaviour (R2 rejections, listing)
//! and end-to-end generation snapshots (R6). The generation tests require the
//! pinned `esp-generate` on PATH; they self-skip if it is absent so the suite
//! still passes on a machine/CI lane without it (the compile matrix is separate).

use std::path::Path;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xiao-generate"))
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
}
