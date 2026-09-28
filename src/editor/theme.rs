//! The theme file being edited on the editor's Theme page, and what the editor
//! knows about each module's theme properties.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use super::schema::WIDGETS;
use crate::colors::NAMED_COLORS;
use crate::config::{LineSegment, Widget};
use crate::powerline::{default_padding, theme_module};
use crate::themes::{color_code, infer_theme_property_kind, validate_theme, ThemePropertyKind};

/// Every module's theme properties, with what they style and their fallback.
/// Shared with the website's configuration reference.
const THEME_OPTIONS: &str = include_str!("../../scripts/site-screenshots/theme-options.json");

/// A new custom theme starts as a copy of the example theme.
pub const STARTER_THEME: &str = include_str!("../../example_theme.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    Color,
    ColorList,
    Str,
    /// One of a fixed set of strings, cycled through rather than typed.
    Choice(&'static [&'static str]),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropSpec {
    pub key: String,
    pub kind: PropKind,
    pub help: String,
    /// What applies when the key is absent, e.g. `defaults.fg`.
    pub fallback: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub props: Vec<PropSpec>,
}

pub fn module_specs() -> &'static [ModuleSpec] {
    static SPECS: OnceLock<Vec<ModuleSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        let options: Value =
            serde_json::from_str(THEME_OPTIONS).expect("theme-options.json parses");
        options
            .as_object()
            .expect("theme-options.json is an object")
            .iter()
            .filter(|(name, _)| !name.starts_with('_'))
            .map(|(name, module)| ModuleSpec {
                name: name.clone(),
                aliases: module["aliases"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                props: module["properties"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|row| PropSpec {
                        key: row[0].as_str().unwrap_or_default().to_string(),
                        kind: match row[1].as_str() {
                            Some("color") => PropKind::Color,
                            Some("color list") => PropKind::ColorList,
                            Some(kind) if kind.starts_with("choice") => {
                                inferred_kind(row[0].as_str().unwrap_or_default())
                            }
                            _ => PropKind::Str,
                        },
                        help: row[2].as_str().unwrap_or_default().to_string(),
                        fallback: row[3].as_str().unwrap_or_default().to_string(),
                    })
                    .collect(),
            })
            .collect()
    })
}

fn defaults_props() -> Vec<PropSpec> {
    ["fg", "bg"]
        .into_iter()
        .map(|key| PropSpec {
            key: key.into(),
            kind: PropKind::Color,
            help: format!(
                "Default {} for anything a module does not set. Required.",
                if key == "fg" {
                    "text colour"
                } else {
                    "background"
                }
            ),
            fallback: String::new(),
        })
        .collect()
}

/// A property's kind as the theme loader validates it.
fn inferred_kind(key: &str) -> PropKind {
    match infer_theme_property_kind(key) {
        Some(ThemePropertyKind::ColorList) => PropKind::ColorList,
        Some(ThemePropertyKind::Color) => PropKind::Color,
        Some(ThemePropertyKind::Choice(variants)) => PropKind::Choice(variants),
        _ => PropKind::Str,
    }
}

/// What a theme module's `padding` falls back to, as the widgets themed under
/// it declare in code: `large`, or `small for small_spacer, large for
/// large_spacer` when they differ.
pub fn module_default_padding(module: &str) -> Option<&'static str> {
    static DEFAULTS: OnceLock<Vec<(&'static str, String)>> = OnceLock::new();
    let defaults = DEFAULTS.get_or_init(|| {
        let widgets = WIDGETS
            .iter()
            .filter_map(|spec| Some((spec.name, serde_json::from_value(spec.template()).ok()?)))
            .chain([
                (
                    "error",
                    Widget::from(LineSegment::Error {
                        message: String::new(),
                    }),
                ),
                (
                    "unknown",
                    Widget::from(LineSegment::Unknown {
                        name: String::new(),
                    }),
                ),
            ]);
        let mut by_module: Vec<(&'static str, Vec<(&'static str, String)>)> = Vec::new();
        for (name, widget) in widgets {
            let (Some(module), Some(padding)) =
                (theme_module(&widget.segment), default_padding(&widget))
            else {
                continue;
            };
            let padding = (name, padding.to_string());
            match by_module.iter_mut().find(|(m, _)| *m == module) {
                Some((_, widgets)) => widgets.push(padding),
                None => by_module.push((module, vec![padding])),
            }
        }
        by_module
            .into_iter()
            .map(|(module, widgets)| {
                let (_, first) = &widgets[0];
                let text = if widgets.iter().all(|(_, padding)| padding == first) {
                    first.clone()
                } else {
                    widgets
                        .iter()
                        .map(|(name, padding)| format!("{padding} for {name}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                (module, text)
            })
            .collect()
    });
    defaults
        .iter()
        .find(|(m, _)| *m == module)
        .map(|(_, padding)| padding.as_str())
}

/// A property the file sets that the editor has no description for.
fn inferred_prop(key: &str) -> PropSpec {
    PropSpec {
        key: key.to_string(),
        kind: inferred_kind(key),
        help: String::new(),
        fallback: String::new(),
    }
}

/// One line of the Theme page's module list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeEntry {
    Defaults,
    /// A module, by its canonical name (or its name in the file when the
    /// editor does not know it).
    Module(String),
}

impl ThemeEntry {
    pub fn label(&self) -> &str {
        match self {
            ThemeEntry::Defaults => "defaults",
            ThemeEntry::Module(name) => name,
        }
    }
}

pub struct ThemeDoc {
    root: Value,
    /// `None` for a theme that has not been written yet.
    saved: Option<Value>,
    undo: Vec<Value>,
    redo: Vec<Value>,
}

impl ThemeDoc {
    pub fn load(text: &str) -> Result<ThemeDoc, String> {
        let root: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
        validate_theme(&root)?;
        Ok(ThemeDoc {
            saved: Some(root.clone()),
            root,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn new_from_starter() -> ThemeDoc {
        let mut doc = ThemeDoc::load(STARTER_THEME).expect("the example theme is valid");
        doc.saved = None;
        doc
    }

    pub fn root(&self) -> &Value {
        &self.root
    }

    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.root)
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.root.clone());
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(previous) => {
                self.redo.push(std::mem::replace(&mut self.root, previous));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.root, next));
                true
            }
            None => false,
        }
    }

    fn modules(&self) -> Option<&Map<String, Value>> {
        self.root.get("modules").and_then(Value::as_object)
    }

    /// The key the module is stored under: its canonical name, or an older
    /// alias when only that is in the file.
    fn storage_key(&self, name: &str) -> String {
        let modules = self.modules();
        let present = |key: &str| modules.is_some_and(|m| m.contains_key(key));
        if present(name) {
            return name.to_string();
        }
        module_specs()
            .iter()
            .find(|spec| spec.name == name)
            .and_then(|spec| spec.aliases.iter().find(|alias| present(alias)))
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn entries(&self) -> Vec<ThemeEntry> {
        let mut entries = vec![ThemeEntry::Defaults];
        entries.extend(
            module_specs()
                .iter()
                .map(|spec| ThemeEntry::Module(spec.name.clone())),
        );
        // Modules in the file that the editor has no description for.
        if let Some(modules) = self.modules() {
            for name in modules.keys() {
                let known = module_specs()
                    .iter()
                    .any(|spec| spec.name == *name || spec.aliases.contains(name));
                if !known {
                    entries.push(ThemeEntry::Module(name.clone()));
                }
            }
        }
        entries
    }

    fn section(&self, entry: &ThemeEntry) -> Option<&Map<String, Value>> {
        match entry {
            ThemeEntry::Defaults => self.root.get("defaults").and_then(Value::as_object),
            ThemeEntry::Module(name) => self
                .modules()
                .and_then(|m| m.get(&self.storage_key(name)))
                .and_then(Value::as_object),
        }
    }

    /// The properties shown for an entry with their values in the file,
    /// followed by any the file sets that the editor has no description for.
    pub fn props(&self, entry: &ThemeEntry) -> Vec<(PropSpec, Option<Value>)> {
        let mut specs = match entry {
            ThemeEntry::Defaults => defaults_props(),
            ThemeEntry::Module(name) => module_specs()
                .iter()
                .find(|spec| spec.name == *name)
                .map(|spec| spec.props.clone())
                .unwrap_or_default(),
        };
        // A padding falls back to what the module's widgets declare.
        if let ThemeEntry::Module(name) = entry {
            let padding = specs.iter_mut().find(|spec| spec.key == "padding");
            if let (Some(spec), Some(default)) = (padding, module_default_padding(name)) {
                spec.fallback = default.to_string();
            }
        }
        let section = self.section(entry);
        if let Some(section) = section {
            for key in section.keys() {
                if !specs.iter().any(|spec| spec.key == *key) {
                    specs.push(inferred_prop(key));
                }
            }
        }
        specs
            .into_iter()
            .map(|spec| {
                let value = section.and_then(|s| s.get(&spec.key)).cloned();
                (spec, value)
            })
            .collect()
    }

    /// Sets (or with `None`, removes) a property. Rejected if superline would
    /// refuse to load the result.
    pub fn set(
        &mut self,
        entry: &ThemeEntry,
        key: &str,
        value: Option<Value>,
    ) -> Result<(), String> {
        let mut next = self.with(entry, key, value)?;
        validate_theme(&next)?;
        std::mem::swap(&mut next, &mut self.root);
        if next != self.root {
            self.undo.push(next);
            self.redo.clear();
        }
        Ok(())
    }

    /// A copy of the theme with one property changed, for previewing a colour
    /// before it is picked.
    pub fn with(
        &self,
        entry: &ThemeEntry,
        key: &str,
        value: Option<Value>,
    ) -> Result<Value, String> {
        let mut next = self.root.clone();
        let root = next.as_object_mut().ok_or("the theme is not an object")?;
        let (section_key, holder) = match entry {
            ThemeEntry::Defaults => {
                if value.is_none() {
                    return Err(format!("defaults.{key} is required"));
                }
                ("defaults".to_string(), root)
            }
            ThemeEntry::Module(name) => {
                let stored = self.storage_key(name);
                let modules = root
                    .entry("modules")
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .ok_or("modules is not an object")?;
                (stored, modules)
            }
        };
        let section = holder
            .entry(section_key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        let map = section
            .as_object_mut()
            .ok_or("theme section is not an object")?;
        match value {
            Some(value) => {
                map.insert(key.to_string(), value);
            }
            None => {
                map.shift_remove(key);
            }
        }
        if map.is_empty() && !matches!(entry, ThemeEntry::Defaults) {
            holder.shift_remove(&section_key);
        }
        Ok(next)
    }

    /// The colour code a property resolves to, following `defaults.*`
    /// fallbacks.
    pub fn resolve(
        &self,
        entry: &ThemeEntry,
        spec: &PropSpec,
        value: Option<&Value>,
    ) -> Option<u8> {
        if matches!(spec.kind, PropKind::Choice(_)) {
            return None;
        }
        if let Some(value) = value {
            return color_code(value).or_else(|| value.as_array()?.first().and_then(color_code));
        }
        let fallback = spec.fallback.trim_matches(['[', ']']);
        let default = fallback
            .strip_prefix("defaults.")
            .or(match (entry, spec.kind) {
                (_, PropKind::Str) => None,
                _ if spec.key.ends_with("bg") || spec.key.ends_with("colors") => Some("bg"),
                _ => Some("fg"),
            })?;
        self.default_color(default)
    }

    pub fn default_color(&self, key: &str) -> Option<u8> {
        self.root.get("defaults")?.get(key).and_then(color_code)
    }

    /// Colours to draw a module's name in: its first foreground and
    /// background properties, as the prompt would.
    pub fn swatch(&self, entry: &ThemeEntry) -> (Option<u8>, Option<u8>) {
        let props = self.props(entry);
        let pick = |suffix: &str| {
            props
                .iter()
                .filter(|(spec, _)| matches!(spec.kind, PropKind::Color | PropKind::ColorList))
                .find(|(spec, _)| {
                    spec.key == suffix
                        || spec.key.ends_with(&format!("_{suffix}"))
                        || (suffix == "bg" && spec.key.ends_with("colors"))
                })
                .and_then(|(spec, value)| self.resolve(entry, spec, value.as_ref()))
                .or_else(|| self.default_color(suffix))
        };
        (pick("fg"), pick("bg"))
    }
}

/// Parses a colour typed as a name or a 0-255 code.
pub fn parse_color(text: &str) -> Result<Value, String> {
    let text = text.trim();
    if let Ok(code) = text.parse::<u8>() {
        return Ok(Value::from(code));
    }
    if crate::colors::Color::from_name(text).is_some() {
        return Ok(Value::from(text));
    }
    Err(format!(
        "{text:?} is not a colour name or a code from 0 to 255"
    ))
}

/// Parses a comma- or space-separated list of colours.
pub fn parse_color_list(text: &str) -> Result<Value, String> {
    let colors = text
        .split([',', ' '])
        .filter(|part| !part.trim().is_empty())
        .map(parse_color)
        .collect::<Result<Vec<_>, _>>()?;
    if colors.is_empty() {
        return Err("give at least one colour".into());
    }
    Ok(Value::Array(colors))
}

/// Parses a typed choice, such as a padding.
pub fn parse_choice(text: &str, variants: &[&str]) -> Result<Value, String> {
    let text = text.trim();
    if variants.contains(&text) {
        Ok(Value::from(text))
    } else {
        Err(format!("{text:?} is not one of {}", variants.join(", ")))
    }
}

/// The choice after (or before) a choice property's value. An unset property
/// steps from its fallback. `None` when the property is not a choice.
pub fn step_choice(spec: &PropSpec, value: Option<&Value>, forward: bool) -> Option<&'static str> {
    let PropKind::Choice(variants) = spec.kind else {
        return None;
    };
    let current = match value {
        Some(value) => variants.iter().position(|v| value.as_str() == Some(v)),
        None => variants.iter().position(|v| spec.fallback.starts_with(v)),
    };
    let count = variants.len();
    let next = match current {
        Some(i) if forward => (i + 1) % count,
        Some(i) => (i + count - 1) % count,
        None if forward => 0,
        None => count - 1,
    };
    variants.get(next).copied()
}

/// How a colour value is written back: as a name when the value it replaces
/// was a name and this code has one, otherwise as a number.
pub fn color_value(code: u8, prefer_name: bool) -> Value {
    if prefer_name {
        if let Some((name, _)) = NAMED_COLORS.iter().find(|(_, color)| color.to_u8() == code) {
            return Value::from(*name);
        }
    }
    Value::from(code)
}

pub fn color_names(code: u8) -> Vec<&'static str> {
    NAMED_COLORS
        .iter()
        .filter(|(_, color)| color.to_u8() == code)
        .map(|(name, _)| *name)
        .collect()
}

/// The text a property value is edited as.
pub fn edit_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> ThemeDoc {
        ThemeDoc::load(
            r#"{
                "defaults": { "fg": 223, "bg": "black" },
                "modules": {
                    "py": { "env_bg": 22 },
                    "future_module": { "fg": 1, "glow": "yes" }
                }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn every_documented_module_is_listed() {
        let specs = module_specs();
        assert!(specs.iter().any(|s| s.name == "git" && s.props.len() > 10));
        assert!(specs.iter().all(|s| !s.props.is_empty()));
        let python = specs.iter().find(|s| s.name == "python").unwrap();
        assert_eq!(python.aliases, ["py"]);
    }

    #[test]
    fn documented_property_kinds_match_what_the_loader_infers() {
        for module in module_specs() {
            for prop in &module.props {
                let inferred = inferred_prop(&prop.key).kind;
                assert_eq!(inferred, prop.kind, "{}.{}", module.name, prop.key);
            }
        }
    }

    #[test]
    fn the_starter_theme_is_valid() {
        let doc = ThemeDoc::new_from_starter();
        assert!(doc.is_dirty());
    }

    #[test]
    fn unknown_modules_and_properties_are_listed_after_known_ones() {
        let doc = doc();
        let entries = doc.entries();
        assert_eq!(entries[0], ThemeEntry::Defaults);
        assert_eq!(
            entries.last(),
            Some(&ThemeEntry::Module("future_module".into()))
        );
        let props = doc.props(&ThemeEntry::Module("future_module".into()));
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].0.kind, PropKind::Color);
    }

    #[test]
    fn edits_go_to_the_alias_already_in_the_file() {
        let mut doc = doc();
        let python = ThemeEntry::Module("python".into());
        doc.set(&python, "env_fg", Some(json!(15))).unwrap();
        assert_eq!(
            doc.root()["modules"]["py"],
            json!({ "env_bg": 22, "env_fg": 15 })
        );
        assert!(doc.root()["modules"].get("python").is_none());
    }

    #[test]
    fn new_modules_are_added_and_emptied_ones_removed() {
        let mut doc = doc();
        let git = ThemeEntry::Module("git".into());
        doc.set(&git, "clean_bg", Some(json!("blue"))).unwrap();
        assert_eq!(doc.root()["modules"]["git"], json!({ "clean_bg": "blue" }));
        doc.set(&git, "clean_bg", None).unwrap();
        assert!(doc.root()["modules"].get("git").is_none());
        assert!(doc.undo());
        assert!(doc.root()["modules"].get("git").is_some());
    }

    #[test]
    fn invalid_values_and_missing_defaults_are_rejected() {
        let mut doc = doc();
        let before = doc.root().clone();
        let git = ThemeEntry::Module("git".into());
        assert!(doc
            .set(&git, "clean_bg", Some(json!("not_a_colour")))
            .is_err());
        assert!(doc.set(&ThemeEntry::Defaults, "fg", None).is_err());
        assert_eq!(doc.root(), &before);
        assert!(!doc.is_dirty());
    }

    #[test]
    fn documented_choices_list_the_variants_the_loader_accepts() {
        let options: Value = serde_json::from_str(THEME_OPTIONS).unwrap();
        let mut choices = 0;
        for (module, spec) in options.as_object().unwrap() {
            for row in spec["properties"].as_array().into_iter().flatten() {
                let (key, kind) = (row[0].as_str().unwrap(), row[1].as_str().unwrap());
                let Some(listed) = kind.strip_prefix("choice: ") else {
                    continue;
                };
                let listed: Vec<&str> = listed.split(", ").collect();
                let PropKind::Choice(variants) = inferred_kind(key) else {
                    panic!("{module}.{key} is documented as a choice");
                };
                assert_eq!(variants, listed, "{module}.{key}");
                choices += 1;
            }
        }
        assert!(choices > 20, "every module documents its padding");
    }

    #[test]
    fn documented_padding_defaults_are_the_ones_the_widgets_declare() {
        let options: Value = serde_json::from_str(THEME_OPTIONS).unwrap();
        for (module, spec) in options.as_object().unwrap() {
            let rows = spec["properties"].as_array().into_iter().flatten();
            for row in rows.filter(|row| row[0] == "padding") {
                assert_eq!(
                    row[3].as_str(),
                    module_default_padding(module),
                    "modules.{module}.padding in theme-options.json"
                );
            }
        }
    }

    #[test]
    fn every_widget_declares_a_padding_and_documents_it() {
        let documented: Vec<&str> = module_specs()
            .iter()
            .filter(|spec| spec.props.iter().any(|p| p.key == "padding"))
            .map(|spec| spec.name.as_str())
            .collect();
        for spec in WIDGETS {
            let Ok(widget) = serde_json::from_value::<Widget>(spec.template()) else {
                assert!(!spec.listed, "{} template parses", spec.name);
                continue;
            };
            let layout = matches!(spec.name, "separator" | "padding");
            assert_eq!(
                default_padding(&widget).is_none(),
                layout,
                "{} declares a padding",
                spec.name
            );
            if let Some(module) = theme_module(&widget.segment).filter(|_| !layout) {
                assert!(
                    documented.contains(&module),
                    "{} documents padding",
                    spec.name
                );
            }
        }
        for (module, padding) in [
            ("cwd", "left"),
            ("last_cmd_duration", "left"),
            ("cmd", "small"),
            ("shell", "small"),
            ("git", "large"),
            ("readonly", "large"),
            ("text", "large"),
            ("python", "large; venv label right"),
            ("spacer", "small for small_spacer, large for large_spacer"),
            ("error", "large"),
            ("unknown", "large"),
        ] {
            assert_eq!(module_default_padding(module), Some(padding), "{module}");
        }
        assert_eq!(module_default_padding("update"), None);
    }

    #[test]
    fn padding_is_one_of_four_choices() {
        let mut doc = doc();
        let git = ThemeEntry::Module("git".into());
        doc.set(&git, "padding", Some(json!("left"))).unwrap();
        assert_eq!(doc.root()["modules"]["git"], json!({ "padding": "left" }));
        let props = doc.props(&git);
        let (spec, value) = props.iter().find(|(s, _)| s.key == "padding").unwrap();
        assert_eq!(
            spec.kind,
            PropKind::Choice(&["small", "large", "left", "right"])
        );
        assert_eq!(spec.fallback, "large");
        assert_eq!(doc.resolve(&git, spec, value.as_ref()), None);
        // The legacy `py` key still shows python's own default.
        let python = doc.props(&ThemeEntry::Module("python".into()));
        let (spec, _) = python.iter().find(|(s, _)| s.key == "padding").unwrap();
        assert_eq!(spec.fallback, "large; venv label right");
        for bad in [json!(1), json!("wide")] {
            assert!(doc.set(&git, "padding", Some(bad)).is_err());
        }
        assert_eq!(doc.root()["modules"]["git"], json!({ "padding": "left" }));
    }

    #[test]
    fn choices_step_from_the_value_or_the_fallback() {
        let spec = |fallback: &str| PropSpec {
            key: "padding".into(),
            kind: PropKind::Choice(&["small", "large", "left", "right"]),
            help: String::new(),
            fallback: fallback.into(),
        };
        let step = |fallback, value: Option<Value>, forward| {
            step_choice(&spec(fallback), value.as_ref(), forward)
        };
        assert_eq!(step("large", Some(json!("small")), true), Some("large"));
        assert_eq!(step("large", Some(json!("right")), true), Some("small"));
        assert_eq!(step("large", Some(json!("small")), false), Some("right"));
        assert_eq!(step("large", None, true), Some("left"));
        assert_eq!(step("left", None, false), Some("large"));
        assert_eq!(
            step("large; right for the venv label", None, true),
            Some("left")
        );
        assert_eq!(step("", None, true), Some("small"));
        assert_eq!(step("", None, false), Some("right"));
        assert_eq!(step("", Some(json!(2)), true), Some("small"));
        let colour = PropSpec {
            kind: PropKind::Color,
            ..spec("")
        };
        assert_eq!(step_choice(&colour, None, true), None);
    }

    #[test]
    fn unset_properties_resolve_through_the_defaults() {
        let doc = doc();
        let git = ThemeEntry::Module("git".into());
        let props = doc.props(&git);
        let (spec, value) = props.iter().find(|(s, _)| s.key == "clean_bg").unwrap();
        assert_eq!(doc.resolve(&git, spec, value.as_ref()), Some(0));
        let (spec, value) = props.iter().find(|(s, _)| s.key == "clean_fg").unwrap();
        assert_eq!(doc.resolve(&git, spec, value.as_ref()), Some(223));
    }

    #[test]
    fn colours_parse_from_names_codes_and_lists() {
        assert_eq!(parse_color("31").unwrap(), json!(31));
        assert_eq!(parse_color(" blue ").unwrap(), json!("blue"));
        assert!(parse_color("256").is_err());
        assert!(parse_color("bleu").is_err());
        assert_eq!(
            parse_color_list("red, 166 72").unwrap(),
            json!(["red", 166, 72])
        );
        assert!(parse_color_list(" , ").is_err());
        let paddings = &["small", "large", "left", "right"];
        assert_eq!(parse_choice(" left ", paddings).unwrap(), json!("left"));
        assert!(parse_choice("wide", paddings).is_err());
        assert!(parse_choice("1", paddings).is_err());
        assert_eq!(color_value(4, true), json!("blue"));
        assert_eq!(color_value(4, false), json!(4));
        assert_eq!(color_value(99, true), json!(99));
    }
}
