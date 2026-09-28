//! The 256-colour picker's layout: the 16 standard and intense colours, the
//! 6×6×6 cube as six blocks (one per green level, rows red, columns blue), and
//! the greyscale ramp. Arrow keys move between cells as they appear on screen.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// Columns per cell: a right-aligned three-digit code and a space.
const CELL: u16 = 4;
/// Room for the "Standard:" style row labels.
const LABEL: u16 = 10;
/// Space between the left and right columns of cube blocks.
const BLOCK_GAP: u16 = 4;

/// The grey ramp is the widest row.
pub const WIDTH: u16 = LABEL + 12 * CELL;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    code: u8,
    x: u16,
    y: u16,
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
    let mut cells = Vec::with_capacity(256);
    for code in 0..16u8 {
        cells.push(Cell {
            code,
            x: LABEL + (code % 8) as u16 * CELL,
            y: (code / 8) as u16,
        });
    }
    let gap = spaced as u16;
    for green in 0..6u8 {
        let (block_row, side) = (green % 3, green / 3);
        let left = side as u16 * (6 * CELL + BLOCK_GAP);
        let top = cube_top(spaced) + block_row as u16 * (6 + gap);
        for red in 0..6u8 {
            for blue in 0..6u8 {
                cells.push(Cell {
                    code: 16 + 36 * red + 6 * green + blue,
                    x: left + blue as u16 * CELL,
                    y: top + red as u16,
                });
            }
        }
    }
    for step in 0..24u8 {
        cells.push(Cell {
            code: 232 + step,
            x: LABEL + (step % 12) as u16 * CELL,
            y: grays_top(spaced) + (step / 12) as u16,
        });
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
pub fn step(code: u8, direction: Direction) -> u8 {
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
pub fn render(buf: &mut Buffer, area: Rect, selected: u8) {
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
        if cell.y >= area.height || cell.x + CELL > area.width {
            continue;
        }
        let mut style = Style::new()
            .bg(Color::Indexed(cell.code))
            .fg(contrast(cell.code));
        if cell.code == selected {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        buf.set_string(
            area.x + cell.x,
            area.y + cell.y,
            format!("{:>3} ", cell.code),
            style,
        );
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
            let mut codes: Vec<u8> = cells(spaced).iter().map(|c| c.code).collect();
            codes.sort();
            assert_eq!(codes, (0..=255).collect::<Vec<u8>>());
        }
    }

    #[test]
    fn cells_do_not_overlap_and_fit_the_width() {
        let cells = cells(true);
        for (i, a) in cells.iter().enumerate() {
            assert!(a.x + CELL <= WIDTH, "{} overflows", a.code);
            for b in &cells[i + 1..] {
                assert!(
                    a.y != b.y || a.x + CELL <= b.x || b.x + CELL <= a.x,
                    "{} overlaps {}",
                    a.code,
                    b.code
                );
            }
        }
    }

    #[test]
    fn cube_blocks_follow_the_screen_layout() {
        use Direction::*;
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
