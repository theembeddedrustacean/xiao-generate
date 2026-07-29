//! Post-generation transforms, all table-driven from the registry (R4): add the
//! template's recommended/driver crates to `Cargo.toml` (via `toml_edit`, so
//! formatting is preserved), then install the template `main.rs` with XIAO pin
//! wiring. esp-generate already emitted the correct target/toolchain/backend
//! stack, so we never touch `.cargo/config.toml` or `rust-toolchain.toml`.

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::model::Config;
use crate::registry::{self, Backend, ESP_IDF_HAL_VERSION};
use crate::templates;

pub mod uferris;

pub fn apply(cfg: &Config) -> Result<()> {
    add_dependencies(cfg)?;
    install_main(cfg)?;
    Ok(())
}

fn add_dependencies(cfg: &Config) -> Result<()> {
    let cargo_path = cfg.out_dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&cargo_path)
        .with_context(|| format!("reading {}", cargo_path.display()))?;
    let mut doc: DocumentMut = text
        .parse()
        .with_context(|| format!("parsing generated {}", cargo_path.display()))?;

    let deps = doc
        .as_table_mut()
        .entry("dependencies")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .context("[dependencies] is not a table in the generated Cargo.toml")?;

    if cfg.uferris.is_some() {
        uferris::add_bsp_dep(deps, cfg)?;
    } else if matches!(cfg.stack.backend, Backend::CargoGenerate) {
        // ESP `std` base: add the XIAO's esp-idf-hal on top of the template's
        // esp-idf-svc (R9 — the std path adds the recommended crate set, just
        // like no_std, against the esp-idf ecosystem). Pinned in the registry.
        add_dep(deps, "esp-idf-hal", &format!(r#""{ESP_IDF_HAL_VERSION}""#))?;
    } else if let Some(t) = registry::template(&cfg.template_id) {
        for d in t.extra_deps {
            add_dep(deps, d.name, d.spec)?;
        }
    }

    // Per-variant default driver crates (the "some XIAOs ship crates by default"
    // hook — M2 redesign v2). Data-only from the registry; `&[]` for every current
    // variant, so this is a no-op today but wired for later additions. Applied to
    // ALL stacks, before the user's own extra crates.
    for d in cfg.variant.default_crates {
        add_dep(deps, d.name, d.spec)?;
    }

    // User-requested extra crates (`--add-crate` / TUI free-form). Applies to
    // ALL stacks, layered on last so a user pin wins over nothing but does not
    // silently shadow a registry-required driver crate unless they name it
    // (M2 redesign, point 5). Specs were validated at resolve time.
    for spec in &cfg.extra_crates {
        let (name, value) = crate::model::crate_dependency(spec);
        add_dep(deps, &name, &value)?;
    }

    std::fs::write(&cargo_path, doc.to_string())
        .with_context(|| format!("writing {}", cargo_path.display()))?;
    Ok(())
}

/// Insert (or overwrite) a dependency. `spec` is a TOML value literal.
pub(crate) fn add_dep(deps: &mut Table, name: &str, spec: &str) -> Result<()> {
    let value: Value = spec
        .parse()
        .with_context(|| format!("parsing dependency spec for '{name}': {spec}"))?;
    deps.insert(name, Item::Value(value));
    Ok(())
}

fn install_main(cfg: &Config) -> Result<()> {
    // KEY DECISION ("esp-generate owns the base"): for the esp-hal (EspGenerate)
    // stack + a BASE project (no component add-on, µFerris off), do NOT inject our
    // own main — esp-generate already emitted a valid main for whatever
    // flashing/logging/framework combo was requested (passed as `-o` flags), so
    // every base combo is upstream-verified by construction. We still applied
    // `extra_crates` in `add_dependencies`; nothing else is layered on top.
    // Component add-ons and µFerris still inject their bodies below; the std
    // (CargoGenerate) path is unchanged.
    // The same rule holds for the non-ESP direct scaffold (M4): it writes the
    // base `src/main.rs` for the resolved (chip, stack, framework) cell itself
    // from a compile-verified body, so there is nothing to layer on for a base
    // project. Component add-ons fall through and overwrite it below.
    if matches!(cfg.stack.backend, Backend::EspGenerate | Backend::NonEspScaffold)
        && cfg.uferris.is_none()
        && registry::is_base_template(&cfg.template_id)
    {
        return Ok(());
    }

    let is_std = matches!(cfg.stack.backend, Backend::CargoGenerate);
    let is_nonesp = matches!(cfg.stack.backend, Backend::NonEspScaffold);

    let body = if cfg.uferris.is_some() {
        templates::uferris_body(&cfg.template_id)
    } else if is_std {
        // ESP `std` base: blink (LED variants) or log-hello (no LED), selected
        // by hardware — the synthetic `base` id has no TEMPLATES entry.
        Some(templates::std_body(cfg.variant.led.is_some()))
    } else {
        templates::body(&cfg.template_id)
    }
    .with_context(|| format!("no embedded template body for '{}'", cfg.template_id))?;

    // A template that needs an LED must be paired with a variant that has one.
    // The registry keeps these consistent; this guards against a bad edit.
    // (Std `base` handles the LED/no-LED split itself above, so skip it there.)
    if !is_std {
        if let Some(t) = registry::template(&cfg.template_id) {
            if t.needs_led && cfg.variant.led.is_none() {
                anyhow::bail!(
                    "template '{}' needs an onboard LED but variant '{}' has none",
                    t.id,
                    cfg.variant.id
                );
            }
        }
    }

    let mut rendered = templates::render(body, cfg.variant.led);

    // The µFerris body sizes its `.dram2_uninit` heap from a per-chip figure —
    // `dram2_seg` is a different size on every SoC, so there is no one literal
    // that links everywhere. Resolve it here rather than defaulting: a missing
    // entry is a registry/template mismatch we want to fail on now, with the
    // fix spelled out, instead of emitting a constant that overflows the region
    // at link time in the user's project.
    if rendered.contains(templates::DRAM2_HEAP_BYTES) {
        let bytes = templates::dram2_heap_bytes(cfg.variant.chip).with_context(|| {
            format!(
                "no `.dram2_uninit` heap size recorded for chip '{}' (variant '{}'), which the \
                 µFerris template needs. Add it to `templates::dram2_heap_bytes` — the value is \
                 the size of that chip's `dram2_seg` in esp-hal's `ld/{}/memory.x`, less ~1 KiB \
                 of slack.",
                cfg.variant.chip, cfg.variant.id, cfg.variant.chip
            )
        })?;
        rendered = rendered.replace(templates::DRAM2_HEAP_BYTES, &bytes.to_string());
    }

    // esp-generate/uferris scaffold the app entry at src/bin/main.rs; the
    // esp-idf-template std layout and the non-ESP scaffold (single bin) both use
    // src/main.rs.
    let main_path = if is_std || is_nonesp {
        cfg.out_dir.join("src").join("main.rs")
    } else {
        cfg.out_dir.join("src").join("bin").join("main.rs")
    };
    std::fs::write(&main_path, rendered)
        .with_context(|| format!("writing template to {}", main_path.display()))?;
    Ok(())
}
