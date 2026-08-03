//! Ferris, the Summary panel's centrepiece.
//!
//! The art is produced at runtime by [RASCII](https://github.com/orhnk/RASCII)
//! from the official logo, not from hand-tuned tables. RASCII picks one character
//! per cell from that cell's pixel luminance and paints it in the cell's own
//! 24-bit colour, so the picture is carried almost entirely by **colour** rather
//! than by glyph variety — on flat vector art like Ferris ~99% of visible cells
//! land on the same glyph.
//!
//! The practical consequence is that detail scales with the **number of cells**
//! and nothing else. The crab is drawn at ONE fixed size — [`ART_WIDTH`] columns
//! — everywhere and in every view: compact, but large enough that the eyes,
//! claws and mouth all read. It never grows to fill a wide panel; a panel too
//! small to hold that fixed size simply shows no crab rather than a clipped or
//! shrunken one.
//!
//! Two properties fall out of rendering from the image itself and are worth
//! naming, because both were requirements:
//!
//! - **Theme independence.** The inks come from the PNG's own pixels, so the crab
//!   looks identical in light and dark mode without a special case.
//! - **Correct proportions.** RASCII derives the row count from the source aspect
//!   ratio (halving it for the ~1:2 terminal cell), so the crab is never stretched
//!   to fill a box.
//!
//! Output is plain text with SGR colour, so it renders identically in every
//! terminal — no Kitty/iTerm2/Sixel protocol detection and no HiDPI scaling.

use std::io::Write as _;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use ansi_to_tui::IntoText;
use rascii_art::{charsets, render_to, RenderOptions};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};

/// The logo, embedded so the binary is self-contained — nothing is resolved
/// relative to the working directory at runtime.
const FERRIS_PNG: &[u8] = include_bytes!("../../assets/ferris.png");

/// The one width the crab is ever rendered at, in terminal columns.
///
/// Kept compact — this is a corner mascot, not the content — but large enough
/// that the eyes, the gap between the claws and the mouth all resolve; below
/// ~30 columns those collapse and it reads as a plain orange blob. It is a hard
/// constant, not a starting point: the art is this size in every view and at
/// every terminal size, never scaled to fit.
const ART_WIDTH: u32 = 34;

/// RASCII opens the image by path, so the embedded bytes are spilled to a temp
/// file once per process. The [`tempfile::NamedTempFile`] is parked in the static
/// so the file outlives every render and is cleaned up on exit.
fn png_path() -> Option<&'static Path> {
    static FILE: OnceLock<Option<tempfile::NamedTempFile>> = OnceLock::new();
    FILE.get_or_init(|| {
        let mut f = tempfile::Builder::new()
            .prefix("xiao-generate-ferris")
            .suffix(".png")
            .tempfile()
            .ok()?;
        f.write_all(FERRIS_PNG).ok()?;
        f.flush().ok()?;
        Some(f)
    })
    .as_ref()
    .map(|f| f.path())
}

/// Render at `width` columns and convert RASCII's ANSI into ratatui spans.
///
/// Cached on the width: the draw loop re-runs on every event, and this decodes a
/// multi-megapixel PNG. Without the cache a keypress would re-decode it.
fn art(width: u32) -> Option<Text<'static>> {
    static CACHE: OnceLock<Mutex<Option<(u32, Text<'static>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));

    // A poisoned lock only means some other thread panicked mid-render; the
    // cached art is still sound to reuse, so recover rather than propagate.
    let mut slot = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((w, text)) = slot.as_ref() {
        if *w == width {
            return Some(text.clone());
        }
    }

    // `render_to` takes the path as a string, so a non-UTF-8 temp dir means no
    // crab rather than a panic.
    let path = png_path()?.to_str()?;
    let mut buf = String::new();
    render_to(
        path,
        &mut buf,
        &RenderOptions::new()
            .width(width)
            .colored(true)
            .charset(charsets::BLOCK),
    )
    .ok()?;

    let mut text = buf.into_text().ok()?;

    // Drop the background off every span.
    //
    // RASCII colours the FOREGROUND only — its output contains no `48;2`
    // sequences at all — but `ansi-to-tui` still stamps `bg: Some(Color::Reset)`
    // on each span it produces. `Reset` is not "no opinion": it resolves to the
    // terminal's own default background and takes precedence over the `Paragraph`
    // style, so it punched a slab of the terminal default through the Summary
    // panel — a white rectangle behind the crab in the dark theme.
    //
    // Since the background carries no information here, clearing it lets the
    // themed panel background show through the transparent parts of the logo,
    // in both themes. The foreground is untouched, so the crab keeps the image's
    // own colours and stays theme-independent.
    for line in &mut text.lines {
        for span in &mut line.spans {
            span.style.bg = None;
        }
    }

    match_left_eye_to_right(&mut text);

    *slot = Some((width, text.clone()));
    Some(text)
}

/// The 2x2 grid of the fixed art's eyes, in (rows, cols): how many rows tall an
/// eye is from its glint row down, and the columns spanned relative to the glint.
/// The eye is 3 rows (glint, pupil, under-shading) by 4 columns (glint one in from
/// the left edge). Cells just outside are plain orange in both halves, so a copy
/// that grazes them is invisible.
const EYE_ROWS: usize = 3;
const EYE_DC: std::ops::RangeInclusive<i32> = -1..=2;

/// Make the LEFT eye pixel-identical to the right eye.
///
/// RASCII downsamples the two eyes to slightly different cell patterns — the left
/// eye came out with an extra bright cell on the glint row and a different
/// under-eye shading cell — so the eyes did not match. The earlier fixes were both
/// wrong: mirroring the whole crab shifts an eye and flips its glint inward (the
/// crab goes cross-eyed), and normalising only the white glint leaves the shape
/// cells different. The eyes only truly match if every cell matches.
///
/// So copy the right eye's cell block verbatim onto the left as a straight,
/// UN-flipped translation. Both eyes then share one appearance AND one gaze
/// direction (the glint stays on the same side of each eye — parallel, not
/// converging).
///
/// The eyes are found by their glints: the one left/right column pair that is
/// bright in BOTH halves at mirror-image columns (the stray extra bright cell has
/// no bright mirror, so it is not mistaken for the glint). The right eye's
/// [`EYE_ROWS`]x4 block around that glint is then written onto the left glint.
fn match_left_eye_to_right(text: &mut Text<'static>) {
    let w = text.width();
    if w < 2 {
        return;
    }
    let rows = text.lines.len();

    let is_glint =
        |fg: Option<Color>| matches!(fg, Some(Color::Rgb(r, g, b)) if r >= 150 && g >= 150 && b >= 150);

    // Expand every row to a full-width (glyph, fg) grid.
    let mut grid: Vec<Vec<(char, Option<Color>)>> = text
        .lines
        .iter()
        .map(|line| {
            let mut cells = Vec::with_capacity(w);
            for span in &line.spans {
                for ch in span.content.chars() {
                    cells.push((ch, span.style.fg));
                }
            }
            cells.resize(w, (' ', None));
            cells
        })
        .collect();

    // Locate the eyes: the topmost column `c` in the left half whose cell AND its
    // mirror `w-1-c` are both glints. That pair is the two eyes' glints.
    let mut pair = None;
    'find: for (r, row) in grid.iter().enumerate() {
        for c in 0..w / 2 {
            if is_glint(row[c].1) && is_glint(row[w - 1 - c].1) {
                pair = Some((r, c, w - 1 - c));
                break 'find;
            }
        }
    }
    let Some((glint_row, left_col, right_col)) = pair else {
        return;
    };

    // Translate the right eye's block onto the left, glints aligned.
    for dr in 0..EYE_ROWS {
        let r = glint_row + dr;
        if r >= rows {
            break;
        }
        for dc in EYE_DC {
            let src = right_col as i32 + dc;
            let dst = left_col as i32 + dc;
            if (0..w as i32).contains(&src) && (0..w as i32).contains(&dst) {
                grid[r][dst as usize] = grid[r][src as usize];
            }
        }
    }

    // Re-coalesce each row's equal-fg runs back into spans.
    for (line, cells) in text.lines.iter_mut().zip(grid) {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut run = String::new();
        let mut run_fg: Option<Option<Color>> = None;
        for (ch, fg) in cells {
            if run_fg != Some(fg) {
                if let Some(prev) = run_fg.take() {
                    spans.push(fg_span(std::mem::take(&mut run), prev));
                }
                run_fg = Some(fg);
            }
            run.push(ch);
        }
        if let Some(prev) = run_fg {
            spans.push(fg_span(run, prev));
        }
        line.spans = spans;
    }
}

/// A span with only a foreground colour set (background stays inherited).
fn fg_span(content: String, fg: Option<Color>) -> Span<'static> {
    match fg {
        Some(c) => Span::styled(content, Style::default().fg(c)),
        None => Span::raw(content),
    }
}

/// The crab at its fixed [`ART_WIDTH`], centred as a block within a panel
/// interior of `cols` x `rows` — each line prefixed with enough spaces to centre
/// the whole block as a unit.
///
/// The size never depends on `cols`/`rows`; they are only used to decide whether
/// the crab fits. `None` when the interior cannot hold the fixed art (in either
/// dimension), so the caller omits it entirely rather than clipping or shrinking
/// it.
pub fn lines(cols: u16, rows: u16) -> Option<Vec<Line<'static>>> {
    let text = art(ART_WIDTH)?;

    let art_w = text.width();
    let art_h = text.lines.len();
    if usize::from(cols) < art_w || usize::from(rows) < art_h {
        return None;
    }

    let pad = " ".repeat((usize::from(cols) - art_w) / 2);

    Some(
        text.lines
            .into_iter()
            .map(|line| {
                if pad.is_empty() {
                    return line;
                }
                let mut spans = Vec::with_capacity(line.spans.len() + 1);
                spans.push(Span::raw(pad.clone()));
                spans.extend(line.spans);
                Line::from(spans)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    /// The embedded asset must be a real PNG, or RASCII cannot decode it and the
    /// crab silently never renders. A `render_to` at the fixed width is the direct
    /// proof it decodes; the signature check turns a bad embed into a clear
    /// failure here rather than a mysteriously missing crab.
    #[test]
    fn the_embedded_logo_is_a_decodable_png() {
        assert_eq!(
            &super::FERRIS_PNG[..8],
            b"\x89PNG\r\n\x1a\n",
            "assets/ferris.png is not a PNG"
        );
        assert!(
            super::art(super::ART_WIDTH).is_some(),
            "RASCII could not render the embedded logo"
        );
    }

    /// The two eyes must be pixel-identical — every cell of the left eye equal to
    /// the corresponding cell of the right eye in both glyph and colour. This is
    /// exactly what `match_left_eye_to_right` copies, so it guards that the copy
    /// happened and stays correct if the render shifts.
    #[test]
    fn the_two_eyes_are_identical() {
        use ratatui::style::Color;
        let text = super::art(super::ART_WIDTH).expect("art");
        let w = text.lines.iter().map(|l| l.width()).max().unwrap_or(0);
        let grid: Vec<Vec<(char, Option<Color>)>> = text
            .lines
            .iter()
            .map(|line| {
                let mut cells = Vec::new();
                for s in &line.spans {
                    for ch in s.content.chars() {
                        cells.push((ch, s.style.fg));
                    }
                }
                cells.resize(w, (' ', None));
                cells
            })
            .collect();
        let is_glint =
            |fg: Option<Color>| matches!(fg, Some(Color::Rgb(r, g, b)) if r >= 150 && g >= 150 && b >= 150);

        // Same eye-finding rule as the fix: the mirror-symmetric glint pair.
        let mut pair = None;
        'find: for (r, row) in grid.iter().enumerate() {
            for c in 0..w / 2 {
                if is_glint(row[c].1) && is_glint(row[w - 1 - c].1) {
                    pair = Some((r, c, w - 1 - c));
                    break 'find;
                }
            }
        }
        let (gr, lc, rc) = pair.expect("the render must have a symmetric eye-glint pair");

        for dr in 0..super::EYE_ROWS {
            for dc in super::EYE_DC {
                let r = gr + dr;
                let l = (lc as i32 + dc) as usize;
                let rr = (rc as i32 + dc) as usize;
                assert_eq!(
                    grid[r][l], grid[r][rr],
                    "eyes differ at row {r}: left col {l} = {:?} vs right col {rr} = {:?}",
                    grid[r][l], grid[r][rr]
                );
            }
        }
    }

    /// A panel too small for the fixed-size crab yields nothing, rather than
    /// specks or a clipped shell.
    #[test]
    fn a_tiny_panel_gets_no_crab() {
        assert!(super::lines(10, 3).is_none());
        assert!(super::lines(0, 0).is_none());
        // One row short of the art's height must also omit it, not clip it.
        let art_h = super::art(super::ART_WIDTH).unwrap().lines.len() as u16;
        assert!(super::lines(200, art_h - 1).is_none(), "a too-short panel must omit the crab");
    }

    /// The crab is ONE fixed size regardless of how much room the panel has — the
    /// whole point of this request. A big panel must not grow it.
    #[test]
    fn the_crab_is_the_same_fixed_size_in_every_panel() {
        let sizes = [(40u16, 20u16), (80, 30), (160, 50), (300, 120)];
        let mut dims: Option<(usize, usize)> = None;
        for (cols, rows) in sizes {
            let lines = super::lines(cols, rows)
                .unwrap_or_else(|| panic!("{cols}x{rows} should hold the fixed-size crab"));
            let h = lines.len();
            // Width of the art itself, ignoring the centring pad.
            let w = lines
                .iter()
                .flat_map(|l| l.spans.iter())
                .filter(|s| s.style.fg.is_some())
                .flat_map(|s| s.content.chars())
                .count()
                .max(1);
            match dims {
                None => dims = Some((h, w)),
                Some((ph, pw)) => {
                    assert_eq!(h, ph, "crab height changed with panel size ({cols}x{rows})");
                    assert_eq!(w, pw, "crab ink width changed with panel size ({cols}x{rows})");
                }
            }
        }
        // …and it really is the compact fixed size, not something a fitter landed
        // on: tall enough for the face to read, short enough to stay a corner
        // mascot. If ART_WIDTH is retuned this bound moves with it deliberately.
        let h = dims.unwrap().0;
        assert!(
            (11..=15).contains(&h),
            "the fixed crab is {h} rows tall — outside the intended compact range"
        );
    }

    /// The rendered art must fit inside the box it was asked for, and be colour.
    #[test]
    fn the_rendered_crab_fits_its_box_and_carries_colour() {
        let cols = 60u16;
        let rows = 24u16;
        let lines = super::lines(cols, rows).expect("a 60x24 panel should hold a crab");

        assert!(!lines.is_empty(), "no lines rendered");
        assert!(
            lines.len() <= usize::from(rows),
            "rendered {} rows into a {rows}-row budget",
            lines.len()
        );
        for (i, line) in lines.iter().enumerate() {
            assert!(
                line.width() <= usize::from(cols),
                "row {i} is {} wide, past the {cols}-column budget",
                line.width()
            );
        }
        // Colour is the whole picture here — glyph variety carries almost none of
        // it — so art with no foreground colours would be a silent failure.
        let coloured = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.fg.is_some())
            .count();
        assert!(coloured > 0, "the art carries no foreground colour");
    }

    /// No span may carry a background colour.
    ///
    /// `ansi-to-tui` stamps `bg: Some(Color::Reset)` on everything it converts,
    /// even though RASCII only ever writes foreground colour. `Reset` outranks the
    /// `Paragraph`'s themed style and resolves to the *terminal's* default
    /// background, which painted a slab of it behind the crab — a white rectangle
    /// in the dark theme. The art must inherit the panel background instead.
    #[test]
    fn the_art_carries_no_background_colour() {
        let lines = super::lines(60, 24).expect("art");
        for (i, line) in lines.iter().enumerate() {
            for (j, span) in line.spans.iter().enumerate() {
                assert_eq!(
                    span.style.bg, None,
                    "row {i} span {j} sets bg={:?}; it would override the panel's theme",
                    span.style.bg
                );
            }
        }
    }

    /// Re-rendering at one width must be stable — the cache returns clones, and a
    /// clone that diverged from the original would show up as a flickering crab.
    #[test]
    fn repeated_renders_at_the_same_width_agree() {
        let a = super::lines(50, 20).expect("art");
        let b = super::lines(50, 20).expect("art");
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.width(), y.width());
        }
    }
}








