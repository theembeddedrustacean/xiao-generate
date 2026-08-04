#!/bin/bash
# ---------------------------------------------------------------------------
# xiao-generate build + flash test matrix.
#
#   ./scripts/matrix.sh build [--chip <chip>] [--include-std] [--list]
#   ./scripts/matrix.sh flash --chip <chip> [--no-option-sweep]
#
# `build` needs NO hardware: it generates every supported combination into a
# work dir and runs `cargo build --release` on each.
# `flash` needs ONE physically-connected board and walks the combinations for
# that chip, pausing before every flash so a human can put the board into its
# bootloader.
#
# THE MATRIX IS NOT HARDCODED. Every axis is harvested at run time from the
# tool's own `--list-chips` / `--list-stacks` / `--list-templates` output. Each
# stack's flashing/logging/framework axes come from its OWN `--list-stacks`
# lines (the per-stack SUPPORTED subset, e.g. rp2040 flashing = elf2uf2-rs,
# uf2deploy, probe-rs) -- never the universal catalog -- so an unsupported tool for a stack
# is never enumerated, only the real gaps are. Add a variant, a stack, a
# template or a support entry to src/registry.rs and this script picks it up
# with no edit.
#
# Two axes are deliberately NOT swept, because sweeping them would rebuild
# byte-identical projects rather than test anything new:
#   * FLASHING is a code-affecting axis ONLY on esp-hal (there it maps to
#     different esp-generate `-o` flags). On every other stack the flashing
#     choice changes only the summary/prereqs, so we build the stack's DEFAULT
#     flashing tool once. LOGGING (defmt vs none) changes generation everywhere
#     and is always swept.
#   * A base template (base-blink/base-hello) on two variants that share the
#     same silicon (e.g. xiao-esp32s3 & xiao-esp32s3-sense) is byte-identical --
#     only the onboard components differ. The base is enumerated ONCE per
#     silicon under a representative variant, tagged `<chip>(+sense)`; the
#     sense-only component templates still enumerate under their own variant.
#
# Status vocabulary (one per row):
#   PASS    generated AND `cargo build --release` succeeded
#   FAIL    generation or build failed unexpectedly  (log path is printed)
#   GATED   xiao-generate refused the combination at config time with exit 2.
#           That is the tool working as designed (a registry gap), not a bug.
#   SKIP    could not be attempted on THIS machine (missing toolchain/tool).
#           A SKIP is never a pass — the reason and the fix are printed.
#   BLOCKED (flash mode only) the route exists but running it would be unsafe
#           or cannot be verified; the reason is printed.
#
# Written for macOS' stock bash 3.2 -- no associative arrays, no `readarray`.
# ---------------------------------------------------------------------------
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
BIN="$REPO_ROOT/target/release/xiao-generate"

MODE=""
ONLY_CHIP=""
INCLUDE_STD=0
LIST_ONLY=0
OPTION_SWEEP=1
WORK=""
KEEP_PROJECTS=0

# ---------------------------------------------------------------------------
# pretty
# ---------------------------------------------------------------------------
if [ -t 1 ]; then
  C_RED=$'\033[31m'; C_GRN=$'\033[32m'; C_YEL=$'\033[33m'
  C_BLU=$'\033[34m'; C_DIM=$'\033[2m'; C_BLD=$'\033[1m'; C_OFF=$'\033[0m'
else
  C_RED=""; C_GRN=""; C_YEL=""; C_BLU=""; C_DIM=""; C_BLD=""; C_OFF=""
fi

say()  { printf '%s\n' "$*"; }
hdr()  { printf '\n%s%s%s\n' "$C_BLD" "$*" "$C_OFF"; }
dim()  { printf '%s%s%s\n' "$C_DIM" "$*" "$C_OFF"; }

usage() {
  cat <<'EOF'
xiao-generate build + flash test matrix

USAGE
  ./scripts/matrix.sh build [OPTIONS]        # no hardware needed
  ./scripts/matrix.sh flash --chip <chip>    # ONE board connected

OPTIONS (build)
  --chip <chip>       restrict the matrix to one variant (see --list-chips)
  --include-std       also test the ESP `std` (--stack esp-idf) rows.
                      OFF BY DEFAULT for two reasons:
                        1. the pinned esp-idf-template revision does not
                           resolve under cargo-generate's shallow clone
                           (known-broken upstream, fix in flight), and
                        2. the first esp-idf build downloads and compiles the
                           whole ESP-IDF SDK (8-20 minutes per row).
  --list              print the enumerated matrix and exit (no generation)
  --keep-projects     keep every generated project dir (default: keep only
                      the ones that FAILed, to save disk)
  --work-dir <dir>    where to generate + build (default: a fresh temp dir)

OPTIONS (flash)
  --chip <chip>       REQUIRED. The variant of the board that is plugged in.
  --no-option-sweep   only flash one row per (stack, template, framework) with
                      the default flashing/logging, instead of the full
                      flashing x logging sweep. Far fewer button presses.
  --work-dir <dir>    as above

EXAMPLES
  ./scripts/matrix.sh build                      # the whole no-hardware matrix
  ./scripts/matrix.sh build --chip xiao-esp32c6  # one variant
  ./scripts/matrix.sh build --list               # just show me the rows
  ./scripts/matrix.sh flash --chip xiao-rp2040 --no-option-sweep
EOF
}

# ---------------------------------------------------------------------------
# arg parsing
# ---------------------------------------------------------------------------
[ $# -ge 1 ] || { usage; exit 2; }
MODE=$1; shift
case "$MODE" in
  build|flash) ;;
  -h|--help|help) usage; exit 0 ;;
  *) say "error: unknown mode '$MODE' (expected 'build' or 'flash')"; usage; exit 2 ;;
esac

while [ $# -gt 0 ]; do
  case "$1" in
    --chip)            ONLY_CHIP=${2:-}; shift 2 ;;
    --include-std)     INCLUDE_STD=1; shift ;;
    --list)            LIST_ONLY=1; shift ;;
    --no-option-sweep) OPTION_SWEEP=0; shift ;;
    --keep-projects)   KEEP_PROJECTS=1; shift ;;
    --work-dir)        WORK=${2:-}; shift 2 ;;
    -h|--help)         usage; exit 0 ;;
    *) say "error: unknown option '$1'"; usage; exit 2 ;;
  esac
done

if [ "$MODE" = flash ] && [ -z "$ONLY_CHIP" ]; then
  say "error: 'flash' needs --chip <chip> (which board is plugged in?)"
  exit 2
fi

# ---------------------------------------------------------------------------
# work dir
# ---------------------------------------------------------------------------
if [ -z "$WORK" ]; then
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/xiao-matrix.XXXXXX")
fi
mkdir -p "$WORK/logs" "$WORK/projects" "$WORK/targets"
ROWS="$WORK/rows.tsv"
RESULTS="$WORK/results.tsv"
: >"$ROWS"
: >"$RESULTS"

# ---------------------------------------------------------------------------
# 0. the generator itself
# ---------------------------------------------------------------------------
hdr "== xiao-generate build + flash matrix =="
say  "repo:      $REPO_ROOT"
say  "work dir:  $WORK"

if [ ! -x "$BIN" ] || [ -n "$(find "$REPO_ROOT/src" -name '*.rs' -newer "$BIN" -print -quit 2>/dev/null)" ]; then
  say  "building the generator (cargo build --release) ..."
  ( cd "$REPO_ROOT" && cargo build --release ) >"$WORK/logs/_xiao-generate-build.log" 2>&1 || {
    say "${C_RED}error: could not build xiao-generate${C_OFF}"
    say "see $WORK/logs/_xiao-generate-build.log"
    exit 1
  }
fi
say  "generator: $BIN ($("$BIN" --version 2>/dev/null | head -1))"

# xg <args...> -> runs the generator non-interactively, never inherits stdin
xg() { "$BIN" "$@" </dev/null; }

# ---------------------------------------------------------------------------
# 1. host toolchain preflight (recorded once, used to SKIP rows honestly)
# ---------------------------------------------------------------------------
have() { command -v "$1" >/dev/null 2>&1; }

HAVE_ESP_GENERATE=0;  have esp-generate                       && HAVE_ESP_GENERATE=1
HAVE_CARGO_GEN=0;     cargo generate --version >/dev/null 2>&1 && HAVE_CARGO_GEN=1
HAVE_ESPFLASH=0;      have espflash                           && HAVE_ESPFLASH=1
HAVE_ELF2UF2=0;       have elf2uf2-rs                         && HAVE_ELF2UF2=1
HAVE_UF2DEPLOY=0;     have uf2deploy                          && HAVE_UF2DEPLOY=1
HAVE_HF2=0;           have hf2                                && HAVE_HF2=1
HAVE_PROBE_RS=0;      have probe-rs                           && HAVE_PROBE_RS=1

# Custom (espup-linked) toolchains are not listable with `rustup target list
# --toolchain esp`, so probe the toolchain itself for the Xtensa target.
HAVE_ESP_TOOLCHAIN=0
if rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  if rustup run esp rustc --print target-list 2>/dev/null | grep -q '^xtensa-esp32s3-none-elf$'; then
    HAVE_ESP_TOOLCHAIN=1
  fi
fi

DEFAULT_TOOLCHAIN=$(rustup show active-toolchain 2>/dev/null | awk '{print $1}')
INSTALLED_TARGETS=$(rustup target list --installed 2>/dev/null || true)

target_installed() { printf '%s\n' "$INSTALLED_TARGETS" | grep -qx -- "$1"; }

hdr "-- host toolchain --"
printf '  %-34s %s\n' "default rust toolchain"  "${DEFAULT_TOOLCHAIN:-unknown}"
if [ $HAVE_ESP_TOOLCHAIN = 1 ]; then
  printf '  %-34s %s\n' "Xtensa 'esp' toolchain"  "${C_GRN}present${C_OFF} ($(rustup run esp rustc -V 2>/dev/null | awk '{print $2}'))"
else
  printf '  %-34s %s\n' "Xtensa 'esp' toolchain"  "${C_YEL}MISSING${C_OFF} -> cargo install espup --locked && espup install"
fi
for t in thumbv6m-none-eabi thumbv7em-none-eabihf thumbv8m.main-none-eabihf \
         riscv32imc-unknown-none-elf riscv32imac-unknown-none-elf; do
  if target_installed "$t"; then
    printf '  %-34s %s\n' "$t" "${C_GRN}installed${C_OFF}"
  else
    printf '  %-34s %s\n' "$t" "${C_YEL}MISSING${C_OFF} -> rustup target add $t"
  fi
done
_tool_line() { # name, have, install-hint
  if [ "$2" = 1 ]; then printf '  %-34s %s\n' "$1" "${C_GRN}present${C_OFF}"
  else printf '  %-34s %s\n' "$1" "${C_YEL}MISSING${C_OFF} -> $3"; fi
}
_tool_line "esp-generate (esp-hal backend)" "$HAVE_ESP_GENERATE" "cargo install esp-generate --version 1.3.0 --locked"
_tool_line "cargo-generate (esp-idf backend)" "$HAVE_CARGO_GEN"  "cargo install cargo-generate --locked"
_tool_line "espflash (ESP flashing)"        "$HAVE_ESPFLASH"     "cargo install espflash --locked"
_tool_line "elf2uf2-rs (RP2040/RP2350 flash)" "$HAVE_ELF2UF2"    "cargo install elf2uf2-rs"
_tool_line "uf2deploy (nRF/SAMD UF2 flash)" "$HAVE_UF2DEPLOY"    "cargo install uf2deploy"
_tool_line "hf2 (SAMD21 flash)"             "$HAVE_HF2"          "cargo install hf2-cli"

# ---------------------------------------------------------------------------
# 2. harvest the registry from the tool's own output
# ---------------------------------------------------------------------------
# ALL_CHIPS is the whole registry (before any --chip filter); it is what tells
# the silicon-fold whether a variant has a twin, so `<chip>(+sense)` still shows
# even when the run is filtered to the base variant alone.
ALL_CHIPS=$(xg --list-chips | awk '/^  xiao-/ {print $1}')
[ -n "$ALL_CHIPS" ] || { say "${C_RED}error: --list-chips produced no variants${C_OFF}"; exit 1; }
CHIPS=$ALL_CHIPS

if [ -n "$ONLY_CHIP" ]; then
  if ! printf '%s\n' "$ALL_CHIPS" | grep -qx -- "$ONLY_CHIP"; then
    say "${C_RED}error: unknown chip '$ONLY_CHIP'${C_OFF}"
    say "known chips:"; printf '  %s\n' $ALL_CHIPS
    exit 2
  fi
  CHIPS=$ONLY_CHIP
fi

# stacks_of <chip> -> lines: "<stack>\t<target>\t<fws>\t<flashing>\t<logging>"
# Each list is comma-separated with the FIRST id its default; harvested straight
# from the stack's own `--list-stacks` lines (the per-stack SUPPORTED subset).
#   frameworks = the "frameworks:" available list UNION the "<fw>: [gap] ..."
#                entries (so gaps are still enumerated and gated).
#   flashing / logging = the "flashing:"/"logging:" support lines verbatim.
# All three are empty when the axis is not live for that stack (esp-idf std,
# whose toolchain the template fixes -- it prints none of these lines).
stacks_of() {
  xg --list-stacks --chip "$1" | awk '
    /^  [^ ]/                 { if (id!="") emit(); id=$1; tgt=""; fws=""; fl=""; lg=""; next }
    /^ +target: /             { tgt=$2; next }
    /^ +frameworks: /         { s=$0; sub(/^ +frameworks: /,"",s); gsub(/, /,",",s); fws=s; next }
    /^ +flashing: /           { s=$0; sub(/^ +flashing: /,"",s); gsub(/, /,",",s); fl=s; next }
    /^ +logging: /            { s=$0; sub(/^ +logging: /,"",s); gsub(/, /,",",s); lg=s; next }
    /^ +[a-z0-9_-]+: \[gap\]/ { f=$1; sub(/:$/,"",f); fws = (fws=="" ? f : fws "," f); next }
    END { if (id!="") emit() }
    function emit() { printf "%s\t%s\t%s\t%s\t%s\n", id, tgt, fws, fl, lg }
  '
}

# templates_of <chip> <stack> -> lines "<template>\t<ok|gap>"
templates_of() {
  xg --list-templates --chip "$1" | awk -v want="$2" '
    /^  \[.* stack\]$/ { cur=$1; sub(/^\[/,"",cur); inblk = (cur==want); next }
    /^  --uferris/     { inblk=0; next }
    /^    [^ ]/ {
      if (!inblk) next
      st = (index($0,"unavailable]")>0) ? "gap" : "ok"
      printf "%s\t%s\n", $1, st
    }
  '
}

# uferris_of <chip> -> "" (not offered) | "ok" | "gap"
uferris_of() {
  xg --list-templates --chip "$1" | awk '
    /^  --uferris/ { print (index($0,"unavailable]")>0) ? "gap" : "ok"; found=1 }
    END { if (!found) print "" }
  '
}

# ---------------------------------------------------------------------------
# 3. enumerate the matrix
# ---------------------------------------------------------------------------
# row TSV: id  label  chip  stack  target  template  uferris  extra-args  note
#   chip  = the real variant id, used to GENERATE (`--chip`) and to pick the
#           flash route -- never folded.
#   label = the display id, which is `chip` plus `(+sense)` when this row's base
#           template also stands in for a same-silicon twin (the fold).
ROW_N=0
emit_row() { # label chip stack target template uferris extra note
  ROW_N=$((ROW_N + 1))
  printf 'row%03d\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$ROW_N" "$1" "$2" "$3" "$4" "$5" "$6" "$7" "$8" >>"$ROWS"
}

# A "base" template is the implicit working project (base / base-blink /
# base-hello) -- mirrors registry::is_base_template. Only on a base template of
# an esp-hal stack do FLASHING/LOGGING/FRAMEWORK actually change what is
# generated (see model::option_groups_apply); a component add-on template
# carries its own fixed option set, so sweeping those axes there would build
# the identical project N times.
is_base_template() {
  case "$1" in base|base-blink|base-hello) return 0 ;; *) return 1 ;; esac
}

# esp-hal is the one stack where the flashing choice changes the generated code
# (it maps to esp-generate `-o` flags). Detected from the target triple exactly
# as skip_reason() classifies backends: ESP no_std targets are riscv32*-none-elf
# / xtensa-*-none-elf (esp-idf ends in -espidf; the non-ESP scaffolds are thumb*).
is_esp_hal_target() {
  case "$1" in riscv32*-none-elf|xtensa-*-none-elf) return 0 ;; *) return 1 ;; esac
}

hdr "-- enumerating the matrix --"
for chip in $CHIPS; do
  # --- silicon fold (dedup sense / non-sense base templates) ---
  # Two variants that share silicon are named `<base>` and `<base>-<suffix>`
  # (xiao-esp32s3 / xiao-esp32s3-sense), so a variant T folds into E exactly when
  # T = "E-<suffix>". BASE templates are byte-identical across such a pair (same
  # silicon; only onboard components differ), so only the representative -- the
  # base id E present in this run -- emits them; a folded twin emits only its own
  # component templates. `is_base_emitter` is 1 unless some OTHER in-run chip is
  # our base id.
  is_base_emitter=1
  for other in $CHIPS; do
    [ "$other" = "$chip" ] && continue
    case "$chip" in "$other"-*) is_base_emitter=0; break ;; esac
  done
  # The `(+suffix)` tag lists the same-silicon twins this chip's base stands in
  # for. Derived from the WHOLE registry (ALL_CHIPS), so it shows even when the
  # run is filtered to the base variant alone -- that is the fold made visible.
  fold_tag=""
  for t in $ALL_CHIPS; do
    case "$t" in "$chip"-*) fold_tag="$fold_tag+${t#"$chip"-}" ;; esac
  done
  if [ -n "$fold_tag" ]; then base_label="$chip($fold_tag)"; else base_label="$chip"; fi

  stacks_of "$chip" | while IFS=$'\t' read -r stack target fws fl_sup lg_sup; do
    [ -n "$stack" ] || continue

    is_std=0
    case "$target" in *-espidf) is_std=1 ;; esac
    if [ $is_std = 1 ] && [ $INCLUDE_STD = 0 ]; then
      continue
    fi

    # framework values for this stack (support list, gaps included)
    fw_list=$(printf '%s' "$fws" | tr ',' ' ')
    [ -n "$fw_list" ] || fw_list="-"     # "-" = do not pass --framework

    # per-stack flashing/logging SUPPORT (first id = default), from --list-stacks
    fl_all=$(printf '%s' "$fl_sup" | tr ',' ' ')
    lg_all=$(printf '%s' "$lg_sup" | tr ',' ' ')
    fl_default=$(printf '%s' "$fl_all" | awk '{print $1}')
    lg_default=$(printf '%s' "$lg_all" | awk '{print $1}')

    # FLASHING is a code-affecting sweep only on esp-hal; elsewhere it is a
    # summary/prereq-only choice, so build the DEFAULT once. LOGGING changes
    # generation (defmt vs none) everywhere, so it is swept on every stack.
    esp_hal=0; is_esp_hal_target "$target" && esp_hal=1

    templates_of "$chip" "$stack" | while IFS=$'\t' read -r tmpl tstate; do
      [ -n "$tmpl" ] || continue
      note="-"
      [ "$tstate" = gap ] && note="template-gap"

      if is_base_template "$tmpl"; then
        # Fold: a base template rides on the representative variant only.
        [ "$is_base_emitter" = 1 ] || continue

        # flashing values to sweep
        if [ $esp_hal = 1 ] && [ $OPTION_SWEEP = 1 ]; then
          fl_vals="$fl_all"
        else
          fl_vals="$fl_default"
        fi
        # logging values to sweep
        if [ $OPTION_SWEEP = 1 ]; then lg_vals="$lg_all"; else lg_vals="$lg_default"; fi
        # esp-idf leaves both empty (axis not live) -> a single "-" placeholder
        [ -n "$fl_vals" ] || fl_vals="-"
        [ -n "$lg_vals" ] || lg_vals="-"

        # The tool still gates illegal esp-hal cells (e.g. probe-rs + log) and
        # real framework gaps (embassy-on-blocking-HAL, rtic) itself, with exit 2.
        for fl in $fl_vals; do
          for lg in $lg_vals; do
            for fw in $fw_list; do
              extra="--template $tmpl"
              [ "$fl" != "-" ] && extra="$extra --flashing $fl"
              [ "$lg" != "-" ] && extra="$extra --logging $lg"
              [ "$fw" != "-" ] && extra="$extra --framework $fw"
              emit_row "$base_label" "$chip" "$stack" "$target" "$tmpl" 0 "$extra" "$note"
            done
          done
        done
      else
        # An onboard-component add-on template pins its own option set (the three
        # axes are locked), so it is one row -- and it belongs to THIS variant's
        # silicon-specific components, so it is never folded onto a twin.
        emit_row "$chip" "$chip" "$stack" "$target" "$tmpl" 0 "--template $tmpl" "$note"
      fi
    done
  done

  # uFerris rows are a base-like mode (not a component): identical across a
  # silicon twin pair, so they fold onto the representative exactly like a base
  # template. One row per uferris-capable stack.
  if [ "$is_base_emitter" = 1 ]; then
    uf=$(uferris_of "$chip")
    if [ -n "$uf" ]; then
      ufnote="-"; [ "$uf" = gap ] && ufnote="uferris-gap"
      stacks_of "$chip" | while IFS=$'\t' read -r stack target fws fl_sup lg_sup; do
        [ -n "$stack" ] || continue
        case "$target" in *-espidf) continue ;; esac   # uFerris is esp-hal only
        emit_row "$base_label" "$chip" "$stack" "$target" "blinky(uferris)" 1 "--uferris" "$ufnote"
      done
    fi
  fi
done

# the subshells above appended straight to $ROWS; renumber for a clean sequence
awk -F'\t' 'BEGIN{OFS="\t"} {$1=sprintf("row%03d",NR); print}' "$ROWS" >"$ROWS.tmp" && mv "$ROWS.tmp" "$ROWS"
TOTAL=$(wc -l <"$ROWS" | tr -d ' ')
say "$TOTAL combinations enumerated from the registry"
if [ $INCLUDE_STD = 0 ]; then
  dim "(ESP std / --stack esp-idf rows excluded; pass --include-std to add them)"
fi

if [ $LIST_ONLY = 1 ]; then
  hdr "-- matrix --"
  # columns: id  label  stack  template  extra   ($2 is the display label, $3 the
  # real generation chip; the fold shows in $2 as `<chip>(+sense)`).
  awk -F'\t' '{printf "  %-7s %-24s %-11s %-16s %s\n", $1,$2,$4,$6,$8}' "$ROWS"
  say ""
  say "work dir: $WORK"
  exit 0
fi

# ---------------------------------------------------------------------------
# 4. shared helpers for running a row
# ---------------------------------------------------------------------------
PASS=0; FAILED=0; SKIPPED=0; GATED=0; BLOCKED=0
FAIL_LIST="$WORK/failures.txt"; : >"$FAIL_LIST"

record() { # id status detail
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$RESULTS"
  case "$2" in
    PASS)    PASS=$((PASS+1));       printf '  %s%-7s%s %s\n' "$C_GRN" PASS    "$C_OFF" "$3" ;;
    FAIL)    FAILED=$((FAILED+1));   printf '  %s%-7s%s %s\n' "$C_RED" FAIL    "$C_OFF" "$3"
             printf '%s\t%s\n' "$1" "$3" >>"$FAIL_LIST" ;;
    GATED)   GATED=$((GATED+1));     printf '  %s%-7s%s %s\n' "$C_BLU" GATED   "$C_OFF" "$3" ;;
    SKIP)    SKIPPED=$((SKIPPED+1)); printf '  %s%-7s%s %s\n' "$C_YEL" SKIP    "$C_OFF" "$3" ;;
    BLOCKED) BLOCKED=$((BLOCKED+1)); printf '  %s%-7s%s %s\n' "$C_YEL" BLOCKED "$C_OFF" "$3" ;;
  esac
}

# why this row cannot be attempted on this host (empty = go ahead)
skip_reason() { # target stack
  local target=$1 stack=$2
  case "$target" in
    xtensa-*)
      [ $HAVE_ESP_TOOLCHAIN = 1 ] || {
        printf 'Xtensa `esp` toolchain not installed (needed for %s) -- cargo install espup --locked && espup install' "$target"; return; } ;;
    *-espidf)
      [ $HAVE_CARGO_GEN = 1 ] || { printf 'cargo-generate not installed -- cargo install cargo-generate --locked'; return; } ;;
    *)
      target_installed "$target" || {
        printf 'rust target %s not installed -- rustup target add %s' "$target" "$target"; return; } ;;
  esac
  case "$target" in
    riscv32*-none-elf|xtensa-*-none-elf)
      [ $HAVE_ESP_GENERATE = 1 ] || {
        printf 'esp-generate not installed (esp-hal backend) -- cargo install esp-generate --version 1.3.0 --locked'; return; } ;;
  esac
  case "$target" in
    *-espidf)
      [ $HAVE_CARGO_GEN = 1 ] || { printf 'cargo-generate not installed -- cargo install cargo-generate --locked'; return; } ;;
  esac
  printf ''
}

# Generate one row. MUST NOT be called in a command substitution -- it reports
# through globals so the caller can see the tool's own error text.
#   return 0 = generated, 2 = gated by xiao-generate (exit 2), 1 = real failure
GEN_MSG=""; GEN_DIR=""; GEN_LOG=""
generate_row() { # id chip stack extra
  local id=$1 chip=$2 stack=$3 extra=$4
  GEN_DIR="$WORK/projects/$id"
  GEN_LOG="$WORK/logs/$id.gen.log"
  GEN_MSG=""
  rm -rf "$GEN_DIR"
  local rc=0
  # `cmd && rc=0 || rc=$?` is the set -e-safe way to capture an exit status:
  # toggling `set +e/-e` inside a helper leaks the setting back to the caller.
  # shellcheck disable=SC2086
  xg --headless --chip "$chip" --stack "$stack" $extra --out "$GEN_DIR" >"$GEN_LOG" 2>&1 \
    && rc=0 || rc=$?
  [ $rc -eq 0 ] && return 0
  GEN_MSG=$(sed -n 's/^error: //p' "$GEN_LOG" | head -1)
  [ -n "$GEN_MSG" ] || GEN_MSG=$(tr '\n' ' ' <"$GEN_LOG" | cut -c1-200)
  [ $rc -eq 2 ] && return 2
  return 1
}

# Build a generated project. Same rule: call it directly, not in $( ).
#   return 0 = built, 1 = failed. Sets BUILD_LOG / BUILD_ELF / BUILD_ERR.
BUILD_LOG=""; BUILD_ELF=""; BUILD_ERR=""
build_row() { # id dir target
  local id=$1 dir=$2 target=$3
  local tdir="$WORK/targets/$(printf '%s' "$target" | tr './' '__')"
  BUILD_LOG="$WORK/logs/$id.build.log"
  BUILD_ELF="$tdir/$target/release/$id"
  BUILD_ERR=""
  # Guard: `cd ""` is a silent no-op in bash and would build THIS repo instead
  # of the generated project. Never let that be mistaken for a matrix result.
  if [ -z "$dir" ] || [ ! -d "$dir" ]; then
    BUILD_ERR="internal: generated project dir '$dir' does not exist"
    return 1
  fi
  local rc=0
  ( cd "$dir" && CARGO_TARGET_DIR="$tdir" cargo build --release ) >"$BUILD_LOG" 2>&1 </dev/null \
    && rc=0 || rc=$?
  if [ $rc -eq 0 ] && [ -f "$BUILD_ELF" ]; then return 0; fi
  BUILD_ERR=$(grep -m1 -E '^error(\[|:)' "$BUILD_LOG" 2>/dev/null || true)
  [ -n "$BUILD_ERR" ] || BUILD_ERR=$(grep -m1 -E 'error' "$BUILD_LOG" 2>/dev/null || true)
  if [ -z "$BUILD_ERR" ]; then
    if [ $rc -eq 0 ]; then BUILD_ERR="cargo succeeded but no ELF at $BUILD_ELF"
    else BUILD_ERR="cargo build --release exited $rc"; fi
  fi
  return 1
}

# ELF32 e_entry (little-endian word at file offset 24). Used to prove where a
# firmware image is actually linked before we consider writing it to a board
# whose bootloader lives in the same flash.
elf_entry() {
  local e
  e=$(od -An -tx4 -j24 -N4 "$1" 2>/dev/null | tr -d ' \n')
  case "$e" in [0-9a-fA-F][0-9a-fA-F]*) printf '%s' "$e" ;; *) printf '' ;; esac
}

# ---------------------------------------------------------------------------
# 5a. BUILD MODE
# ---------------------------------------------------------------------------
run_build_mode() {
  hdr "-- build matrix ($TOTAL rows) --"
  dim "generating into $WORK/projects, building with a per-target shared CARGO_TARGET_DIR"
  local n=0
  while IFS=$'\t' read -r id disp chip stack target tmpl uferris extra note; do
    n=$((n+1))
    local label
    label=$(printf '[%d/%d] %-24s %-11s %s' "$n" "$TOTAL" "$disp" "$stack" "$(printf '%s' "$extra" | sed 's/--template //;s/--flashing /fl=/;s/--logging /lg=/;s/--framework /fw=/')")

    local why
    why=$(skip_reason "$target" "$stack")
    if [ -n "$why" ]; then
      record "$id" SKIP "$label :: $why"
      continue
    fi

    local grc=0
    generate_row "$id" "$chip" "$stack" "$extra" && grc=0 || grc=$?
    case $grc in
      2) record "$id" GATED "$label :: $GEN_MSG"; rm -rf "$GEN_DIR"; continue ;;
      1) record "$id" FAIL  "$label :: generation failed: $GEN_MSG  (log: $GEN_LOG)"; continue ;;
    esac

    local brc=0
    build_row "$id" "$GEN_DIR" "$target" && brc=0 || brc=$?
    if [ $brc -eq 0 ]; then
      local ent; ent=$(elf_entry "$BUILD_ELF")
      record "$id" PASS "$label :: elf entry 0x${ent:-????????}"
      [ $KEEP_PROJECTS = 1 ] || rm -rf "$GEN_DIR"
    else
      record "$id" FAIL "$label :: $BUILD_ERR  (log: $BUILD_LOG)"
    fi
  done <"$ROWS"
}

# ---------------------------------------------------------------------------
# 5b. FLASH MODE
# ---------------------------------------------------------------------------
pause_for() { # message
  printf '\n  %s%s%s\n' "$C_BLD" "ACTION REQUIRED" "$C_OFF"
  printf '  %s\n' "$1"
  printf '  press ENTER when ready (or Ctrl-C to abort) ... '
  read -r _ </dev/tty || true
  printf '\n'
}

wait_for_volume() { # volume-name  -> 0 found, 1 not
  local vol=$1 i=0
  while [ $i -lt 20 ]; do
    [ -d "/Volumes/$vol" ] && return 0
    sleep 1; i=$((i+1))
  done
  return 1
}

# Run a flashing command, keeping it INTERACTIVE (espflash may prompt to choose
# a serial port) while still capturing a log and the tool's real exit status.
# errexit is saved and restored so this never leaks the setting to the caller.
run_flash() { # logfile cmd...
  local log=$1; shift
  local rc=0 restore=""
  case $- in *e*) restore="set -e" ;; *) restore="set +e" ;; esac
  set +e
  "$@" 2>&1 </dev/tty | tee "$log"
  rc=${PIPESTATUS[0]}
  $restore
  return $rc
}

# Every flash_* helper reports through FLASH_MSG rather than stdout, because
# stdout has to stay attached to the terminal: the helpers print instructions
# for the human and stream the flasher's own progress. Return codes:
#   0 = flashed   1 = flash failed   3 = tool missing (SKIP)   4 = unsafe (BLOCKED)
FLASH_MSG=""

flash_esp() { # id chip elf dir
  local id=$1 chip=$2 elf=$3 dir=$4
  if [ $HAVE_ESPFLASH = 0 ]; then
    FLASH_MSG='espflash not installed -- cargo install espflash --locked'; return 3
  fi
  # The exact chip id comes from the generated .cargo/config.toml runner line
  # (`runner = "espflash flash --monitor --chip esp32c6"`), so it is backend
  # data, not a mapping invented here.
  local espchip
  espchip=$(sed -n 's/.*--chip \([a-z0-9]*\).*/\1/p' "$dir/.cargo/config.toml" 2>/dev/null | head -1)
  [ -n "$espchip" ] || espchip=$(printf '%s' "$chip" | sed -n 's/^xiao-\(esp32[a-z][0-9]*\).*/\1/p')
  pause_for "Plug the $chip into USB-C. No button press is normally needed -- espflash
  resets it over the USB-Serial-JTAG / DTR-RTS lines. If it is not detected,
  hold BOOT, tap RESET, release BOOT, then continue."
  if run_flash "$WORK/logs/$id.flash.log" espflash flash --chip "$espchip" "$elf"; then
    FLASH_MSG="espflash flash --chip $espchip reported success"; return 0
  fi
  FLASH_MSG="espflash failed (see $WORK/logs/$id.flash.log)"; return 1
}

flash_rp() { # id chip elf  (RP2040 / RP2350 -- BOOTSEL mass storage)
  local id=$1 chip=$2 elf=$3
  local vol
  case "$chip" in
    *rp2040) vol=RPI-RP2 ;;
    *rp2350) vol=RP2350 ;;
    *) vol=RPI-RP2 ;;
  esac
  if [ $HAVE_ELF2UF2 = 0 ]; then
    FLASH_MSG='elf2uf2-rs not installed -- cargo install elf2uf2-rs'; return 3
  fi
  pause_for "Hold the BOOT ('B') button on the $chip and plug in USB-C.
  (Already plugged in? hold B, tap RESET, release B.)
  A volume named '$vol' must appear in Finder."
  if ! wait_for_volume "$vol"; then
    FLASH_MSG="/Volumes/$vol never appeared -- board is not in BOOTSEL mode"; return 1
  fi
  # elf2uf2-rs -d converts the ELF to UF2 and copies it onto the mounted BOOTSEL
  # drive in one step -- no probe, no separate objcopy. NOTE: elf2uf2-rs' RP2350
  # output has been reported broken upstream (JoNil/elf2uf2-rs#38); if an RP2350
  # write fails here, check that issue before suspecting the generated firmware.
  if run_flash "$WORK/logs/$id.flash.log" elf2uf2-rs -d "$elf"; then
    FLASH_MSG="elf2uf2-rs -d -> /Volumes/$vol reported success"; return 0
  fi
  FLASH_MSG="elf2uf2-rs -d failed (see $WORK/logs/$id.flash.log)"; return 1
}

flash_nrf() { # id chip elf  (Adafruit nRF52 UF2 bootloader, double-tap RESET)
  local id=$1 chip=$2 elf=$3
  local vol
  case "$chip" in
    *sense) vol=XIAO-SENSE ;;
    *)      vol=XIAO-BOOT  ;;
  esac
  # SAFETY GATE -- an invariant check on whatever ELF we were handed, not an
  # assertion about what xiao-generate currently emits.
  #
  # The stock XIAO nRF52840 ships the Adafruit nRF52 bootloader with SoftDevice
  # S140 7.3.0, which fixes the application window at 0x27000..0xEA000:
  #   low   0x27000  MBR (0x0) + SoftDevice S140 7.3.0 occupy everything below.
  #                  The bootloader jumps to SD_SIZE_GET(MBR_SIZE), so 0x27000
  #                  is exact, not a floor -- linking higher simply never boots.
  #   high  0xEA000  USER_FLASH_END = BOOTLOADER_REGION_START - DFU_APP_DATA_RESERVED
  #                  = 0xF4000 - 0xA000            (uf2cfg.h, dfu_types.h, Makefile)
  # Both bounds must hold. An image linked below 0x27000 would be written over
  # the MBR + SoftDevice, bricking the bootloader -- recovery needs SWD, which
  # we assume does not exist. An image running past 0xEA000 is worse than
  # rejected: ghostfat.c's in_app_space() drops out-of-window blocks silently,
  # so it flashes "successfully" with its tail missing.
  local ent; ent=$(elf_entry "$elf")
  [ -n "$ent" ] || { FLASH_MSG="could not read the ELF entry point of $elf -- refusing to write flash blind"; return 4; }
  local entdec; entdec=$((0x$ent))
  if [ "$entdec" -lt $((0x27000)) ]; then
    FLASH_MSG="UNSAFE, refusing: this ELF is linked below the application window (entry 0x$ent < 0x27000). The stock XIAO nRF52840 ships the Adafruit nRF52 bootloader with SoftDevice S140 7.3.0 -- MBR at 0x0, SoftDevice up to 0x27000, application from 0x27000. Written at 0x0 this image destroys the MBR+SoftDevice (recovery needs SWD, and there is no probe); relocated to 0x27000 every absolute address in it is wrong. Fix: relink at ORIGIN = 0x27000, LENGTH = 780K (the window ends at USER_FLASH_END = 0xEA000, not at the bootloader at 0xF4000), then this route runs automatically"
    return 4
  fi
  if [ $HAVE_UF2DEPLOY = 0 ]; then
    FLASH_MSG='uf2deploy not installed -- cargo install uf2deploy'; return 3
  fi
  # Upper bound of the same invariant, now delegated to the tool. The Adafruit
  # UF2 bootloader silently DROPS blocks at or above USER_FLASH_END (0xEA000)
  # rather than failing (ghostfat.c in_app_space), so an oversized image would
  # "flash" with its tail missing. The old route measured the raw .bin to catch
  # this; uf2deploy converts the ELF directly, so we rely on it emitting UF2 from
  # the ELF's own load addresses -- which memory_nrf52840.x links into the
  # 0x27000..0xEA000 (780 KiB) application window.
  pause_for "Double-tap the RESET button on the $chip (two taps within ~500 ms).
  A volume named '$vol' must appear in Finder."
  wait_for_volume "$vol" || { FLASH_MSG="/Volumes/$vol never appeared -- board did not enter the UF2 bootloader"; return 1; }
  # uf2deploy converts the ELF to UF2 (family nrf52840 == 0xADA52840) and copies
  # it to the mounted bootloader volume in one step. `--path auto` locates the
  # drive, sidestepping the macOS FAT-volume xattr quirk that made a plain `cp`
  # unreliable and let the board reboot mid-copy.
  if run_flash "$WORK/logs/$id.flash.log" uf2deploy deploy --family nrf52840 --path auto "$elf"; then
    FLASH_MSG="uf2deploy deploy (family nrf52840) -> /Volumes/$vol reported success"; return 0
  fi
  FLASH_MSG="uf2deploy deploy failed (see $WORK/logs/$id.flash.log)"; return 1
}

flash_samd() { # id chip elf  (Seeed uf2-samdx1 bootloader, volume "Arduino")
  local id=$1 chip=$2 elf=$3
  local vol=Arduino
  # SAFETY GATE: the UF2 bootloader occupies the first 8 KiB, so the app must
  # be linked at 0x2000 or above.
  local ent; ent=$(elf_entry "$elf")
  [ -n "$ent" ] || { FLASH_MSG="could not read the ELF entry point of $elf -- refusing to write flash blind"; return 4; }
  local entdec; entdec=$((0x$ent))
  if [ "$entdec" -lt $((0x2000)) ]; then
    FLASH_MSG="UNSAFE, refusing: this ELF is linked at 0x$ent, below the 8 KiB UF2 bootloader (the app must start at 0x2000). Writing it would overwrite the bootloader and recovery needs SWD"
    return 4
  fi
  if [ $HAVE_HF2 = 1 ]; then
    pause_for "Short the two RST pads on the $chip twice quickly (the XIAO SAMD21 has
  no reset button -- bridge the pads with tweezers or a wire). The orange LED
  pulses and a volume named '$vol' appears."
    wait_for_volume "$vol" || dim "  (no /Volumes/$vol -- trying HF2 over HID anyway)"
    # hf2's `elf` subcommand takes the ELF directly. VID/PID are Seeed's, from
    # Seeed-Studio/uf2-samdx1 boards/XIAO_m0/board_config.h.
    if run_flash "$WORK/logs/$id.flash.log" hf2 --vid 0x2886 --pid 0x002f elf "$elf"; then
      FLASH_MSG='hf2 elf reported success'; return 0
    fi
    FLASH_MSG="hf2 failed (see $WORK/logs/$id.flash.log). macOS note: Terminal needs Input Monitoring permission for HID access"; return 1
  fi
  # No HID flasher -- fall back to the shared UF2 route (uf2deploy), which the
  # registry now lists as a SAMD21 flashing option alongside hf2.
  if [ $HAVE_UF2DEPLOY = 0 ]; then
    FLASH_MSG='no SAMD21 flasher -- cargo install hf2-cli  (or: cargo install uf2deploy for the UF2 mass-storage route)'; return 3
  fi
  pause_for "Short the two RST pads on the $chip twice quickly. A volume named '$vol' appears."
  wait_for_volume "$vol" || { FLASH_MSG="/Volumes/$vol never appeared"; return 1; }
  # uf2deploy converts the ELF to UF2 (family samd21 == 0x68ed2b88, app base
  # 0x2000 from the ELF's own load addresses) and copies it to the mounted
  # volume in one step. `--path auto` handles the macOS FAT-volume xattr quirk
  # that made a plain `cp` unreliable. NOTE: confirm uf2deploy's SAMD family
  # name (`samd21`) against `uf2deploy --help` for your version.
  if run_flash "$WORK/logs/$id.flash.log" uf2deploy deploy --family samd21 --path auto "$elf"; then
    FLASH_MSG='uf2deploy deploy (family samd21) reported success'; return 0
  fi
  FLASH_MSG="uf2deploy deploy failed (see $WORK/logs/$id.flash.log)"; return 1
}

run_flash_mode() {
  hdr "-- flash matrix for $ONLY_CHIP ($TOTAL rows) --"
  say  "Every row below generates, builds, and then FLASHES the board. You will be"
  say  "asked to put the board into its bootloader before each write."
  say  ""
  say  "${C_BLD}This verifies that flashing succeeded -- nothing more.${C_OFF} There is no probe and"
  say  "no serial capture here, so the script never claims the firmware ran."
  if [ $OPTION_SWEEP = 1 ]; then
    dim "($TOTAL rows including the flashing x logging sweep; --no-option-sweep cuts it down)"
  fi
  if [ $HAVE_PROBE_RS = 1 ]; then
    dim "(probe-rs is installed but is NOT used: every route below is USB/UF2 only, no SWD probe required)"
  fi
  pause_for "Connect ONE $ONLY_CHIP board now."

  local n=0
  while IFS=$'\t' read -r id disp chip stack target tmpl uferris extra note; do
    n=$((n+1))
    local label
    label=$(printf '[%d/%d] %-11s %s' "$n" "$TOTAL" "$stack" "$(printf '%s' "$extra" | sed 's/--template //;s/--flashing /fl=/;s/--logging /lg=/;s/--framework /fw=/')")
    hdr "$label"

    local why; why=$(skip_reason "$target" "$stack")
    if [ -n "$why" ]; then record "$id" SKIP "$label :: $why"; continue; fi

    local grc=0
    generate_row "$id" "$chip" "$stack" "$extra" && grc=0 || grc=$?
    case $grc in
      2) record "$id" GATED "$label :: $GEN_MSG"; continue ;;
      1) record "$id" FAIL  "$label :: generation failed: $GEN_MSG"; continue ;;
    esac
    local brc=0
    build_row "$id" "$GEN_DIR" "$target" && brc=0 || brc=$?
    if [ $brc -ne 0 ]; then
      record "$id" FAIL "$label :: build failed: $BUILD_ERR (log: $BUILD_LOG)"; continue
    fi

    # NOTE: called directly, NOT in $( ) -- these helpers own the terminal
    # (they print the physical steps and stream the flasher's output).
    local rc=0
    FLASH_MSG=""
    case "$chip" in
      *esp32*)         flash_esp  "$id" "$chip" "$BUILD_ELF" "$GEN_DIR" && rc=0 || rc=$? ;;
      *rp2040|*rp2350) flash_rp   "$id" "$chip" "$BUILD_ELF"            && rc=0 || rc=$? ;;
      *nrf52840*)      flash_nrf  "$id" "$chip" "$BUILD_ELF"            && rc=0 || rc=$? ;;
      *samd21*)        flash_samd "$id" "$chip" "$BUILD_ELF"            && rc=0 || rc=$? ;;
      *)               FLASH_MSG="no flashing route implemented for $chip"; rc=3 ;;
    esac
    case $rc in
      0) record "$id" PASS    "$label :: FLASH REPORTED OK (not verified as running) -- $FLASH_MSG" ;;
      3) record "$id" SKIP    "$label :: $FLASH_MSG" ;;
      4) record "$id" BLOCKED "$label :: $FLASH_MSG" ;;
      *) record "$id" FAIL    "$label :: $FLASH_MSG" ;;
    esac
  done <"$ROWS"
}

# ---------------------------------------------------------------------------
# 6. go
# ---------------------------------------------------------------------------
START=$(date +%s)
if [ "$MODE" = build ]; then run_build_mode; else run_flash_mode; fi
END=$(date +%s)

# ---------------------------------------------------------------------------
# 7. summary
# ---------------------------------------------------------------------------
hdr "== summary =="
printf '  %-9s %s\n' "total"   "$TOTAL"
printf '  %s%-9s %s%s\n' "$C_GRN" "PASS"    "$PASS"    "$C_OFF"
printf '  %s%-9s %s%s\n' "$C_RED" "FAIL"    "$FAILED"  "$C_OFF"
printf '  %s%-9s %s%s\n' "$C_YEL" "SKIP"    "$SKIPPED" "$C_OFF"
printf '  %s%-9s %s%s\n' "$C_BLU" "GATED"   "$GATED"   "$C_OFF"
if [ "$BLOCKED" -gt 0 ]; then
  printf '  %s%-9s %s%s\n' "$C_YEL" "BLOCKED" "$BLOCKED" "$C_OFF"
fi
printf '  %-9s %ss\n' "elapsed" "$((END-START))"

if [ "$SKIPPED" -gt 0 ]; then
  hdr "-- skipped (NOT passes) --"
  awk -F'\t' '$2=="SKIP"{print "  " $1 "  " $3}' "$RESULTS"
fi
if [ "$FAILED" -gt 0 ]; then
  hdr "-- failures --"
  awk -F'\t' '$2=="FAIL"{print "  " $1 "  " $3}' "$RESULTS"
fi

hdr "-- artefacts --"
say "  row list:  $ROWS"
say "  results:   $RESULTS"
say "  logs:      $WORK/logs"
say "  target dirs (safe to delete): $WORK/targets"

[ "$FAILED" -eq 0 ] || exit 1
exit 0
