//! The config being edited, held as raw JSON so that anything the editor does
//! not model (unknown widgets, their options, extra keys) survives a save.

use serde_json::{Map, Value};

use super::schema::{self, OptionSpec, Shape, WidgetSpec};
use crate::config::LineSegment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn key(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegPos {
    pub row: usize,
    pub side: Side,
    pub index: usize,
}

/// One line of the layout tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Settings,
    Row(usize),
    Side(usize, Side),
    Segment(SegPos),
}

/// What an option edit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Settings,
    Segment(SegPos),
}

pub struct Document {
    root: Value,
    saved: Value,
    undo: Vec<Value>,
    redo: Vec<Value>,
}

impl Document {
    pub fn new(root: Value) -> Result<Self, String> {
        let rows = root
            .get("rows")
            .and_then(Value::as_array)
            .ok_or("the config has no \"rows\" array")?;
        for (i, row) in rows.iter().enumerate() {
            if !row.get("left").is_some_and(Value::is_array) {
                return Err(format!("row {} has no \"left\" array", i + 1));
            }
            if !matches!(row.get("right"), None | Some(Value::Null | Value::Array(_))) {
                return Err(format!(
                    "row {} has a \"right\" that is not an array",
                    i + 1
                ));
            }
        }
        Ok(Document {
            saved: root.clone(),
            root,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn root(&self) -> &Value {
        &self.root
    }

    pub fn is_dirty(&self) -> bool {
        self.root != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.root.clone();
    }

    fn rows(&self) -> &Vec<Value> {
        self.root["rows"].as_array().expect("validated in new")
    }

    fn rows_mut(root: &mut Value) -> &mut Vec<Value> {
        root["rows"].as_array_mut().expect("validated in new")
    }

    pub fn row_count(&self) -> usize {
        self.rows().len()
    }

    pub fn segments(&self, row: usize, side: Side) -> &[Value] {
        self.rows()[row]
            .get(side.key())
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn segment(&self, pos: SegPos) -> &Value {
        &self.segments(pos.row, pos.side)[pos.index]
    }

    fn side_mut(root: &mut Value, row: usize, side: Side) -> &mut Vec<Value> {
        let row = &mut Self::rows_mut(root)[row];
        let slot = &mut row[side.key()];
        if !slot.is_array() {
            *slot = Value::Array(Vec::new());
        }
        slot.as_array_mut().unwrap()
    }

    pub fn entries(&self) -> Vec<Entry> {
        let mut entries = vec![Entry::Settings];
        for row in 0..self.row_count() {
            entries.push(Entry::Row(row));
            for side in [Side::Left, Side::Right] {
                entries.push(Entry::Side(row, side));
                for index in 0..self.segments(row, side).len() {
                    entries.push(Entry::Segment(SegPos { row, side, index }));
                }
            }
        }
        entries
    }

    /// Applies `change` to a copy of the config and keeps it (with an undo
    /// step) if it succeeds.
    fn edit<T>(
        &mut self,
        change: impl FnOnce(&mut Value) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut next = self.root.clone();
        let result = change(&mut next)?;
        if next != self.root {
            let previous = std::mem::replace(&mut self.root, next);
            self.undo.push(previous);
            self.redo.clear();
        }
        Ok(result)
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(previous) => {
                let current = std::mem::replace(&mut self.root, previous);
                self.redo.push(current);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                let current = std::mem::replace(&mut self.root, next);
                self.undo.push(current);
                true
            }
            None => false,
        }
    }

    /// Replaces the whole config, e.g. after it was edited outside the editor.
    pub fn replace(&mut self, root: Value) -> Result<(), String> {
        let fresh = Document::new(root)?;
        let _ = self.edit(|value| {
            *value = fresh.root;
            Ok(())
        });
        self.mark_saved();
        Ok(())
    }

    pub fn insert(&mut self, row: usize, side: Side, index: usize, segment: Value) -> SegPos {
        let _ = self.edit(|root| {
            let segments = Self::side_mut(root, row, side);
            segments.insert(index.min(segments.len()), segment);
            Ok(())
        });
        SegPos { row, side, index }
    }

    pub fn remove(&mut self, pos: SegPos) {
        let _ = self.edit(|root| {
            Self::side_mut(root, pos.row, pos.side).remove(pos.index);
            Ok(())
        });
    }

    pub fn duplicate(&mut self, pos: SegPos) -> SegPos {
        let copy = self.segment(pos).clone();
        self.insert(pos.row, pos.side, pos.index + 1, copy)
    }

    /// Moves a segment one step. Past either end of a side it crosses into the
    /// neighbouring side (left and right of each row, top to bottom).
    pub fn move_segment(&mut self, pos: SegPos, down: bool) -> Option<SegPos> {
        let len = self.segments(pos.row, pos.side).len();
        let target = if down {
            if pos.index + 1 < len {
                Some(SegPos {
                    index: pos.index + 1,
                    ..pos
                })
            } else {
                match pos.side {
                    Side::Left => Some((pos.row, Side::Right)),
                    Side::Right if pos.row + 1 < self.row_count() => {
                        Some((pos.row + 1, Side::Left))
                    }
                    Side::Right => None,
                }
                .map(|(row, side)| SegPos {
                    row,
                    side,
                    index: 0,
                })
            }
        } else if pos.index > 0 {
            Some(SegPos {
                index: pos.index - 1,
                ..pos
            })
        } else {
            match pos.side {
                Side::Right => Some((pos.row, Side::Left)),
                Side::Left if pos.row > 0 => Some((pos.row - 1, Side::Right)),
                Side::Left => None,
            }
            .map(|(row, side)| SegPos {
                row,
                side,
                index: self.segments(row, side).len(),
            })
        }?;

        let _ = self.edit(|root| {
            let segment = Self::side_mut(root, pos.row, pos.side).remove(pos.index);
            Self::side_mut(root, target.row, target.side).insert(target.index, segment);
            Ok(())
        });
        Some(target)
    }

    pub fn add_row(&mut self, index: usize) -> usize {
        let _ = self.edit(|root| {
            let rows = Self::rows_mut(root);
            let index = index.min(rows.len());
            rows.insert(index, serde_json::json!({ "left": [] }));
            Ok(())
        });
        index.min(self.row_count() - 1)
    }

    pub fn remove_row(&mut self, row: usize) -> Result<(), String> {
        if self.row_count() <= 1 {
            return Err("the config needs at least one row".into());
        }
        self.edit(|root| {
            Self::rows_mut(root).remove(row);
            Ok(())
        })
    }

    pub fn move_row(&mut self, row: usize, down: bool) -> Option<usize> {
        let target = if down {
            (row + 1 < self.row_count()).then_some(row + 1)
        } else {
            row.checked_sub(1)
        }?;
        let _ = self.edit(|root| {
            Self::rows_mut(root).swap(row, target);
            Ok(())
        });
        Some(target)
    }

    /// The options shown for a target, with their current values (`None` when
    /// the key is absent).
    pub fn options(&self, target: Target) -> Vec<(OptionSpec, Option<&Value>)> {
        match target {
            Target::Settings => {
                let mut options = vec![(schema::THEME, self.root.get("theme"))];
                let update = self.root.get("update");
                for spec in schema::UPDATE {
                    options.push((*spec, update.and_then(|u| u.get(spec.key))));
                }
                options
            }
            Target::Segment(pos) => {
                let segment = self.segment(pos);
                let Some(spec) = widget_spec(segment) else {
                    return Vec::new();
                };
                spec.options()
                    .iter()
                    .map(|option| (*option, option_value(segment, spec, option.key)))
                    .collect()
            }
        }
    }

    /// Sets (or with `None`, removes) one option. A segment edit is rejected if
    /// superline could not parse the result.
    pub fn set_option(
        &mut self,
        target: Target,
        key: &str,
        value: Option<Value>,
    ) -> Result<(), String> {
        match target {
            Target::Settings => self.edit(|root| {
                let root = root.as_object_mut().ok_or("config is not an object")?;
                if key == schema::THEME.key {
                    let value = value.ok_or("theme is required")?;
                    root.insert(key.into(), value);
                    return Ok(());
                }
                let update = root
                    .entry("update")
                    .or_insert_with(|| Value::Object(Map::new()));
                if !update.is_object() {
                    *update = Value::Object(Map::new());
                }
                let map = update.as_object_mut().unwrap();
                match value {
                    Some(value) => {
                        map.insert(key.into(), value);
                    }
                    None => {
                        map.shift_remove(key);
                    }
                }
                if map.is_empty() {
                    root.shift_remove("update");
                }
                Ok(())
            }),
            Target::Segment(pos) => self.edit(|root| {
                let segment = &mut Self::side_mut(root, pos.row, pos.side)[pos.index];
                let spec = widget_spec(segment).ok_or("unknown widget")?;
                set_segment_option(segment, spec, key, value)?;
                serde_json::from_value::<LineSegment>(segment.clone())
                    .map_err(|e| e.to_string())?;
                Ok(())
            }),
        }
    }
}

pub fn widget_spec(segment: &Value) -> Option<&'static WidgetSpec> {
    schema::segment_name(segment).and_then(schema::find)
}

fn option_value<'a>(segment: &'a Value, spec: &WidgetSpec, key: &str) -> Option<&'a Value> {
    let (_, inner) = segment.as_object()?.iter().next()?;
    match spec.shape {
        Shape::Value(_) => Some(inner),
        _ => inner.get(key),
    }
}

fn set_segment_option(
    segment: &mut Value,
    spec: &WidgetSpec,
    key: &str,
    value: Option<Value>,
) -> Result<(), String> {
    // Keep whichever name (or alias) the config already uses.
    let name = schema::segment_name(segment)
        .ok_or("unrecognised segment")?
        .to_string();
    match spec.shape {
        Shape::Unit => Err(format!("{name} has no options")),
        Shape::Value(option) => {
            let value = value.ok_or(format!("{} is required", option.key))?;
            *segment = Value::Object(Map::from_iter([(name, value)]));
            Ok(())
        }
        Shape::Object(_) => {
            let mut options = match segment {
                Value::Object(map) => match map.values().next() {
                    Some(Value::Object(inner)) => inner.clone(),
                    _ => Map::new(),
                },
                _ => Map::new(),
            };
            match value {
                Some(value) => {
                    options.insert(key.into(), value);
                }
                None => {
                    options.shift_remove(key);
                }
            }
            *segment = if options.is_empty() {
                Value::String(name)
            } else {
                Value::Object(Map::from_iter([(name, Value::Object(options))]))
            };
            Ok(())
        }
    }
}

/// A compact rendering of a scalar option value. Word-like strings (choices,
/// file names) are left unquoted.
pub fn show_value(value: &Value) -> String {
    match value {
        Value::String(s)
            if !s.is_empty()
                && s.chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/')) =>
        {
            s.clone()
        }
        Value::String(s) => format!("{s:?}"),
        other => other.to_string(),
    }
}

/// The widget's name plus a short summary of the options it sets.
pub fn describe(segment: &Value) -> (String, String) {
    let name = schema::segment_name(segment)
        .map(str::to_string)
        .unwrap_or_else(|| "?".into());
    let spec = widget_spec(segment);
    let detail = match (spec.map(|s| s.shape), segment) {
        (None, _) => "unknown widget, kept as-is".into(),
        (Some(Shape::Value(_)), Value::Object(map)) => {
            map.values().next().map(show_value).unwrap_or_default()
        }
        (Some(_), Value::Object(map)) => match map.values().next() {
            Some(Value::Object(inner)) => inner
                .iter()
                .map(|(key, value)| format!("{key}={}", show_value(value)))
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        },
        _ => String::new(),
    };
    (name, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Document {
        Document::new(json!({
            "theme": "rainbow",
            "rows": [
                { "left": ["read_only", "git"], "right": ["battery"] },
                { "left": ["cmd"] }
            ]
        }))
        .unwrap()
    }

    fn pos(row: usize, side: Side, index: usize) -> SegPos {
        SegPos { row, side, index }
    }

    #[test]
    fn rejects_configs_without_rows() {
        assert!(Document::new(json!({ "theme": "rainbow" })).is_err());
        assert!(Document::new(json!({ "rows": [{ "right": [] }] })).is_err());
    }

    #[test]
    fn entries_list_every_row_side_and_segment() {
        let entries = doc().entries();
        assert_eq!(entries[0], Entry::Settings);
        assert_eq!(entries[1], Entry::Row(0));
        assert_eq!(entries[2], Entry::Side(0, Side::Left));
        assert_eq!(entries[4], Entry::Segment(pos(0, Side::Left, 1)));
        assert_eq!(entries.len(), 1 + (1 + 2 + 3) + (1 + 2 + 1));
    }

    #[test]
    fn moving_past_the_end_of_a_side_crosses_into_the_next() {
        let mut doc = doc();
        let moved = doc.move_segment(pos(0, Side::Left, 1), true).unwrap();
        assert_eq!(moved, pos(0, Side::Right, 0));
        assert_eq!(
            doc.segments(0, Side::Right),
            &[json!("git"), json!("battery")]
        );

        let moved = doc.move_segment(pos(0, Side::Right, 1), true).unwrap();
        assert_eq!(moved, pos(1, Side::Left, 0));
        assert_eq!(
            doc.segments(1, Side::Left),
            &[json!("battery"), json!("cmd")]
        );

        // Into a row that has no right side yet.
        let moved = doc.move_segment(pos(1, Side::Left, 1), true).unwrap();
        assert_eq!(moved, pos(1, Side::Right, 0));
        assert!(doc.move_segment(moved, true).is_none());
    }

    #[test]
    fn moving_up_from_the_start_lands_at_the_end_of_the_previous_side() {
        let mut doc = doc();
        let moved = doc.move_segment(pos(1, Side::Left, 0), false).unwrap();
        assert_eq!(moved, pos(0, Side::Right, 1));
        assert!(doc.move_segment(pos(0, Side::Left, 0), false).is_none());
    }

    #[test]
    fn undo_and_redo_restore_each_step() {
        let mut doc = doc();
        let original = doc.root().clone();
        doc.remove(pos(0, Side::Left, 0));
        let removed = doc.root().clone();
        assert!(doc.is_dirty());
        assert!(doc.undo());
        assert_eq!(doc.root(), &original);
        assert!(!doc.is_dirty());
        assert!(doc.redo());
        assert_eq!(doc.root(), &removed);
    }

    #[test]
    fn setting_an_option_expands_a_bare_segment() {
        let mut doc = doc();
        let git = Target::Segment(pos(0, Side::Left, 1));
        doc.set_option(git, "backend", Some(json!("cli"))).unwrap();
        assert_eq!(
            doc.segment(pos(0, Side::Left, 1)),
            &json!({ "git": { "backend": "cli" } })
        );

        doc.set_option(git, "backend", None).unwrap();
        assert_eq!(doc.segment(pos(0, Side::Left, 1)), &json!("git"));
    }

    #[test]
    fn invalid_option_values_are_rejected() {
        let mut doc = Document::new(json!({
            "theme": "rainbow",
            "rows": [{ "left": [{ "ai_usage": { "provider": "claude" } }] }]
        }))
        .unwrap();
        let usage = Target::Segment(pos(0, Side::Left, 0));
        let before = doc.root().clone();
        assert!(doc
            .set_option(
                usage,
                "session_time_remaining_only_at_limit",
                Some(json!(2.0))
            )
            .is_err());
        assert!(doc.set_option(usage, "provider", None).is_err());
        assert_eq!(doc.root(), &before);
    }

    #[test]
    fn value_widgets_replace_their_value_and_keep_aliases() {
        let mut doc = Document::new(json!({
            "theme": "rainbow",
            "rows": [{ "left": [{ "padding": 2 }, { "sdkman": { "jdk": false } }] }]
        }))
        .unwrap();
        doc.set_option(
            Target::Segment(pos(0, Side::Left, 0)),
            "width",
            Some(json!(4)),
        )
        .unwrap();
        assert_eq!(doc.segment(pos(0, Side::Left, 0)), &json!({ "padding": 4 }));

        doc.set_option(
            Target::Segment(pos(0, Side::Left, 1)),
            "version",
            Some(json!(false)),
        )
        .unwrap();
        assert_eq!(
            doc.segment(pos(0, Side::Left, 1)),
            &json!({ "sdkman": { "jdk": false, "version": false } })
        );
    }

    #[test]
    fn update_settings_drop_the_block_when_emptied() {
        let mut doc = doc();
        doc.set_option(Target::Settings, "auto", Some(json!(false)))
            .unwrap();
        assert_eq!(doc.root()["update"], json!({ "auto": false }));
        doc.set_option(Target::Settings, "auto", None).unwrap();
        assert!(doc.root().get("update").is_none());
    }

    #[test]
    fn unknown_widgets_survive_edits_around_them() {
        let future = json!({ "future_widget": { "colour": "teal" } });
        let mut doc = Document::new(json!({
            "theme": "rainbow",
            "rows": [{ "left": [future.clone(), "git"] }]
        }))
        .unwrap();
        doc.move_segment(pos(0, Side::Left, 1), false).unwrap();
        assert_eq!(doc.segment(pos(0, Side::Left, 1)), &future);
        assert!(doc
            .options(Target::Segment(pos(0, Side::Left, 1)))
            .is_empty());
    }

    #[test]
    fn keeps_at_least_one_row() {
        let mut doc = doc();
        doc.remove_row(0).unwrap();
        assert!(doc.remove_row(0).is_err());
        assert_eq!(doc.add_row(1), 1);
        assert_eq!(doc.row_count(), 2);
    }

    #[test]
    fn describes_segments_by_their_options() {
        assert_eq!(describe(&json!("git")), ("git".into(), String::new()));
        assert_eq!(
            describe(&json!({ "cwd": { "max_length": 60, "wanted_seg_num": 5 } })),
            ("cwd".into(), "max_length=60 wanted_seg_num=5".into())
        );
        assert_eq!(
            describe(&json!({ "padding": 2 })),
            ("padding".into(), "2".into())
        );
    }
}
