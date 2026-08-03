# xiao-generate

Generate ready-to-build Rust projects for [Seeed XIAO](https://www.seeedstudio.com/xiao-series-page)
modules.

One command produces a project that compiles: the right target triple, the right
HAL at a version that is actually verified to work with it, a linker script and
`build.rs` where the chip needs them, an onboard-LED blink (or a serial hello on
the boards without a user LED), and rust-analyzer configuration so your editor
does not light up with a spurious `can't find crate for test`.

Nine XIAO variants across three architectures, each with its real stacks — ESP
`no_std` and `std`, blocking HALs and embassy HALs — and an optional
[µFerris](https://crates.io/crates/uferris-bsp) BSP mode on the ESP RISC-V parts.

Everything the tool knows about a board lives in one declarative registry, and
anything that cannot be generated correctly is a **declared gap** with a reason
rather than a project that does not build.

---

## Install

```sh
cargo install xiao-generate      # from crates.io
# or, from a local clone:
cargo install --path .
```

That is the only install command. The two ESP stacks shell out to a pinned
upstream generator (`esp-generate` for `no_std`, `cargo-generate` for `std`), and
`xiao-generate` installs the one it needs **on demand, the first time you
generate an ESP project** — it checks on every run, and if the backend is
missing it asks before installing:

```
esp-generate is required for this stack but isn't installed.
  install it now with `cargo install esp-generate --version 1.3.0 --locked`? [Y/n]
```

- Answer `y` (or just Enter) and it installs the pinned version and continues.
- `--yes` / `-y` installs without asking (for scripts and CI).
- `--no-install` never installs — it prints the `cargo install` command and stops,
  so you can run it yourself.

Non-ESP boards need no backend at all (the Cortex-M project is written directly),
so none of this applies to them. These backends are `xiao-generate`'s own
dependencies; the prerequisites for the *generated project* are a separate list —
see [Prerequisites](#prerequisites).

---

## Usage

There are two modes, and they resolve the same configuration through the same
validation before either one generates anything.

### Headless

Forced with `--headless`, and implied whenever `--chip` is given.

```sh
xiao-generate --chip xiao-rp2040
```

`--chip` is **required** — there is no default device. Every other axis falls
back to a registry default (the variant's default stack, its base template,
espflash, no logging or framework, a name derived from the variant), so
`--chip <chip>` alone is enough for a working project.

```sh
# ESP32-C6, async, with defmt logging
xiao-generate --chip xiao-esp32c6 --framework embassy --logging defmt

# ESP32-C3 on the std / ESP-IDF stack (--std is shorthand for --stack esp-idf)
xiao-generate --chip xiao-esp32c3 --std

# S3 Sense camera template, into a named directory
xiao-generate --chip xiao-esp32s3-sense --template camera-snapshot --name cam -o ~/dev/cam

# RP2040 with the embassy HAL and the embassy executor
xiao-generate --chip xiao-rp2040 --stack embassy-rp --framework embassy

# µFerris BSP mode
xiao-generate --chip xiao-esp32c3 --uferris
```

Useful listings: `--list-chips`, `--list-stacks [--chip X]`,
`--list-templates [--chip X]`.

### Interactive TUI

The default when stdout is a terminal and no `--chip`/`--headless`/`--list-*` was
given, or forced with `--tui`.

```sh
xiao-generate --name my-project
```

`--name` is **required** to launch the TUI — the crate name is a CLI concern, not
a TUI field. Pick everything on one screen; unavailable combinations are greyed
out with the reason. The summary screen shows the equivalent headless command,
so anything you configure interactively can be scripted afterwards.

---

## Supported devices

| Variant | Arch | Stacks (first = default) | Target | µFerris |
| --- | --- | --- | --- | --- |
| `xiao-esp32c3` | RISC-V | `esp-hal`, `esp-idf` | `riscv32imc-unknown-none-elf` / `riscv32imc-esp-espidf` | yes |
| `xiao-esp32c6` | RISC-V | `esp-hal`, `esp-idf` | `riscv32imac-unknown-none-elf` / `riscv32imac-esp-espidf` | yes |
| `xiao-esp32s3` | Xtensa | `esp-hal`, `esp-idf` | `xtensa-esp32s3-none-elf` / `xtensa-esp32s3-espidf` | gap |
| `xiao-esp32s3-sense` | Xtensa | `esp-hal`, `esp-idf` | `xtensa-esp32s3-none-elf` / `xtensa-esp32s3-espidf` | gap |
| `xiao-rp2040` | Cortex-M0+ | `rp2040-hal`, `embassy-rp` | `thumbv6m-none-eabi` | — |
| `xiao-rp2350` | Cortex-M33 | `rp235x-hal`, `embassy-rp` | `thumbv8m.main-none-eabihf` | — |
| `xiao-nrf52840` | Cortex-M4F | `nrf-hal`, `embassy-nrf` | `thumbv7em-none-eabihf` | — |
| `xiao-nrf52840-sense` | Cortex-M4F | `nrf-hal`, `embassy-nrf` | `thumbv7em-none-eabihf` | — |
| `xiao-samd21` | Cortex-M0+ | `atsamd-hal` | `thumbv6m-none-eabi` | — |

The HAL axis and the framework axis are orthogonal: an embassy HAL builds fine
with `--framework none`, and `--framework embassy` requires an embassy HAL (a
blocking HAL has no `embassy-time` driver, so it is refused with that reason).

Onboard-component templates beyond the base project are available where there is
a compile-verified body: `camera-snapshot` and `sd-list` on the S3 Sense.
`mic-capture`, `rgb-led` and `imu-read` are declared gaps — run
`--list-templates --chip <chip>` for the current state and the reason.

---

## Prerequisites

Each board needs a build target (or toolchain) and one USB flash tool — the
default flash routes need no debug probe. You don't have to read this: after
generating, the tool prints exactly what your project needs and whether it is
installed, and `xiao-generate --check --chip <chip>` prints the same without
generating anything.

| Device | Build | Flash | To use `--flashing probe-rs` |
| --- | --- | --- | --- |
| ESP32-C3 | `rustup target add riscv32imc-unknown-none-elf` | `cargo install espflash --locked` | `cargo install probe-rs-tools --locked` — built-in USB-JTAG, no probe |
| ESP32-C5 | `rustup target add riscv32imac-unknown-none-elf` | `cargo install espflash --locked` (≥ 4.5) | `cargo install probe-rs-tools --locked` (≥ 0.31) — built-in USB-JTAG, no probe |
| ESP32-C6 | `rustup target add riscv32imac-unknown-none-elf` | `cargo install espflash --locked` | `cargo install probe-rs-tools --locked` — built-in USB-JTAG, no probe |
| ESP32-S3 (+Sense) | `cargo install espup --locked && espup install` | `cargo install espflash --locked` | `cargo install probe-rs-tools --locked` — built-in USB-JTAG, no probe |
| RP2040 | `rustup target add thumbv6m-none-eabi` | `brew install picotool` | `cargo install probe-rs-tools --locked` **+ an external SWD probe** |
| RP2350 | `rustup target add thumbv8m.main-none-eabihf` | `brew install picotool` | `cargo install probe-rs-tools --locked` **+ an external SWD probe** |
| nRF52840 (+Sense) | `rustup target add thumbv7em-none-eabihf` | `cargo install cargo-binutils && rustup component add llvm-tools`, then put `uf2conv.py` + `uf2families.json` ([microsoft/uf2](https://github.com/microsoft/uf2) `utils/`) on `PATH` | `cargo install probe-rs-tools --locked` **+ an external SWD probe** |
| SAMD21 | `rustup target add thumbv6m-none-eabi` | `cargo install hf2-cli` | `cargo install probe-rs-tools --locked` **+ an external SWD probe** |

**Enter the bootloader before flashing** — ESP: just plug in USB-C. RP2040 /
RP2350: hold **B** while plugging in. nRF52840: double-tap **RESET**. SAMD21:
short the two **RST** pads twice.

**`--std` (ESP-IDF) builds** (C3 / C6 / S3 only) additionally need
`cargo install ldproxy`, `python3` + `git` on `PATH`, and — on C3 / C6 —
`rustup toolchain install nightly --component rust-src` (the S3 reuses its `esp`
toolchain). The first build compiles the whole ESP-IDF SDK, so it is slow.

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
    ✔ rust-objcopy (cargo-binutils)
    ✗ uf2conv.py (+ uf2families.json)
        install: curl -LO .../uf2conv.py && curl -LO .../uf2families.json && chmod +x uf2conv.py   # then put BOTH on PATH
        · uf2conv.py reads uf2families.json from beside itself — both files must be installed

Next steps:
  cd blinky
  cargo build
```

Every `✔` and `✗` there is a live probe of your machine, not a static list — the
report above is from a host that has `rust-objcopy` but not `uf2conv.py`, and
only the missing one is given an install command.

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
