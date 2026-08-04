# `xiao-generate`

[![crates.io](https://img.shields.io/crates/v/xiao-generate.svg)](https://crates.io/crates/xiao-generate)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#licence)

Generate ready-to-build Rust projects for the [Seeed XIAO](https://www.seeedstudio.com/xiao-series-page)
line of SoCs & modules.

At present, this tool supports ten XIAO variants across three architectures, including ESP `no_std` and `std`, blocking HALs and embassy HALs. (A
[µFerris](https://crates.io/crates/uferris-bsp) BSP mode for the ESP RISC-V parts is planned but not supported yet.)

Supported variants: the XIAO-ESP32-C3/C5/C6/S3 (and S3 Sense), XIAO-RP2040, XIAO-RP2350, XIAO-nRF52840 (and Sense), and XIAO-SAMD21. Support for additional devices will be added as they become available.


## Quickstart

To generate a template you first need to install `xiao-generate`:

```sh
cargo install xiao-generate
```

Afterward, there are two modes, and they resolve the same configuration through the same validation before either one generates anything.

### Interactive TUI

This is the simplest and recommended path. Every time you want to generate a new project, navigate to the folder path where you want the project created in and enter the following command:

```sh
xiao-generate --name my-project
```

`--name` is **required** to launch the TUI — this is the name you are giving to your project.

### Headless

Passing `--chip` runs the CLI headlessly — no TUI. `--chip` is the only required
flag (there is no default device); every other option falls back to a registry
default, so `xiao-generate --chip <chip>` alone produces a working project.

```sh
xiao-generate --chip xiao-rp2040
```

**Options** (which values are valid for a given board come from the `--list-*`
commands below):

| Flag | What it sets | Values (default in **bold**) |
| --- | --- | --- |
| `--chip <id>` *(required)* | XIAO variant | see `--list-chips` |
| `--stack <id>` | crate stack | per chip (`--list-stacks --chip <id>`); ESP defaults to **esp-hal** |
| `--std` | shorthand for `--stack esp-idf` | — |
| `--espidf-version <v>` | ESP-IDF SDK version (esp-idf stack only) | **v5.5.3**, v5.4.3, v5.3.4, master |
| `--template <id>` / `-t` | project template | **base** + component templates per chip (`--list-templates --chip <id>`) |
| `--framework <id>` | execution model | **none** (bare-metal), embassy (requires an embassy HAL) |
| `--logging <id>` | logging frontend | **none**, defmt; `log` on esp-hal only |
| `--flashing <id>` | flash tool | per chip (`--list-stacks --chip <id>`); default is the board's no-probe cargo route |
| `--editor <id>` | rust-analyzer client config | **none**, vscode, helix, neovim, zed |
| `--add-crate <spec>` | extra dependency (repeatable) | `name` or `name@version` |
| `--name <name>` | crate name | derived from the variant |
| `--out <dir>` / `-o` | output directory | `./<name>` |
| `--yes` / `-y` | install missing backends/tools without asking | — |
| `--no-install` | never auto-install; just print the command | — |

Examples:

```sh
# ESP32-C6, async, with defmt logging
xiao-generate --chip xiao-esp32c6 --framework embassy --logging defmt

# ESP32-C3 on the std / ESP-IDF stack, pinning the ESP-IDF SDK version
xiao-generate --chip xiao-esp32c3 --std --espidf-version v5.4.3

# S3 Sense camera template, into a named directory
xiao-generate --chip xiao-esp32s3-sense --template camera-snapshot --name cam -o ~/dev/cam

# RP2040 with the embassy HAL and the embassy executor
xiao-generate --chip xiao-rp2040 --stack embassy-rp --framework embassy
```

Inspect what a board supports without generating: `--list-chips`,
`--list-stacks [--chip <id>]`, `--list-templates [--chip <id>]`, and
`--check --chip <id>` (reports the host prerequisites for that project).





## Supported devices

| Variant | Arch | Stacks (first = default) | Target |
| --- | --- | --- | --- |
| `xiao-esp32c3` | RISC-V | `esp-hal`, `esp-idf` | `riscv32imc-unknown-none-elf` / `riscv32imc-esp-espidf` |
| `xiao-esp32c6` | RISC-V | `esp-hal`, `esp-idf` | `riscv32imac-unknown-none-elf` / `riscv32imac-esp-espidf` |
| `xiao-esp32c5` | RISC-V | `esp-hal` | `riscv32imac-unknown-none-elf` |
| `xiao-esp32s3` | Xtensa | `esp-hal`, `esp-idf` | `xtensa-esp32s3-none-elf` / `xtensa-esp32s3-espidf` |
| `xiao-esp32s3-sense` | Xtensa | `esp-hal`, `esp-idf` | `xtensa-esp32s3-none-elf` / `xtensa-esp32s3-espidf` |
| `xiao-rp2040` | Cortex-M0+ | `rp2040-hal`, `embassy-rp` | `thumbv6m-none-eabi` |
| `xiao-rp2350` | Cortex-M33 | `rp235x-hal`, `embassy-rp` | `thumbv8m.main-none-eabihf` |
| `xiao-nrf52840` | Cortex-M4F | `nrf-hal`, `embassy-nrf` | `thumbv7em-none-eabihf` |
| `xiao-nrf52840-sense` | Cortex-M4F | `nrf-hal`, `embassy-nrf` | `thumbv7em-none-eabihf` |
| `xiao-samd21` | Cortex-M0+ | `atsamd-hal` | `thumbv6m-none-eabi` |

The µFerris BSP mode (ESP32-C3 / C6 first) is planned but not supported yet.

The HAL axis and the framework axis are orthogonal: an embassy HAL builds fine
with `--framework none`, and `--framework embassy` requires an embassy HAL (a
blocking HAL has no `embassy-time` driver, so it is refused with that reason).

Onboard-component templates beyond the base project are available where there is
a compile-verified body: `camera-snapshot` and `sd-list` on the S3 Sense.
`mic-capture`, `rgb-led` and `imu-read` are declared gaps — run
`--list-templates --chip <chip>` for the current state and the reason.

---

## Prerequisites

For everything except ESP-IDF `std` development, **you don't install anything by
hand.** `xiao-generate` works out exactly what each project needs — the right
Rust target and a flash tool — and every one of those installs with `cargo
install` or `rustup`. It installs its own ESP backends on first use, and after
generating it reports each prerequisite with a live ✔/✗ and **offers to install
the missing ones for you** — all with a simple `[Y/n]` prompt (`--yes` installs
without asking, `--no-install` just prints the commands). `xiao-generate --check
--chip <chip>` reports the same without generating.

The **one exception is ESP-IDF (`std`) development**. That toolchain — the C
ESP-IDF SDK and its per-OS system packages — is set up outside this tool and
differs by operating system, so it's documented upstream:
[esp-idf-template → Prerequisites](https://github.com/esp-rs/esp-idf-template#prerequisites)
and the [Rust on ESP Book](https://esp-rs.github.io/book/). (The first `--std`
build then compiles the whole ESP-IDF SDK, so expect it to be slow.)

**Enter the bootloader before flashing** — ESP: just plug in USB-C. RP2040 /
RP2350: hold **B** while plugging in. nRF52840: double-tap **RESET**. SAMD21:
short the two **RST** pads twice.

---

## After generating

The summary ends with the prerequisite report and the next steps:

```
$ xiao-generate --chip xiao-nrf52840 --name blinky

✔ Generated XIAO nRF52840 project
  crate:        blinky
  stack:        nrf-hal (nrf-hal)
  target:       thumbv7em-none-eabihf
  framework:    none
  µFerris BSP:  no
  editor:       VS Code

  equivalent command:
    xiao-generate --chip xiao-nrf52840

  prerequisites to build:
    ✔ Rust target thumbv7em-none-eabihf

  prerequisites to flash:
    ✗ uf2deploy
        install: cargo install uf2deploy
        · converts the ELF to UF2 and copies it to the mounted UF2 drive (nRF: double-tap RESET) — no probe needed

Next steps:
  cd blinky
  cargo build
```

Every `✔` and `✗` there is a live probe of your machine, not a static list — the
report above is from a host that has the Rust target installed but not
`uf2deploy`, and only the missing one is given an install command.

## Editor support

Every generated project ships a project-level `rust-analyzer.toml` that
suppresses the spurious `E0463: can't find crate for test` you would otherwise
get on a bare-metal target — this is always written, whatever `--editor` you
pick.

Editor-specific client config is opt-in. The default is `--editor none`, which
writes no editor files; pass `--editor vscode`, `helix`, `neovim`, or `zed` to
also drop that editor's settings into the project (the client config is the part
actually verified to clear the diagnostic in-editor, as the `.toml` alone does
not in current rust-analyzer).

## Licence

MIT OR Apache-2.0
