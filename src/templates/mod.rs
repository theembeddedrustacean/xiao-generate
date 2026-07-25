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
        "camera-snapshot" => include_str!("files/camera_snapshot.rs"),
        "sd-list" => include_str!("files/sd_list.rs"),
        _ => return None,
    })
}

/// Body for a µFerris template id, or `None` if unknown.
pub fn uferris_body(id: &str) -> Option<&'static str> {
    Some(match id {
        "blinky" => include_str!("files/uferris_blinky.rs"),
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
