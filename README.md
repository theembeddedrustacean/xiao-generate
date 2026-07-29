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
cargo install --path .
```

`xiao-generate` shells out to a pinned upstream generator for the two ESP stacks,
so install the one for the stack you plan to use:

| Stack you want | Also install |
| --- | --- |
| ESP `no_std` (`esp-hal`) | `cargo install esp-generate --version 1.3.0 --locked` |
| ESP `std` (`esp-idf`) | `cargo install cargo-generate` (plus `git` on `PATH`) |
| Everything non-ESP | nothing — the Cortex-M project is written directly |

These are `xiao-generate`'s own dependencies. The prerequisites for the
*generated project* are a separate list — see [Prerequisites](#prerequisites).

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

These are the host tools a **generated project** needs. Only the ones your
configuration actually uses matter — an RP2040 project has nothing to do with
espup, and an ESP-IDF project has nothing to do with picotool.

**You do not have to work this out from the table.** After generating, the tool
prints exactly this list for the project it just made, probes each entry on your
machine, marks it installed or missing, and shows the install command for the
missing ones. `--check` prints the same report without generating anything:

```sh
xiao-generate --check --chip xiao-nrf52840
```

### To build

| Applies to | Prerequisite | Install |
| --- | --- | --- |
| ESP32-C3, `esp-hal` | Rust target `riscv32imc-unknown-none-elf` | `rustup target add riscv32imc-unknown-none-elf` |
| ESP32-C6, `esp-hal` | Rust target `riscv32imac-unknown-none-elf` | `rustup target add riscv32imac-unknown-none-elf` |
| ESP32-S3(-Sense), **either stack** | Xtensa `esp` toolchain | `cargo install espup --locked && espup install` |
| ESP32-C3/C6, `esp-idf` | nightly toolchain + `rust-src` | `rustup toolchain install nightly --component rust-src` |
| **any** `esp-idf` project | `ldproxy` | `cargo install ldproxy` |
| **any** `esp-idf` project | `python3`, `git` on `PATH` | (system) |
| RP2040, SAMD21 | Rust target `thumbv6m-none-eabi` | `rustup target add thumbv6m-none-eabi` |
| RP2350 | Rust target `thumbv8m.main-none-eabihf` | `rustup target add thumbv8m.main-none-eabihf` |
| nRF52840(-Sense) | Rust target `thumbv7em-none-eabihf` | `rustup target add thumbv7em-none-eabihf` |

Three details that catch people out:

- **Xtensa is not a rustup target.** `rustup target add xtensa-esp32s3-none-elf`
  does not work and never will — the Xtensa backend is not in upstream LLVM. The
  `esp` toolchain that `espup install` links is the only way to build for the
  ESP32-S3, on `no_std` and `std` alike.
- **The RISC-V ESP `std` targets are not rustup targets either.** They are
  `build-std` targets, compiled from source, which is why they need *nightly*
  with `rust-src` — and notably **not** espup, whose RISC-V support is on the
  stable channel. Both are pinned by the generated project's
  `rust-toolchain.toml`, so rustup will also install them on the first build.
- **`cmake` and `ninja` are not prerequisites.** `esp-idf-sys` installs them
  itself through `idf_tools.py`, along with the ESP-IDF SDK and its GCC/clang
  toolchains. Only `python3` and `git` have to be there first. Expect the first
  ESP-IDF build to take a long time — it downloads and builds the whole SDK.

µFerris mode adds nothing to this list: `uferris-bsp` is an ordinary cargo
dependency on the `esp-hal` stack.

### To flash

The non-ESP routes below are all **USB / UF2** — no SWD debug probe required.

| Board | Prerequisite | Install |
| --- | --- | --- |
| ESP, `esp-hal` (default) | `espflash` | `cargo install espflash --locked` |
| ESP, `esp-hal` with `--flashing probe-rs` | `probe-rs` (needs an SWD probe) | `cargo install probe-rs-tools --locked` |
| ESP, `esp-idf` | `espflash` (the template's cargo runner) | `cargo install espflash --locked` |
| RP2040, RP2350 | `picotool` | `brew install picotool` |
| nRF52840(-Sense) | `rust-objcopy` | `cargo install cargo-binutils && rustup component add llvm-tools` |
| nRF52840(-Sense) | `uf2conv.py` **and** `uf2families.json` | from [github.com/microsoft/uf2](https://github.com/microsoft/uf2) (`utils/`) — put both on `PATH`; uf2conv.py reads the JSON from beside itself |
| SAMD21 | `hf2-cli` | `cargo install hf2-cli` |

Getting each board into its bootloader:

| Board | How |
| --- | --- |
| ESP | just plug in USB-C — espflash resets the chip itself |
| RP2040 / RP2350 | hold **B** while plugging in (or hold **B**, tap **RESET**, release **B**); a `RPI-RP2` / `RP2350` volume appears |
| nRF52840 | double-tap **RESET** within ~500 ms; a `XIAO-BOOT` / `XIAO-SENSE` volume appears |
| SAMD21 | short the two **RST** pads twice quickly (there is no reset button); an `Arduino` volume appears |

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

Generated projects carry rust-analyzer configuration that suppresses the
spurious `E0463: can't find crate for test` you would otherwise get on a
bare-metal target. VS Code by default; pick another with `--editor helix`,
`--editor neovim`, `--editor zed`, or `--editor none` to write no editor files.

## Licence

MIT OR Apache-2.0
