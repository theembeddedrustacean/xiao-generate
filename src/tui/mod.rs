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

mod ferris;

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
    /// One checkbox in the EDITOR SUPPORT section (index into `registry::EDITORS`).
    Editor(usize),
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
    /// Selected index into `registry::EDITORS` — a single-select radio list like
    /// the toolchain sections, with `none` a normal catalog entry.
    editor_idx: usize,
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
            // The same registry default the CLI uses, so the TUI and a bare
            // headless run start from an identical selection.
            editor_idx: registry::EDITORS
                .iter()
                .position(|e| e.id == registry::default_editor().id)
                .unwrap_or(0),
            uferris_on: false,
            power_board_on: false,
            async_on: false,
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
        // EDITOR SUPPORT is live on every stack — the spurious `test`-crate
        // diagnostic, and its fix, are identical across all three backends — so its
        // checkboxes are always focusable and nothing here ever greys out. It also
        // survives µFerris: the BSP owns the toolchain, not the user's editor.
        for i in 0..registry::EDITORS.len() {
            rows.push(Row::Editor(i));
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
        self.async_on = false;
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
            // Nothing under BSP SUPPORT is pre-selected: enabling µFerris does
            // not silently turn async on. The async row shows its own gap hint
            // while it is off, so the consequence is visible rather than assumed.
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
            Row::Editor(i) => self.editor_idx = i,
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
            // Always forwarded explicitly (`none` included — it is a real catalog
            // value, not "nothing given"). The axis is live on every stack, so
            // unlike the toolchain categories there is nothing to suppress here.
            editor: Some(registry::EDITORS[self.editor_idx].id.to_string()),
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

    // ---- EDITOR SUPPORT ----
    // A radio list like the toolchain sections above, with `none` a normal catalog
    // entry. It is the one section that is live on EVERY stack — the diagnostic it
    // configures away is backend-independent — so its header carries no reason and
    // no row ever greys. Per-editor caveats ride in the hint column, the same place
    // µFerris puts its eligibility hint.
    lines.push(Line::from(""));
    lines.push(heading(theme, "EDITOR SUPPORT"));
    for (i, e) in registry::EDITORS.iter().enumerate() {
        let selected = app.editor_idx == i;
        // Show the caveat only for the editor actually selected — an unselected
        // row emits nothing, so its caveat does not apply yet.
        let hint = match e.caveat {
            Some(c) if selected => c,
            _ => "",
        };
        lines.push(radio_line(
            theme,
            focus == Row::Editor(i),
            selected,
            false, // never greyed: every editor applies on every stack
            false, // never inactive
            "  ",
            e.display,
            hint,
        ));
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
            // One line, not two: `display` already opens with the stack id
            // ("esp-hal · no_std"), so printing `id` above it just said the name
            // twice.
            lines.push(row("Stack:", cfg.stack.display.to_string()));
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
            lines.push(row("Editor:", cfg.editor.display.to_string()));

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

    // Ferris, the crab — the Summary panel's centrepiece, rendered from the logo
    // by RASCII (see `ferris`) at one fixed size, centred horizontally as a block.
    //
    // It is ANCHORED to the bottom of the panel, a fixed `FERRIS_BOTTOM_MARGIN`
    // rows up, NOT centred in the space below the summary text. Centring made its
    // row depend on how many lines the summary happened to have, so the crab
    // visibly jumped up and down as options changed the text above it. Anchoring
    // to the bottom pins its absolute row for a given panel size — the summary
    // text can grow or shrink and the crab stays put. A fixed gap is left below it
    // so it sits low without touching the border.
    //
    // Drawn only when there is room for the whole crab plus that gap AND at least
    // `FERRIS_MIN_GAP` blank rows between it and the text; otherwise omitted
    // entirely (never wrapped or clipped) via `ferris::lines` returning None.
    const FERRIS_BOTTOM_MARGIN: u16 = 1;
    const FERRIS_MIN_GAP: u16 = 1;
    let interior_w = area.width.saturating_sub(2);
    let interior_h = area.height.saturating_sub(2);
    let content_h = wrapped_height(&lines, interior_w);
    let below = interior_h.saturating_sub(content_h);
    let art_budget = below.saturating_sub(FERRIS_BOTTOM_MARGIN + FERRIS_MIN_GAP);
    if let Some(art) = ferris::lines(interior_w, art_budget) {
        // Absolute top row of the crab = interior_h - art_h - bottom_margin, which
        // is independent of content_h — that is what stops the drift. Expressed as
        // padding to append after the text, that is `below - art_h - margin`, and
        // it is >= FERRIS_MIN_GAP by construction of `art_budget` above.
        let top_pad = usize::from(below) - art.len() - usize::from(FERRIS_BOTTOM_MARGIN);
        for _ in 0..top_pad {
            lines.push(Line::from(""));
        }
        lines.extend(art);
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
    /// The Summary panel's whole rect (borders included), replicating `draw`'s
    /// layout split so tests can locate the panel without hardcoding geometry.
    /// The interior is this inset by one cell on every side.
    fn summary_interior_rect(w: u16, h: u16) -> Rect {
        let area = Rect::new(0, 0, w, h);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[1]);
        cols[1]
    }

    fn summary_interior(w: u16, h: u16) -> (u16, u16) {
        let r = summary_interior_rect(w, h);
        (r.width.saturating_sub(2), r.height.saturating_sub(2))
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

    /// Enabling µFerris makes the power-board and async rows live, but selects
    /// NOTHING for the user — async stays off until they choose it, and the row
    /// says so via its gap hint rather than quietly defaulting to a value.
    #[test]
    fn uferris_enabled_makes_power_and_async_live_without_selecting_them() {
        let mut app = app_on("xiao-esp32c3", 0);
        app.uferris_on = true;
        app.ensure_focus_valid();
        let s = render(&app, W, H);
        assert!(s.contains("[x] Enable µFerris BSP"), "µFerris on:\n{s}");
        assert!(s.contains("Megalops"), "power board hint live when µFerris on:\n{s}");
        assert!(
            s.contains("[ ] async (embassy)"),
            "async must NOT be pre-selected under µFerris:\n{s}"
        );

        // Consequence of nothing being pre-selected: µFerris on its own is not yet
        // a generatable config, because the only verified µFerris body is the async
        // one. The summary must say so rather than offer a command that would fail
        // (`uferris_async_off_is_refused_in_summary` pins the wording).
        assert!(
            !flat(&s).contains("--uferris"),
            "µFerris without async is a gap — no equivalent command should be offered:\n{s}"
        );

        // Once the user picks async themselves, it becomes a real config.
        app.async_on = true;
        let s = render(&app, W, H);
        assert!(s.contains("[x] async (embassy)"), "async on after the user selects it:\n{s}");
        assert!(s.contains("embassy (verified)"), "async hint live:\n{s}");
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

    // ---- EDITOR SUPPORT section ----

    /// Index of an editor in the catalog (test helper).
    fn ed_pos(id: &str) -> usize {
        registry::EDITORS.iter().position(|e| e.id == id).unwrap()
    }

    #[test]
    fn editor_section_renders_the_whole_catalog_with_none_preselected() {
        let s = render(&app_on("xiao-esp32c6", 0), W, H);
        assert!(s.contains("EDITOR SUPPORT"), "section heading present:\n{s}");
        for e in registry::EDITORS {
            assert!(s.contains(e.display), "editor `{}` must be listed:\n{s}", e.display);
        }
        // A radio list, styled exactly like the toolchain sections: the registry
        // default is selected, everything else (including `none`) is an open slot.
        assert!(s.contains("(•) none"), "none selected by default:\n{s}");
        assert!(s.contains("( ) VS Code"), "VS Code offered, unselected:\n{s}");
        assert!(s.contains("( ) Helix"), "Helix offered, unselected:\n{s}");
        assert!(s.contains("( ) Zed"), "Zed offered, unselected:\n{s}");
        assert!(s.contains("( ) Neovim"), "Neovim offered, unselected:\n{s}");
    }

    /// EDITOR is the one section that is live on EVERY stack — the diagnostic it
    /// fixes is backend-independent — so it must stay focusable and un-greyed
    /// everywhere, including where all three toolchain sections are dead.
    #[test]
    fn editor_section_is_live_on_every_stack_and_under_uferris() {
        let mut cases: Vec<(&str, usize)> = Vec::new();
        for (i, v) in registry::VARIANTS.iter().enumerate() {
            let _ = i;
            for si in 0..v.stacks.len() {
                cases.push((v.id, si));
            }
        }
        for (chip, si) in cases {
            let app = app_on(chip, si);
            let rows = app.focusable_rows();
            for e in 0..registry::EDITORS.len() {
                assert!(
                    rows.contains(&Row::Editor(e)),
                    "{chip}/stack{si}: editor row {e} must be focusable"
                );
            }
            let s = render(&app, W, H);
            assert!(s.contains("EDITOR SUPPORT"), "{chip}/stack{si}: section missing");
            assert!(!s.contains("(-) VS Code"), "{chip}/stack{si}: editors never grey");
        }

        // µFerris takes over the toolchain sections, but not the user's editor.
        let mut uf = app_on("xiao-esp32c3", 0);
        // Pick an editor FIRST, so this proves a real user choice survives rather
        // than just observing the default sit there.
        uf.focus = Row::Editor(ed_pos("vscode"));
        uf.activate();
        uf.uferris_on = true;
        uf.ensure_focus_valid();
        assert!(
            uf.focusable_rows().contains(&Row::Editor(ed_pos("vscode"))),
            "editor rows survive µFerris"
        );
        assert!(render(&uf, W, H).contains("(•) VS Code"), "editor selection survives µFerris");
    }

    #[test]
    fn selecting_an_editor_updates_the_config_and_the_equivalent_command() {
        let mut app = app_on("xiao-esp32c6", 0);

        // Single-select: choosing Helix REPLACES the default `none`.
        app.focus = Row::Editor(ed_pos("helix"));
        app.activate();
        let s = render(&app, W, H);
        assert!(s.contains("(•) Helix"), "Helix selected after activate:\n{s}");
        // Single-select: the editor axis holds ONE index, so picking Helix
        // necessarily clears the default. Asserted on the resolved config rather
        // than by searching for "(•) none" — `none` is also a LOGGING and a
        // FRAMEWORK entry, so that string cannot tell the sections apart.
        let (cfg, _) = app.resolved().unwrap();
        assert_eq!(cfg.editor.id, "helix", "choosing Helix must replace the default");
        assert!(flat(&s).contains("--editor helix"), "command carries the editor:\n{s}");
        assert!(s.contains("Editor:"), "summary shows the editor row:\n{s}");
        assert!(s.contains("Helix"), "summary lists the choice:\n{s}");

        // VS Code is now a NON-default choice, so it must show up in the command.
        app.focus = Row::Editor(ed_pos("vscode"));
        app.activate();
        let (cfg, _) = app.resolved().unwrap();
        assert_eq!(cfg.editor.id, "vscode");
        assert_eq!(cfg.editor.path, Some(".vscode/settings.json"));
        let s = render(&app, W, H);
        assert!(flat(&s).contains("--editor vscode"), "command carries --editor vscode:\n{s}");

        // …and back to `none`, which IS the default, so the flag drops off again.
        app.focus = Row::Editor(ed_pos("none"));
        app.activate();
        let (cfg, _) = app.resolved().unwrap();
        assert_eq!(cfg.editor.id, "none");
        assert!(cfg.editor.path.is_none(), "`none` writes nothing");
        assert_eq!(model::equivalent_command(&cfg), "xiao-generate --chip xiao-esp32c6");
    }

    /// Neovim's file needs `exrc` + `:trust`, so the TUI must say so rather than
    /// imply selecting it is enough. The caveat shows only once that editor is
    /// actually selected (an unselected row emits nothing).
    #[test]
    fn neovim_caveat_is_surfaced_only_while_it_is_selected() {
        let mut app = app_on("xiao-esp32c6", 0);
        let s = render(&app, W, H);
        assert!(!s.contains("exrc"), "no caveat while Neovim is unselected:\n{s}");

        app.focus = Row::Editor(ed_pos("neovim"));
        app.activate();
        let s = render(&app, W, H);
        assert!(s.contains("(•) Neovim"), "Neovim selected:\n{s}");
        assert!(s.contains("exrc"), "caveat shown once Neovim is selected:\n{s}");
        // …and it is the registry's caveat, not TUI-local prose.
        assert!(
            registry::editor("neovim").unwrap().caveat.is_some(),
            "the caveat must come from the registry"
        );
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
            // Scoped to the sections ABOVE EDITOR SUPPORT: the editor axis stays
            // live under µFerris and its default renders as `(•) none`, which
            // would otherwise trip this check.
            let toolchain = on.split("EDITOR SUPPORT").next().unwrap_or(&on);
            assert!(
                !toolchain.contains(filled),
                "no toolchain selection under µFerris (`{filled}`):\n{on}"
            );
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
    /// The glyphs RASCII's `block` charset can paint the crab with. Shading
    /// characters, not the quadrant set: RASCII picks one per cell by luminance,
    /// and on flat vector art nearly every cell lands on `░` — the picture is
    /// carried by each cell's colour, not by glyph variety.
    fn is_crab_glyph(sym: &str) -> bool {
        matches!(sym, "░" | "▒" | "▓" | "█")
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


    /// Interior left column of the Summary panel (just inside its left border) —
    /// replicates `draw`'s layout split so the crab's centring can be checked.
    fn summary_ix(w: u16, h: u16) -> u16 {
        summary_interior_rect(w, h).x + 1
    }


    // -----------------------------------------------------------------------
    // Ferris, measured on the REAL rendered buffer. The art itself is produced at
    // runtime by RASCII from the logo (see the `ferris` module, which unit-tests
    // the fit arithmetic); what matters here is that `draw_summary` places it
    // correctly and drops it when the panel cannot hold it.
    // -----------------------------------------------------------------------

    /// Page width whose Summary interior is too narrow to earn a crab, so the
    /// width guard is exercised rather than assumed.
    const NARROW_W: u16 = 60;

    /// Page height that leaves too few spare rows under the summary text for any
    /// crab, exercising the height guard.
    const SHORT_H: u16 = 26;

    /// A panel with no room must render cleanly with NO crab, rather than a
    /// clipped or wrapped one. This is the failure the fit guard exists to prevent.
    #[test]
    fn crab_is_omitted_when_the_panel_cannot_hold_it() {
        for (w, h, why) in [(NARROW_W, H, "too narrow"), (W, SHORT_H, "too short")] {
            let buf = render_buf(&app_on("xiao-esp32c6", 0), w, h);
            assert!(
                crab_cells(&buf).is_empty(),
                "a {why} panel ({w}x{h}) still painted crab glyphs"
            );
        }
    }

    /// On a panel with room, the crab is actually painted, sits inside the Summary
    /// panel, and is horizontally centred there as a block.
    #[test]
    fn crab_is_painted_and_centred_in_the_summary_panel() {
        let (iw, _) = summary_interior(W, H);
        let ix = summary_ix(W, H);
        let buf = render_buf(&app_on("xiao-esp32c6", 0), W, H);
        let cells = crab_cells(&buf);
        assert!(!cells.is_empty(), "no crab painted on a {W}x{H} page");

        let lo = cells.iter().map(|c| c.0).min().unwrap();
        let hi = cells.iter().map(|c| c.0).max().unwrap();
        assert!(lo >= ix, "crab starts at {lo}, left of the Summary interior at {ix}");
        assert!(
            hi < ix + iw,
            "crab ends at {hi}, past the Summary interior ending at {}",
            ix + iw
        );

        // Centred as a unit: the slack either side must match within a column
        // (an odd remainder cannot be split evenly).
        let left = lo - ix;
        let right = (ix + iw - 1).saturating_sub(hi);
        assert!(
            left.abs_diff(right) <= 1,
            "crab is off-centre: {left} columns of slack on the left, {right} on the right"
        );
    }

    /// The crab must NOT move vertically when the summary text above it changes
    /// length. This is the drift the anchor-to-bottom placement fixes: it used to
    /// be centred in the space below the text, so its row tracked the text height
    /// and it visibly jumped as options were toggled.
    ///
    /// Two configs with genuinely different summary lengths must paint the crab's
    /// top edge on the same row. An ESP variant (universal toolchain sections +
    /// µFerris line) and an RP2040 variant (neither) differ by several rows, so
    /// the equality below is a real invariant, not a coincidence — the content
    /// bottoms are asserted different to prove that.
    #[test]
    fn crab_does_not_drift_when_the_summary_length_changes() {
        // Top row of the crab, and the last row carrying summary *text*, both
        // confined to the Summary panel INTERIOR — the scan must exclude the
        // full-width bottom status bar, or every config reports text on the last
        // page row and the precondition can never hold.
        let probe = |app: &App| -> (u16, u16) {
            let rect = summary_interior_rect(W, H);
            let (ix, iy) = (rect.x + 1, rect.y + 1);
            let (iw, ih) = (rect.width - 2, rect.height - 2);
            let buf = render_buf(app, W, H);
            let crab_top = crab_cells(&buf).iter().map(|c| c.1).min();
            let mut text_bottom = iy;
            for y in iy..iy + ih {
                for x in ix..ix + iw {
                    if let Some(cell) = buf.cell((x, y)) {
                        let s = cell.symbol();
                        if !s.trim().is_empty() && !is_crab_glyph(s) {
                            text_bottom = y;
                        }
                    }
                }
            }
            (crab_top.expect("a crab must be painted at 140x44"), text_bottom)
        };

        // esp-hal (shows FLASHING/LOGGING/FRAMEWORK in the summary) vs esp-idf
        // (option groups do not apply, so those lines are absent) — a several-row
        // difference in summary length.
        let (esp_crab, esp_text) = probe(&app_on("xiao-esp32c6", 0));
        let (rp_crab, rp_text) = probe(&app_on("xiao-esp32c6", 1));

        assert_ne!(
            esp_text, rp_text,
            "precondition: the two summaries must differ in length for this test to mean anything"
        );
        assert_eq!(
            esp_crab, rp_crab,
            "crab drifted: top row {esp_crab} on ESP vs {rp_crab} on RP2040 — it must be \
             anchored to the panel, not to the summary text"
        );
    }

    /// The crab must look identical in both themes.
    ///
    /// Its inks come from the logo's own pixels rather than the palette, so this
    /// is a property of rendering from the image — but it was an explicit
    /// requirement, and a future "theme the art" change would silently break it.
    #[test]
    fn crab_colour_does_not_follow_the_theme() {
        let mut dark = app_on("xiao-esp32c6", 0);
        dark.dark = true;
        let mut light = app_on("xiao-esp32c6", 0);
        light.dark = false;

        let d = crab_cells(&render_buf(&dark, W, H));
        let l = crab_cells(&render_buf(&light, W, H));
        assert!(!d.is_empty() && !l.is_empty(), "both themes must paint a crab");
        assert_eq!(d.len(), l.len(), "themes painted different numbers of crab cells");
        for (a, b) in d.iter().zip(l.iter()) {
            assert_eq!(
                (a.0, a.1, &a.2, a.3),
                (b.0, b.1, &b.2, b.3),
                "a crab cell differs between dark and light themes"
            );
        }
    }

    /// Not run in the normal suite; dumps the crab's silhouette and the fitted
    /// width for a range of terminal sizes, so the art can be eyeballed without
    /// launching the TUI:
    ///   cargo test --bin xiao-generate -- --ignored --nocapture dump_ferris_fit
    #[test]
    #[ignore]
    fn dump_ferris_fit() {
        for (w, h) in [(100, 30), (120, 35), (140, 40), (170, 45), (200, 50)] {
            let (iw, _) = summary_interior(w, h);
            let buf = render_buf(&app_on("xiao-esp32c6", 0), w, h);
            let cells = crab_cells(&buf);
            println!("\n===== terminal {w}x{h} — summary interior {iw} =====");
            if cells.is_empty() {
                println!("(no crab — panel too small)");
                continue;
            }
            let (x0, x1) = (
                cells.iter().map(|c| c.0).min().unwrap(),
                cells.iter().map(|c| c.0).max().unwrap(),
            );
            let (y0, y1) = (
                cells.iter().map(|c| c.1).min().unwrap(),
                cells.iter().map(|c| c.1).max().unwrap(),
            );
            println!("art {}x{} cells at ({x0},{y0})", x1 - x0 + 1, y1 - y0 + 1);
            for y in y0..=y1 {
                let row: String = (x0..=x1)
                    .map(|x| {
                        buf.cell((x, y))
                            .filter(|c| is_crab_glyph(c.symbol()))
                            .map_or('.', |_| '#')
                    })
                    .collect();
                println!("{row}");
            }
        }
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


}
