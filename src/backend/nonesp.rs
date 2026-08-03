//! Non-ESP (Cortex-M) backend (M4) — a **direct file scaffold**.
//!
//! Unlike the two ESP stacks there is no external generator here. A bare-metal
//! Cortex-M project has no irreducible bootstrap: it is `Cargo.toml` +
//! `.cargo/config.toml` + (`memory.x` + `build.rs`) + `src/main.rs`, so we write
//! that set ourselves from [`registry::Scaffold`] data (D1's "shell out" rule
//! exists because backends have no library API — it does not apply when there is
//! no backend to shell out to). Benefits: the output is XIAO-accurate (raw HALs
//! + our registry LED pins, never a wrong-board BSP), it is uniform across all
//! four chips, and non-ESP users need nothing on PATH beyond `cargo` and the
//! target.
//!
//! This module contains **no chip logic** — every version, feature, rustflag and
//! linker script comes from the registry (R4), and every combination it can emit
//! was compile-verified (see the compile matrix in `tests/generate.rs` and
//! research/hal-framework-matrix.md).

use anyhow::{Context, Result};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::model::Config;
use crate::registry::{self, Scaffold};
use crate::templates;

pub fn scaffold(cfg: &Config) -> Result<()> {
    let sc = cfg.stack.scaffold.with_context(|| {
        format!(
            "internal error: stack '{}' uses the non-ESP scaffold backend but declares no \
             scaffold data",
            cfg.stack.id
        )
    })?;

    let out = &cfg.out_dir;
    std::fs::create_dir_all(out.join("src"))
        .with_context(|| format!("creating {}", out.join("src").display()))?;
    std::fs::create_dir_all(out.join(".cargo"))
        .with_context(|| format!("creating {}", out.join(".cargo").display()))?;

    write(out.join("Cargo.toml"), &cargo_toml(cfg, sc)?)?;
    write(out.join(".cargo/config.toml"), &cargo_config(cfg, sc))?;

    if let Some(id) = sc.memory_x {
        let body = templates::memory_x(id)
            .with_context(|| format!("internal error: no memory.x body for id '{id}'"))?;
        write(out.join("memory.x"), body)?;
        write(out.join("build.rs"), &build_rs(cfg, sc))?;
    }

    // The base `src/main.rs`. Component add-ons (none are ungated on non-ESP in
    // M4) are layered on afterwards by `patch::install_main`. When `--logging
    // defmt`, `nonesp_render` also threads in the defmt global logger + one
    // `defmt::info!` (the deps and the `-Tdefmt.x` link arg are added above/below).
    let body = templates::nonesp_base_body(cfg.variant.chip, cfg.stack.id, cfg.framework.id)
        .with_context(|| {
            format!(
                "internal error: no verified base body for {} / {} / framework {}",
                cfg.variant.chip, cfg.stack.id, cfg.framework.id
            )
        })?;
    write(
        out.join("src/main.rs"),
        &templates::nonesp_render(body, cfg.variant.led, defmt_on(cfg)),
    )?;

    Ok(())
}

/// Whether this project wires in defmt logging (`--logging defmt`). On the non-ESP
/// scaffold `defmt` is the only non-`none` logging choice.
fn defmt_on(cfg: &Config) -> bool {
    cfg.logging.id == "defmt"
}

fn write(path: std::path::PathBuf, contents: &str) -> Result<()> {
    std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

/// Whether the embassy executor/time layer is part of this project.
fn embassy_on(cfg: &Config) -> bool {
    cfg.framework.id == "embassy"
}

/// Build the project manifest with `toml_edit` — the same machinery the patch
/// layer then uses to add template/user crates on top.
fn cargo_toml(cfg: &Config, sc: &Scaffold) -> Result<String> {
    let mut doc = DocumentMut::new();

    let mut package = Table::new();
    package["name"] = value(cfg.project_name.as_str());
    package["version"] = value("0.1.0");
    package["edition"] = value("2021");
    doc["package"] = Item::Table(package);

    let mut deps = Table::new();
    // The HAL crate: a bare version string when it needs no features, an inline
    // table otherwise. `embassy_hal_features` (the embassy-time driver) is added
    // ONLY under `--framework embassy`: an embassy HAL with the time-driver
    // feature but no `embassy-time` user drags the whole time stack in for
    // nothing, and a blocking HAL cannot host it at all.
    let mut feats: Vec<&str> = sc.hal_features.to_vec();
    if embassy_on(cfg) {
        feats.extend_from_slice(sc.embassy_hal_features);
    }
    if feats.is_empty() {
        deps[sc.hal_crate] = value(sc.hal_version);
    } else {
        let mut t = toml_edit::InlineTable::new();
        t.insert("version", sc.hal_version.into());
        let mut arr = Array::new();
        for f in feats {
            arr.push(f);
        }
        t.insert("features", toml_edit::Value::Array(arr));
        deps[sc.hal_crate] = value(t);
    }

    for d in sc.base_deps {
        crate::patch::add_dep(&mut deps, d.name, d.spec)?;
    }
    if embassy_on(cfg) {
        for d in registry::EMBASSY_FRAMEWORK_DEPS {
            crate::patch::add_dep(&mut deps, d.name, d.spec)?;
        }
    }
    // `--logging defmt`: the defmt macros + the RTT global logger. Added as plain
    // deps (no HAL `defmt` feature is enabled), so they compose with any HAL. The
    // `-Tdefmt.x` link arg goes into the rustflags / build.rs below, and the base
    // body is rendered with the logger + a `defmt::info!` by `nonesp_render`.
    if defmt_on(cfg) {
        crate::patch::add_dep(&mut deps, "defmt", &format!(r#""{}""#, registry::DEFMT_VERSION))?;
        crate::patch::add_dep(
            &mut deps,
            "defmt-rtt",
            &format!(r#""{}""#, registry::DEFMT_RTT_VERSION),
        )?;
    }
    doc["dependencies"] = Item::Table(deps);

    // Bare-metal debug builds are large; -O1 keeps them comfortably inside the
    // smallest flash we target (SAMD21's 256 KiB) without losing debuginfo.
    let mut dev = Table::new();
    dev["opt-level"] = value(1);
    let mut release = Table::new();
    release["debug"] = value(2);
    release["opt-level"] = value("s");
    let mut profile = Table::new();
    // Implicit: render as `[profile.dev]` / `[profile.release]` headers rather
    // than a `profile = { … }` inline value (which cargo ignores entirely).
    profile.set_implicit(true);
    profile["dev"] = Item::Table(dev);
    profile["release"] = Item::Table(release);
    doc["profile"] = Item::Table(profile);

    Ok(doc.to_string())
}

/// NOTE on the `[target.<triple>]` header: it must stay **unquoted**, even for
/// `thumbv8m.main-none-eabihf`. Cargo looks config keys up by splitting on `.`,
/// so the quoted form `[target."thumbv8m.main-none-eabihf"]` — which is the
/// TOML-correct way to write one key containing a dot — is silently never
/// matched, the rustflags are dropped, and the RP2350 links without `link.x`
/// into an ELF with no vector table and a 0x0 entry point that still "builds".
/// The upstream rp235x templates all use the unquoted (nested-table) spelling
/// for exactly this reason. Covered by `rp2350_config_target_header_is_unquoted`
/// in tests/generate.rs.
fn cargo_config(cfg: &Config, sc: &Scaffold) -> String {
    let mut s = format!("[build]\ntarget = \"{}\"\n", cfg.stack.target);
    // `--logging defmt` needs `-Tdefmt.x`, which must follow `link.x`. A stack
    // that carries its link scripts in rustflags (every stack but embassy-rp on
    // RP2040, which routes them through build.rs) gets defmt.x appended here; the
    // one build.rs stack gets it there instead (see `build_rs`).
    let mut rustflags: Vec<&str> = sc.rustflags.to_vec();
    if defmt_on(cfg) && !sc.rustflags.is_empty() {
        rustflags.push("-C");
        rustflags.push("link-arg=-Tdefmt.x");
    }
    if !rustflags.is_empty() {
        s.push_str(&format!("\n[target.{}]\nrustflags = [\n", cfg.stack.target));
        for chunk in rustflags.chunks(2) {
            let rendered = chunk
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", ");
            s.push_str(&format!("    {rendered},\n"));
        }
        s.push_str("]\n");
    }
    s
}

fn build_rs(cfg: &Config, sc: &Scaffold) -> String {
    // See `cargo_config`: only a stack with EMPTY rustflags carries its link args
    // through build.rs, so that is where its defmt.x goes (appended last, after
    // link.x / link-rp.x).
    let mut link_args: Vec<&str> = sc.link_args.to_vec();
    if defmt_on(cfg) && sc.rustflags.is_empty() {
        link_args.push("-Tdefmt.x");
    }
    let extra = link_args
        .iter()
        .map(|a| format!("    println!(\"cargo:rustc-link-arg-bins={a}\");\n"))
        .collect::<String>();
    templates::nonesp_build_rs().replace("{{LINK_ARGS}}", &extra)
}
