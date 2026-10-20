//! Paints a grid into the PC image: 600 × 360 RGBA, a 6 × 12 px cell per character, in
//! JetBrains Mono (spec "The PC screen"). Each distinct glyph is rasterised once with
//! `swash` and kept for every attach. Colours, bold, dim, inverse and the cursor block
//! are drawn; underlines, strikeout and blinking are not.

use std::collections::HashMap;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

use crate::{COLUMNS, ROWS};

/// A cell's size in the PC image.
pub(crate) const CELL_WIDTH: u32 = 6;
pub(crate) const CELL_HEIGHT: u32 = 12;
/// The PC image's size: [`COLUMNS`] × [`ROWS`] cells.
pub const PC_IMAGE_WIDTH: u32 = COLUMNS as u32 * CELL_WIDTH;
pub const PC_IMAGE_HEIGHT: u32 = ROWS as u32 * CELL_HEIGHT;

/// JetBrains Mono, the variable font (`desktop/world-assets/third-party/jetbrains-mono`).
static FONT: &[u8] =
    include_bytes!("../../../world-assets/third-party/jetbrains-mono/JetBrainsMono[wght].ttf");

/// The text and glass colours where the session sets none.
const FOREGROUND: [u8; 3] = [229, 229, 229];
const BACKGROUND: [u8; 3] = [0, 0, 0];

/// One rasterised glyph: coverage, placed relative to the cell's left edge and baseline.
struct Glyph {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

pub(crate) struct Painter {
    context: ScaleContext,
    /// Each glyph drawn so far, by character and boldness; `None` if the font lacks it.
    glyphs: HashMap<(char, bool), Option<Glyph>>,
    /// The font size that makes an advance [`CELL_WIDTH`] px wide.
    pixels_per_em: f32,
    /// The baseline's distance from a cell's top, in px.
    baseline: i32,
}

impl Painter {
    pub(crate) fn new() -> Painter {
        let font = font();
        let metrics = font.metrics(&[]);
        let advance = font
            .glyph_metrics(&[])
            .advance_width(font.charmap().map('M'));
        let pixels_per_em = CELL_WIDTH as f32 * metrics.units_per_em as f32 / advance;
        // The font's line (ascent + descent) is a little taller than a cell: split the
        // difference so capitals and descenders both stay inside.
        let line = metrics.ascent + metrics.descent;
        let baseline = (CELL_HEIGHT as f32 * metrics.ascent / line).round() as i32;
        Painter {
            context: ScaleContext::new(),
            glyphs: HashMap::new(),
            pixels_per_em,
            baseline,
        }
    }

    /// Paints `lines` (viewport lines, top first) of `term` into `rgba`, a
    /// [`PC_IMAGE_WIDTH`] × [`PC_IMAGE_HEIGHT`] RGBA image.
    pub(crate) fn paint(
        &mut self,
        term: &Term<VoidListener>,
        lines: impl Iterator<Item = usize>,
        rgba: &mut [u8],
    ) {
        let grid = term.grid();
        let colors = term.colors();
        let cursor = term
            .mode()
            .contains(TermMode::SHOW_CURSOR)
            .then_some(grid.cursor.point);
        for line in lines.filter(|line| *line < ROWS as usize) {
            let row = &grid[Line(line as i32)];
            let cells: Vec<(Look, &Cell)> = (0..COLUMNS as usize)
                .map(|column| {
                    let at_cursor = cursor.is_some_and(|point| {
                        point.line.0 == line as i32 && point.column.0 == column
                    });
                    (
                        Look::of(&row[Column(column)], colors, at_cursor),
                        &row[Column(column)],
                    )
                })
                .collect();
            // Backgrounds first: a wide glyph spans the next cell's background.
            for (column, (look, _)) in cells.iter().enumerate() {
                fill(
                    rgba,
                    column as u32 * CELL_WIDTH,
                    line as u32 * CELL_HEIGHT,
                    CELL_WIDTH,
                    look.background,
                );
            }
            for (column, (look, cell)) in cells.iter().enumerate() {
                if !draws_glyph(cell) {
                    continue;
                }
                let width = if cell.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                let bold = cell.flags.contains(Flags::BOLD);
                let x = column as u32 * CELL_WIDTH;
                let y = line as u32 * CELL_HEIGHT;
                let (pixels_per_em, baseline) = (self.pixels_per_em, self.baseline);
                let context = &mut self.context;
                let glyph = self
                    .glyphs
                    .entry((cell.c, bold))
                    .or_insert_with(|| rasterise(context, cell.c, bold, pixels_per_em));
                if let Some(glyph) = glyph {
                    blend(
                        rgba,
                        glyph,
                        x,
                        y,
                        width * CELL_WIDTH,
                        baseline,
                        look.foreground,
                    );
                }
            }
        }
    }
}

/// The font, parsed (cheaply, from the embedded bytes).
fn font() -> FontRef<'static> {
    FontRef::from_index(FONT, 0).expect("the embedded JetBrains Mono parses")
}

fn draws_glyph(cell: &Cell) -> bool {
    cell.c != ' '
        && cell.c != '\0'
        && !cell
            .flags
            .intersects(Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
}

fn rasterise(
    context: &mut ScaleContext,
    character: char,
    bold: bool,
    pixels_per_em: f32,
) -> Option<Glyph> {
    let font = font();
    let id = match font.charmap().map(character) {
        0 => font.charmap().map(stand_in(character)?),
        id => id,
    };
    if id == 0 {
        return None;
    }
    let weight = if bold { 700.0 } else { 400.0 };
    let mut scaler = context
        .builder(font)
        .size(pixels_per_em)
        .hint(true)
        .variations(&[("wght", weight)])
        .build();
    let image = Render::new(&[Source::Outline])
        .format(Format::Alpha)
        .render(&mut scaler, id)?;
    Some(Glyph {
        left: image.placement.left,
        top: image.placement.top,
        width: image.placement.width,
        height: image.placement.height,
        coverage: image.data,
    })
}

/// A shape JetBrains Mono has for a symbol Claude Code draws that it lacks (found
/// against the font in ticket 10), so the PC image shows the mark rather than a gap.
fn stand_in(character: char) -> Option<char> {
    Some(match character {
        '⏵' => '▸',
        '⏺' | '◉' | '◐' | '◑' | '⬢' => '●',
        '⎿' => '└',
        '✻' | '✶' | '✳' | '✢' | '✽' | '★' => '*',
        '✓' | '✔' => '√',
        '✗' | '✘' => '✕',
        '○' => '◯',
        '☐' | '⧉' => '□',
        '☑' | '⏹' => '■',
        '⏸' => '▪',
        _ => return None,
    })
}

/// A cell's two colours after inverse, dim and the cursor.
struct Look {
    foreground: [u8; 3],
    background: [u8; 3],
}

impl Look {
    fn of(cell: &Cell, colors: &Colors, at_cursor: bool) -> Look {
        let mut foreground = cell.fg;
        if cell.flags.contains(Flags::DIM) {
            if let Color::Named(named) = foreground {
                foreground = Color::Named(named.to_dim());
            }
        }
        let mut foreground = rgb(foreground, colors);
        if cell.flags.contains(Flags::DIM) && !matches!(cell.fg, Color::Named(_)) {
            foreground = foreground.map(|channel| (channel as u32 * 2 / 3) as u8);
        }
        let background = rgb(cell.bg, colors);
        let (foreground, background) = if cell.flags.contains(Flags::INVERSE) != at_cursor {
            (background, foreground)
        } else {
            (foreground, background)
        };
        Look {
            foreground,
            background,
        }
    }
}

fn rgb(color: Color, colors: &Colors) -> [u8; 3] {
    let set = match color {
        Color::Spec(rgb) => return [rgb.r, rgb.g, rgb.b],
        Color::Indexed(index) => colors[index as usize],
        Color::Named(named) => colors[named],
    };
    if let Some(set) = set {
        return [set.r, set.g, set.b];
    }
    match color {
        Color::Indexed(index) => palette(index),
        Color::Named(named) => named_default(named),
        Color::Spec(_) => unreachable!("returned above"),
    }
}

fn named_default(named: NamedColor) -> [u8; 3] {
    let dim = |color: [u8; 3]| color.map(|channel| (channel as u32 * 2 / 3) as u8);
    match named {
        NamedColor::Foreground | NamedColor::Cursor => FOREGROUND,
        NamedColor::Background => BACKGROUND,
        NamedColor::BrightForeground => [255, 255, 255],
        NamedColor::DimForeground => dim(FOREGROUND),
        // DimBlack..DimWhite follow Foreground in order; BrightForeground is after them.
        dimmed if (dimmed as usize) > NamedColor::Cursor as usize => dim(palette(
            (dimmed as usize - NamedColor::DimBlack as usize) as u8,
        )),
        normal => palette(normal as u8),
    }
}

/// The xterm 256-colour palette.
fn palette(index: u8) -> [u8; 3] {
    const SIXTEEN: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    match index {
        0..=15 => SIXTEEN[index as usize],
        16..=231 => {
            let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
            let index = index - 16;
            [level(index / 36), level(index / 6 % 6), level(index % 6)]
        }
        232..=255 => {
            let grey = 8 + (index - 232) * 10;
            [grey, grey, grey]
        }
    }
}

fn fill(rgba: &mut [u8], x: u32, y: u32, width: u32, color: [u8; 3]) {
    for row in y..y + CELL_HEIGHT {
        for column in x..(x + width).min(PC_IMAGE_WIDTH) {
            let at = ((row * PC_IMAGE_WIDTH + column) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }
}

/// Blends `glyph` in `color` over the cell box at `x`, `y` (`width` px wide), clipped
/// to the box.
fn blend(
    rgba: &mut [u8],
    glyph: &Glyph,
    x: u32,
    y: u32,
    width: u32,
    baseline: i32,
    color: [u8; 3],
) {
    let right = (x + width).min(PC_IMAGE_WIDTH) as i32;
    let bottom = (y + CELL_HEIGHT) as i32;
    for gy in 0..glyph.height as i32 {
        let py = y as i32 + baseline - glyph.top + gy;
        if py < y as i32 || py >= bottom {
            continue;
        }
        for gx in 0..glyph.width as i32 {
            let px = x as i32 + glyph.left + gx;
            if px < x as i32 || px >= right {
                continue;
            }
            let alpha = glyph.coverage[(gy * glyph.width as i32 + gx) as usize] as u32;
            if alpha == 0 {
                continue;
            }
            let at = ((py as u32 * PC_IMAGE_WIDTH + px as u32) * 4) as usize;
            for channel in 0..3 {
                let under = rgba[at + channel] as u32;
                rgba[at + channel] =
                    ((color[channel] as u32 * alpha + under * (255 - alpha)) / 255) as u8;
            }
        }
    }
}
