//! The editor's Theme page: the custom theme file the config names, one module
//! at a time, with a colour picker.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

use super::model::Target;
use super::theme::{
    color_names, color_value, edit_text, parse_choice, parse_color, parse_color_list, step_choice,
    PropKind, PropSpec, ThemeDoc, ThemeEntry,
};
use super::{glyphs, picker};
use super::{json, panel, schema, write_atomic, App, Focus, InputPurpose, Mode};

/// Rows PageUp/PageDown move in the icon browser.
const ICON_PAGE: usize = 10;

/// A theme file the config has pointed at during this session.
pub(super) enum Slot {
    Loaded(ThemeDoc),
    Missing,
    Broken(String),
}

pub(super) struct ThemePage {
    slots: HashMap<PathBuf, Slot>,
    cursor: usize,
    prop_cursor: usize,
    focus: Focus,
    list: ListState,
}

impl ThemePage {
    pub(super) fn new() -> ThemePage {
        ThemePage {
            slots: HashMap::new(),
            cursor: 0,
            prop_cursor: 0,
            focus: Focus::Layout,
            list: ListState::default(),
        }
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.slots
            .values()
            .any(|slot| matches!(slot, Slot::Loaded(doc) if doc.is_dirty()))
    }

    /// Writes every changed theme, returning the files written.
    pub(super) fn save(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut written = Vec::new();
        for (path, slot) in &mut self.slots {
            if let Slot::Loaded(doc) = slot {
                if doc.is_dirty() {
                    write_atomic(path, &json::to_pretty_theme(doc.root()))
                        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
                    doc.mark_saved();
                    written.push(path.clone());
                }
            }
        }
        Ok(written)
    }
}

fn load_slot(path: &Path) -> Slot {
    match std::fs::read_to_string(path) {
        Ok(text) => match ThemeDoc::load(&text) {
            Ok(doc) => Slot::Loaded(doc),
            Err(error) => Slot::Broken(error),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Slot::Missing,
        Err(error) => Slot::Broken(error.to_string()),
    }
}

impl App {
    /// The theme file the config names, or `None` for a built-in theme.
    fn theme_path(&self) -> Option<PathBuf> {
        let name = self.doc.root().get("theme")?.as_str()?;
        if name == "rainbow" || name == "simple" {
            return None;
        }
        let dir = self.path.parent().unwrap_or(Path::new("."));
        Some(dir.join(name))
    }

    /// Reads the named theme the first time the config points at it.
    pub(super) fn load_theme_slot(&mut self) {
        if let Some(path) = self.theme_path() {
            self.theme
                .slots
                .entry(path)
                .or_insert_with_key(|path| load_slot(path));
        }
    }

    fn slot(&self) -> Option<&Slot> {
        self.theme.slots.get(&self.theme_path()?)
    }

    fn theme_doc(&self) -> Option<&ThemeDoc> {
        match self.slot()? {
            Slot::Loaded(doc) => Some(doc),
            _ => None,
        }
    }

    fn theme_doc_mut(&mut self) -> Option<&mut ThemeDoc> {
        let path = self.theme_path()?;
        match self.theme.slots.get_mut(&path)? {
            Slot::Loaded(doc) => Some(doc),
            _ => None,
        }
    }

    fn theme_entry(&self) -> Option<ThemeEntry> {
        let entries = self.theme_doc()?.entries();
        entries
            .get(self.theme.cursor.min(entries.len() - 1))
            .cloned()
    }

    fn theme_prop(&self) -> Option<(ThemeEntry, PropSpec, Option<Value>)> {
        let entry = self.theme_entry()?;
        let props = self.theme_doc()?.props(&entry);
        let (spec, value) = props.get(self.theme.prop_cursor)?.clone();
        Some((entry, spec, value))
    }

    /// The in-memory theme for the preview, with the colour under the picker
    /// applied.
    pub(super) fn preview_theme(&self) -> Option<Value> {
        let doc = self.theme_doc()?;
        let pending = match &self.mode {
            Mode::ColorPicker { code, .. } => Some(Value::from(*code)),
            Mode::IconBrowser { query, selected } => glyphs::search(query)
                .get(*selected)
                .map(|glyph| Value::from(glyph.ch.to_string())),
            _ => None,
        };
        if let (Some(value), Some((entry, spec, _))) = (pending, self.theme_prop()) {
            if let Ok(theme) = doc.with(&entry, &spec.key, Some(value)) {
                return Some(theme);
            }
        }
        Some(doc.root().clone())
    }

    pub(super) fn theme_undo(&mut self, redo: bool) {
        let done = self
            .theme_doc_mut()
            .map(|doc| if redo { doc.redo() } else { doc.undo() })
            .unwrap_or(false);
        if done {
            self.theme_changed();
        } else {
            self.set_status(
                if redo {
                    "Nothing to redo"
                } else {
                    "Nothing to undo"
                },
                false,
            );
        }
    }

    fn theme_changed(&mut self) {
        let count = self.theme_doc().map_or(1, |doc| doc.entries().len());
        self.theme.cursor = self.theme.cursor.min(count - 1);
        let props = self
            .theme_entry()
            .and_then(|entry| Some(self.theme_doc()?.props(&entry).len()))
            .unwrap_or(0);
        self.theme.prop_cursor = self.theme.prop_cursor.min(props.saturating_sub(1));
        self.preview_stale = true;
    }

    fn set_theme_prop(&mut self, value: Option<Value>) -> bool {
        let Some((entry, spec, _)) = self.theme_prop() else {
            return false;
        };
        let result = match self.theme_doc_mut() {
            Some(doc) => doc.set(&entry, &spec.key, value),
            None => return false,
        };
        match result {
            Ok(()) => {
                self.theme_changed();
                true
            }
            Err(error) => {
                self.set_status(error, true);
                false
            }
        }
    }

    /// Points the config at a theme file, creating it from the example theme
    /// when it does not exist yet.
    fn use_new_theme(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            self.set_status("Give the theme a file name", true);
            return false;
        }
        if name == "rainbow" || name == "simple" {
            self.set_status(format!("{name} is a built-in theme name"), true);
            return false;
        }
        let dir = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let path = dir.join(name);
        let slot = match load_slot(&path) {
            Slot::Missing => Slot::Loaded(ThemeDoc::new_from_starter()),
            Slot::Broken(error) => {
                self.set_status(format!("{name} exists but is not a theme: {error}"), true);
                return false;
            }
            loaded => loaded,
        };
        let created = matches!(&slot, Slot::Loaded(doc) if doc.is_dirty());
        self.theme.slots.insert(path, slot);
        if let Err(error) =
            self.doc
                .set_option(Target::Settings, schema::THEME.key, Some(Value::from(name)))
        {
            self.set_status(error, true);
            return false;
        }
        if !self.theme_choices.iter().any(|choice| choice == name) {
            self.theme_choices.push(name.to_string());
        }
        self.changed();
        self.theme.cursor = 0;
        self.theme.prop_cursor = 0;
        self.set_status(
            if created {
                format!("Created {name} from the example theme (s to save)")
            } else {
                format!("Switched to {name}")
            },
            false,
        );
        true
    }

    /// Applies text typed on the Theme page. Returns whether the input is done.
    pub(super) fn submit_theme_input(&mut self, purpose: InputPurpose, text: &str) -> bool {
        match purpose {
            InputPurpose::NewTheme => self.use_new_theme(text),
            _ => {
                let Some((_, spec, _)) = self.theme_prop() else {
                    return true;
                };
                let parsed = match spec.kind {
                    PropKind::Color => parse_color(text),
                    PropKind::ColorList => parse_color_list(text),
                    PropKind::Str => Ok(Value::from(text)),
                    PropKind::Choice(variants) => parse_choice(text, variants),
                };
                match parsed {
                    Ok(value) => self.set_theme_prop(Some(value)),
                    Err(error) => {
                        self.set_status(error, true);
                        false
                    }
                }
            }
        }
    }

    fn open_theme_input(&mut self, purpose: InputPurpose, buffer: String) {
        self.mode = Mode::Input {
            cursor: buffer.chars().count(),
            buffer,
            purpose,
        };
    }

    fn start_new_theme(&mut self) {
        let suggestion = match self.slot() {
            Some(Slot::Missing) => {
                let name = self.doc.root()["theme"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.use_new_theme(&name);
                return;
            }
            _ => {
                let dir = self.path.parent().unwrap_or(Path::new("."));
                (1..)
                    .map(|n| match n {
                        1 => "theme.json".to_string(),
                        n => format!("theme-{n}.json"),
                    })
                    .find(|name| !dir.join(name).exists())
                    .unwrap_or_default()
            }
        };
        self.open_theme_input(InputPurpose::NewTheme, suggestion);
    }

    pub(super) fn on_theme_key(&mut self, key: KeyEvent) {
        if self.theme_doc().is_none() {
            if key.code == KeyCode::Char('n') {
                self.start_new_theme();
            }
            return;
        }
        let entries = self.theme_doc().map_or(0, |doc| doc.entries().len());
        match self.theme.focus {
            Focus::Layout => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.theme.cursor = self.theme.cursor.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.theme.cursor = (self.theme.cursor + 1).min(entries - 1)
                }
                KeyCode::Home | KeyCode::Char('g') => self.theme.cursor = 0,
                KeyCode::End | KeyCode::Char('G') => self.theme.cursor = entries - 1,
                KeyCode::PageUp => self.theme.cursor = self.theme.cursor.saturating_sub(10),
                KeyCode::PageDown => self.theme.cursor = (self.theme.cursor + 10).min(entries - 1),
                KeyCode::Char('n') => self.start_new_theme(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                    self.theme.focus = Focus::Options;
                    self.theme.prop_cursor = 0;
                }
                _ => {}
            },
            Focus::Options => {
                let props = self
                    .theme_entry()
                    .and_then(|entry| Some(self.theme_doc()?.props(&entry).len()))
                    .unwrap_or(0);
                let Some((entry, spec, value)) = self.theme_prop() else {
                    self.theme.focus = Focus::Layout;
                    return;
                };
                match key.code {
                    KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => {
                        self.theme.focus = Focus::Layout
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.theme.prop_cursor == 0 {
                            self.theme.focus = Focus::Layout;
                        } else {
                            self.theme.prop_cursor -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.theme.prop_cursor = (self.theme.prop_cursor + 1).min(props - 1)
                    }
                    KeyCode::Enter
                    | KeyCode::Char(' ')
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Char('h')
                    | KeyCode::Char('l')
                        if matches!(spec.kind, PropKind::Choice(_)) =>
                    {
                        let forward = !matches!(key.code, KeyCode::Left | KeyCode::Char('h'));
                        if let Some(next) = step_choice(&spec, value.as_ref(), forward) {
                            self.set_theme_prop(Some(Value::from(next)));
                        }
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if spec.kind == PropKind::Color => {
                        let code = self
                            .theme_doc()
                            .and_then(|doc| doc.resolve(&entry, &spec, value.as_ref()))
                            .unwrap_or(0);
                        self.mode = Mode::ColorPicker {
                            code,
                            prefer_name: value.as_ref().is_some_and(Value::is_string),
                        };
                        self.preview_stale = true;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if spec.kind == PropKind::Str => {
                        let current = value
                            .as_ref()
                            .map(edit_text)
                            .or_else(|| glyphs::fallback_text(&spec.fallback))
                            .and_then(|text| text.chars().next());
                        let selected = current
                            .and_then(|ch| glyphs::all().iter().position(|g| g.ch == ch))
                            .unwrap_or(0);
                        self.mode = Mode::IconBrowser {
                            query: String::new(),
                            selected,
                        };
                    }
                    KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('i') => {
                        let text = value.as_ref().map(edit_text).unwrap_or_default();
                        self.open_theme_input(InputPurpose::ThemeProperty, text);
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
                        if spec.kind == PropKind::Color =>
                    {
                        let code = self
                            .theme_doc()
                            .and_then(|doc| doc.resolve(&entry, &spec, value.as_ref()))
                            .unwrap_or(0);
                        let next = if matches!(key.code, KeyCode::Right | KeyCode::Char('l')) {
                            code.wrapping_add(1)
                        } else {
                            code.wrapping_sub(1)
                        };
                        self.set_theme_prop(Some(Value::from(next)));
                    }
                    KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace
                        if value.is_some() && self.set_theme_prop(None) =>
                    {
                        self.set_status(format!("Reset {}", spec.key), false);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Returns the picker's new state, or `None` once it closes.
    pub(super) fn on_color_picker_key(
        &mut self,
        key: KeyEvent,
        code: u8,
        prefer_name: bool,
    ) -> Option<Mode> {
        let moved = match key.code {
            KeyCode::Esc => {
                self.preview_stale = true;
                return None;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.set_theme_prop(Some(color_value(code, prefer_name)));
                return None;
            }
            KeyCode::Char('i') => {
                return Some(Mode::Input {
                    cursor: code.to_string().len(),
                    buffer: code.to_string(),
                    purpose: InputPurpose::ThemeProperty,
                });
            }
            KeyCode::Left | KeyCode::Char('h') => picker::step(code, picker::Direction::Left),
            KeyCode::Right | KeyCode::Char('l') => picker::step(code, picker::Direction::Right),
            KeyCode::Up | KeyCode::Char('k') => picker::step(code, picker::Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => picker::step(code, picker::Direction::Down),
            KeyCode::Home => 0,
            KeyCode::End => 255,
            _ => code,
        };
        if moved != code {
            self.preview_stale = true;
        }
        Some(Mode::ColorPicker {
            code: moved,
            prefer_name,
        })
    }

    /// Returns the browser's new state, or `None` once it closes.
    pub(super) fn on_icon_browser_key(
        &mut self,
        key: KeyEvent,
        mut query: String,
        mut selected: usize,
    ) -> Option<Mode> {
        let count = glyphs::search(&query).len();
        let last = count.saturating_sub(1);
        match key.code {
            KeyCode::Esc => {
                self.preview_stale = true;
                return None;
            }
            KeyCode::Enter => {
                if let Some(glyph) = glyphs::search(&query).get(selected) {
                    self.set_theme_prop(Some(Value::from(glyph.ch.to_string())));
                }
                return None;
            }
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down => selected = (selected + 1).min(last),
            KeyCode::PageUp => selected = selected.saturating_sub(ICON_PAGE),
            KeyCode::PageDown => selected = (selected + ICON_PAGE).min(last),
            KeyCode::Home => selected = 0,
            KeyCode::End => selected = last,
            KeyCode::Backspace => {
                query.pop();
                selected = 0;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                query.clear();
                selected = 0;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                query.push(c);
                selected = 0;
            }
            _ => {}
        }
        self.preview_stale = true;
        Some(Mode::IconBrowser { query, selected })
    }

    // ---- drawing ----

    pub(super) fn draw_icon_browser(
        &self,
        frame: &mut Frame,
        area: Rect,
        query: &str,
        selected: usize,
    ) {
        let title = match self.theme_prop() {
            Some((entry, spec, _)) => format!(" Nerd Font icons · {}.{} ", entry.label(), spec.key),
            None => " Nerd Font icons ".to_string(),
        };
        let popup = super::centered(area, 70, area.height.saturating_sub(4).max(12));
        frame.render_widget(Clear, popup);
        let block = panel(&title, true);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let [input, list_area, hints] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        let matches = glyphs::search(query);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("search: ").dark_gray(),
                Span::raw(query.to_string()),
                Span::raw(format!("   {} icons", matches.len())).dark_gray(),
            ])),
            input,
        );
        frame.set_cursor_position((
            input.x + 8 + unicode_width::UnicodeWidthStr::width(query) as u16,
            input.y,
        ));

        // Only the rows on screen are built: the full list is ~11k glyphs.
        let height = list_area.height as usize;
        let offset = selected.saturating_sub(height.saturating_sub(1));
        let items: Vec<ListItem> = matches
            .iter()
            .skip(offset)
            .take(height)
            .map(|glyph| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!(" {}  ", glyph.ch)).bold(),
                    Span::raw(format!("{:<44}", glyph.name)),
                    Span::raw(format!("U+{:04X}", glyph.ch as u32)).dark_gray(),
                ]))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(selected - offset.min(selected)));
        frame.render_stateful_widget(
            List::new(items).highlight_style(Style::new().bg(Color::Blue).fg(Color::White)),
            list_area,
            &mut state,
        );
        if matches.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from("no matching icon").dark_gray()),
                list_area,
            );
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("type").bold(),
                Span::raw(" search names or codes  ").dark_gray(),
                Span::raw("↑↓").bold(),
                Span::raw(" choose  ").dark_gray(),
                Span::raw("⏎").bold(),
                Span::raw(" use  ").dark_gray(),
                Span::raw("esc").bold(),
                Span::raw(" cancel").dark_gray(),
            ])),
            hints,
        );
    }

    pub(super) fn theme_focus(&self) -> &Focus {
        &self.theme.focus
    }

    pub(super) fn draw_theme(&mut self, frame: &mut Frame, area: Rect) {
        let name = self
            .doc
            .root()
            .get("theme")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let message = match (self.theme_path(), self.slot()) {
            (None, _) => Some(vec![
                Line::from(format!(
                    "The config uses the built-in {name} theme, which can't be edited."
                )),
                Line::default(),
                Line::from(vec![
                    Span::raw("n").bold(),
                    Span::raw(" create a custom theme file, starting from the example theme"),
                ]),
                Line::from(vec![
                    Span::raw("1").bold(),
                    Span::raw(" back to the layout, where Settings picks another theme"),
                ]),
            ]),
            (Some(_), Some(Slot::Missing)) => Some(vec![
                Line::from(format!("{name} does not exist yet.")),
                Line::default(),
                Line::from(vec![
                    Span::raw("n").bold(),
                    Span::raw(format!(" create {name} from the example theme")),
                ]),
            ]),
            (Some(_), Some(Slot::Broken(error))) => Some(vec![
                Line::from(format!("{name} can't be edited here:")).red(),
                Line::from(error.clone()).red(),
                Line::default(),
                Line::from("Fix the file by hand, or pick another theme in Settings (1)."),
            ]),
            _ => None,
        };
        if let Some(message) = message {
            let input = match &self.mode {
                Mode::Input {
                    buffer,
                    cursor,
                    purpose: InputPurpose::NewTheme,
                } => Some((buffer.clone(), *cursor)),
                _ => None,
            };
            let block = panel(" Theme ", true);
            let inner = block.inner(area);
            let mut lines = message;
            if let Some((buffer, cursor)) = &input {
                lines.push(Line::default());
                lines.push(Line::from(vec![
                    Span::raw("file name: ").dark_gray(),
                    Span::raw(buffer.clone()).underlined(),
                ]));
                let before: String = buffer.chars().take(*cursor).collect();
                let x =
                    inner.x + 11 + unicode_width::UnicodeWidthStr::width(before.as_str()) as u16;
                frame.set_cursor_position((x, inner.y + lines.len() as u16 - 1));
            }
            frame.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .block(block),
                area,
            );
            return;
        }

        let [left, right] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(area);
        self.draw_theme_modules(frame, left, &name);
        self.draw_theme_props(frame, right);
    }

    fn draw_theme_modules(&mut self, frame: &mut Frame, area: Rect, name: &str) {
        let Some(doc) = self.theme_doc() else {
            return;
        };
        let focused = self.theme.focus == Focus::Layout;
        let items: Vec<ListItem> = doc
            .entries()
            .iter()
            .map(|entry| {
                let (fg, bg) = doc.swatch(entry);
                let swatch = Style::new()
                    .fg(fg.map_or(Color::Reset, Color::Indexed))
                    .bg(bg.map_or(Color::Reset, Color::Indexed));
                let set = doc
                    .props(entry)
                    .iter()
                    .filter(|(_, value)| value.is_some())
                    .count();
                let mut spans = vec![Span::styled(format!(" {} ", entry.label()), swatch)];
                if set > 0 && *entry != ThemeEntry::Defaults {
                    spans.push(Span::raw(format!("  {set} set")).dark_gray());
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let highlight = if focused {
            Style::new().bg(Color::Blue).fg(Color::White)
        } else {
            Style::new().bg(Color::DarkGray)
        };
        let list = List::new(items)
            .block(panel(&format!(" {name} "), focused))
            .highlight_symbol("▸")
            .highlight_style(highlight);
        self.theme.list.select(Some(self.theme.cursor));
        frame.render_stateful_widget(list, area, &mut self.theme.list);
    }

    fn draw_theme_props(&self, frame: &mut Frame, area: Rect) {
        let (Some(doc), Some(entry)) = (self.theme_doc(), self.theme_entry()) else {
            return;
        };
        let focused = self.theme.focus == Focus::Options;
        let block = panel(&format!(" {} ", entry.label()), focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [body, help] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(inner);

        let props = doc.props(&entry);
        let key_width = props.iter().map(|(s, _)| s.key.len()).max().unwrap_or(0);
        let mut lines = Vec::new();
        let mut cursor_position = None;
        for (i, (spec, value)) in props.iter().enumerate() {
            let selected = focused && i == self.theme.prop_cursor;
            let key_style = if selected {
                Style::new().bold().fg(Color::Blue)
            } else {
                Style::new()
            };
            let mut spans = vec![
                Span::raw(if selected { "▸ " } else { "  " }),
                Span::styled(format!("{:key_width$}  ", spec.key), key_style),
            ];
            if let (true, Mode::Input { buffer, cursor, .. }) = (selected, &self.mode) {
                let before: String = buffer.chars().take(*cursor).collect();
                let x = 2 + key_width + 2 + unicode_width::UnicodeWidthStr::width(before.as_str());
                cursor_position = Some((inner.x + x as u16, i));
                spans.push(Span::raw(buffer.clone()).underlined());
            } else {
                spans.extend(prop_value_spans(doc, &entry, spec, value.as_ref()));
            }
            lines.push(Line::from(spans));
        }

        let visible = body.height as usize;
        let scroll = (self.theme.prop_cursor + 1).saturating_sub(visible);
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), body);
        if let Some((x, row)) = cursor_position {
            frame.set_cursor_position((x, body.y + (row - scroll) as u16));
        }

        if let Some((spec, _)) = props.get(self.theme.prop_cursor).filter(|_| focused) {
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

    pub(super) fn draw_color_picker(&self, frame: &mut Frame, area: Rect, code: u8) {
        let title = match self.theme_prop() {
            Some((entry, spec, _)) => format!(" {}.{} ", entry.label(), spec.key),
            None => " Colour ".to_string(),
        };
        // Grid, a blank line, the selection and the key hints, inside a border
        // with a line of padding on each side.
        let footer = 3;
        let spaced = area.height >= picker::height(true) + footer + 2;
        let popup = super::centered(area, picker::WIDTH + 4, picker::height(spaced) + footer + 2);
        frame.render_widget(Clear, popup);
        let block = panel(&title, true);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let inner = Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
            ..inner
        };
        let [grid, info] = Layout::vertical([
            Constraint::Length(picker::height(spaced)),
            Constraint::Length(footer),
        ])
        .areas(inner);
        picker::render(frame.buffer_mut(), grid, code);

        let names = color_names(code);
        let mut lines = vec![Line::default()];
        lines.push(Line::from(vec![
            Span::styled("      ", Style::new().bg(Color::Indexed(code))),
            Span::raw(format!(" {code}")).bold(),
            Span::raw(if names.is_empty() {
                String::new()
            } else {
                format!("  {}", names.join(", "))
            })
            .dark_gray(),
        ]));
        lines.push(Line::from(vec![
            Span::raw("←↑↓→").bold(),
            Span::raw(" move  ").dark_gray(),
            Span::raw("⏎").bold(),
            Span::raw(" pick  ").dark_gray(),
            Span::raw("i").bold(),
            Span::raw(" type  ").dark_gray(),
            Span::raw("esc").bold(),
            Span::raw(" cancel").dark_gray(),
        ]));
        frame.render_widget(Paragraph::new(lines), info);
    }
}

/// An icon or symbol value: the text itself, then its code points and glyph
/// name, the same way for a set value and a default.
fn icon_spans(text: &str, default: bool) -> Vec<Span<'static>> {
    let shown = Span::raw(glyphs::visible(text));
    let mut detail = format!("  {}", glyphs::describe(text));
    if default {
        detail.push_str(" (default)");
    }
    vec![
        if default {
            shown.dark_gray()
        } else {
            shown.yellow()
        },
        Span::raw(detail).dark_gray(),
    ]
}

fn prop_value_spans(
    doc: &ThemeDoc,
    entry: &ThemeEntry,
    spec: &PropSpec,
    value: Option<&Value>,
) -> Vec<Span<'static>> {
    let swatch = |code: Option<u8>| match code {
        Some(code) => Span::styled("  ", Style::new().bg(Color::Indexed(code))),
        None => Span::raw("  "),
    };
    match (spec.kind, value) {
        (PropKind::ColorList, Some(Value::Array(items))) => {
            let mut spans: Vec<Span> = items
                .iter()
                .map(|item| swatch(crate::themes::color_code(item)))
                .collect();
            spans.push(Span::raw(format!(" {}", edit_text(value.unwrap()))).yellow());
            spans
        }
        (PropKind::Str, Some(value)) => icon_spans(&edit_text(value), false),
        (PropKind::Choice(_), Some(value)) => vec![Span::raw(edit_text(value)).yellow()],
        (PropKind::Choice(_), None) if spec.fallback.is_empty() => {
            vec![Span::raw("unset").dark_gray()]
        }
        (PropKind::Choice(_), None) => {
            vec![Span::raw(format!("{} (default)", spec.fallback)).dark_gray()]
        }
        (PropKind::Str, None) => match glyphs::fallback_text(&spec.fallback) {
            Some(text) => icon_spans(&text, true),
            None if spec.fallback.is_empty() => vec![Span::raw("unset").dark_gray()],
            None => vec![Span::raw(format!("default: {}", spec.fallback)).dark_gray()],
        },
        (_, Some(value)) => vec![
            swatch(doc.resolve(entry, spec, Some(value))),
            Span::raw(format!(" {}", edit_text(value))).yellow(),
        ],
        (_, None) => {
            let resolved = doc.resolve(entry, spec, None);
            let label = if spec.fallback.is_empty() {
                "unset".to_string()
            } else {
                spec.fallback.clone()
            };
            let mut spans = vec![swatch(resolved), Span::raw(format!(" {label}")).dark_gray()];
            if let Some(code) = resolved {
                spans.push(Span::raw(format!(" ({code})")).dark_gray());
            }
            spans
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyModifiers;
    use serde_json::json;

    use super::super::model::{Document, Entry, SegPos, Side};
    use super::*;

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "superline-editor-padding-{label}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn theme_padding_cycles_through_its_choices() {
        let dir = scratch_dir("theme");
        std::fs::write(
            dir.join("theme.json"),
            r#"{ "defaults": { "fg": 15, "bg": 0 }, "modules": {} }"#,
        )
        .unwrap();
        let config = json!({ "theme": "theme.json", "rows": [{ "left": ["git"] }] });
        let mut app = App::new(Document::new(config).unwrap(), dir.join("config.json"));
        app.load_theme_slot();
        press(&mut app, KeyCode::Char('2'));
        let git = ThemeEntry::Module("git".into());
        let doc = app.theme_doc().unwrap();
        let git_row = doc.entries().iter().position(|e| *e == git).unwrap();
        let props = doc.props(&git);
        let padding_row = props.iter().position(|(s, _)| s.key == "padding").unwrap();
        app.theme.cursor = git_row;
        press(&mut app, KeyCode::Enter);
        app.theme.prop_cursor = padding_row;
        let padding =
            |app: &App| app.theme_doc().unwrap().root()["modules"]["git"]["padding"].clone();

        // Unset, it steps on from git's own padding, large.
        for (key, expected) in [
            (KeyCode::Right, "left"),
            (KeyCode::Char(' '), "right"),
            (KeyCode::Enter, "small"),
            (KeyCode::Char('l'), "large"),
            (KeyCode::Left, "small"),
            (KeyCode::Char('h'), "right"),
        ] {
            press(&mut app, key);
            assert_eq!(padding(&app), json!(expected), "{key:?}");
        }
        assert!(matches!(app.mode, Mode::Normal));

        press(&mut app, KeyCode::Char('x'));
        assert_eq!(app.theme_doc().unwrap().root()["modules"], json!({}));
        press(&mut app, KeyCode::Left);
        assert_eq!(padding(&app), json!("small"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn widget_padding_cycles_through_its_choices_and_unset() {
        let dir = scratch_dir("widget");
        let config = json!({ "theme": "rainbow", "rows": [{ "left": ["battery"] }] });
        let mut app = App::new(Document::new(config).unwrap(), dir.join("config.json"));
        let battery = SegPos {
            row: 0,
            side: Side::Left,
            index: 0,
        };
        app.select(Entry::Segment(battery));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.current_option().unwrap().1.key, "padding");

        for (key, expected) in [
            (KeyCode::Right, json!({ "battery": { "padding": "small" } })),
            (KeyCode::Enter, json!({ "battery": { "padding": "large" } })),
            (KeyCode::Right, json!({ "battery": { "padding": "left" } })),
            (KeyCode::Right, json!({ "battery": { "padding": "right" } })),
            (KeyCode::Right, json!("battery")),
            (KeyCode::Left, json!({ "battery": { "padding": "right" } })),
        ] {
            press(&mut app, key);
            assert_eq!(app.doc.segment(battery), &expected, "{key:?}");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
