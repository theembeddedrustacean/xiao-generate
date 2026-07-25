//! µFerris mode: add the `uferris-bsp` dependency with the variant's chip
//! feature + `embassy` (+ `power-board` when requested). The scaffold and
//! `main.rs` are otherwise the embassy scaffold + the uferris example blinky
//! (see `templates::uferris_body`). Feature names come from uferris-bsp's
//! Cargo.toml (`power-board` is hyphenated; the README's `powerboard` is stale).

use anyhow::{Context, Result};
use toml_edit::{Array, InlineTable, Item, Table, Value};

use crate::model::Config;
use crate::registry::UFERRIS_BSP_VERSION;

pub fn add_bsp_dep(deps: &mut Table, cfg: &Config) -> Result<()> {
    let chip_feature = cfg
        .variant
        .uferris_chip_feature
        .context("µFerris requested for a variant with no uferris chip feature")?;

    let mut features = Array::new();
    features.push(chip_feature);
    features.push("embassy");
    if cfg.uferris.as_ref().map(|u| u.power_board).unwrap_or(false) {
        features.push("power-board");
    }

    let mut tbl = InlineTable::new();
    tbl.insert("version", Value::from(UFERRIS_BSP_VERSION));
    tbl.insert("features", Value::Array(features));

    deps.insert("uferris-bsp", Item::Value(Value::InlineTable(tbl)));
    Ok(())
}
