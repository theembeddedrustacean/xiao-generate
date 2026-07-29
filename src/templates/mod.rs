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
