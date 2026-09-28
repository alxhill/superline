//! The theme picker: the bundled themes and the theme files next to the
//! config, each drawn in the preview while it is highlighted. Choosing a
//! bundled theme installs its file into the config directory.

use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use super::model::Target;
use super::theme::palette;
use super::theme_page::Slot;
use super::{panel, schema, theme_choices, App, InputPurpose, Mode};
use crate::themes::{bundled_theme, theme_path, validate_theme};

/// Colours in each theme's swatch strip.
const PALETTE: usize = 8;

pub(super) struct ThemeChoice {
    /// The name the config gives it.
    name: String,
    path: PathBuf,
    /// A bundled theme whose file is not in the config directory yet.
    installs: bool,
    theme: Result<Value, String>,
}

impl App {
    pub(super) fn open_theme_picker(&mut self) {
        let dir = self.config_dir().to_path_buf();
        let mut names = theme_choices(&self.path);
        // Themes created this session but not saved yet.
        for name in &self.theme_choices {
            if !names
                .iter()
                .any(|n| theme_path(&dir, n) == theme_path(&dir, name))
            {
                names.push(name.clone());
            }
        }
        let current = self.doc.root().get("theme").and_then(Value::as_str);
        if let Some(current) = current {
            if !names
                .iter()
                .any(|n| theme_path(&dir, n) == theme_path(&dir, current))
            {
                names.insert(0, current.to_string());
            }
        }
        let current = current.map(|name| theme_path(&dir, name));
        let choices: Vec<ThemeChoice> = names.into_iter().map(|name| self.choice(name)).collect();
        let selected = choices
            .iter()
            .position(|choice| Some(&choice.path) == current.as_ref())
            .unwrap_or(0);
        self.theme_choices = choices.iter().map(|choice| choice.name.clone()).collect();
        self.mode = Mode::ThemePicker { choices, selected };
        self.preview_stale = true;
    }

    fn choice(&self, name: String) -> ThemeChoice {
        let path = theme_path(self.config_dir(), &name);
        let exists = path.exists();
        let bundled = bundled_theme(self.config_dir(), &path);
        let theme = match (self.theme.slots.get(&path), bundled) {
            (Some(Slot::Loaded(doc)), _) => Ok(doc.root().clone()),
            _ if exists => read_theme_file(&path),
            (_, Some(text)) => serde_json::from_str(text).map_err(|e| e.to_string()),
            _ => Err("does not exist yet".into()),
        };
        ThemeChoice {
            name,
            installs: !exists && bundled.is_some(),
            path,
            theme,
        }
    }

    /// The theme highlighted in the picker, for the preview.
    pub(super) fn picker_theme(&self) -> Option<Option<Value>> {
        match &self.mode {
            Mode::ThemePicker { choices, selected } => {
                Some(choices.get(*selected)?.theme.clone().ok())
            }
            _ => None,
        }
    }

    /// Points the config at a theme, installing a bundled theme's file.
    pub(super) fn use_theme(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            self.set_status("Give the theme a file name", true);
            return false;
        }
        let path = theme_path(self.config_dir(), name);
        let installs = !path.exists() && bundled_theme(self.config_dir(), &path).is_some();
        if let Err(error) =
            self.doc
                .set_option(Target::Settings, schema::THEME.key, Some(Value::from(name)))
        {
            self.set_status(error, true);
            return false;
        }
        let dir = self.config_dir();
        if !self
            .theme_choices
            .iter()
            .any(|choice| theme_path(dir, choice) == path)
        {
            self.theme_choices.push(name.to_string());
        }
        self.changed();
        self.load_theme_slot();
        if self.status.as_ref().is_some_and(|status| status.error) {
            return true;
        }
        let file = path
            .file_name()
            .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        self.set_status(
            if installs && path.exists() {
                format!("Switched to {name} and installed {file} (s to save the config)")
            } else {
                format!("Switched to {name} (s to save the config)")
            },
            false,
        );
        true
    }

    /// Returns the picker's new state, or `None` once it closes.
    pub(super) fn on_theme_picker_key(
        &mut self,
        key: KeyEvent,
        choices: Vec<ThemeChoice>,
        mut selected: usize,
    ) -> Option<Mode> {
        let last = choices.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.preview_stale = true;
                return None;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(choice) = choices.get(selected) {
                    self.use_theme(&choice.name.clone());
                }
                self.preview_stale = true;
                return None;
            }
            KeyCode::Char('i') => {
                let buffer = self
                    .doc
                    .root()
                    .get("theme")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.preview_stale = true;
                return Some(Mode::Input {
                    cursor: buffer.chars().count(),
                    buffer,
                    purpose: InputPurpose::ThemeName,
                });
            }
            KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => selected = (selected + 1).min(last),
            KeyCode::Home | KeyCode::Char('g') => selected = 0,
            KeyCode::End | KeyCode::Char('G') => selected = last,
            _ => {}
        }
        self.preview_stale = true;
        Some(Mode::ThemePicker { choices, selected })
    }

    pub(super) fn draw_theme_picker(
        &self,
        frame: &mut Frame,
        area: Rect,
        choices: &[ThemeChoice],
        selected: usize,
    ) {
        let dir = self.config_dir();
        let current = self
            .doc
            .root()
            .get("theme")
            .and_then(Value::as_str)
            .map(|name| theme_path(dir, name));
        let name_width = choices
            .iter()
            .map(|choice| choice.name.width())
            .max()
            .unwrap_or(0);
        let items: Vec<ListItem> = choices
            .iter()
            .enumerate()
            .map(|(i, choice)| {
                // Only the name is highlighted, so the swatches keep their colours.
                let name = Span::raw(format!(" {:name_width$} ", choice.name)).bold();
                let mut spans = if i == selected {
                    vec![Span::raw("▸"), name.fg(Color::White).bg(Color::Blue)]
                } else {
                    vec![Span::raw(" "), name]
                };
                spans.push(Span::raw(" "));
                match &choice.theme {
                    Ok(theme) => {
                        let colors = palette(theme, PALETTE);
                        for code in &colors {
                            spans.push(Span::styled("  ", Style::new().bg(Color::Indexed(*code))));
                        }
                        spans.push(Span::raw("  ".repeat(PALETTE - colors.len())));
                    }
                    Err(_) => spans.push(Span::raw("  ".repeat(PALETTE))),
                }
                let about = match &choice.theme {
                    Err(error) => Span::raw(format!("  {error}")).red(),
                    Ok(_) if Some(&choice.path) == current.as_ref() => {
                        Span::raw("  in use").green()
                    }
                    Ok(_) if choice.installs => {
                        Span::raw(format!("  built in, installs {}", file_name(&choice.path)))
                            .dark_gray()
                    }
                    Ok(_) => Span::raw(format!("  {}", file_name(&choice.path))).dark_gray(),
                };
                spans.push(about);
                ListItem::new(Line::from(spans))
            })
            .collect();

        let width = 72.min(area.width);
        let popup = super::centered(area, width, choices.len() as u16 + 4);
        frame.render_widget(Clear, popup);
        let block = panel(" Pick a theme ", true);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let [list_area, hints] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(inner);
        let mut state = ListState::default().with_selected(Some(selected));
        frame.render_stateful_widget(List::new(items), list_area, &mut state);
        super::draw_scrollbar(frame, list_area, choices.len(), state.offset());
        frame.render_widget(
            Paragraph::new(vec![
                Line::default(),
                Line::from(vec![
                    Span::raw("↑↓").bold(),
                    Span::raw(" preview  ").dark_gray(),
                    Span::raw("⏎").bold(),
                    Span::raw(" use  ").dark_gray(),
                    Span::raw("i").bold(),
                    Span::raw(" type a file name  ").dark_gray(),
                    Span::raw("esc").bold(),
                    Span::raw(" cancel").dark_gray(),
                ]),
            ]),
            hints,
        );
    }
}

fn read_theme_file(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let theme: Value = serde_json::from_str(&text).map_err(|e| format!("not valid JSON: {e}"))?;
    validate_theme(&theme)?;
    Ok(theme)
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |f| f.to_string_lossy().into_owned(),
    )
}
