//! Turns the prompt superline prints (with bare escapes) into styled ratatui
//! lines for the preview.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn to_lines(text: &str) -> Vec<Line<'static>> {
    let mut style = Style::default();
    text.lines()
        .map(|line| parse_line(line, &mut style))
        .collect()
}

fn parse_line(line: &str, style: &mut Style) -> Line<'static> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if !c.is_control() {
                current.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                let mut params = String::new();
                let mut command = None;
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        command = Some(c);
                        break;
                    }
                    params.push(c);
                }
                if command == Some('m') {
                    if !current.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut current), *style));
                    }
                    apply_sgr(&params, style);
                }
            }
            // OSC (hyperlinks): skip to BEL or ST.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, *style));
    }
    Line::from(spans)
}

fn apply_sgr(params: &str, style: &mut Style) {
    let codes: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params.split(';').map(|p| p.parse().unwrap_or(0)).collect()
    };
    let mut codes = codes.into_iter();
    while let Some(code) = codes.next() {
        match code {
            0 => *style = Style::default(),
            1 => *style = style.add_modifier(Modifier::BOLD),
            2 => *style = style.add_modifier(Modifier::DIM),
            3 => *style = style.add_modifier(Modifier::ITALIC),
            4 => *style = style.add_modifier(Modifier::UNDERLINED),
            22 => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => *style = style.remove_modifier(Modifier::ITALIC),
            24 => *style = style.remove_modifier(Modifier::UNDERLINED),
            30..=37 => style.fg = Some(Color::Indexed((code - 30) as u8)),
            40..=47 => style.bg = Some(Color::Indexed((code - 40) as u8)),
            90..=97 => style.fg = Some(Color::Indexed((code - 90 + 8) as u8)),
            100..=107 => style.bg = Some(Color::Indexed((code - 100 + 8) as u8)),
            39 => style.fg = None,
            49 => style.bg = None,
            38 | 48 => {
                let color = match codes.next() {
                    Some(5) => codes.next().map(|n| Color::Indexed(n as u8)),
                    Some(2) => {
                        let (r, g, b) = (codes.next(), codes.next(), codes.next());
                        match (r, g, b) {
                            (Some(r), Some(g), Some(b)) => {
                                Some(Color::Rgb(r as u8, g as u8, b as u8))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if code == 38 {
                    style.fg = color;
                } else {
                    style.bg = color;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_resets_become_span_styles() {
        let lines =
            to_lines("\x1b[48;5;31m\x1b[38;5;15m ~/dev \x1b[0m\x1b[38;5;31m\u{e0b0}\x1b[0m");
        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        assert_eq!(spans[0].content, " ~/dev ");
        assert_eq!(spans[0].style.bg, Some(Color::Indexed(31)));
        assert_eq!(spans[0].style.fg, Some(Color::Indexed(15)));
        assert_eq!(spans[1].content, "\u{e0b0}");
        assert_eq!(spans[1].style.bg, None);
        assert_eq!(spans[1].style.fg, Some(Color::Indexed(31)));
    }

    #[test]
    fn hyperlinks_keep_only_their_label() {
        let lines = to_lines("\x1b]8;;https://example.com\x1b\\#12\x1b]8;;\x1b\\ done");
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "#12 done");
    }

    #[test]
    fn splits_rows() {
        assert_eq!(to_lines("one\ntwo\n").len(), 2);
    }
}
