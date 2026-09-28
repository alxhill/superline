//! Nerd Font glyph names for the icon browser, and the one way the editor
//! shows an icon value: the glyph, then its code points and name.

use std::io::Read;
use std::sync::OnceLock;

use flate2::read::DeflateDecoder;

/// `hexcode name` lines, sorted by name. Regenerate with
/// `scripts/nerd-glyphs/generate.py`.
const TABLE: &[u8] = include_bytes!("nerd_glyphs.deflate");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub ch: char,
    pub name: &'static str,
}

pub fn all() -> &'static [Glyph] {
    static GLYPHS: OnceLock<Vec<Glyph>> = OnceLock::new();
    GLYPHS.get_or_init(|| {
        let mut text = String::new();
        if DeflateDecoder::new(TABLE)
            .read_to_string(&mut text)
            .is_err()
        {
            return Vec::new();
        }
        let text: &'static str = text.leak();
        text.lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let (code, name) = line.split_once(' ')?;
                let ch = char::from_u32(u32::from_str_radix(code, 16).ok()?)?;
                Some(Glyph { ch, name })
            })
            .collect()
    })
}

/// Glyphs whose name contains every search term. A term can also be a code
/// point (`f10fe`, `U+F10FE`) or the glyph itself.
pub fn search(query: &str) -> Vec<Glyph> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if terms.is_empty() {
        return all().to_vec();
    }
    let code_term = |term: &str| {
        let hex = term
            .strip_prefix("u+")
            .or_else(|| term.strip_prefix("0x"))
            .unwrap_or(term);
        u32::from_str_radix(hex, 16).ok()
    };
    let mut matches: Vec<(u8, Glyph)> = all()
        .iter()
        .filter(|glyph| {
            terms.iter().all(|term| {
                glyph.name.contains(term.as_str())
                    || code_term(term) == Some(glyph.ch as u32)
                    || term.chars().eq(std::iter::once(glyph.ch))
            })
        })
        // Names where the first term is a whole word come first, then those
        // with a word starting with it, so "key" lists `md-key` before
        // `md-keyboard` before `md-monkey`.
        .map(|glyph| {
            let first = terms[0].as_str();
            let mut parts = glyph.name.split(['-', '_']);
            let rank = if glyph.name.split(['-', '_']).any(|part| part == first) {
                0
            } else if parts.any(|part| part.starts_with(first)) {
                1
            } else {
                2
            };
            (rank, *glyph)
        })
        .collect();
    // The table is sorted by name, and the sort is stable.
    matches.sort_by_key(|(rank, _)| *rank);
    matches.into_iter().map(|(_, glyph)| glyph).collect()
}

pub fn name_of(ch: char) -> Option<&'static str> {
    all()
        .iter()
        .find(|glyph| glyph.ch == ch)
        .map(|glyph| glyph.name)
}

/// The code points of `text` and, for a single known glyph, its name:
/// `U+F10FE md-kubernetes`.
pub fn describe(text: &str) -> String {
    if text.is_empty() {
        return "empty: hidden".into();
    }
    let codes: Vec<String> = text
        .chars()
        .map(|ch| format!("U+{:04X}", ch as u32))
        .collect();
    let mut chars = text.chars();
    let name = match (chars.next(), chars.next()) {
        (Some(ch), None) => name_of(ch),
        _ => None,
    };
    match name {
        Some(name) => format!("{} {name}", codes.join(" ")),
        None => codes.join(" "),
    }
}

/// Turns a documented fallback (`U+F10FE`, `"⚿"`) into the text it stands
/// for, when it names one.
pub fn fallback_text(fallback: &str) -> Option<String> {
    if let Some(hex) = fallback.strip_prefix("U+") {
        return u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .map(String::from);
    }
    if fallback.starts_with('"') {
        return serde_json::from_str::<String>(fallback).ok();
    }
    None
}

/// Control characters and other invisible text shown as escapes, so every
/// value reads the same way whatever it contains.
pub fn visible(text: &str) -> String {
    text.chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_loads() {
        let glyphs = all();
        assert!(glyphs.len() > 10_000, "{}", glyphs.len());
        assert_eq!(name_of('\u{f10fe}'), Some("md-kubernetes"));
    }

    #[test]
    fn search_matches_names_codes_and_glyphs() {
        let found = search("kubernetes");
        assert!(found.iter().any(|g| g.name == "md-kubernetes"));
        assert!(found.iter().all(|g| g.name.contains("kubernetes")));

        assert_eq!(search("U+F10FE")[0].name, "md-kubernetes");
        assert_eq!(search("f10fe")[0].name, "md-kubernetes");
        assert_eq!(search("\u{f10fe}")[0].name, "md-kubernetes");
        assert!(search("md kubernetes")
            .iter()
            .all(|g| g.name.starts_with("md-")));
        assert!(search("zzzz-no-such-glyph").is_empty());
    }

    #[test]
    fn whole_word_matches_rank_first_in_name_order() {
        let found = search("key");
        let whole = |g: &Glyph| g.name.split(['-', '_']).any(|part| part == "key");
        let first_other = found.iter().position(|g| !whole(g)).unwrap();
        assert!(first_other > 0);
        assert!(found[first_other..].iter().all(|g| !whole(g)));
        let names: Vec<&str> = found[..first_other].iter().map(|g| g.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn word_starts_rank_first() {
        let found = search("git");
        let first_other = found
            .iter()
            .position(|g| !g.name.split(['-', '_']).any(|p| p.starts_with("git")));
        if let Some(first_other) = first_other {
            assert!(found[first_other..]
                .iter()
                .all(|g| !g.name.split(['-', '_']).any(|p| p.starts_with("git"))));
        }
    }

    #[test]
    fn icons_are_described_the_same_way_whatever_they_are() {
        assert_eq!(describe("\u{f10fe}"), "U+F10FE md-kubernetes");
        assert_eq!(describe("⚿"), "U+26BF");
        assert_eq!(describe("$"), "U+0024");
        assert_eq!(describe(""), "empty: hidden");
    }

    #[test]
    fn documented_fallbacks_become_text() {
        assert_eq!(fallback_text("U+F10FE").as_deref(), Some("\u{f10fe}"));
        assert_eq!(fallback_text("\"⚿\"").as_deref(), Some("⚿"));
        assert_eq!(fallback_text("symbol"), None);
    }
}
