//! Editing a list value one entry at a time. `ListEdit` holds a list and its
//! selected entry and adds, inserts, deletes, reorders and replaces entries.
//! The rest of this module is the Theme page's colour list editor built on it,
//! which shows each colour as a swatch and edits it with the colour picker.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use super::theme::{
    color_names, color_value, edit_text, parse_color, PropKind, PropSpec, ThemeEntry,
};
use super::{panel, App, InputPurpose, Mode};
use crate::themes::color_code;

/// Where a new or changed entry goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Replaces the entry at this position.
    Replace(usize),
    /// Becomes a new entry at this position.
    Insert(usize),
}

/// A list and its selected entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ListEdit<T> {
    items: Vec<T>,
    cursor: usize,
}

impl<T: Clone> ListEdit<T> {
    pub fn new(items: Vec<T>, cursor: usize) -> ListEdit<T> {
        let mut list = ListEdit { items, cursor: 0 };
        list.select(cursor);
        list
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selected(&self) -> Option<&T> {
        self.items.get(self.cursor)
    }

    /// Selects the entry at `index`, or the last one when it is past the end.
    pub fn select(&mut self, index: usize) {
        self.cursor = index.min(self.items.len().saturating_sub(1));
    }

    /// The selected entry.
    pub fn here(&self) -> Slot {
        Slot::Replace(self.cursor)
    }

    /// Just after the selected entry.
    pub fn after(&self) -> Slot {
        Slot::Insert((self.cursor + 1).min(self.items.len()))
    }

    /// Just before the selected entry.
    pub fn before(&self) -> Slot {
        Slot::Insert(self.cursor)
    }

    /// Puts `item` in `slot` and selects it. A slot past the end appends.
    pub fn put(&mut self, slot: Slot, item: T) {
        self.cursor = match slot {
            Slot::Replace(index) if index < self.items.len() => {
                self.items[index] = item;
                index
            }
            Slot::Replace(_) => {
                self.items.push(item);
                self.items.len() - 1
            }
            Slot::Insert(index) => {
                let index = index.min(self.items.len());
                self.items.insert(index, item);
                index
            }
        };
    }

    /// Removes the selected entry and selects the one that takes its place.
    pub fn remove(&mut self) -> Option<T> {
        if self.items.is_empty() {
            return None;
        }
        let item = self.items.remove(self.cursor);
        self.select(self.cursor);
        Some(item)
    }

    /// Swaps the selected entry with the one above or below it, keeping it
    /// selected. Returns false at either end.
    pub fn shift(&mut self, down: bool) -> bool {
        let target = if down {
            self.cursor + 1
        } else {
            match self.cursor.checked_sub(1) {
                Some(target) => target,
                None => return false,
            }
        };
        if target >= self.items.len() {
            return false;
        }
        self.items.swap(self.cursor, target);
        self.cursor = target;
        true
    }

    /// Copies the selected entry to just after it and selects the copy.
    pub fn duplicate(&mut self) -> bool {
        let Some(item) = self.selected().cloned() else {
            return false;
        };
        self.put(self.after(), item);
        true
    }
}

/// Columns `numbered_lines` draws before each entry of a list this long.
pub fn number_width(len: usize) -> usize {
    2 + len.to_string().len() + 2
}

/// One line per entry: a marker on the selected one, its position from 1,
/// then the entry's own spans.
pub fn numbered_lines(rows: Vec<Vec<Span<'static>>>, selected: usize) -> Vec<Line<'static>> {
    let width = rows.len().to_string().len();
    rows.into_iter()
        .enumerate()
        .map(|(i, spans)| {
            let number = Span::raw(format!("{:>width$}  ", i + 1));
            let mut line = if i == selected {
                vec![Span::raw("▸ "), number.bold().fg(Color::Blue)]
            } else {
                vec![Span::raw("  "), number.dark_gray()]
            };
            line.extend(spans);
            Line::from(line)
        })
        .collect()
}

/// The Theme page's open colour list: the selected entry, and where the
/// colour picker's or a typed colour goes while one is open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenList {
    pub cursor: usize,
    pub pending: Option<Slot>,
}

/// The colour list property under the Theme page's cursor.
struct Current {
    entry: ThemeEntry,
    spec: PropSpec,
    /// The file does not set the list, so it holds the fallback colours.
    unset: bool,
    list: ListEdit<Value>,
    pending: Option<Slot>,
}

/// Columns of a colour's swatch.
const SWATCH: usize = 4;

fn swatch(code: Option<u8>) -> Span<'static> {
    let blank = " ".repeat(SWATCH);
    match code {
        Some(code) => Span::styled(blank, Style::new().bg(Color::Indexed(code))),
        None => Span::raw(blank),
    }
}

/// A colour entry: its swatch, the value as written, and its code or names.
fn color_spans(item: &Value, fallback: bool) -> Vec<Span<'static>> {
    let code = color_code(item);
    let text = Span::raw(edit_text(item));
    let detail = match (item, code) {
        (Value::String(_), Some(code)) => code.to_string(),
        (_, Some(code)) => color_names(code).join(", "),
        _ => String::new(),
    };
    vec![
        swatch(code),
        Span::raw("  "),
        if fallback {
            text.dark_gray()
        } else {
            text.yellow()
        },
        Span::raw(format!("  {detail}")).dark_gray(),
    ]
}

impl App {
    fn color_list(&self) -> Option<Current> {
        let open = self.theme.color_list?;
        let (entry, spec, value) = self.theme_prop()?;
        if spec.kind != PropKind::ColorList {
            return None;
        }
        let (items, unset) = match value {
            Some(Value::Array(items)) => (items, false),
            _ => (self.fallback_colors(&spec), true),
        };
        Some(Current {
            entry,
            spec,
            unset,
            list: ListEdit::new(items, open.cursor),
            pending: open.pending,
        })
    }

    /// The colours an unset list falls back to, e.g. `[defaults.bg]`.
    fn fallback_colors(&self, spec: &PropSpec) -> Vec<Value> {
        let key = spec
            .fallback
            .trim_matches(['[', ']'])
            .strip_prefix("defaults.")
            .unwrap_or("bg");
        self.theme_doc()
            .and_then(|doc| doc.root().get("defaults")?.get(key).cloned())
            .into_iter()
            .collect()
    }

    /// Whether the Theme page's property panel is showing a colour list.
    pub(super) fn color_list_open(&self) -> bool {
        self.color_list().is_some()
    }

    pub(super) fn open_color_list(&mut self) {
        self.theme.color_list = Some(OpenList::default());
    }

    /// Where the open colour picker's colour goes, when it is for a list entry.
    pub(super) fn list_pending(&self) -> Option<Slot> {
        self.color_list()?.pending
    }

    /// Writes the list back if it changed and keeps its selected entry
    /// selected. Returns false when the theme rejects it.
    fn store_list(&mut self, before: &[Value], list: ListEdit<Value>) -> bool {
        let cursor = list.cursor();
        if list.items() != before && !self.set_theme_prop(Some(Value::Array(list.into_items()))) {
            return false;
        }
        self.theme.color_list = Some(OpenList {
            cursor,
            pending: None,
        });
        true
    }

    fn open_list_picker(&mut self, list: &ListEdit<Value>, slot: Slot) {
        let selected = list.selected();
        self.theme.color_list = Some(OpenList {
            cursor: list.cursor(),
            pending: Some(slot),
        });
        self.mode = Mode::ColorPicker {
            code: selected.and_then(color_code).unwrap_or(0),
            prefer_name: selected.is_some_and(Value::is_string),
        };
        self.preview_stale = true;
    }

    pub(super) fn on_color_list_key(&mut self, key: KeyEvent) {
        let Some(Current { spec, mut list, .. }) = self.color_list() else {
            self.theme.color_list = None;
            return self.on_theme_key(key);
        };
        let before = list.items().to_vec();
        let cursor = list.cursor();
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let mut status = None;
        match key.code {
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => {
                self.theme.color_list = None;
                return;
            }
            KeyCode::Up | KeyCode::Char('K') if shift => {
                list.shift(false);
            }
            KeyCode::Down | KeyCode::Char('J') if shift => {
                list.shift(true);
            }
            KeyCode::Char('K') => {
                list.shift(false);
            }
            KeyCode::Char('J') => {
                list.shift(true);
            }
            KeyCode::Up | KeyCode::Char('k') => list.select(cursor.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => list.select(cursor + 1),
            KeyCode::Home | KeyCode::Char('g') => list.select(0),
            KeyCode::End | KeyCode::Char('G') => list.select(usize::MAX),
            KeyCode::Enter | KeyCode::Char(' ') => {
                return self.open_list_picker(&list, list.here())
            }
            KeyCode::Char('a') | KeyCode::Char('+') => {
                return self.open_list_picker(&list, list.after())
            }
            KeyCode::Char('I') | KeyCode::Insert => {
                return self.open_list_picker(&list, list.before())
            }
            KeyCode::Char('i') => {
                let buffer = list.selected().map(edit_text).unwrap_or_default();
                self.theme.color_list = Some(OpenList {
                    cursor,
                    pending: Some(list.here()),
                });
                self.mode = Mode::Input {
                    cursor: buffer.chars().count(),
                    buffer,
                    purpose: InputPurpose::ListItem,
                };
                return;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                if let Some(code) = list.selected().and_then(color_code) {
                    let next = if matches!(key.code, KeyCode::Right | KeyCode::Char('l')) {
                        code.wrapping_add(1)
                    } else {
                        code.wrapping_sub(1)
                    };
                    list.put(list.here(), Value::from(next));
                }
            }
            KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                if list.len() > 1 {
                    if let Some(removed) = list.remove() {
                        status = Some((
                            format!("Removed {} (u to undo)", edit_text(&removed)),
                            false,
                        ));
                    }
                } else {
                    status = Some((
                        format!(
                            "{} needs at least one colour (esc, then x resets it)",
                            spec.key
                        ),
                        true,
                    ));
                }
            }
            KeyCode::Char('c') => {
                list.duplicate();
            }
            _ => {}
        }
        if self.store_list(&before, list) {
            if let Some((text, error)) = status {
                self.set_status(text, error);
            }
        }
    }

    /// Puts the picker's colour in the list entry it was opened for. Picking
    /// an entry's own colour again keeps it as written.
    pub(super) fn pick_list_color(&mut self, code: u8, prefer_name: bool) {
        let Some(Current {
            list,
            pending: Some(slot),
            ..
        }) = self.color_list()
        else {
            return;
        };
        let kept = match slot {
            Slot::Replace(index) => list.items().get(index),
            Slot::Insert(_) => None,
        }
        .filter(|item| color_code(item) == Some(code))
        .cloned();
        let value = kept.unwrap_or_else(|| color_value(code, prefer_name));
        self.put_list_color(list, slot, value);
    }

    /// Applies a colour typed for a list entry. Returns whether the input is
    /// done.
    pub(super) fn submit_list_color(&mut self, text: &str) -> bool {
        let value = match parse_color(text) {
            Ok(value) => value,
            Err(error) => {
                self.set_status(error, true);
                return false;
            }
        };
        match self.color_list() {
            Some(Current {
                list,
                pending: Some(slot),
                ..
            }) => self.put_list_color(list, slot, value),
            _ => true,
        }
    }

    fn put_list_color(&mut self, mut list: ListEdit<Value>, slot: Slot, value: Value) -> bool {
        let before = list.items().to_vec();
        list.put(slot, value);
        self.store_list(&before, list)
    }

    /// The list with the picker's colour in its pending slot, for the preview.
    pub(super) fn list_preview(&self, code: u8) -> Option<Value> {
        let Current {
            mut list, pending, ..
        } = self.color_list()?;
        list.put(pending?, Value::from(code));
        Some(Value::Array(list.into_items()))
    }

    /// Names the list entry the colour picker is choosing, for its title.
    pub(super) fn list_pick_label(&self) -> String {
        match self.list_pending() {
            Some(Slot::Replace(index)) => format!(" · colour {}", index + 1),
            Some(Slot::Insert(index)) => format!(" · new colour {}", index + 1),
            None => String::new(),
        }
    }

    /// Draws the open colour list in place of the property list.
    pub(super) fn draw_color_list(&self, frame: &mut Frame, area: Rect) {
        let Some(Current {
            entry,
            spec,
            unset,
            list,
            pending,
        }) = self.color_list()
        else {
            return;
        };
        let block = panel(&format!(" {} · {} ", entry.label(), spec.key), true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [body, help] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(inner);

        let mut lines = Vec::new();
        if unset {
            let fallback = match spec.fallback.as_str() {
                "" => "defaults.bg",
                fallback => fallback,
            };
            lines.push(
                Line::from(format!(
                    "Unset, so {fallback} applies. Any change writes the list."
                ))
                .dark_gray(),
            );
            lines.push(Line::default());
        }
        let header = lines.len();

        // The picker's colour or the typed text is shown in the slot it will
        // fill.
        let rows = list
            .items()
            .iter()
            .map(|item| color_spans(item, unset))
            .collect();
        let mut rows = ListEdit::new(rows, list.cursor());
        let mut typing = None;
        match (&self.mode, pending) {
            (Mode::ColorPicker { code, .. }, Some(slot)) => {
                rows.put(slot, color_spans(&Value::from(*code), false))
            }
            (
                Mode::Input {
                    buffer,
                    cursor,
                    purpose: InputPurpose::ListItem,
                },
                Some(slot),
            ) => {
                let code = parse_color(buffer).ok().as_ref().and_then(color_code);
                rows.put(
                    slot,
                    vec![
                        swatch(code),
                        Span::raw("  "),
                        Span::raw(buffer.clone()).underlined(),
                    ],
                );
                let before: String = buffer.chars().take(*cursor).collect();
                typing = Some(before.width());
            }
            _ => {}
        }
        let (count, selected) = (rows.len(), rows.cursor());
        lines.extend(numbered_lines(rows.into_items(), selected));

        let row = header + selected;
        let scroll = (row + 1).saturating_sub(body.height as usize);
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), body);
        if let Some(before) = typing {
            let x = number_width(count) + SWATCH + 2 + before;
            frame.set_cursor_position((body.x + x as u16, body.y + (row - scroll) as u16));
        }

        let mut text = spec.help.clone();
        if !spec.fallback.is_empty() {
            text.push_str(&format!(" Unset: {}.", spec.fallback));
        }
        frame.render_widget(
            Paragraph::new(Line::from(text).dark_gray())
                .wrap(Wrap { trim: true })
                .block(Block::new().borders(Borders::TOP).dark_gray()),
            help,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Page;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn put_replaces_or_inserts_and_selects_the_entry() {
        let mut list = ListEdit::new(vec!['a', 'b', 'c'], 1);
        list.put(list.here(), 'B');
        assert_eq!(list.items(), ['a', 'B', 'c']);
        list.put(list.after(), 'x');
        assert_eq!(
            (list.items(), list.cursor()),
            (&['a', 'B', 'x', 'c'][..], 2)
        );
        list.put(list.before(), 'y');
        assert_eq!(
            (list.items(), list.cursor()),
            (&['a', 'B', 'y', 'x', 'c'][..], 2)
        );
        list.put(Slot::Insert(99), 'z');
        assert_eq!((list.items().last(), list.cursor()), (Some(&'z'), 5));
    }

    #[test]
    fn remove_selects_the_entry_that_takes_its_place() {
        let mut list = ListEdit::new(vec![1, 2, 3], 1);
        assert_eq!(list.remove(), Some(2));
        assert_eq!((list.items(), list.cursor()), (&[1, 3][..], 1));
        assert_eq!(list.remove(), Some(3));
        assert_eq!((list.items(), list.cursor()), (&[1][..], 0));
        assert_eq!(list.remove(), Some(1));
        assert_eq!(list.remove(), None);
        assert_eq!(list.after(), Slot::Insert(0));
    }

    #[test]
    fn shift_moves_the_selected_entry_and_stops_at_the_ends() {
        let mut list = ListEdit::new(vec![1, 2, 3], 0);
        assert!(!list.shift(false));
        assert!(list.shift(true));
        assert!(list.shift(true));
        assert_eq!((list.items(), list.cursor()), (&[2, 3, 1][..], 2));
        assert!(!list.shift(true));
        assert!(list.shift(false));
        assert_eq!((list.items(), list.cursor()), (&[2, 1, 3][..], 1));
    }

    #[test]
    fn duplicate_copies_the_entry_after_itself() {
        let mut list = ListEdit::new(vec![1, 2], 9);
        assert_eq!(list.cursor(), 1);
        list.select(0);
        assert!(list.duplicate());
        assert_eq!((list.items(), list.cursor()), (&[1, 1, 2][..], 1));
        assert!(!ListEdit::<u8>::new(vec![], 0).duplicate());
    }

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn shift(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::SHIFT));
    }

    /// Clears the input and types `text`.
    fn type_text(app: &mut App, text: &str) {
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    /// An editor on the Theme page with `cwd.bg_colors` under the cursor, on
    /// a theme whose `cwd` module is `cwd`.
    fn editor(name: &str, cwd: Value) -> (App, Scratch) {
        let dir = std::env::temp_dir().join(format!(
            "superline-list-editor-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let theme = json!({
            "defaults": { "fg": "white", "bg": "black" },
            "modules": { "cwd": cwd }
        });
        std::fs::write(dir.join("theme.json"), theme.to_string()).unwrap();
        let config = dir.join("config.json");
        let rows = json!({ "theme": "theme.json", "rows": [{ "left": ["cwd"] }] });
        std::fs::write(&config, rows.to_string()).unwrap();

        let mut app = App::new(crate::editor::load(&config).unwrap(), config);
        app.page = Page::Theme;
        app.load_theme_slot();
        let cwd = ThemeEntry::Module("cwd".into());
        let doc = app.theme_doc().unwrap();
        let module = doc.entries().iter().position(|e| *e == cwd).unwrap();
        let prop = doc
            .props(&cwd)
            .iter()
            .position(|(spec, _)| spec.key == "bg_colors")
            .unwrap();
        press(&mut app, KeyCode::Home);
        for _ in 0..module {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        for _ in 0..prop {
            press(&mut app, KeyCode::Down);
        }
        (app, Scratch(dir))
    }

    fn colors(app: &App) -> Option<Value> {
        app.theme_doc().unwrap().root()["modules"]
            .get("cwd")?
            .get("bg_colors")
            .cloned()
    }

    #[test]
    fn the_picker_edits_the_selected_entry_with_a_live_preview() {
        let (mut app, _dir) = editor("pick", json!({ "bg_colors": ["red", "orange", 31] }));
        press(&mut app, KeyCode::Enter);
        assert!(app.color_list_open());
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.mode,
            Mode::ColorPicker {
                code: 130,
                prefer_name: true
            }
        ));
        press(&mut app, KeyCode::End);
        assert_eq!(
            app.preview_theme().unwrap()["modules"]["cwd"]["bg_colors"],
            json!(["red", 255, 31])
        );
        assert_eq!(colors(&app), Some(json!(["red", "orange", 31])));
        press(&mut app, KeyCode::Enter);
        assert_eq!(colors(&app), Some(json!(["red", 255, 31])));
        assert!(app.color_list_open());

        // Picking an entry's own colour again keeps it as written.
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert_eq!(colors(&app), Some(json!(["red", 255, 31])));
    }

    #[test]
    fn entries_are_added_inserted_copied_and_deleted() {
        let (mut app, _dir) = editor("add", json!({ "bg_colors": ["red", "orange"] }));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Enter);
        assert_eq!(colors(&app), Some(json!(["red", "black", "orange"])));

        press(&mut app, KeyCode::Char('I'));
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Enter);
        assert_eq!(colors(&app), Some(json!(["red", 255, "black", "orange"])));

        press(&mut app, KeyCode::Char('d'));
        assert_eq!(colors(&app), Some(json!(["red", "black", "orange"])));
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(
            colors(&app),
            Some(json!(["red", "black", "black", "orange"]))
        );

        // Cancelling the picker adds nothing.
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Esc);
        assert_eq!(
            colors(&app),
            Some(json!(["red", "black", "black", "orange"]))
        );
        assert!(app.color_list_open());
    }

    #[test]
    fn the_last_colour_is_not_deleted() {
        let (mut app, _dir) = editor("last", json!({ "bg_colors": ["red"] }));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(colors(&app), Some(json!(["red"])));
        assert!(app.status.as_ref().is_some_and(|status| status.error));
    }

    #[test]
    fn entries_move_with_shift_arrows_and_j_k() {
        let (mut app, _dir) = editor("move", json!({ "bg_colors": ["red", "orange", 31] }));
        press(&mut app, KeyCode::Enter);
        shift(&mut app, KeyCode::Down);
        assert_eq!(colors(&app), Some(json!(["orange", "red", 31])));
        shift(&mut app, KeyCode::Char('J'));
        assert_eq!(colors(&app), Some(json!(["orange", 31, "red"])));
        press(&mut app, KeyCode::Char('K'));
        assert_eq!(colors(&app), Some(json!(["orange", "red", 31])));
        shift(&mut app, KeyCode::Up);
        shift(&mut app, KeyCode::Up);
        assert_eq!(colors(&app), Some(json!(["red", "orange", 31])));
    }

    #[test]
    fn typed_colours_replace_or_insert_an_entry() {
        let (mut app, _dir) = editor("type", json!({ "bg_colors": ["red", "orange"] }));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('i'));
        assert!(matches!(&app.mode, Mode::Input { buffer, .. } if buffer == "orange"));
        type_text(&mut app, "bleu");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::Input { .. }));
        assert!(app.status.as_ref().is_some_and(|status| status.error));
        type_text(&mut app, "blue");
        press(&mut app, KeyCode::Enter);
        assert_eq!(colors(&app), Some(json!(["red", "blue"])));

        // `i` in the picker types the new entry.
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('i'));
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        assert_eq!(colors(&app), Some(json!(["red", "blue", 42])));
    }

    #[test]
    fn arrows_step_the_selected_colour() {
        let (mut app, _dir) = editor("step", json!({ "bg_colors": ["red", 31] }));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Right);
        assert_eq!(colors(&app), Some(json!([2, 31])));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(colors(&app), Some(json!([2, 30])));
    }

    #[test]
    fn an_unset_list_starts_from_its_fallback() {
        let (mut app, _dir) = editor("unset", json!({ "path_fg": "white" }));
        press(&mut app, KeyCode::Enter);
        assert!(app.color_list_open());
        press(&mut app, KeyCode::Down);
        assert_eq!(colors(&app), None);
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(colors(&app), Some(json!(["black", "black"])));
    }

    #[test]
    fn undo_keeps_the_list_open_and_esc_returns_to_the_properties() {
        let (mut app, _dir) = editor("undo", json!({ "bg_colors": ["red", "orange"] }));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(colors(&app), Some(json!(["red"])));
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(colors(&app), Some(json!(["red", "orange"])));
        assert!(app.color_list_open());

        press(&mut app, KeyCode::Esc);
        assert!(!app.color_list_open());
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(colors(&app), None);
    }

    #[test]
    fn the_whole_list_can_still_be_typed() {
        let (mut app, _dir) = editor("raw", json!({ "bg_colors": ["red"] }));
        press(&mut app, KeyCode::Char('i'));
        type_text(&mut app, "1, green 3");
        press(&mut app, KeyCode::Enter);
        assert_eq!(colors(&app), Some(json!([1, "green", 3])));
        assert!(!app.color_list_open());
    }

    #[test]
    fn the_open_list_shows_each_colour() {
        let (mut app, _dir) = editor("draw", json!({ "bg_colors": ["red", "orange", 31] }));
        press(&mut app, KeyCode::Enter);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let screen: String = (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row + "\n"
            })
            .collect();
        let gap = " ".repeat(2 + SWATCH + 2);
        assert!(screen.contains("cwd · bg_colors"), "{screen}");
        assert!(screen.contains(&format!("▸ 1{gap}red  1")), "{screen}");
        assert!(screen.contains(&format!("  2{gap}orange  130")), "{screen}");
        assert!(
            screen.contains(&format!("  3{gap}31  turquoise_blue")),
            "{screen}"
        );
        assert!(screen.contains("a/I add/insert"), "{screen}");
    }
}
