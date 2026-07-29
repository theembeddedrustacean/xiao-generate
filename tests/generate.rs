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
        // flashing/logging are not a live axis on the non-ESP scaffold.
        (&["--chip", "xiao-rp2040", "--flashing", "probe-rs"], "--flashing"),
        (&["--chip", "xiao-rp2040", "--logging", "defmt"], "--logging"),
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
