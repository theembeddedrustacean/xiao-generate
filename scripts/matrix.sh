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
# tool's own `--list-chips` / `--list-stacks` / `--list-templates` output, plus
# two read-only probes that make xiao-generate print its own catalogs
# (`--flashing __probe__` -> "Available: espflash, probe-rs"). Add a variant,
# a stack, a template or a catalog entry to src/registry.rs and this script
# picks it up with no edit.
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
HAVE_PICOTOOL=0;      have picotool                           && HAVE_PICOTOOL=1
HAVE_ELF2UF2=0;       have elf2uf2-rs                         && HAVE_ELF2UF2=1
HAVE_HF2=0;           have hf2                                && HAVE_HF2=1
HAVE_OBJCOPY=0;       have rust-objcopy                       && HAVE_OBJCOPY=1
HAVE_UF2CONV=0;       have uf2conv.py                         && HAVE_UF2CONV=1
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
_tool_line "picotool (RP2040/RP2350 flash)" "$HAVE_PICOTOOL"     "brew install picotool"
_tool_line "hf2 (SAMD21 flash)"             "$HAVE_HF2"          "cargo install hf2-cli"
_tool_line "rust-objcopy (ELF->bin)"        "$HAVE_OBJCOPY"      "cargo install cargo-binutils && rustup component add llvm-tools"
_tool_line "uf2conv.py (bin->UF2)"          "$HAVE_UF2CONV"      "curl -LO https://raw.githubusercontent.com/microsoft/uf2/master/utils/uf2conv.py && curl -LO https://raw.githubusercontent.com/microsoft/uf2/master/utils/uf2families.json  # then chmod +x and put both on PATH"

# ---------------------------------------------------------------------------
# 2. harvest the registry from the tool's own output
# ---------------------------------------------------------------------------
CHIPS=$(xg --list-chips | awk '/^  xiao-/ {print $1}')
[ -n "$CHIPS" ] || { say "${C_RED}error: --list-chips produced no variants${C_OFF}"; exit 1; }

if [ -n "$ONLY_CHIP" ]; then
  if ! printf '%s\n' "$CHIPS" | grep -qx -- "$ONLY_CHIP"; then
    say "${C_RED}error: unknown chip '$ONLY_CHIP'${C_OFF}"
    say "known chips:"; printf '  %s\n' $CHIPS
    exit 2
  fi
  CHIPS=$ONLY_CHIP
fi

# stacks_of <chip> -> lines: "<stack>\t<target>\t<fw1,fw2,...>"
# frameworks = the stack's whole support list (available ones from the
# "frameworks:" line UNION the ones listed as "<fw>: [gap] ..."). Empty when
# FRAMEWORK is not a live axis for that stack (the esp-idf std stack).
stacks_of() {
  xg --list-stacks --chip "$1" | awk '
    /^  [^ ]/                 { if (id!="") emit(); id=$1; tgt=""; fws=""; next }
    /^ +target: /             { tgt=$2; next }
    /^ +frameworks: /         { s=$0; sub(/^ +frameworks: /,"",s); gsub(/, /,",",s); fws=s; next }
    /^ +[a-z0-9_-]+: \[gap\]/ { f=$1; sub(/:$/,"",f); fws = (fws=="" ? f : fws "," f); next }
    END { if (id!="") emit() }
    function emit() { printf "%s\t%s\t%s\n", id, tgt, fws }
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

# The universal FLASHING / LOGGING catalogs, straight out of the tool: feeding
# it an impossible value makes it print the whole catalog. Harvested from a
# stack where the axis is live (an ESP no_std stack); the same probe tells us
# whether the axis is live at all for a given stack.
# probe_catalog <chip> <stack> <flag>  -> catalog csv, or "" if not a live axis
probe_catalog() {
  local out
  # NOTE: --out must be a *valid crate name*, because Config::resolve validates
  # the output dir's final component BEFORE it ever looks at the toolchain
  # catalogs -- a dotfile path here would make the probe report the wrong error.
  # Nothing is ever created: the probe always exits 2 at config time.
  out=$(xg --headless --chip "$1" --stack "$2" --"$3" __xg_probe__ \
           --out "$WORK/probe_never_created" 2>&1 >/dev/null) || true
  case "$out" in
    *"unknown --$3 value"*)
      printf '%s\n' "$out" | sed -e 's/.*Available: //' -e 's/, /,/g' ;;
    *) printf '' ;;
  esac
}

# ---------------------------------------------------------------------------
# 3. enumerate the matrix
# ---------------------------------------------------------------------------
# row TSV: id  chip  stack  target  template  uferris  extra-args  note
ROW_N=0
emit_row() { # chip stack target template uferris extra note
  ROW_N=$((ROW_N + 1))
  printf 'row%03d\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$ROW_N" "$1" "$2" "$3" "$4" "$5" "$6" "$7" >>"$ROWS"
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

hdr "-- enumerating the matrix --"
for chip in $CHIPS; do
  stacks_of "$chip" | while IFS=$'\t' read -r stack target fws; do
    [ -n "$stack" ] || continue

    is_std=0
    case "$target" in *-espidf) is_std=1 ;; esac
    if [ $is_std = 1 ] && [ $INCLUDE_STD = 0 ]; then
      continue
    fi

    fl_cat=$(probe_catalog "$chip" "$stack" flashing)
    lg_cat=$(probe_catalog "$chip" "$stack" logging)

    # framework values for this stack (support list, gaps included)
    fw_list=$(printf '%s' "$fws" | tr ',' ' ')
    [ -n "$fw_list" ] || fw_list="-"     # "-" = do not pass --framework

    templates_of "$chip" "$stack" | while IFS=$'\t' read -r tmpl tstate; do
      [ -n "$tmpl" ] || continue
      note="-"
      [ "$tstate" = gap ] && note="template-gap"

      if [ -n "$fl_cat" ] && [ -n "$lg_cat" ]; then
        # esp-hal: FLASHING / LOGGING / FRAMEWORK are all live axes...
        if is_base_template "$tmpl" && [ $OPTION_SWEEP = 1 ]; then
          # ...but ONLY on a base project (model::option_groups_apply). Sweep
          # the full cross product; the tool gates the illegal cells itself
          # (e.g. probe-rs + log) with exit 2.
          for fl in $(printf '%s' "$fl_cat" | tr ',' ' '); do
            for lg in $(printf '%s' "$lg_cat" | tr ',' ' '); do
              for fw in $fw_list; do
                extra="--template $tmpl --flashing $fl --logging $lg"
                [ "$fw" != "-" ] && extra="$extra --framework $fw"
                emit_row "$chip" "$stack" "$target" "$tmpl" 0 "$extra" "$note"
              done
            done
          done
        elif is_base_template "$tmpl"; then
          for fw in $fw_list; do
            extra="--template $tmpl"
            [ "$fw" != "-" ] && extra="$extra --framework $fw"
            emit_row "$chip" "$stack" "$target" "$tmpl" 0 "$extra" "$note"
          done
        else
          # An onboard-component add-on template pins its own esp-generate
          # option set, so the three axes are locked and sweeping them would
          # build the byte-identical project N times. One row.
          emit_row "$chip" "$stack" "$target" "$tmpl" 0 "--template $tmpl" "$note"
        fi
      else
        # non-ESP scaffold (FRAMEWORK live) or esp-idf std (nothing live).
        for fw in $fw_list; do
          extra="--template $tmpl"
          [ "$fw" != "-" ] && extra="$extra --framework $fw"
          emit_row "$chip" "$stack" "$target" "$tmpl" 0 "$extra" "$note"
        done
      fi
    done
  done

  # uFerris rows are a mode, not a template -- one per uferris-capable stack.
  uf=$(uferris_of "$chip")
  if [ -n "$uf" ]; then
    ufnote="-"; [ "$uf" = gap ] && ufnote="uferris-gap"
    stacks_of "$chip" | while IFS=$'\t' read -r stack target fws; do
      [ -n "$stack" ] || continue
      case "$target" in *-espidf) continue ;; esac   # uFerris is esp-hal only
      emit_row "$chip" "$stack" "$target" "blinky(uferris)" 1 "--uferris" "$ufnote"
    done
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
  awk -F'\t' '{printf "  %-7s %-21s %-11s %-16s %s\n", $1,$2,$3,$5,$7}' "$ROWS"
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
  while IFS=$'\t' read -r id chip stack target tmpl uferris extra note; do
    n=$((n+1))
    local label
    label=$(printf '[%d/%d] %-21s %-11s %s' "$n" "$TOTAL" "$chip" "$stack" "$(printf '%s' "$extra" | sed 's/--template //;s/--flashing /fl=/;s/--logging /lg=/;s/--framework /fw=/')")

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
  if [ $HAVE_PICOTOOL = 0 ]; then
    if [ $HAVE_ELF2UF2 = 1 ] && [ "$vol" = RPI-RP2 ]; then
      pause_for "Hold the BOOT ('B') button on the $chip and plug in USB-C.
  (Already plugged in? hold B, tap RESET, release B.)
  A volume named '$vol' must appear in Finder."
      wait_for_volume "$vol" || { FLASH_MSG="/Volumes/$vol never appeared -- board is not in BOOTSEL mode"; return 1; }
      if run_flash "$WORK/logs/$id.flash.log" elf2uf2-rs deploy "$elf"; then
        FLASH_MSG="elf2uf2-rs deploy -> /Volumes/$vol reported success"; return 0
      fi
      FLASH_MSG="elf2uf2-rs deploy failed (see $WORK/logs/$id.flash.log)"; return 1
    fi
    FLASH_MSG='picotool not installed -- brew install picotool  (elf2uf2-rs is a fallback for RP2040 ONLY; its RP2350 output is known-broken, JoNil/elf2uf2-rs#38)'
    return 3
  fi
  pause_for "Hold the BOOT ('B') button on the $chip and plug in USB-C.
  (Already plugged in? hold B, tap RESET, release B.)
  A volume named '$vol' must appear in Finder."
  if ! wait_for_volume "$vol"; then
    FLASH_MSG="/Volumes/$vol never appeared -- board is not in BOOTSEL mode"; return 1
  fi
  # picotool reads the ELF directly. `-t elf` is REQUIRED: cargo's output file
  # has no extension and picotool otherwise infers the type from it.
  # -u skip-identical sectors, -v verify the write, -x reboot into the app.
  if run_flash "$WORK/logs/$id.flash.log" picotool load -u -v -x -t elf "$elf"; then
    FLASH_MSG='picotool load -t elf reported success (write verified by picotool -v)'; return 0
  fi
  FLASH_MSG="picotool load failed (see $WORK/logs/$id.flash.log)"; return 1
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
  if [ $HAVE_OBJCOPY = 0 ]; then
    FLASH_MSG='rust-objcopy not installed -- cargo install cargo-binutils && rustup component add llvm-tools'; return 3
  fi
  if [ $HAVE_UF2CONV = 0 ]; then
    FLASH_MSG='uf2conv.py not installed -- fetch utils/uf2conv.py AND utils/uf2families.json from github.com/microsoft/uf2 and put both on PATH (uf2conv.py needs the json next to it)'; return 3
  fi
  local bin="$WORK/projects/$id.bin" uf2="$WORK/projects/$id.uf2"
  rust-objcopy -O binary "$elf" "$bin" || { FLASH_MSG='rust-objcopy -O binary failed'; return 1; }
  # Upper bound of the same invariant. The .bin is the exact byte range that
  # gets written, based at 0x27000, so its size is the honest measure of how
  # far the image reaches -- more so than any single section address.
  # ghostfat.c drops blocks at or above USER_FLASH_END rather than failing the
  # write, so an oversized image would report success with its tail missing.
  local binsz; binsz=$(wc -c < "$bin" | tr -d ' ')
  local binend=$(( 0x27000 + binsz ))
  if [ "$binend" -gt $((0xEA000)) ]; then
    FLASH_MSG=$(printf 'UNSAFE, refusing: image is %s bytes, reaching 0x%X -- past USER_FLASH_END (0xEA000). The Adafruit UF2 bootloader silently DROPS blocks at or above that address (ghostfat.c in_app_space), so this would flash "successfully" with its tail missing. The application window is 0x27000..0xEA000 = 780 KiB' "$binsz" "$binend")
    return 4
  fi
  uf2conv.py "$bin" -c -b 0x27000 -f 0xADA52840 -o "$uf2" || { FLASH_MSG='uf2conv.py failed'; return 1; }
  pause_for "Double-tap the RESET button on the $chip (two taps within ~500 ms).
  A volume named '$vol' must appear in Finder."
  wait_for_volume "$vol" || { FLASH_MSG="/Volumes/$vol never appeared -- board did not enter the UF2 bootloader"; return 1; }
  # `cp -X` skips extended attributes: a plain `cp` to the FAT bootloader volume
  # fails its xattr step on macOS and would false-FAIL an otherwise good write.
  # The board also reboots the instant the last UF2 block lands, so cp can lose
  # the volume mid-copy -- either way cp's exit status is unreliable here. The
  # authoritative, probe-free success signal is the volume UNMOUNTING (checked
  # below), so a copy error is deliberately not fatal on its own.
  cp -X "$uf2" "/Volumes/$vol/" 2>/dev/null || true
  sync
  # The UF2 bootloader reboots into the app as soon as the write completes, so
  # the volume disappearing is the only success signal available without a probe.
  local i=0
  while [ $i -lt 15 ]; do [ -d "/Volumes/$vol" ] || break; sleep 1; i=$((i+1)); done
  if [ -d "/Volumes/$vol" ]; then
    FLASH_MSG="UF2 copied but /Volumes/$vol did not unmount -- the bootloader may have rejected the image"; return 1
  fi
  FLASH_MSG="UF2 (family 0xADA52840, base 0x27000) written; /Volumes/$vol unmounted"; return 0
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
  if [ $HAVE_OBJCOPY = 0 ] || [ $HAVE_UF2CONV = 0 ]; then
    FLASH_MSG='no SAMD21 flasher -- cargo install hf2-cli  (or: cargo install cargo-binutils && rustup component add llvm-tools, plus uf2conv.py + uf2families.json from github.com/microsoft/uf2)'; return 3
  fi
  local bin="$WORK/projects/$id.bin" uf2="$WORK/projects/$id.uf2"
  rust-objcopy -O binary "$elf" "$bin" || { FLASH_MSG='rust-objcopy -O binary failed'; return 1; }
  uf2conv.py "$bin" -c -b 0x2000 -f 0x68ed2b88 -o "$uf2" || { FLASH_MSG='uf2conv.py failed'; return 1; }
  pause_for "Short the two RST pads on the $chip twice quickly. A volume named '$vol' appears."
  wait_for_volume "$vol" || { FLASH_MSG="/Volumes/$vol never appeared"; return 1; }
  # cp -X: skip extended attributes, which a plain cp cannot write to the FAT
  # bootloader volume on macOS (it would report failure for an otherwise good write).
  cp -X "$uf2" "/Volumes/$vol/" 2>/dev/null || true
  sync
  local i=0
  while [ $i -lt 15 ]; do [ -d "/Volumes/$vol" ] || break; sleep 1; i=$((i+1)); done
  FLASH_MSG="UF2 (family 0x68ed2b88, base 0x2000) written to /Volumes/$vol"; return 0
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
  while IFS=$'\t' read -r id chip stack target tmpl uferris extra note; do
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
