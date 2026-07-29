//! Interactive Ratatui front-end (M2, redesigned v3 — UNIFY) — a **single-page
//! form**, not a wizard. Every choice is visible on one screen at once; entries
//! that a device/stack doesn't support **grey out** with a reason. Generation is
//! triggered from the same page.
//!
//! v3 framing: xiao-generate is a **unifying platform**. Flashing / Logging /
//! Framework are **universal, device-agnostic toolchain categories** — the full
//! catalog is shown for every device, and a stack's registry support list decides
//! what is selectable vs greyed. esp-generate is an *implementation detail of one
//! backend* and never appears in the UI or summary (no "ESP-GENERATE OPTIONS"
//! heading, no `-o` flags, no "esp opts:" line).
//!
//! Layout (top to bottom):
//!   TARGET          Device (cycler) + Stack (radio list)
//!   FLASHING TOOL   radio list — full catalog, unsupported greyed
//!   LOGGING         radio list
//!   FRAMEWORK       radio list
//!   BSP SUPPORT     "Enable µFerris BSP" toggle + power/async (greyed until
//!                   enabled) + a dim note about future template selection
//!
//! The Component add-on row and the whole EXTRAS section (extra crates + output
//! dir) are intentionally NOT in the TUI (v3): they remain headless-only CLI
//! power-user features (`--template` / `--add-crate` / `--out`). See NOTES.md for
//! the deliberate TUI-vs-CLI split.
//!
//! Two themes (`dark` default, `light`), toggled live with `t`. Dark = a genuinely
//! dark BACKGROUND (block/paragraph `Style::bg`), not just dark text. The fixed
//! convention: active/selected = BRIGHT (accent/bold on the themed bg); disabled =
//! DIM. Every focusable row is its own line; Space/→ selects/toggles it.
//!
//! The form builds the same `model::RawConfig` the headless CLI does, resolves it
//! through `Config::resolve_for_tui` (shared validation, never duplicated — R4/M2),
//! and hands a resolved `Config` back to `main` for generation on the one shared
//! generation path. All grey-out decisions derive from registry data via the pure
//! helpers in `model` (`category_live` / `category_grey_reason` for the section
//! headers, `choice_grey_reason` for individual rows) — no fabricated data, no
//! per-variant UI logic. Section greying is PER-CATEGORY: on the non-ESP scaffold
//! FRAMEWORK is a live axis while flashing and logging are display-only.
//!
//! The draw functions are pure over `(&App, &Theme)`, so the whole page is
//! rendered headless into a `TestBackend` buffer in the tests below.

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::model::{self, Config, RawConfig, Readiness};
use crate::registry::{self, Stack, ToolCategory, Variant};

/// What an interactive session produced.
pub enum Outcome {
    /// User quit without confirming.
    Cancelled,
    /// A valid but not-yet-implemented (non-ESP) selection was confirmed: show
    /// this message, generate nothing.
    Gated(String),
    /// A ready selection was confirmed: the caller runs scaffold + patch + summary.
    Generate(Config),
}

// ---------------------------------------------------------------------------
// Theme. Every colour the TUI paints lives here; two instances. Convention:
// brighter = more active. Dark theme = a real dark BACKGROUND (`bg`), light
// foregrounds, accent for the selected/focused row; light theme = light bg.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Theme {
    name: &'static str,
    /// Panel background — painted across configure/summary/footer/header.
    bg: Color,
    /// Base foreground for otherwise-unstyled cells.
    fg: Color,
    heading: Color,
    /// Selected radio/toggle marker + selected value (the "on" colour).
    accent: Color,
    /// Focused-row label (the strongest foreground).
    active: Color,
    /// Normal, enabled (but not focused/selected) text.
    enabled: Color,
    /// Disabled / greyed / ineligible — recessive.
    dim: Color,
    ok: Color,
    warn: Color,
    err: Color,
    border: Color,
    summary_key: Color,
    summary_val: Color,
    header_bg: Color,
    header_fg: Color,
    footer_fg: Color,
}

impl Theme {
    /// Dark theme — a Rust/Ferris palette (the "crab effect"): warm near-black
    /// background, Ferris coral-salmon for the selection/active highlight, warm
    /// rust-orange headings/titles, muted-rust borders. Semantic ok/warn/err stay
    /// green/amber/red so READY / gate / error states remain unambiguous.
    fn dark() -> Theme {
        Theme {
            name: "dark",
            bg: Color::Rgb(24, 20, 22),
            fg: Color::Rgb(224, 216, 212),
            heading: Color::Rgb(214, 90, 58),
            accent: Color::Rgb(247, 118, 82),
            active: Color::Rgb(255, 236, 228),
            enabled: Color::Rgb(178, 168, 162),
            dim: Color::Rgb(120, 104, 100),
            ok: Color::Rgb(122, 200, 130),
            warn: Color::Rgb(232, 192, 96),
            err: Color::Rgb(240, 120, 110),
            border: Color::Rgb(150, 60, 40),
            summary_key: Color::Rgb(168, 150, 144),
            summary_val: Color::Rgb(238, 230, 226),
            header_bg: Color::Rgb(247, 118, 82),
            header_fg: Color::Rgb(28, 18, 16),
            footer_fg: Color::Rgb(168, 150, 144),
        }
    }

    /// Light theme: light background, saturated darker inks that stay readable
    /// (no near-white on white).
    fn light() -> Theme {
        Theme {
            name: "light",
            bg: Color::Rgb(247, 247, 250),
            fg: Color::Rgb(32, 32, 38),
            heading: Color::Rgb(150, 30, 140),
            accent: Color::Rgb(0, 92, 190),
            active: Color::Rgb(0, 0, 0),
            enabled: Color::Rgb(55, 55, 62),
            dim: Color::Rgb(162, 162, 172),
            ok: Color::Rgb(0, 130, 45),
            warn: Color::Rgb(175, 95, 0),
            err: Color::Rgb(190, 0, 35),
            border: Color::Rgb(0, 92, 190),
            summary_key: Color::Rgb(92, 92, 100),
            summary_val: Color::Rgb(0, 0, 0),
            header_bg: Color::Rgb(0, 92, 190),
            header_fg: Color::Rgb(255, 255, 255),
            footer_fg: Color::Rgb(70, 70, 78),
        }
    }

    /// Base style for a whole panel (fills the themed background).
    fn base(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    fn hint_style(&self) -> Style {
        Style::default().fg(self.dim)
    }
}

// ---------------------------------------------------------------------------
// Focus model. Every catalog entry / stack / toggle is its own focusable row
// (spec: "every catalog entry is its own focusable row"). We store the focused
// row by identity and recompute the ordered list of focusable rows each frame,
// so rows appearing/disappearing (µFerris power/async, greyed choices) never
// leave focus stranded.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row {
    Device,
    Stack(usize),
    Flashing(usize),
    Logging(usize),
    Framework(usize),
    Uferris,
    PowerBoard,
    Async,
}

struct App {
    variant_idx: usize,
    stack_idx: usize,
    /// Selected index into each universal catalog (`FLASHING_TOOLS` / `LOGGING` /
    /// `FRAMEWORKS`), NOT into a per-stack subset.
    flashing_idx: usize,
    logging_idx: usize,
    framework_idx: usize,
    uferris_on: bool,
    power_board_on: bool,
    /// µFerris async (embassy). Default ON — the only verified path. Off is a
    /// documented gap (resolution refuses it; summary shows the refusal).
    async_on: bool,
    /// Project (crate) name, passed in from the CLI `--name` flag (required before
    /// the TUI launches). The name is a CLI concern, not an editable TUI field —
    /// it is fed into `raw_config` and shown, read-only, on the summary.
    name: String,
    /// The focused row, by identity.
    focus: Row,
    /// `true` = dark theme (default), `false` = light. Toggled with `t`.
    dark: bool,
    /// Set when the session finishes; consumed by `launch`.
    outcome: Option<Outcome>,
}

/// The catalog index of a stack's default choice for a category (or 0 when the
/// stack lists no support of its own — a greyed section still needs a marker).
fn default_cat_idx(stack: &Stack, cat: ToolCategory) -> usize {
    match stack.default_tool(cat) {
        Some(c) => cat.catalog().iter().position(|x| x.id == c.id).unwrap_or(0),
        None => 0,
    }
}

impl App {
    fn new(name: String) -> App {
        let mut app = App {
            variant_idx: 0,
            stack_idx: 0,
            flashing_idx: 0,
            logging_idx: 0,
            framework_idx: 0,
            uferris_on: false,
            power_board_on: false,
            async_on: true,
            name,
            focus: Row::Device,
            dark: true,
            outcome: None,
        };
        app.reset_cats();
        app
    }

    fn theme(&self) -> Theme {
        if self.dark {
            Theme::dark()
        } else {
            Theme::light()
        }
    }

    fn variant(&self) -> &'static Variant {
        &registry::VARIANTS[self.variant_idx]
    }

    fn current_stack(&self) -> &'static Stack {
        &self.variant().stacks[self.stack_idx]
    }

    fn uferris_enabled(&self) -> bool {
        model::uferris_available(self.variant(), self.current_stack())
    }

    /// Whether ONE toolchain section is a live, editable axis for the current
    /// stack. Per-category, not whole-stack: on the non-ESP scaffold FRAMEWORK is
    /// live (embassy is a real, generatable choice on the embassy HALs) while
    /// flashing and logging are display-only. esp-hal: all three live; esp-idf:
    /// none. Derived from `model::category_live` — the TUI fabricates nothing.
    fn cat_live(&self, cat: ToolCategory) -> bool {
        model::category_live(self.current_stack(), cat)
    }

    fn cat_idx(&self, cat: ToolCategory) -> usize {
        match cat {
            ToolCategory::Flashing => self.flashing_idx,
            ToolCategory::Logging => self.logging_idx,
            ToolCategory::Framework => self.framework_idx,
        }
    }

    fn set_cat_idx(&mut self, cat: ToolCategory, idx: usize) {
        match cat {
            ToolCategory::Flashing => self.flashing_idx = idx,
            ToolCategory::Logging => self.logging_idx = idx,
            ToolCategory::Framework => self.framework_idx = idx,
        }
    }

    /// Union of the OTHER two categories' currently-selected internal esp flags
    /// (drives the per-choice grey-out — e.g. probe-rs on flashing greys `log`).
    fn sibling_esp_opts(&self, skip: ToolCategory) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        for cat in ToolCategory::ALL {
            if cat == skip {
                continue;
            }
            for o in cat.catalog()[self.cat_idx(cat)].esp_options {
                if !v.contains(o) {
                    v.push(*o);
                }
            }
        }
        v
    }

    /// `Some(reason)` if a catalog choice is greyed for the current selection.
    fn choice_grey(&self, cat: ToolCategory, idx: usize) -> Option<String> {
        let choice = &cat.catalog()[idx];
        model::choice_grey_reason(
            self.variant(),
            self.current_stack(),
            cat,
            choice,
            &self.sibling_esp_opts(cat),
        )
    }

    /// Reset each category to its stack default (used on variant/stack change).
    fn reset_cats(&mut self) {
        let s = self.current_stack();
        self.flashing_idx = default_cat_idx(s, ToolCategory::Flashing);
        self.logging_idx = default_cat_idx(s, ToolCategory::Logging);
        self.framework_idx = default_cat_idx(s, ToolCategory::Framework);
    }

    /// After any change, snap a category whose current choice became greyed back
    /// to an eligible one (prefer the stack default). Only meaningful on the live
    /// esp-hal axis; on greyed sections `choice_grey` for the default is `None`.
    fn normalize_cats(&mut self) {
        for cat in ToolCategory::ALL {
            if self.choice_grey(cat, self.cat_idx(cat)).is_none() {
                continue;
            }
            let def = default_cat_idx(self.current_stack(), cat);
            let mut chosen = def;
            if self.choice_grey(cat, def).is_some() {
                for j in 0..cat.catalog().len() {
                    if self.choice_grey(cat, j).is_none() {
                        chosen = j;
                        break;
                    }
                }
            }
            self.set_cat_idx(cat, chosen);
        }
    }

    /// The ordered list of focusable rows for the current state.
    fn focusable_rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Device];
        for i in 0..self.variant().stacks.len() {
            rows.push(Row::Stack(i));
        }
        // A toolchain section contributes focusable rows only when THAT category is
        // a live axis for the stack (esp-hal: all three; non-ESP: FRAMEWORK only;
        // esp-idf: none) AND µFerris is off — µFerris supplies its own toolchain, so
        // those picks don't apply and all three sections go inactive (dimmed, no
        // selection). `ToolCategory::ALL` is flashing, logging, framework, so the
        // row order matches the on-screen order.
        if !self.uferris_on {
            for cat in ToolCategory::ALL {
                if !self.cat_live(cat) {
                    continue;
                }
                for i in 0..cat.catalog().len() {
                    if self.choice_grey(cat, i).is_none() {
                        rows.push(cat_row(cat, i));
                    }
                }
            }
        }
        if self.uferris_enabled() {
            rows.push(Row::Uferris);
        }
        if self.uferris_on {
            rows.push(Row::PowerBoard);
            rows.push(Row::Async);
        }
        rows
    }

    fn ensure_focus_valid(&mut self) {
        let rows = self.focusable_rows();
        if !rows.iter().any(|r| *r == self.focus) {
            self.focus = rows.first().copied().unwrap_or(Row::Device);
        }
    }

    // ---- navigation / mutation ----

    fn focus_move(&mut self, delta: isize) {
        let rows = self.focusable_rows();
        if rows.is_empty() {
            return;
        }
        let cur = rows.iter().position(|r| *r == self.focus).unwrap_or(0);
        let next = cycle(cur, delta, rows.len());
        self.focus = rows[next];
    }

    fn move_variant(&mut self, delta: isize) {
        self.variant_idx = cycle(self.variant_idx, delta, registry::VARIANTS.len());
        self.stack_idx = 0;
        self.uferris_on = false;
        self.power_board_on = false;
        self.async_on = true;
        self.reset_cats();
        self.normalize_cats();
        self.ensure_focus_valid();
    }

    fn select_stack(&mut self, i: usize) {
        self.stack_idx = i;
        self.reset_cats();
        if !self.uferris_enabled() {
            self.uferris_on = false;
            self.power_board_on = false;
        }
        self.normalize_cats();
        self.ensure_focus_valid();
    }

    fn select_cat(&mut self, cat: ToolCategory, idx: usize) {
        if self.choice_grey(cat, idx).is_some() {
            return;
        }
        self.set_cat_idx(cat, idx);
        // Selecting one choice can shift the eligibility of the others.
        self.normalize_cats();
        self.ensure_focus_valid();
    }

    fn toggle_uferris(&mut self) {
        self.uferris_on = !self.uferris_on;
        if self.uferris_on {
            self.async_on = true;
        } else {
            self.power_board_on = false;
            // µFerris off: the toolchain sections become a live, editable axis
            // again — restore their normal per-stack defaults (espflash / none /
            // none), discarding any pick the user had made before enabling µFerris.
            self.reset_cats();
            self.normalize_cats();
        }
        self.ensure_focus_valid();
    }

    fn toggle_theme(&mut self) {
        self.dark = !self.dark;
    }

    /// Space / → / l — select the focused radio row or flip the focused toggle.
    fn activate(&mut self) {
        match self.focus {
            Row::Device => self.move_variant(1),
            Row::Stack(i) => self.select_stack(i),
            Row::Flashing(i) => self.select_cat(ToolCategory::Flashing, i),
            Row::Logging(i) => self.select_cat(ToolCategory::Logging, i),
            Row::Framework(i) => self.select_cat(ToolCategory::Framework, i),
            Row::Uferris => self.toggle_uferris(),
            Row::PowerBoard => self.power_board_on = !self.power_board_on,
            Row::Async => self.async_on = !self.async_on,
        }
    }

    /// ← / h — only the Device cycler steps backward (radios select via activate).
    fn back(&mut self) {
        if self.focus == Row::Device {
            self.move_variant(-1);
        }
    }

    /// Build the same `RawConfig` the CLI would from these selections. The TUI
    /// never sets a component template, extra crates, or a custom output dir
    /// (headless-only in v3), so those are always defaults here. The project name
    /// is the required `--name` passed in from the CLI (not a TUI field).
    fn raw_config(&self) -> RawConfig {
        // Forward a toolchain id only for the categories that are a LIVE axis on
        // this stack — `Config::resolve` hard-rejects a value on a non-live
        // category ("--flashing does not apply to the 'embassy-rp' stack"). So this
        // must stay per-category: on the non-ESP scaffold FRAMEWORK is forwarded
        // (that is what makes `embassy` generatable from the TUI) while flashing and
        // logging MUST resolve to `None`, or non-ESP generation breaks outright.
        let forward = |cat: ToolCategory| -> Option<String> {
            self.cat_live(cat)
                .then(|| cat.catalog()[self.cat_idx(cat)].id.to_string())
        };
        let (flashing, logging, framework) = (
            forward(ToolCategory::Flashing),
            forward(ToolCategory::Logging),
            forward(ToolCategory::Framework),
        );
        RawConfig {
            chip: self.variant().id.to_string(),
            stack: Some(self.current_stack().id.to_string()),
            template: None,
            name: Some(self.name.clone()),
            out_dir: None,
            uferris: self.uferris_on,
            power_board: self.power_board_on,
            no_async: self.uferris_on && !self.async_on,
            flashing,
            logging,
            framework,
            extra_crates: Vec::new(),
        }
    }

    fn resolved(&self) -> Result<(Config, Readiness), String> {
        Config::resolve_for_tui(self.raw_config())
    }

    fn cancel(&mut self) -> bool {
        self.outcome = Some(Outcome::Cancelled);
        true
    }

    fn try_generate(&mut self) -> bool {
        match self.resolved() {
            Ok((cfg, Readiness::Ready)) => {
                self.outcome = Some(Outcome::Generate(cfg));
                true
            }
            Ok((_, Readiness::Gated(msg))) => {
                self.outcome = Some(Outcome::Gated(msg));
                true
            }
            Err(_) => false,
        }
    }

    /// Handle a key. Returns `true` when the session should end (`outcome` set).
    fn on_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => return self.cancel(),
            KeyCode::Enter => return self.try_generate(),
            KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => self.focus_move(1),
            KeyCode::BackTab | KeyCode::Up | KeyCode::Char('k') => self.focus_move(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => self.activate(),
            KeyCode::Left | KeyCode::Char('h') => self.back(),
            KeyCode::Char('q') => return self.cancel(),
            KeyCode::Char('t') => self.toggle_theme(),
            _ => {}
        }
        false
    }
}

/// Wrapping index move: cycles inside `[0, len)`.
fn cycle(idx: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let len_i = len as isize;
    let next = (idx as isize + delta).rem_euclid(len_i);
    next as usize
}

// ---------------------------------------------------------------------------
// Rendering (pure over (&App, &Theme) — exercised headlessly by the tests below).
// ---------------------------------------------------------------------------

/// Ferris (the Rust crab) — the Summary panel's centrepiece, rendered as COLORED
/// QUADRANT-block terminal art. This is "V4-B": a 36×11 grid produced from the
/// official CC0 logo (rustacean.net, `flat-happy`) and embedded here VERBATIM. Unlike
/// the earlier half-block art, every cell carries its OWN glyph — one of the quadrant
/// blocks (U+2596..U+259F plus `▀▄▌▐█` and space), packing 2×2 subpixels per cell —
/// so the renderer draws `FERRIS_GLYPH[r][c]` rather than a fixed `▀`/`▄`.
/// Three parallel tables describe each cell:
///   `FERRIS_GLYPH` the character to draw
///   `FERRIS_FG`    the glyph ink   (`t` in the source data)
///   `FERRIS_BG`    the cell behind (`b` in the source data)
/// Colour codes: `.` transparent (the themed panel background shows through),
/// `o` body orange, `d` dark-red shading, `k` black (eyes/smile), `w` white (glints).
/// The block's LEADING blanks are significant, so it is centred as a unit with a
/// uniform left-pad and never re-padded per line. Drawn ONLY when the panel interior
/// can hold it (≥ `FERRIS_W` wide AND ≥ `FERRIS_H` spare rows below the summary text —
/// see `draw_summary`); on a narrow/short panel it is omitted entirely rather than
/// wrapped or clipped.
const FERRIS_W: usize = 36;
const FERRIS_H: usize = 11;

/// Per-cell glyphs, embedded verbatim from the V4-B data.
const FERRIS_GLYPH: [&str; FERRIS_H] = [
    "            ▀▖▘█▀██▀█▝▗▀            ",
    "          ▝▀████████████▀▘          ",
    "▘▖▌█▀   ▝▀████████████████▀▘   ▀█▐▗▝",
    "██▘██▐▗▘████████████████████▝▖▌██▝██",
    "▌████▘▘██████████████████████▝▝████▐",
    " ▝▖█▝▖█████████▀▖▖██▀▖▖███████▗▘█▗▘ ",
    "    ▖██████████▄▘███▄▘█████████▗    ",
    "   ▌█▗▀▀▀██████▝▄▘██▝▄▘████▀▀▀▖█▐   ",
    "   ▝█▝▄▖▄▄▄▝▀▀▀▀▀▝▀▚▀▀▀▀▘▄▄▄▗▄▘█▘   ",
    "    ▝█▖▖                    ▗▗█▘    ",
    "      ▖                      ▗      ",
];

/// Per-cell glyph-ink colour codes, embedded verbatim from the V4-B data.
const FERRIS_FG: [&str; FERRIS_H] = [
    ".............o.o.oo.o.o.............",
    "............oooooooooooo............",
    ".o.o......oooooooooooooooo......o.o.",
    "oo.oo.o.oooooooooooooooooooo.o.oo.oo",
    ".ooooo.oooooooooooooooooooooo.ooooo.",
    ".o.o..oooooooooowkooowkooooooo..o.o.",
    ".....ooooooooookwkookwkoooooooo.....",
    "....odoooooooookokookokooooooodo....",
    "...oo..d..oooooookodoooooo..d..oo...",
    "....ooo......................ooo....",
    "....................................",
];

/// Per-cell background colour codes, embedded verbatim from the V4-B data.
const FERRIS_BG: [&str; FERRIS_H] = [
    "............o.o.o..o.o.o............",
    "..........oo............oo..........",
    "o.o.o...oo................oo...o.o.o",
    "..o..o.o....................o.o..o..",
    "o.....o......................o.....o",
    "..o.oo.........wko..wko.......oo.o..",
    "....o..........wk...wk.........o....",
    "...o.oddd......oko..oko....dddo.o...",
    ".....od.ddddddddddkkdddddddd.do.....",
    ".......o....................o.......",
    "......o......................o......",
];

/// The crab's four inks — FIXED truecolor in BOTH themes. Unlike every other widget
/// they do NOT follow `theme.accent` (salmon in dark, blue in light); pinning them
/// keeps the mascot's identity stable across a `t` theme toggle. Only the empty
/// space around/inside the crab (transparent cells) tracks the theme.
const FERRIS_ORANGE: Color = Color::Rgb(247, 76, 0);
const FERRIS_DARK: Color = Color::Rgb(163, 40, 0);
const FERRIS_BLACK: Color = Color::Rgb(0, 0, 0);
const FERRIS_WHITE: Color = Color::Rgb(255, 255, 255);

/// Ink for a colour code, or `None` for transparent (`.`).
fn ferris_ink(code: u8) -> Option<Color> {
    match code {
        b'o' => Some(FERRIS_ORANGE),
        b'd' => Some(FERRIS_DARK),
        b'k' => Some(FERRIS_BLACK),
        b'w' => Some(FERRIS_WHITE),
        _ => None,
    }
}

/// The quadrant-block glyph covering exactly the subpixels `g` does NOT cover.
/// Used to express a cell whose GLYPH INK is transparent but whose BACKGROUND is
/// coloured: painting `complement(g)` in that colour leaves the glyph's own
/// subpixels untouched, which is visually identical to an unset background but keeps
/// the crab theme-independent (an unset foreground would inherit `theme.fg` and paint
/// a stray themed smudge). Space and `█` are each other's complement.
fn ferris_complement(g: char) -> char {
    match g {
        ' ' => '█',
        '█' => ' ',
        '▀' => '▄',
        '▄' => '▀',
        '▌' => '▐',
        '▐' => '▌',
        '▘' => '▟',
        '▟' => '▘',
        '▝' => '▙',
        '▙' => '▝',
        '▖' => '▜',
        '▜' => '▖',
        '▗' => '▛',
        '▛' => '▗',
        '▚' => '▞',
        '▞' => '▚',
        other => other,
    }
}

/// One cell's glyph + style. The cell's own glyph is drawn with `fg` = its ink and
/// `bg` = its background; a transparent background is left UNSET so the themed panel
/// shows through. A transparent ink over a coloured background is drawn as the
/// complement glyph in that colour (see `ferris_complement`), and a fully transparent
/// cell is a plain space with no background at all.
fn ferris_cell(glyph: char, ink: Option<Color>, back: Option<Color>) -> (char, Style) {
    match (ink, back) {
        (None, None) => (' ', Style::default()),
        (Some(f), Some(b)) => (glyph, Style::default().fg(f).bg(b)),
        (Some(f), None) => (glyph, Style::default().fg(f)),
        (None, Some(b)) => (ferris_complement(glyph), Style::default().fg(b)),
    }
}

/// The crab as `FERRIS_H` styled `Line`s, each prefixed with `left_pad` spaces so the
/// block is centred as a unit. Consecutive cells that share a style are merged into a
/// single span so a row is a handful of spans rather than 36.
fn ferris_lines(left_pad: usize) -> Vec<Line<'static>> {
    let pad = " ".repeat(left_pad);
    let mut out = Vec::with_capacity(FERRIS_H);
    for r in 0..FERRIS_H {
        let ink = FERRIS_FG[r].as_bytes();
        let back = FERRIS_BG[r].as_bytes();
        let mut spans: Vec<Span<'static>> = Vec::new();
        if !pad.is_empty() {
            spans.push(Span::raw(pad.clone()));
        }
        let mut run = String::new();
        let mut run_style: Option<Style> = None;
        for (c, g) in FERRIS_GLYPH[r].chars().enumerate() {
            let (glyph, style) = ferris_cell(g, ferris_ink(ink[c]), ferris_ink(back[c]));
            if run_style == Some(style) {
                run.push(glyph);
            } else {
                if let Some(s) = run_style.take() {
                    spans.push(Span::styled(std::mem::take(&mut run), s));
                }
                run.push(glyph);
                run_style = Some(style);
            }
        }
        if let Some(s) = run_style {
            spans.push(Span::styled(run, s));
        }
        out.push(Line::from(spans));
    }
    out
}

/// Exact rendered height (rows) of `lines` laid out at `width` with the same
/// word-wrap the Summary panel uses. Rendered headlessly into a scratch buffer so
/// the crab can be positioned in the real leftover space beneath the text, rather
/// than guessed from logical line counts (which ignore wrapping of the equivalent
/// command / gate messages).
fn wrapped_height(lines: &[Line], width: u16) -> u16 {
    if width == 0 || lines.is_empty() {
        return 0;
    }
    let scratch_h: u16 = 256;
    let rect = Rect::new(0, 0, width, scratch_h);
    let mut buf = Buffer::empty(rect);
    Paragraph::new(lines.to_vec())
        .wrap(Wrap { trim: false })
        .render(rect, &mut buf);
    let mut height = 0u16;
    for y in 0..scratch_h {
        for x in 0..width {
            if let Some(cell) = buf.cell((x, y)) {
                let s = cell.symbol();
                if !s.is_empty() && s != " " {
                    height = y + 1;
                    break;
                }
            }
        }
    }
    height
}

fn draw(f: &mut Frame, app: &App) {
    let theme = app.theme();

    // Paint the whole frame with the themed background first so gaps between
    // panels are filled (dark theme = genuinely dark background, not just text).
    f.render_widget(Block::default().style(Style::default().bg(theme.bg)), f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)])
        .split(f.area());

    // Header.
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " xiao-generate ",
            Style::default()
                .fg(theme.header_fg)
                .bg(theme.header_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  one page · universal toolchain · live preview",
            Style::default().fg(theme.enabled),
        ),
    ]))
    .style(theme.base())
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border).bg(theme.bg))
            .style(Style::default().bg(theme.bg)),
    );
    f.render_widget(header, chunks[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(chunks[1]);
    draw_form(f, app, &theme, cols[0]);
    draw_summary(f, app, &theme, cols[1]);

    // Footer (keybindings + current theme + toggle hint).
    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Tab/↑↓", Style::default().fg(theme.accent)),
        Span::styled(" move  ", Style::default().fg(theme.footer_fg)),
        Span::styled("Space/→", Style::default().fg(theme.accent)),
        Span::styled(" select  ", Style::default().fg(theme.footer_fg)),
        Span::styled("t", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" theme:{}  ", theme.name), Style::default().fg(theme.footer_fg)),
        Span::styled("Enter", Style::default().fg(theme.ok)),
        Span::styled(" generate  ", Style::default().fg(theme.footer_fg)),
        Span::styled("Esc", Style::default().fg(theme.err)),
        Span::styled(" quit", Style::default().fg(theme.footer_fg)),
    ]))
    .style(theme.base())
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" keys ")
            .border_style(Style::default().fg(theme.border).bg(theme.bg))
            .style(Style::default().bg(theme.bg)),
    );
    f.render_widget(footer, chunks[2]);
}

fn heading(theme: &Theme, text: &str) -> Line<'static> {
    Line::styled(
        text.to_string(),
        Style::default().fg(theme.heading).add_modifier(Modifier::BOLD),
    )
}

/// The section heading for a toolchain category, with that category's disable
/// reason (if any) shown dim beside it.
fn section_heading(theme: &Theme, cat: ToolCategory, reason: Option<&str>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        cat.heading().to_string(),
        Style::default().fg(theme.heading).add_modifier(Modifier::BOLD),
    )];
    if let Some(r) = reason {
        spans.push(Span::styled(format!("   {r}"), theme.hint_style()));
    }
    Line::from(spans)
}

/// The Device cycler row (`Device   < XIAO … >   n/N`).
fn field_line(theme: &Theme, focused: bool, label: &str, value: String, hint: &str) -> Line<'static> {
    let caret = if focused {
        Span::styled("> ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("  ", Style::default().fg(theme.dim))
    };
    let lstyle = if focused {
        Style::default().fg(theme.active).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.enabled)
    };
    let mut vstyle = Style::default().fg(theme.accent).add_modifier(Modifier::BOLD);
    if focused {
        vstyle = vstyle.add_modifier(Modifier::REVERSED);
    }
    let mut spans = vec![
        caret,
        Span::styled(format!("{label:<10}"), lstyle),
        Span::styled(value, vstyle),
    ];
    if !hint.is_empty() {
        spans.push(Span::styled(format!("   {hint}"), theme.hint_style()));
    }
    Line::from(spans)
}

/// A single radio row: `(•)/( )/(-)  label   hint`. Brighter = more active.
/// `inactive` (µFerris supplies its own toolchain) renders an empty `( )` in the
/// dim colour with no selection — distinct from `greyed` (`(-)`, a per-choice
/// device/constraint exclusion).
fn radio_line(
    theme: &Theme,
    focused: bool,
    selected: bool,
    greyed: bool,
    inactive: bool,
    indent: &str,
    label: &str,
    hint: &str,
) -> Line<'static> {
    let caret = if focused {
        Span::styled("> ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("  ", Style::default().fg(theme.dim))
    };
    let (mark, mut style) = if inactive {
        ("( )", Style::default().fg(theme.dim))
    } else if greyed {
        ("(-)", Style::default().fg(theme.dim))
    } else if selected {
        ("(•)", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
    } else if focused {
        ("( )", Style::default().fg(theme.active).add_modifier(Modifier::BOLD))
    } else {
        ("( )", Style::default().fg(theme.enabled))
    };
    if focused && selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    let mut spans = vec![
        caret,
        Span::styled(indent.to_string(), Style::default().fg(theme.fg)),
        Span::styled(format!("{mark} {label}"), style),
    ];
    if !hint.is_empty() {
        spans.push(Span::styled(format!("   {hint}"), theme.hint_style()));
    }
    Line::from(spans)
}

/// A single toggle row: `[x]/[ ]  label   hint`.
fn toggle_line(
    theme: &Theme,
    focused: bool,
    enabled: bool,
    on: bool,
    indent: &str,
    label: &str,
    hint: &str,
) -> Line<'static> {
    let caret = if focused && enabled {
        Span::styled("> ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("  ", Style::default().fg(theme.dim))
    };
    let boxmark = if on { "[x]" } else { "[ ]" };
    let mut style = if !enabled {
        Style::default().fg(theme.dim)
    } else if on {
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
    } else if focused {
        Style::default().fg(theme.active).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.enabled)
    };
    if focused && enabled && on {
        style = style.add_modifier(Modifier::REVERSED);
    }
    let mut spans = vec![
        caret,
        Span::styled(indent.to_string(), Style::default().fg(theme.fg)),
        Span::styled(format!("{boxmark} {label}"), style),
    ];
    if !hint.is_empty() {
        spans.push(Span::styled(format!("   {hint}"), theme.hint_style()));
    }
    Line::from(spans)
}

fn draw_form(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let v = app.variant();
    let s = app.current_stack();
    let focus = app.focus;

    let mut lines: Vec<Line> = Vec::new();

    // ---- DEVICE: the variant cycler, its own section ----
    lines.push(heading(theme, "DEVICE"));
    lines.push(field_line(
        theme,
        focus == Row::Device,
        "Device",
        format!("< {} >", v.display),
        &format!("{}/{}", app.variant_idx + 1, registry::VARIANTS.len()),
    ));

    // ---- STACK: the radio list of stacks this device supports ----
    lines.push(Line::from(""));
    lines.push(heading(theme, "STACK"));
    for (i, st) in v.stacks.iter().enumerate() {
        // The display string already carries the std/no_std family where it
        // applies; the only extra marker is a subtle "(default)" on the default.
        let hint = if i == 0 { "(default)" } else { "" };
        lines.push(radio_line(
            theme,
            focus == Row::Stack(i),
            app.stack_idx == i,
            false, // stacks are always selectable (even gated ones, to preview)
            false, // never inactive
            "  ",
            st.display,
            hint,
        ));
    }

    // ---- FLASHING TOOL / LOGGING / FRAMEWORK — full catalogs ----
    // Greying is PER-CATEGORY, not per-stack: the non-ESP scaffold keeps FRAMEWORK
    // live (embassy really is generatable on the embassy HALs) while flashing and
    // logging stay display-only, so each section asks its own question.
    // With µFerris ON (only possible on the esp-hal axis) the BSP supplies its
    // own toolchain, so these three sections go INACTIVE: dim heading, no reason
    // text, every radio an empty dim `( )`, none focusable.
    let uferris_suppress = app.uferris_on;
    for cat in ToolCategory::ALL {
        let section_reason = model::category_grey_reason(s, cat);
        let live = section_reason.is_none();
        lines.push(Line::from(""));
        if uferris_suppress {
            lines.push(Line::styled(
                cat.heading().to_string(),
                Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
            ));
        } else {
            lines.push(section_heading(theme, cat, section_reason));
        }
        let sel = app.cat_idx(cat);
        for (i, choice) in cat.catalog().iter().enumerate() {
            let supported = s.support(cat).contains(&choice.id);
            let (focused, selected, greyed, inactive) = if uferris_suppress {
                // µFerris owns the toolchain: no selection, all dim, none focusable.
                (false, false, false, true)
            } else if live {
                // Greying is still registry-derived, but the per-option reason
                // text is no longer shown beside the row — the dim `(-)` marker is
                // enough, and any section-level reason lives at the header.
                let greyed = app.choice_grey(cat, i).is_some();
                (focus == cat_row(cat, i), i == sel, greyed, false)
            } else {
                // Disabled section: everything dim; still show which entries a
                // device supports (default = selected marker), unsupported = (-).
                (false, supported && i == sel, !supported, false)
            };
            lines.push(radio_line(theme, focused, selected, greyed, inactive, "  ", choice.display, ""));
        }
    }

    // ---- BSP SUPPORT ----
    lines.push(Line::from(""));
    lines.push(heading(theme, "BSP SUPPORT"));
    let uf_enabled = app.uferris_enabled();
    // Eligibility hint only on the ENABLED toggle; a greyed µFerris toggle shows
    // no reason beside it — greying alone signals it is unavailable.
    let uf_hint = if uf_enabled { "esp-hal C3/C6" } else { "" };
    lines.push(toggle_line(
        theme,
        focus == Row::Uferris,
        uf_enabled,
        app.uferris_on,
        "  ",
        "Enable µFerris BSP",
        uf_hint,
    ));
    // Power/async hints appear only while their toggles are live (µFerris on);
    // when greyed they carry no reason text.
    let pb_hint = if app.uferris_on { "Megalops (INA219 + SD)" } else { "" };
    lines.push(toggle_line(
        theme,
        focus == Row::PowerBoard,
        app.uferris_on,
        app.power_board_on,
        "    ",
        "power board",
        pb_hint,
    ));
    let async_hint = if !app.uferris_on {
        ""
    } else if app.async_on {
        "embassy (verified)"
    } else {
        "gap — no verified non-async µFerris"
    };
    lines.push(toggle_line(
        theme,
        focus == Row::Async,
        app.uferris_on,
        app.async_on,
        "    ",
        "async (embassy)",
        async_hint,
    ));
    lines.push(Line::styled(
        "    · µFerris template selection is planned for a later version",
        theme.hint_style(),
    ));

    let form = Paragraph::new(lines).style(theme.base()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Configure ")
            .border_style(Style::default().fg(theme.border).bg(theme.bg))
            .style(Style::default().bg(theme.bg)),
    );
    f.render_widget(form, area);
}

/// Map a `(category, index)` to its `Row` variant (for focus comparison).
fn cat_row(cat: ToolCategory, i: usize) -> Row {
    match cat {
        ToolCategory::Flashing => Row::Flashing(i),
        ToolCategory::Logging => Row::Logging(i),
        ToolCategory::Framework => Row::Framework(i),
    }
}

fn draw_summary(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    match app.resolved() {
        Ok((cfg, readiness)) => {
            let key = theme.summary_key;
            let val = theme.summary_val;
            let row = move |k: &str, v: String| {
                Line::from(vec![
                    Span::styled(format!("{k:<11}"), Style::default().fg(key)),
                    Span::styled(v, Style::default().fg(val)),
                ])
            };
            lines.push(row("Name:", cfg.project_name.clone()));
            lines.push(row("Device:", format!("{} ({})", cfg.variant.display, cfg.variant.id)));
            lines.push(row("Stack:", cfg.stack.id.to_string()));
            lines.push(row("  ", cfg.stack.display.to_string()));
            lines.push(row("Target:", cfg.stack.target.to_string()));

            // Universal toolchain — only on the live esp-hal base axis.
            if model::option_groups_apply(&cfg) {
                lines.push(row("Flashing:", cfg.flashing.display.to_string()));
                lines.push(row("Logging:", cfg.logging.display.to_string()));
                lines.push(row("Framework:", cfg.framework.display.to_string()));
            }

            let uf = match &cfg.uferris {
                Some(u) => {
                    let mut s = String::from(if u.async_on { "yes · async" } else { "yes · blocking" });
                    if u.power_board {
                        s.push_str(" · power board");
                    }
                    s
                }
                None => "no".to_string(),
            };
            lines.push(row("µFerris:", uf));

            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Equivalent command:",
                Style::default().fg(theme.summary_val).add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::styled(
                model::equivalent_command(&cfg),
                Style::default().fg(theme.accent),
            ));
            lines.push(Line::from(""));
            match readiness {
                Readiness::Ready => lines.push(Line::styled(
                    "READY — press Enter to generate.",
                    Style::default().fg(theme.ok).add_modifier(Modifier::BOLD),
                )),
                Readiness::Gated(msg) => {
                    lines.push(Line::styled(
                        "NOT YET GENERATABLE",
                        Style::default().fg(theme.warn).add_modifier(Modifier::BOLD),
                    ));
                    lines.push(Line::styled(msg, Style::default().fg(theme.warn)));
                }
            }
        }
        Err(e) => {
            lines.push(Line::styled(
                "INVALID SELECTION",
                Style::default().fg(theme.err).add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::styled(e, Style::default().fg(theme.err)));
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Adjust the greyed/marked field to continue.",
                Style::default().fg(theme.dim),
            ));
        }
    }

    // Ferris, the crab — the Summary panel's centrepiece. Rendered as COLORED half-
    // block art in fixed (theme-independent) inks, centred horizontally as a
    // FERRIS_W-wide block (uniform left-pad, so the shape never breaks) and centred
    // vertically in the leftover rows BELOW the summary content. Drawn only when it
    // actually fits: interior width ≥ FERRIS_W AND ≥ FERRIS_H spare rows under the
    // text. Otherwise omitted entirely — a narrow/short panel degrades to "no crab",
    // never a wrapped or clipped shell.
    let interior_w = area.width.saturating_sub(2);
    let interior_h = area.height.saturating_sub(2);
    let content_h = wrapped_height(&lines, interior_w);
    let leftover = interior_h.saturating_sub(content_h) as usize;
    if interior_w as usize >= FERRIS_W && leftover >= FERRIS_H {
        let left_pad = (interior_w as usize - FERRIS_W) / 2;
        let top_pad = (leftover - FERRIS_H) / 2;
        for _ in 0..top_pad {
            lines.push(Line::from(""));
        }
        lines.extend(ferris_lines(left_pad));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false }).style(theme.base()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Summary ")
            .border_style(Style::default().fg(theme.border).bg(theme.bg))
            .style(Style::default().bg(theme.bg)),
    );
    f.render_widget(p, area);
}

// ---------------------------------------------------------------------------
// Live terminal driver (not exercised by tests — needs a TTY).
// ---------------------------------------------------------------------------

pub fn launch(name: String) -> Result<Outcome> {
    let mut terminal = ratatui::try_init().context("initialising terminal")?;
    let result = run_loop(&mut terminal, name);
    ratatui::restore();
    result
}

fn run_loop(terminal: &mut DefaultTerminal, name: String) -> Result<Outcome> {
    let mut app = App::new(name);
    loop {
        terminal.draw(|f| draw(f, &app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if app.on_key(key.code) {
                return Ok(app.outcome.take().unwrap_or(Outcome::Cancelled));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Headless render tests (TestBackend) — the way the TUI is verified without a
// live terminal, plus the text snapshots dumped for the report.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    const W: u16 = 140;
    const H: u16 = 44;

    fn variant_index(id: &str) -> usize {
        registry::VARIANTS.iter().position(|v| v.id == id).unwrap()
    }

    /// Flatten the rendered buffer to one line so assertions on the (word-wrapped)
    /// equivalent command in the summary panel don't depend on wrap position.
    fn flat(s: &str) -> String {
        s.replace('\n', " ")
    }

    fn buffer_to_string(buf: &Buffer) -> String {
        let area = buf.area;
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                if let Some(cell) = buf.cell((x, y)) {
                    out.push_str(cell.symbol());
                }
            }
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
        }
        out
    }

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    /// Like `render`, but hands back the raw buffer so tests can inspect per-cell
    /// colour (which the flattened string dump cannot show).
    fn render_buf(app: &App, w: u16, h: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Interior (inside-the-borders) width/height of the Summary panel for a page
    /// of size `w`×`h` — replicates `draw`'s layout split so tests can compute the
    /// crab's expected left-pad without hardcoding the percentage geometry.
    fn summary_interior(w: u16, h: u16) -> (u16, u16) {
        let area = Rect::new(0, 0, w, h);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[1]);
        (cols[1].width.saturating_sub(2), cols[1].height.saturating_sub(2))
    }

    fn app_on(variant_id: &str, stack_idx: usize) -> App {
        let mut app = App::new("proj".to_string());
        app.variant_idx = variant_index(variant_id);
        app.stack_idx = stack_idx;
        app.reset_cats();
        app.normalize_cats();
        app.ensure_focus_valid();
        app
    }

    /// Index of a catalog choice by id (test helper).
    fn cat_pos(cat: ToolCategory, id: &str) -> usize {
        cat.catalog().iter().position(|c| c.id == id).unwrap()
    }

    #[test]
    fn form_shows_all_universal_sections_on_one_page() {
        let s = render(&app_on("xiao-esp32c6", 0), W, H);
        for needle in [
            "DEVICE",
            "STACK",
            "FLASHING TOOL",
            "LOGGING",
            "FRAMEWORK",
            "BSP SUPPORT",
            "Device",
            "espflash",
            "probe-rs",
            "embassy",
            "RTIC",
            "Enable µFerris BSP",
            "power board",
            "async",
            "template selection is planned",
            "Summary",
        ] {
            assert!(s.contains(needle), "single page must show `{needle}`:\n{s}");
        }
    }

    #[test]
    fn removed_sections_are_gone_and_no_esp_generate_vocabulary() {
        let s = render(&app_on("xiao-esp32c6", 0), W, H);
        for banned in [
            "ESP-GENERATE OPTIONS",
            "EXTRAS",
            "Component add-on",
            "Extra crates",
            "Output dir",
            "esp opts",
            "esp-generate",
            "esp-backtrace",
            "panic-rtt-target",
            "-o ",
        ] {
            assert!(!s.contains(banned), "page must NOT contain `{banned}`:\n{s}");
        }
    }

    #[test]
    fn theme_toggle_changes_footer_and_defaults_dark() {
        let app = app_on("xiao-esp32c6", 0);
        let dark = render(&app, W, H);
        assert!(dark.contains("theme:dark"), "default theme must be dark:\n{dark}");
        let mut light = app_on("xiao-esp32c6", 0);
        light.dark = false;
        let s = render(&light, W, H);
        assert!(s.contains("theme:light"), "toggled theme must read light:\n{s}");
    }

    #[test]
    fn esp_hal_defaults_selected() {
        let s = render(&app_on("xiao-esp32c6", 0), W, H);
        assert!(s.contains("(•) espflash"), "espflash selected by default:\n{s}");
        assert!(s.contains("( ) probe-rs"), "probe-rs offered but unselected:\n{s}");
        assert!(s.contains("READY"), "C6 esp-hal base must be generatable:\n{s}");
    }

    #[test]
    fn rtic_framework_is_greyed_on_esp_without_reason_text() {
        // RTIC renders as a greyed `(-)` slot on ESP; the per-option reason text is
        // no longer shown beside the row (greying alone is enough).
        let s = render(&app_on("xiao-esp32c6", 0), W, H);
        assert!(s.contains("(-) RTIC"), "RTIC must render as a greyed slot:\n{s}");
        assert!(!s.contains("not supported on ESP"), "no per-option reason beside RTIC:\n{s}");
    }

    #[test]
    fn probe_rs_greys_the_log_logging_choice() {
        let mut app = app_on("xiao-esp32c6", 0);
        app.focus = Row::Flashing(cat_pos(ToolCategory::Flashing, "probe-rs"));
        app.activate(); // select probe-rs
        let s = render(&app, W, H);
        assert!(s.contains("(•) probe-rs"), "probe-rs selected:\n{s}");
        assert!(s.contains("(-) log"), "log must grey out under probe-rs:\n{s}");
        // …but the constraint-graph reason is not shown beside the greyed row.
        assert!(!s.contains("cannot be enabled together"), "no per-option reason beside log:\n{s}");
        assert!(flat(&s).contains("--flashing probe-rs"), "equivalent command shows probe-rs:\n{s}");
    }

    #[test]
    fn esp_idf_std_greys_the_sections() {
        let s = render(&app_on("xiao-esp32s3", 1), W, H);
        assert!(s.contains("esp-idf"), "{s}");
        // Section-level reason is kept at the header…
        assert!(
            s.contains("configured by the esp-idf template"),
            "sections greyed on esp-idf std:\n{s}"
        );
        // …but the µFerris toggle (still rendered, greyed) shows no reason beside it.
        assert!(s.contains("Enable µFerris BSP"), "µFerris toggle present:\n{s}");
        assert!(!s.contains("not on the esp-idf/std stack"), "no per-option µFerris reason:\n{s}");
    }

    /// M4: non-ESP generates for real, so the three sections are no longer greyed
    /// as a block. FLASHING + LOGGING stay display-only (the scaffold ships no
    /// runner and no logging frontend), FRAMEWORK is a live axis, and the whole
    /// selection resolves to a generatable `Config` — not a gate.
    #[test]
    fn non_esp_greys_flashing_and_logging_but_keeps_framework_live() {
        let app = app_on("xiao-rp2040", 0); // rp2040-hal — the blocking HAL
        let s = render(&app, W, H);

        // The pre-M4 world is gone: no "until M4" banner, no gate in the summary.
        assert!(!s.contains("until M4"), "non-ESP is no longer gated until M4:\n{s}");
        assert!(!s.contains("NOT YET GENERATABLE"), "non-ESP generates for real now:\n{s}");
        assert!(s.contains("READY"), "non-ESP selection is ready to generate:\n{s}");

        // Flashing + logging carry the per-category reason at their headers…
        let reason = "chosen by your own tooling, not baked into the project";
        assert_eq!(
            s.matches(reason).count(),
            2,
            "exactly the flashing + logging headers carry the non-ESP reason:\n{s}"
        );
        // …and FRAMEWORK does NOT (it is live, so its header is bare).
        let framework_hdr = s.lines().find(|l| l.contains("FRAMEWORK")).unwrap();
        assert!(!framework_hdr.contains(reason), "FRAMEWORK header must be bare: {framework_hdr}");

        // RTIC is a real catalog entry on the ARM part: visible, but greyed —
        // there is no verified RTIC body, which `choice_grey_reason` reports.
        assert!(s.contains("(-) RTIC"), "RTIC visible but greyed on the ARM part:\n{s}");
        assert!(
            app.choice_grey(ToolCategory::Framework, cat_pos(ToolCategory::Framework, "rtic"))
                .is_some(),
            "RTIC must carry a grey reason"
        );
        // On this blocking HAL `embassy` is a declared gap too.
        assert!(s.contains("(-) embassy"), "embassy greyed on the blocking HAL:\n{s}");

        // Only the FRAMEWORK section contributes focusable rows.
        let rows = app.focusable_rows();
        assert!(
            rows.iter().any(|r| matches!(r, Row::Framework(_))),
            "FRAMEWORK is focusable on non-ESP: {rows:?}"
        );
        for r in &rows {
            assert!(
                !matches!(r, Row::Flashing(_) | Row::Logging(_)),
                "flashing/logging must not be focusable on non-ESP: {rows:?}"
            );
        }
    }

    /// The guard the M4 implementer called out: `Config::resolve` REJECTS a
    /// flashing/logging value on a stack where that category is not live, so the
    /// TUI must keep passing `None` for those two on non-ESP even now that it
    /// forwards FRAMEWORK. Verified by actually resolving, not by inspection.
    #[test]
    fn non_esp_raw_config_omits_flashing_and_logging_but_forwards_framework() {
        for stack_idx in [0, 1] {
            let app = app_on("xiao-rp2040", stack_idx);
            let raw = app.raw_config();
            assert!(raw.flashing.is_none(), "flashing must stay None on non-ESP (stack {stack_idx})");
            assert!(raw.logging.is_none(), "logging must stay None on non-ESP (stack {stack_idx})");
            assert!(
                raw.framework.is_some(),
                "framework IS forwarded on non-ESP (stack {stack_idx})"
            );

            // …and the resolve actually succeeds (this is what would break if a
            // flashing/logging id leaked through).
            let (cfg, readiness) = app
                .resolved()
                .unwrap_or_else(|e| panic!("non-ESP stack {stack_idx} must resolve, got: {e}"));
            assert!(
                matches!(readiness, Readiness::Ready),
                "non-ESP is generatable since M4 (stack {stack_idx})"
            );
            assert_eq!(cfg.variant.id, "xiao-rp2040");
        }
    }

    /// `embassy` is selectable and generatable on an embassy HAL, and refused on a
    /// blocking one (no time driver) — the two halves of the M4 framework gap.
    #[test]
    fn embassy_is_selectable_on_an_embassy_hal_and_refused_on_a_blocking_one() {
        let embassy_pos = cat_pos(ToolCategory::Framework, "embassy");

        // (a) embassy-rp — live choice, resolves with framework = embassy.
        let mut ok = app_on("xiao-rp2040", 1);
        assert!(ok.choice_grey(ToolCategory::Framework, embassy_pos).is_none(), "embassy live");
        ok.focus = Row::Framework(embassy_pos);
        ok.activate();
        assert_eq!(ok.framework_idx, embassy_pos, "embassy selected on the embassy HAL");
        let (cfg, readiness) = ok.resolved().expect("embassy on embassy-rp must resolve");
        assert_eq!(cfg.framework.id, "embassy");
        assert!(matches!(readiness, Readiness::Ready));
        let s = render(&ok, W, H);
        assert!(s.contains("(•) embassy"), "embassy shows selected:\n{s}");
        assert!(flat(&s).contains("--framework embassy"), "command carries the framework:\n{s}");

        // (b) rp2040-hal — greyed, and `activate` refuses to move the selection.
        let mut blocked = app_on("xiao-rp2040", 0);
        let before = blocked.framework_idx;
        assert!(
            blocked.choice_grey(ToolCategory::Framework, embassy_pos).is_some(),
            "embassy greyed on the blocking HAL"
        );
        blocked.focus = Row::Framework(embassy_pos);
        blocked.activate();
        assert_eq!(blocked.framework_idx, before, "greyed embassy must not become selected");

        // Even if forced past the UI, the shared model refuses it outright.
        blocked.framework_idx = embassy_pos;
        let err = blocked.resolved().expect_err("embassy on a blocking HAL must be refused");
        assert!(err.contains("embassy"), "refusal names the framework: {err}");
    }

    /// ESP behaviour is untouched by the per-category switch: esp-hal keeps all
    /// three sections live, esp-idf keeps all three greyed.
    #[test]
    fn esp_section_liveness_is_unchanged_by_per_category_greying() {
        let esp_hal = app_on("xiao-esp32c6", 0);
        for cat in ToolCategory::ALL {
            assert!(esp_hal.cat_live(cat), "esp-hal: {cat:?} stays live");
        }
        let raw = esp_hal.raw_config();
        assert!(
            raw.flashing.is_some() && raw.logging.is_some() && raw.framework.is_some(),
            "esp-hal forwards all three selections"
        );
        let rows = esp_hal.focusable_rows();
        assert!(rows.iter().any(|r| matches!(r, Row::Flashing(_))), "esp-hal flashing focusable");
        assert!(rows.iter().any(|r| matches!(r, Row::Logging(_))), "esp-hal logging focusable");
        assert!(rows.iter().any(|r| matches!(r, Row::Framework(_))), "esp-hal framework focusable");

        let esp_idf = app_on("xiao-esp32s3", 1);
        for cat in ToolCategory::ALL {
            assert!(!esp_idf.cat_live(cat), "esp-idf: {cat:?} stays greyed");
        }
        let raw = esp_idf.raw_config();
        assert!(
            raw.flashing.is_none() && raw.logging.is_none() && raw.framework.is_none(),
            "esp-idf forwards nothing — the template fixes the toolchain"
        );
        let rows = esp_idf.focusable_rows();
        for r in &rows {
            assert!(
                !matches!(r, Row::Flashing(_) | Row::Logging(_) | Row::Framework(_)),
                "no toolchain row is focusable on esp-idf: {rows:?}"
            );
        }
        let s = render(&esp_idf, W, H);
        assert_eq!(
            s.matches("configured by the esp-idf template").count(),
            3,
            "all three esp-idf headers carry the reason:\n{s}"
        );
    }

    #[test]
    fn uferris_enabled_shows_power_async_live_and_the_note() {
        let mut app = app_on("xiao-esp32c3", 0);
        app.uferris_on = true;
        app.ensure_focus_valid();
        let s = render(&app, W, H);
        assert!(s.contains("[x] Enable µFerris BSP"), "µFerris on:\n{s}");
        assert!(s.contains("Megalops"), "power board hint live when µFerris on:\n{s}");
        assert!(s.contains("[x] async (embassy)"), "async on by default under µFerris:\n{s}");
        assert!(s.contains("embassy (verified)"), "async hint live:\n{s}");
        assert!(s.contains("template selection is planned for a later version"), "note shown:\n{s}");
        assert!(flat(&s).contains("--uferris"), "equivalent command carries --uferris:\n{s}");
    }

    #[test]
    fn uferris_async_off_is_refused_in_summary() {
        let mut app = app_on("xiao-esp32c3", 0);
        app.uferris_on = true;
        app.async_on = false;
        let s = render(&app, W, H);
        assert!(s.contains("INVALID SELECTION"), "async-off µFerris must be refused:\n{s}");
        assert!(s.contains("non-async"), "refusal should explain the gap:\n{s}");
    }

    #[test]
    fn summary_has_no_esp_vocabulary_or_removed_lines() {
        let s = render(&app_on("xiao-esp32c6", 0), W, H);
        // Summary keeps universal facts + command; drops Add-on/Crates/Output/esp.
        assert!(s.contains("Device:") && s.contains("Stack:") && s.contains("Target:"));
        assert!(s.contains("Flashing:") && s.contains("Logging:") && s.contains("Framework:"));
        for banned in ["Add-on:", "Crates:", "Output:", "esp opts", "esp-generate"] {
            assert!(!s.contains(banned), "summary must not contain `{banned}`:\n{s}");
        }
    }

    #[test]
    fn uferris_clears_toolchain_selections_and_off_restores_defaults() {
        let mut app = app_on("xiao-esp32c3", 0);

        // Make a NON-default flashing pick so "restore defaults" is observable.
        app.focus = Row::Flashing(cat_pos(ToolCategory::Flashing, "probe-rs"));
        app.activate();
        let before = render(&app, W, H);
        assert!(before.contains("(•) probe-rs"), "probe-rs selected before µFerris:\n{before}");

        // Enable µFerris → the three toolchain sections show NO selected radio and
        // are non-focusable; every catalog entry renders an empty `( )`.
        app.focus = Row::Uferris;
        app.activate();
        assert!(app.uferris_on, "µFerris should be on after activate");
        let on = render(&app, W, H);
        for filled in [
            "(•) espflash",
            "(•) probe-rs",
            "(•) none",
            "(•) log",
            "(•) defmt",
            "(•) embassy",
        ] {
            assert!(!on.contains(filled), "no toolchain selection under µFerris (`{filled}`):\n{on}");
        }
        assert!(on.contains("( ) espflash"), "flashing entries render empty under µFerris:\n{on}");
        // The sections are not in the focusable set while µFerris is on.
        for r in [
            Row::Flashing(cat_pos(ToolCategory::Flashing, "espflash")),
            Row::Logging(cat_pos(ToolCategory::Logging, "none")),
            Row::Framework(cat_pos(ToolCategory::Framework, "none")),
        ] {
            assert!(!app.focusable_rows().contains(&r), "toolchain rows non-focusable under µFerris");
        }

        // Toggle µFerris OFF → sections restore their per-stack DEFAULTS
        // (espflash / none / none), discarding the earlier probe-rs pick.
        app.focus = Row::Uferris;
        app.activate();
        assert!(!app.uferris_on, "µFerris should be off after second activate");
        let off = render(&app, W, H);
        assert!(off.contains("(•) espflash"), "flashing default restored after µFerris off:\n{off}");
        assert!(off.contains("(•) none"), "logging/framework default restored after µFerris off:\n{off}");
        assert!(!off.contains("(•) probe-rs"), "earlier probe-rs pick cleared, default restored:\n{off}");
    }

    /// The block glyphs the colored crab is painted with: the quadrant blocks
    /// (U+2596..U+259F) plus `▀▄▌▐█`. None of them occur anywhere in the form/summary
    /// text or panel borders, so any cell carrying one is unambiguously part of the
    /// crab. A blank crab cell paints a plain space and is (correctly) not counted.
    fn is_crab_glyph(sym: &str) -> bool {
        matches!(
            sym,
            "▀" | "▄" | "▌" | "▐" | "█" | "▖" | "▗" | "▘" | "▙" | "▚" | "▛" | "▜" | "▝" | "▞" | "▟"
        )
    }

    /// Every painted crab cell in a rendered buffer, as (x, y, glyph, fg).
    fn crab_cells(buf: &Buffer) -> Vec<(u16, u16, String, Color)> {
        let a = buf.area;
        let mut v = Vec::new();
        for y in 0..a.height {
            for x in 0..a.width {
                if let Some(cell) = buf.cell((x, y)) {
                    if is_crab_glyph(cell.symbol()) {
                        v.push((x, y, cell.symbol().to_string(), cell.fg));
                    }
                }
            }
        }
        v
    }

    /// Painted-column range [min, max] of the crab art within its FERRIS_W box,
    /// derived from the embedded data so the centring test never hardcodes it.
    fn art_col_bounds() -> (usize, usize) {
        let mut lo = FERRIS_W;
        let mut hi = 0usize;
        for r in 0..FERRIS_H {
            for grid in [FERRIS_FG[r], FERRIS_BG[r]] {
                for (c, ch) in grid.bytes().enumerate() {
                    if ch != b'.' {
                        lo = lo.min(c);
                        hi = hi.max(c);
                    }
                }
            }
        }
        (lo, hi)
    }

    /// Interior left column of the Summary panel (just inside its left border) —
    /// replicates `draw`'s layout split so the crab's centring can be checked.
    fn summary_ix(w: u16, h: u16) -> u16 {
        let area = Rect::new(0, 0, w, h);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[1]);
        cols[1].x + 1
    }

    #[test]
    fn summary_shows_ferris_art_centered() {
        // At the tested size the panel is wide+tall enough: the colored crab is
        // painted, horizontally centred as a FERRIS_W-wide block.
        let buf = render_buf(&app_on("xiao-esp32c6", 0), W, H);
        let cells = crab_cells(&buf);
        assert!(cells.len() > 200, "the colored crab must be painted (got {} cells)", cells.len());

        // The crab is centred as one FERRIS_W-wide block at a uniform left-pad. Its
        // painted columns must therefore land at exactly interior_left + left_pad +
        // (the art's own painted-column offsets) — proving the block is centred, not
        // left-jammed or off by one.
        let (iw, _ih) = summary_interior(W, H);
        let ix = summary_ix(W, H) as usize;
        let left_pad = (iw as usize - FERRIS_W) / 2;
        let (art_lo, art_hi) = art_col_bounds();
        let cmin = cells.iter().map(|c| c.0).min().unwrap() as usize;
        let cmax = cells.iter().map(|c| c.0).max().unwrap() as usize;
        assert_eq!(cmin, ix + left_pad + art_lo, "crab painted left edge must be centred");
        assert_eq!(cmax, ix + left_pad + art_hi, "crab painted right edge must be centred");
        // And the block's own margins inside the interior are symmetric.
        let left_gap = cmin - ix;
        let right_gap = (ix + iw as usize - 1) - cmax;
        assert!(
            (left_gap as i32 - right_gap as i32).abs() <= 1,
            "crab must be horizontally centred (L={left_gap} R={right_gap})"
        );

        // Shown in every summary state — including the gated / non-ESP page.
        let g = crab_cells(&render_buf(&app_on("xiao-rp2040", 0), W, H));
        assert!(g.len() > 200, "crab shown even on the gated page (got {} cells)", g.len());
    }

    /// Page width whose Summary interior is 35 — one column under `FERRIS_W` (36),
    /// so it exercises the width guard right at its boundary.
    const NARROW_W: u16 = 88;

    #[test]
    fn crab_omitted_when_panel_too_narrow() {
        // Interior width < FERRIS_W → no crab, but the panel still renders cleanly.
        let (iw, _ih) = summary_interior(NARROW_W, H);
        assert!((iw as usize) < FERRIS_W, "precondition: narrow panel (iw={iw})");
        assert_eq!(iw as usize, FERRIS_W - 1, "precondition: narrow by exactly one column");
        let buf = render_buf(&app_on("xiao-esp32c6", 0), NARROW_W, H);
        assert!(render(&app_on("xiao-esp32c6", 0), NARROW_W, H).contains("Summary"),
            "panel still renders when too narrow");
        assert!(crab_cells(&buf).is_empty(), "no crab in a too-narrow panel");

        // One column wider the guard opens again — proving it is the WIDTH that
        // omitted the crab, not some unrelated layout accident.
        let (iw2, _) = summary_interior(NARROW_W + 2, H);
        assert!((iw2 as usize) >= FERRIS_W, "precondition: two columns wider fits (iw={iw2})");
        let wider = render_buf(&app_on("xiao-esp32c6", 0), NARROW_W + 2, H);
        assert!(!crab_cells(&wider).is_empty(), "crab returns once the panel is wide enough");
    }

    /// Page height that leaves only 9 spare rows under the summary text — two short
    /// of `FERRIS_H` (11), so it exercises the height guard near its boundary.
    const SHORT_H: u16 = 32;

    #[test]
    fn crab_omitted_when_panel_too_short() {
        // Wide enough, but too few leftover rows below the summary text → no crab.
        let (iw, _ih) = summary_interior(W, SHORT_H);
        assert!((iw as usize) >= FERRIS_W, "precondition: wide enough (iw={iw})");
        let buf = render_buf(&app_on("xiao-esp32c6", 0), W, SHORT_H);
        assert!(render(&app_on("xiao-esp32c6", 0), W, SHORT_H).contains("Summary"),
            "panel still renders when too short");
        assert!(crab_cells(&buf).is_empty(), "no crab when the panel is too short");

        // It really is the HEIGHT that closed the guard, not the width or some other
        // layout accident: the same width two rows taller clears the bar and the crab
        // comes back — so SHORT_H sits just under the threshold.
        let taller = render_buf(&app_on("xiao-esp32c6", 0), W, SHORT_H + 2);
        assert!(!crab_cells(&taller).is_empty(), "crab returns once the panel is tall enough");
    }

    #[test]
    fn crab_color_is_pinned_across_themes() {
        // The crab's inks are FIXED truecolor, identical in BOTH themes (they do NOT
        // follow theme.accent). Compare the painted cells (position + glyph + fg)
        // cell-for-cell between dark and light: they must match exactly.
        let dark = crab_cells(&render_buf(
            &{ let mut a = app_on("xiao-esp32c6", 0); a.dark = true; a }, W, H));
        let light = crab_cells(&render_buf(
            &{ let mut a = app_on("xiao-esp32c6", 0); a.dark = false; a }, W, H));
        assert!(!dark.is_empty(), "crab must render");
        assert_eq!(dark, light, "crab glyphs+fg must be identical across themes");

        // The pinned body orange is present, and the light theme's (blue) accent is
        // NOT used by any crab cell — proving the crab does not follow the theme.
        let fgs: std::collections::HashSet<Color> = dark.iter().map(|c| c.3).collect();
        assert!(fgs.contains(&FERRIS_ORANGE), "crab must paint its pinned body colour");
        assert!(!fgs.contains(&Theme::light().accent), "crab must not use the theme accent");
        assert_ne!(Theme::light().accent, FERRIS_ORANGE, "light accent should differ from the pin");
    }


    /// Not run in the normal suite; run explicitly to dump verbatim snapshots:
    ///   cargo test --bin xiao-generate -- --ignored --nocapture print_form_snapshots
    #[test]
    #[ignore]
    fn print_form_snapshots() {
        let dump = |label: &str, app: &App| {
            println!("\n===== {label} =====");
            print!("{}", render(app, W, H));
        };

        // (a) dark ESP esp-hal full page.
        let a = app_on("xiao-esp32c6", 0);
        dump("(a) dark theme — xiao-esp32c6 / esp-hal full page", &a);

        // (b) light theme, same screen.
        let mut b = app_on("xiao-esp32c6", 0);
        b.dark = false;
        dump("(b) light theme — xiao-esp32c6 / esp-hal full page", &b);

        // (c) flashing=probe-rs greying the `log` logging choice.
        let mut c = app_on("xiao-esp32c6", 0);
        c.focus = Row::Flashing(cat_pos(ToolCategory::Flashing, "probe-rs"));
        c.activate();
        dump("(c) probe-rs greys log — xiao-esp32c6 / esp-hal", &c);

        // (d) esp-idf std stack — sections greyed.
        dump("(d) esp-idf std — xiao-esp32s3 / esp-idf, sections greyed", &app_on("xiao-esp32s3", 1));

        // (e) non-ESP, blocking HAL — flashing/logging greyed, FRAMEWORK live but
        // `embassy` greyed (no time driver on the blocking HAL), RTIC greyed.
        dump(
            "(e) non-ESP — xiao-rp2040 / rp2040-hal (flashing+logging greyed, FRAMEWORK live, embassy greyed)",
            &app_on("xiao-rp2040", 0),
        );

        // (e2) non-ESP, embassy HAL — FRAMEWORK live with `embassy` selectable.
        let mut e2 = app_on("xiao-rp2040", 1);
        e2.focus = Row::Framework(cat_pos(ToolCategory::Framework, "embassy"));
        e2.activate();
        dump(
            "(e2) non-ESP — xiao-rp2040 / embassy-rp with embassy selected (FRAMEWORK live)",
            &e2,
        );

        // (f) µFerris enabled — power/async live + the template note.
        let mut fsnap = app_on("xiao-esp32c3", 0);
        fsnap.uferris_on = true;
        fsnap.ensure_focus_valid();
        dump("(f) µFerris enabled — xiao-esp32c3 / esp-hal (power/async live + note)", &fsnap);
    }

    /// Not run in the normal suite; dumps every painted crab cell as
    ///   x y <glyph> fr fg fb br bg bb
    /// from the REAL rendered TUI buffer, so an external tool can redraw the crab
    /// exactly as the terminal shows it (colored half-blocks on the dark panel bg):
    ///   cargo test --bin xiao-generate -- --ignored --nocapture dump_ferris_cells
    #[test]
    #[ignore]
    fn dump_ferris_cells() {
        let rgb = |c: Color| -> (u8, u8, u8) {
            match c {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (0, 0, 0),
            }
        };
        let buf = render_buf(&app_on("xiao-esp32c6", 0), W, H);
        let (br, bg, bb) = rgb(Theme::dark().bg);
        println!("PANEL {br} {bg} {bb}");
        let a = buf.area;
        for y in 0..a.height {
            for x in 0..a.width {
                if let Some(cell) = buf.cell((x, y)) {
                    if is_crab_glyph(cell.symbol()) {
                        let (fr, fg, fb) = rgb(cell.fg);
                        let (kr, kg, kb) = rgb(cell.bg);
                        println!("{x} {y} {} {fr} {fg} {fb} {kr} {kg} {kb}", cell.symbol());
                    }
                }
            }
        }
    }
}
