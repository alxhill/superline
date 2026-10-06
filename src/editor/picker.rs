//! The 256-colour picker's layout: the 16 standard and intense colours, the
//! 6×6×6 cube as six blocks (one per green level, rows red, columns blue), and
//! the greyscale ramp, plus a `none` cell at the end of the standard row for
//! the terminal's own colour. Arrow keys move between cells as they appear on
//! screen.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::colors::{ColorCode, NONE_NAME};

/// Columns per cell: a right-aligned three-digit code and a space.
const CELL: u16 = 4;
/// Columns of the `none` cell: the name, a space each side.
const NONE_CELL: u16 = 6;
/// Room for the "Standard:" style row labels.
const LABEL: u16 = 10;
/// Space between the left and right columns of cube blocks.
const BLOCK_GAP: u16 = 4;

/// The grey ramp is the widest row.
pub const WIDTH: u16 = LABEL + 12 * CELL;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    code: ColorCode,
    x: u16,
    y: u16,
    width: u16,
}

impl Cell {
    fn palette(code: u8, x: u16, y: u16) -> Cell {
        Cell {
            code: ColorCode::Palette(code),
            x,
            y,
            width: CELL,
        }
    }
}

/// How the editor draws a theme colour: the terminal's own colour is the
/// terminal's default.
pub fn tui_color(code: ColorCode) -> Color {
    match code {
        ColorCode::Terminal => Color::Reset,
        ColorCode::Palette(code) => Color::Indexed(code),
    }
}

/// The height of the grid, with or without blank lines between sections.
pub fn height(spaced: bool) -> u16 {
    grays_top(spaced) + 2
}

fn cube_top(spaced: bool) -> u16 {
    2 + spaced as u16
}

fn grays_top(spaced: bool) -> u16 {
    let gap = spaced as u16;
    cube_top(spaced) + 3 * 6 + 2 * gap + gap
}

fn cells(spaced: bool) -> Vec<Cell> {
    let mut cells = Vec::with_capacity(257);
    for code in 0..16u8 {
        cells.push(Cell::palette(
            code,
            LABEL + (code % 8) as u16 * CELL,
            (code / 8) as u16,
        ));
    }
    // After the standard colours, a cell's gap away.
    cells.push(Cell {
        code: ColorCode::Terminal,
        x: LABEL + 9 * CELL,
        y: 0,
        width: NONE_CELL,
    });
    let gap = spaced as u16;
    for green in 0..6u8 {
        let (block_row, side) = (green % 3, green / 3);
        let left = side as u16 * (6 * CELL + BLOCK_GAP);
        let top = cube_top(spaced) + block_row as u16 * (6 + gap);
        for red in 0..6u8 {
            for blue in 0..6u8 {
                cells.push(Cell::palette(
                    16 + 36 * red + 6 * green + blue,
                    left + blue as u16 * CELL,
                    top + red as u16,
                ));
            }
        }
    }
    for step in 0..24u8 {
        cells.push(Cell::palette(
            232 + step,
            LABEL + (step % 12) as u16 * CELL,
            grays_top(spaced) + (step / 12) as u16,
        ));
    }
    cells
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The code of the cell next to `code` on screen, or `code` at an edge.
pub fn step(code: ColorCode, direction: Direction) -> ColorCode {
    let cells = cells(true);
    let Some(current) = cells.iter().find(|cell| cell.code == code) else {
        return code;
    };
    let dx = |cell: &Cell| cell.x.abs_diff(current.x);
    let next = match direction {
        Direction::Left => cells
            .iter()
            .filter(|c| c.y == current.y && c.x < current.x)
            .max_by_key(|c| c.x),
        Direction::Right => cells
            .iter()
            .filter(|c| c.y == current.y && c.x > current.x)
            .min_by_key(|c| c.x),
        Direction::Up => {
            let row = cells.iter().filter(|c| c.y < current.y).map(|c| c.y).max();
            cells
                .iter()
                .filter(|c| Some(c.y) == row)
                .min_by_key(|c| dx(c))
        }
        Direction::Down => {
            let row = cells.iter().filter(|c| c.y > current.y).map(|c| c.y).min();
            cells
                .iter()
                .filter(|c| Some(c.y) == row)
                .min_by_key(|c| dx(c))
        }
    };
    next.map_or(code, |cell| cell.code)
}

/// Draws the grid into `area`, highlighting `selected`. Sections are spaced
/// out when the area is tall enough.
pub fn render(buf: &mut Buffer, area: Rect, selected: ColorCode) {
    let spaced = area.height >= height(true);
    let label = Style::new().add_modifier(Modifier::BOLD);
    let labels = [
        ("Standard:", 0),
        ("Intense:", 1),
        ("Grays:", grays_top(spaced)),
    ];
    for (text, y) in labels {
        if y < area.height {
            buf.set_string(area.x, area.y + y, text, label);
        }
    }
    for cell in cells(spaced) {
        if cell.y >= area.height || cell.x + cell.width > area.width {
            continue;
        }
        let (mut style, text) = match cell.code {
            ColorCode::Palette(code) => (
                Style::new().bg(Color::Indexed(code)).fg(contrast(code)),
                format!("{code:>3} "),
            ),
            // The terminal's own colours, so it reads as see-through.
            ColorCode::Terminal => (
                Style::new().bg(Color::Reset).fg(Color::Reset),
                format!(" {NONE_NAME} "),
            ),
        };
        if cell.code == selected {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        buf.set_string(area.x + cell.x, area.y + cell.y, text, style);
    }
}

/// Black or white, whichever reads better on the colour.
pub fn contrast(code: u8) -> Color {
    let light = match code {
        7 | 10 | 11 | 14 | 15 => true,
        0..=15 => false,
        16..=231 => {
            let n = code - 16;
            let (r, g, b) = (n / 36, (n / 6) % 6, n % 6);
            r as u16 * 3 + g as u16 * 6 + b as u16 > 15
        }
        _ => code > 243,
    };
    if light {
        Color::Black
    } else {
        Color::White
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_exactly_one_cell() {
        for spaced in [true, false] {
            let cells = cells(spaced);
            let mut codes: Vec<u8> = cells.iter().filter_map(|c| c.code.palette()).collect();
            codes.sort();
            assert_eq!(codes, (0..=255).collect::<Vec<u8>>());
            let none = cells.iter().filter(|c| c.code == ColorCode::Terminal);
            assert_eq!(none.count(), 1);
        }
    }

    #[test]
    fn cells_do_not_overlap_and_fit_the_width() {
        let cells = cells(true);
        for (i, a) in cells.iter().enumerate() {
            assert!(a.x + a.width <= WIDTH, "{:?} overflows", a.code);
            for b in &cells[i + 1..] {
                assert!(
                    a.y != b.y || a.x + a.width <= b.x || b.x + b.width <= a.x,
                    "{:?} overlaps {:?}",
                    a.code,
                    b.code
                );
            }
        }
    }

    #[test]
    fn none_follows_the_standard_colours() {
        use Direction::*;
        let p = ColorCode::Palette;
        assert_eq!(step(p(7), Right), ColorCode::Terminal);
        assert_eq!(step(ColorCode::Terminal, Left), p(7));
        assert_eq!(step(ColorCode::Terminal, Right), ColorCode::Terminal);
        assert_eq!(step(ColorCode::Terminal, Down), p(15));
        assert_eq!(step(p(15), Up), p(7));
    }

    #[test]
    fn the_none_cell_is_drawn_in_the_terminals_colours() {
        let area = Rect::new(0, 0, WIDTH, height(true));
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, ColorCode::Terminal);
        let x = LABEL + 9 * CELL;
        let text: String = (x..x + NONE_CELL)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        assert_eq!(text, " none ");
        let cell = &buf[(x + 1, 0)];
        assert_eq!(cell.bg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn cube_blocks_follow_the_screen_layout() {
        use Direction::*;
        let step = |code: u8, direction| match step(ColorCode::Palette(code), direction) {
            ColorCode::Palette(code) => code,
            ColorCode::Terminal => panic!("stepped onto none"),
        };
        // Across a block row, then over the gap into the next green level.
        assert_eq!(step(16, Right), 17);
        assert_eq!(step(21, Right), 34);
        assert_eq!(step(34, Left), 21);
        // Down a block is the next red level; past its bottom, the block below.
        assert_eq!(step(16, Down), 52);
        assert_eq!(step(196, Down), 22);
        assert_eq!(step(22, Up), 196);
        // Between the standard colours, the cube and the greys.
        assert_eq!(step(0, Down), 8);
        assert_eq!(step(8, Up), 0);
        assert_eq!(step(0, Up), 0);
        assert_eq!(step(226, Down), 236);
        assert_eq!(step(232, Down), 244);
        assert_eq!(step(255, Down), 255);
        assert_eq!(step(255, Right), 255);
    }
}
