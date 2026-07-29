//! Embedded template bodies. Component/µFerris bodies are the *verbatim*,
//! compile-verified adaptations of upstream examples (D4: sourced, not
//! authored); base bodies are minimal and carry a `{{LED_GPIO}}` placeholder.

/// Body for a non-µFerris template id, or `None` if unknown.
///
/// `mic-capture` is intentionally absent — it is a documented gap (gated in
/// `Config::resolve` before generation ever reaches here), so we ship no body.
pub fn body(id: &str) -> Option<&'static str> {
    Some(match id {
        "base-blink" => include_str!("files/base_blink.rs.tmpl"),
        "base-hello" => include_str!("files/base_hello.rs.tmpl"),
        "camera-snapshot" => include_str!("files/camera_snapshot.rs.tmpl"),
        "sd-list" => include_str!("files/sd_list.rs.tmpl"),
        _ => return None,
    })
}

/// Base `src/main.rs` body for a non-ESP `(chip, stack, framework)` cell, or
/// `None` when that cell has no verified body (which the registry must already
/// have refused via `Stack::framework_gap`, so reaching `None` is a bug).
///
/// The HAL axis and the FRAMEWORK axis are orthogonal: an embassy HAL has both
/// an executor body and a bare (`cortex_m_rt::entry`) body. Every body here
/// produced a real ARM ELF — see research/hal-framework-matrix.md.
pub fn nonesp_base_body(chip: &str, stack_id: &str, framework: &str) -> Option<&'static str> {
    Some(match (chip, stack_id, framework) {
        (_, "rp2040-hal", "none") => include_str!("files/nonesp/rp2040_hal_blink.rs.tmpl"),
        (_, "rp235x-hal", "none") => include_str!("files/nonesp/rp235x_hal_blink.rs.tmpl"),
        (_, "nrf-hal", "none") => include_str!("files/nonesp/nrf_hal_blink.rs.tmpl"),
        (_, "atsamd-hal", "none") => include_str!("files/nonesp/samd21_blink.rs.tmpl"),
        // embassy HALs: the same body serves RP2040 and RP2350 (embassy-rp emits
        // the RP2350 IMAGE_DEF itself, so only the Cargo feature differs).
        (_, "embassy-rp", "none") => include_str!("files/nonesp/embassy_rp_blink_bare.rs.tmpl"),
        (_, "embassy-rp", "embassy") => include_str!("files/nonesp/embassy_rp_blink.rs.tmpl"),
        (_, "embassy-nrf", "none") => include_str!("files/nonesp/embassy_nrf_blink_bare.rs.tmpl"),
        (_, "embassy-nrf", "embassy") => include_str!("files/nonesp/embassy_nrf_blink.rs.tmpl"),
        _ => return None,
    })
}

/// A `memory.x` linker-script body by [`crate::registry::Scaffold::memory_x`] id.
pub fn memory_x(id: &str) -> Option<&'static str> {
    Some(match id {
        "rp2040-boot2" => include_str!("files/memory_rp2040_boot2.x"),
        "rp2040-embassy" => include_str!("files/memory_rp2040_embassy.x"),
        "rp2350" => include_str!("files/memory_rp2350.x"),
        "nrf52840" => include_str!("files/memory_nrf52840.x"),
        _ => return None,
    })
}

/// Project-local rust-analyzer settings, emitted into **every** generated
/// project regardless of backend (see `backend::scaffold`).
///
/// Without it every project we generate reports `E0463: can't find crate for
/// test` in the user's editor — see the commentary in the file itself.
pub fn rust_analyzer_toml() -> &'static str {
    include_str!("files/rust_analyzer.toml.tmpl")
}

/// The rust-analyzer **client** configuration for one [`crate::registry::Editor`]
/// id, or `None` if unknown.
///
/// Client config is the delivery path that is *verified* to suppress the
/// diagnostic; a project-local `rust-analyzer.toml` alone does not (see the note
/// in that file). Each body sets the same two keys — `cargo.allTargets` and
/// `check.allTargets` — in whatever spelling and file format that editor reads:
///
/// - **VS Code** `.vscode/settings.json` — flat, dotted, `rust-analyzer.`-prefixed
///   keys, which the extension forwards as client configuration.
/// - **Helix** `.helix/languages.toml` — a `[language-server.rust-analyzer.config]`
///   table, which Helix sends as the server's LSP initialization options.
/// - **Zed** `.zed/settings.json` — `lsp.rust-analyzer.initialization_options`
///   (snake_case), which Zed passes straight through. Zed requires nested objects
///   here and rejects VS Code's dotted style.
/// - **Neovim** `.nvim.lua` — `vim.lsp.config("rust_analyzer", …)`. Neovim is the
///   one editor with no automatic project-local mechanism: the file needs
///   `vim.o.exrc = true` plus `:trust`, which is why the catalog entry carries a
///   caveat and the file says so at the top.
///
/// Keys and shapes are pinned by the tests below; the table is kept in step with
/// the catalog by `every_file_emitting_editor_has_a_config_body`.
pub fn editor_config(id: &str) -> Option<&'static str> {
    Some(match id {
        "vscode" => include_str!("files/vscode_settings.json.tmpl"),
        "helix" => include_str!("files/helix_languages.toml.tmpl"),
        "zed" => include_str!("files/zed_settings.json.tmpl"),
        "neovim" => include_str!("files/nvim.lua.tmpl"),
        _ => return None,
    })
}

/// The generated `build.rs` for a non-ESP project that ships its own `memory.x`.
/// `{{LINK_ARGS}}` is replaced by the scaffold with the stack's extra
/// `cargo:rustc-link-arg-bins=` lines (empty for most stacks).
pub fn nonesp_build_rs() -> &'static str {
    include_str!("files/nonesp_build.rs.tmpl")
}

/// Body for a µFerris template id, or `None` if unknown.
pub fn uferris_body(id: &str) -> Option<&'static str> {
    Some(match id {
        "blinky" => include_str!("files/uferris_blinky.rs.tmpl"),
        _ => return None,
    })
}

/// Placeholder in the µFerris body for the `esp_alloc` heap size.
pub const DRAM2_HEAP_BYTES: &str = "{{DRAM2_HEAP_BYTES}}";

/// Size, in bytes, of the `.dram2_uninit` heap a µFerris project hands
/// `esp_alloc`. `None` for a chip with no recorded figure.
///
/// This has to be per-chip. `dram2_seg` is a fixed window declared in esp-hal's
/// linker script and its size differs between SoCs, so one shared literal cannot
/// be correct everywhere — and emitting one anyway is precisely what broke the
/// ESP32-C6 build. The old value, `66320`, is the C3's region to the byte; the
/// C6's region is 784 bytes smaller, which was the exact reported overflow.
///
/// Nothing but `*(.dram2_uninit)` is placed in `dram2_seg`, and esp-alloc's
/// macro expands to a bare `MaybeUninit<[u8; N]>` with no bookkeeping, so the
/// emitted section size *equals* `N` and the region size is a hard ceiling.
///
/// | chip | `dram2_seg` | capacity | heap emitted | slack |
/// |---|---|---:|---:|---:|
/// | esp32c3 | `0x3FCCE400..0x3FCDE710` | `0x10310` | `0xFF00` | 1040 B |
/// | esp32c6 | `0x4086E610..0x4087E610` | `0x10000` | `0xFC00` | 1024 B |
///
/// Each figure deliberately leaves ~1 KiB rather than consuming the region to
/// the last byte. Saturating it is what produced this bug in the first place: a
/// zero-slack fit re-breaks the link the moment anything else lands in the
/// section, with no margin to absorb it. The lost kilobyte of heap is worth the
/// margin — the µFerris bodies do not allocate meaningfully.
///
/// Only the µFerris-eligible chips need an entry; `uferris_bsp_chips_all_have_a_dram2_heap_size`
/// keeps this table in step with the registry, so a newly-eligible variant fails
/// the test suite rather than silently generating a wrong constant.
pub fn dram2_heap_bytes(chip: &str) -> Option<u32> {
    Some(match chip {
        "esp32c3" => 0xFF00,
        "esp32c6" => 0xFC00,
        _ => return None,
    })
}

/// Body for the ESP `std` (`esp-idf`) stack's synthetic `base` template: a std
/// blink (via `esp-idf-hal`) on variants that carry an onboard LED, a std
/// log-hello (via `esp-idf-svc`) otherwise. Both are adaptations of upstream
/// sources (D4): the blink from esp-idf-hal's `examples/blinky.rs`, the hello
/// from esp-idf-template's own default `main.rs`. The blink uses `{{LED_GPIO}}`
/// exactly like the no_std base-blink, so `render` substitutes it uniformly.
pub fn std_body(has_led: bool) -> &'static str {
    if has_led {
        include_str!("files/std_blink.rs.tmpl")
    } else {
        include_str!("files/std_hello.rs.tmpl")
    }
}

/// Substitute template placeholders. Only the base-blink body uses them
/// (`{{LED_GPIO}}`, `{{LED_INIT_LEVEL}}`); component/µFerris bodies are verbatim.
/// Kept as a plain substitution — no templating engine — because those bodies
/// are real Rust with single braces a `{{ }}`/`{% %}` engine would risk
/// misparsing.
pub fn render(body: &str, led: Option<crate::registry::Led>) -> String {
    match led {
        Some(l) => {
            // Initialise to the OFF level: active-low LED is off when driven High.
            let init_level = match l.active_low {
                Some(true) => "High",
                Some(false) => "Low",
                None => "Low", // polarity undocumented — arbitrary; blink still toggles
            };
            body.replace("{{LED_GPIO}}", &l.gpio.to_string())
                .replace("{{LED_INIT_LEVEL}}", init_level)
        }
        None => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use toml_edit::DocumentMut;

    /// The emitted rust-analyzer config is only useful if the key names are
    /// exactly the ones rust-analyzer understands. Both were read off
    /// `rust-analyzer --print-config-schema` (1.97.1):
    ///
    /// - `rust-analyzer.cargo.allTargets` — "Pass `--all-targets` to cargo invocation."
    /// - `rust-analyzer.check.allTargets` — "Check all targets and tests (`--all-targets`)."
    ///
    /// In a project-local `rust-analyzer.toml` the `rust-analyzer.` prefix is
    /// dropped and the rest nests, so they must appear as `[cargo] allTargets`
    /// and `[check] allTargets`. A typo here fails silently in the user's editor
    /// (rust-analyzer just ignores keys it does not know), which is exactly the
    /// kind of bug a test has to catch.
    #[test]
    fn rust_analyzer_toml_is_valid_and_uses_the_documented_keys() {
        let doc: DocumentMut = super::rust_analyzer_toml()
            .parse()
            .expect("rust-analyzer.toml template must be valid TOML");

        assert_eq!(
            doc["cargo"]["allTargets"].as_bool(),
            Some(false),
            "cargo.allTargets must be false; got:\n{doc}"
        );
        assert_eq!(
            doc["check"]["allTargets"].as_bool(),
            Some(false),
            "check.allTargets must be false; got:\n{doc}"
        );

        // Guard the two spellings that look right but are silently ignored.
        let s = super::rust_analyzer_toml();
        assert!(!s.contains("all_targets"), "wrong casing (snake_case) in:\n{s}");
        assert!(!s.contains("all-targets ="), "wrong casing (kebab-case) in:\n{s}");
        assert!(
            !s.contains("[rust-analyzer"),
            "rust-analyzer.toml keys must NOT carry the `rust-analyzer.` prefix:\n{s}"
        );
    }

    /// The VS Code file is the mirror image: client config, so here the keys DO
    /// carry the `rust-analyzer.` prefix and are flat. It must also be strict
    /// JSON — VS Code tolerates JSONC, but other tooling reading it may not.
    #[test]
    fn vscode_settings_are_strict_json_with_the_prefixed_keys() {
        let s = super::editor_config("vscode").unwrap();
        assert!(s.contains("\"rust-analyzer.cargo.allTargets\": false"), "{s}");
        assert!(s.contains("\"rust-analyzer.check.allTargets\": false"), "{s}");
        assert!(!s.contains("all_targets"), "wrong casing (snake_case) in:\n{s}");

        // Strict-JSON shape: balanced braces, no trailing comma before `}`.
        assert!(!s.contains(",\n}"), "trailing comma is not valid JSON:\n{s}");
        assert_eq!(
            s.matches('{').count(),
            s.matches('}').count(),
            "unbalanced braces in:\n{s}"
        );
    }

    // -----------------------------------------------------------------------
    // Per-editor client config. Each editor spells these two settings its own
    // way, and every one of them fails SILENTLY when mis-spelled (the editor
    // just ignores a key it does not know), so each shape is pinned here.
    // -----------------------------------------------------------------------

    /// The catalog and the body table must never drift apart — a file-emitting
    /// entry with no body would reach `write_editor_config`'s "internal error"
    /// arm, and `none` must conversely have no body at all.
    #[test]
    fn every_file_emitting_editor_has_a_config_body() {
        for e in crate::registry::EDITORS {
            match e.path {
                Some(path) => {
                    let body = super::editor_config(e.id)
                        .unwrap_or_else(|| panic!("editor '{}' has no config body", e.id));
                    assert!(!body.trim().is_empty(), "editor '{}' body is empty", e.id);
                    assert!(
                        !path.is_empty() && !path.starts_with('/'),
                        "editor '{}' path must be project-relative, got '{path}'",
                        e.id
                    );
                }
                None => {
                    assert_eq!(e.id, "none", "only `none` may emit no file");
                    assert!(
                        super::editor_config(e.id).is_none(),
                        "`none` must have no config body"
                    );
                }
            }
        }
        // …and nothing in the body table is missing from the catalog.
        for id in ["vscode", "helix", "zed", "neovim"] {
            assert!(crate::registry::editor(id).is_some(), "'{id}' body has no catalog entry");
        }
    }

    /// EVERY file-emitting editor must actually turn both settings off. This is the
    /// one assertion that ties the whole feature together: whatever the format, the
    /// emitted file has to disable `allTargets` for cargo AND for check.
    #[test]
    fn every_editor_config_disables_both_all_targets() {
        for e in crate::registry::EDITORS.iter().filter(|e| e.path.is_some()) {
            let s = super::editor_config(e.id).unwrap();
            let flat: String = s.chars().filter(|c| !c.is_whitespace()).collect();
            // The two settings, in each format's own punctuation. Lua/TOML use
            // `=`, JSON uses `:`; Helix/VS Code dot the path, Zed/Neovim nest it.
            let cargo_off = flat.contains("cargo.allTargets\":false")   // VS Code
                || flat.contains("cargo.allTargets=false")              // Helix
                || flat.contains("\"cargo\":{\"allTargets\":false}")    // Zed
                || flat.contains("cargo={allTargets=false,}");          // Neovim
            let check_off = flat.contains("check.allTargets\":false")
                || flat.contains("check.allTargets=false")
                || flat.contains("\"check\":{\"allTargets\":false}")
                || flat.contains("check={allTargets=false,}");
            assert!(cargo_off, "editor '{}' does not set cargo.allTargets=false:\n{s}", e.id);
            assert!(check_off, "editor '{}' does not set check.allTargets=false:\n{s}", e.id);
            // The casing traps that fail silently in every one of these editors.
            assert!(!s.contains("all_targets"), "editor '{}' uses snake_case:\n{s}", e.id);
            assert!(!s.contains("allTargets = False"), "editor '{}' uses Python bools:\n{s}", e.id);
        }
    }

    /// Helix reads `[language-server.<name>.config]` and forwards that table as the
    /// server's LSP initialization options. The `rust-analyzer.` prefix must NOT
    /// appear (it is already scoped by the table name), and the server name is
    /// hyphenated here — unlike Neovim's `rust_analyzer`.
    #[test]
    fn helix_languages_toml_uses_the_language_server_config_table() {
        let s = super::editor_config("helix").unwrap();
        let doc: toml_edit::DocumentMut =
            s.parse().expect(".helix/languages.toml must be valid TOML");
        let cfg = &doc["language-server"]["rust-analyzer"]["config"];
        assert_eq!(cfg["cargo"]["allTargets"].as_bool(), Some(false), "got:\n{doc}");
        assert_eq!(cfg["check"]["allTargets"].as_bool(), Some(false), "got:\n{doc}");
        // Helix needs the `[[language]]` entry for the rust-analyzer server to be
        // attached to Rust files at all.
        assert!(s.contains("[[language]]"), "missing the [[language]] entry:\n{s}");
        assert!(s.contains("name = \"rust\""), "language must be rust:\n{s}");
        // Wrong spellings that would silently do nothing.
        assert!(!s.contains("rust_analyzer"), "Helix's server name is hyphenated:\n{s}");
        assert!(
            !s.contains("[language-server.rust-analyzer.settings]"),
            "Helix's key is `config`, not `settings`:\n{s}"
        );
    }

    /// Zed reads `lsp.<server>.initialization_options` — snake_case, and NESTED
    /// objects: its docs explicitly reject VS Code's dotted key style, which would
    /// parse as JSON but never reach rust-analyzer.
    #[test]
    fn zed_settings_use_nested_initialization_options() {
        let s = super::editor_config("zed").unwrap();
        // No serde in this crate's deps, so compare the whitespace-stripped text —
        // enough to pin the whole nesting path, which is the part that is easy to
        // get wrong.
        let flat: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            flat.contains(
                "\"lsp\":{\"rust-analyzer\":{\"initialization_options\":\
                 {\"cargo\":{\"allTargets\":false},\"check\":{\"allTargets\":false}}"
            ),
            "Zed nesting path must be lsp → rust-analyzer → initialization_options:\n{s}"
        );
        assert!(
            !s.contains("initializationOptions"),
            "Zed's key is snake_case `initialization_options`:\n{s}"
        );
        assert!(
            !s.contains("\"rust-analyzer.cargo"),
            "Zed rejects VS Code's dotted style; keys must nest:\n{s}"
        );
        // Strict JSON shape, same guard as the VS Code file.
        assert!(!s.contains(",\n}"), "trailing comma is not valid JSON:\n{s}");
        assert_eq!(
            s.matches('{').count(),
            s.matches('}').count(),
            "unbalanced braces in:\n{s}"
        );
    }

    /// Neovim's file is Lua, keyed by the UNDERSCORED server name with the
    /// hyphenated settings table under it. It is also the only editor whose file
    /// does not load on its own, so the opt-in must be stated in the file itself —
    /// a silent no-op would be worse than emitting nothing.
    #[test]
    fn nvim_lua_targets_rust_analyzer_and_documents_the_exrc_opt_in() {
        let s = super::editor_config("neovim").unwrap();
        assert!(s.contains("vim.lsp.config(\"rust_analyzer\""), "underscored server name:\n{s}");
        assert!(s.contains("[\"rust-analyzer\"]"), "hyphenated settings key:\n{s}");
        // The caveat has to be visible to whoever opens the file.
        assert!(s.contains("exrc"), "must document the exrc opt-in:\n{s}");
        assert!(s.contains(":trust"), "must document the trust step:\n{s}");
        // …and the catalog must carry the same caveat, so the CLI/TUI say it too.
        let e = crate::registry::editor("neovim").unwrap();
        assert!(e.caveat.is_some(), "the neovim catalog entry must carry a caveat");
    }

    /// Every variant that can actually generate a µFerris project must have a
    /// `.dram2_uninit` heap size on file.
    ///
    /// This is the regression guard for the ESP32-C6 link failure: the body used
    /// to carry one hardcoded `66320`, which is the ESP32-C3's `dram2_seg` to the
    /// byte and overflows the C6's smaller region by 784 bytes. The failure mode
    /// was silent at generation time and only surfaced as a linker error in the
    /// user's project, so the check has to live here — making a newly-eligible
    /// variant fail the suite rather than ship a constant sized for a different
    /// chip.
    ///
    /// Variants carrying a `uferris_gap` are excluded: µFerris cannot be selected
    /// for them at all, so there is no body to size. If a gap is ever lifted this
    /// test starts failing, which is the intended prompt to look the figure up.
    #[test]
    fn uferris_capable_chips_all_have_a_dram2_heap_size() {
        for v in crate::registry::VARIANTS
            .iter()
            .filter(|v| v.uferris_chip_feature.is_some() && v.uferris_gap.is_none())
        {
            assert!(
                super::dram2_heap_bytes(v.chip).is_some(),
                "variant '{}' can generate a µFerris project on chip '{}', but \
                 `dram2_heap_bytes` has no entry for it — add one sized from that chip's \
                 `dram2_seg` in esp-hal's `ld/{}/memory.x`",
                v.id,
                v.chip,
                v.chip
            );
        }
    }

    /// The µFerris body must carry the placeholder, not a baked-in number.
    ///
    /// Without this, someone re-inlining a literal (say, when syncing the body
    /// against an upstream example) reintroduces the exact chip-independent
    /// constant this whole mechanism exists to prevent — and it would generate
    /// and build fine on whichever chip the number came from.
    #[test]
    fn the_uferris_body_sizes_its_heap_from_the_placeholder() {
        let body = super::uferris_body("blinky").expect("blinky µFerris body");
        assert!(
            body.contains(super::DRAM2_HEAP_BYTES),
            "the µFerris body must size its heap with {} rather than a literal",
            super::DRAM2_HEAP_BYTES
        );
        assert!(
            !body.contains("66320"),
            "the µFerris body still carries the old chip-independent heap size 66320"
        );
    }

    /// Each recorded heap size must actually fit its chip's `dram2_seg`, with
    /// real slack left over. Capacities are read off esp-hal's linker scripts:
    /// C3 `0x3FCDE710-0x3FCCE400 = 0x10310`, C6 `0x4087E610-0x4086E610 = 0x10000`.
    #[test]
    fn recorded_dram2_heap_sizes_fit_their_region_with_slack() {
        for (chip, capacity) in [("esp32c3", 0x10310_u32), ("esp32c6", 0x10000_u32)] {
            let heap = super::dram2_heap_bytes(chip).expect("chip has a recorded heap size");
            assert!(
                heap < capacity,
                "{chip}: heap {heap:#X} does not fit dram2_seg {capacity:#X}"
            );
            // A zero-slack fit is what broke the C6; require real margin.
            assert!(
                capacity - heap >= 1024,
                "{chip}: heap {heap:#X} leaves only {} bytes of dram2_seg ({capacity:#X}); \
                 keep at least 1 KiB so a later addition to .dram2_uninit does not re-break \
                 the link",
                capacity - heap
            );
        }
    }
}
