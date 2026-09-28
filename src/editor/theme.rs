//! The theme file being edited on the editor's Theme page, and what the editor
//! knows about each module's theme properties.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::colors::{TextAttrs, NAMED_COLORS};
use crate::themes::{color_code, infer_theme_property_kind, validate_theme, ThemePropertyKind};
use crate::themes::{text_attribute_color, text_attribute_key, TEXT_ATTRIBUTES};

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
    Bool,
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
        let attributes = &options["_text_attributes"];
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
                            _ => PropKind::Str,
                        },
                        help: row[2].as_str().unwrap_or_default().to_string(),
                        fallback: row[3].as_str().unwrap_or_default().to_string(),
                    })
                    .flat_map(|spec| {
                        let switches = text_attribute_specs(&spec, attributes);
                        std::iter::once(spec).chain(switches)
                    })
                    .collect(),
            })
            .collect()
    })
}

/// The bold, italic and underline switches listed after a text colour,
/// described by `_text_attributes` in theme-options.json.
fn text_attribute_specs(color: &PropSpec, descriptions: &Value) -> Vec<PropSpec> {
    if color.kind != PropKind::Color {
        return Vec::new();
    }
    TEXT_ATTRIBUTES
        .iter()
        .filter_map(|attr| {
            Some(PropSpec {
                key: text_attribute_key(&color.key, attr)?,
                kind: PropKind::Bool,
                help: descriptions[attr]
                    .as_str()
                    .unwrap_or_default()
                    .replace("{fg}", &color.key),
                fallback: "false".into(),
            })
        })
        .collect()
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

/// A property the file sets that the editor has no description for.
fn inferred_prop(key: &str) -> PropSpec {
    PropSpec {
        key: key.to_string(),
        kind: match infer_theme_property_kind(key) {
            Some(ThemePropertyKind::ColorList) => PropKind::ColorList,
            Some(ThemePropertyKind::Color) => PropKind::Color,
            Some(ThemePropertyKind::Bool) => PropKind::Bool,
            _ => PropKind::Str,
        },
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
        ThemeDoc::load(STARTER_THEME)
            .expect("the example theme is valid")
            .fork()
    }

    /// An unsaved copy, for a new theme file.
    pub fn fork(&self) -> ThemeDoc {
        ThemeDoc {
            root: self.root.clone(),
            saved: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
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
        if let Some(value) = value {
            return color_code(value).or_else(|| value.as_array()?.first().and_then(color_code));
        }
        let fallback = spec.fallback.trim_matches(['[', ']']);
        let default = fallback
            .strip_prefix("defaults.")
            .or(match (entry, spec.kind) {
                (_, PropKind::Str | PropKind::Bool) => None,
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
                .filter(|(spec, _)| spec.kind != PropKind::Str)
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

    /// How the prompt draws the text a bold, italic or underline switch
    /// applies to: its colour, its background, and every switch set for it.
    /// The background is the colour's `_bg` partner, or else the module's
    /// first background.
    pub fn text_sample(&self, entry: &ThemeEntry, switch: &str) -> Option<TextSample> {
        let fg_key = text_attribute_color(switch)?;
        let props = self.props(entry);
        let find = |key: &str| props.iter().find(|(spec, _)| spec.key == key);
        let (fg_spec, fg_value) = find(&fg_key).filter(|(spec, _)| spec.kind == PropKind::Color)?;
        let bg = fg_key
            .strip_suffix("fg")
            .and_then(|prefix| find(&format!("{prefix}bg")))
            .and_then(|(spec, value)| self.resolve(entry, spec, value.as_ref()))
            .or_else(|| self.swatch(entry).1);
        let [bold, italic, underline] = TEXT_ATTRIBUTES.map(|attr| {
            text_attribute_key(&fg_key, attr)
                .and_then(|key| find(&key)?.1.as_ref()?.as_bool())
                .unwrap_or(false)
        });
        Some(TextSample {
            fg: self.resolve(entry, fg_spec, fg_value.as_ref()),
            bg,
            attrs: TextAttrs {
                bold,
                italic,
                underline,
            },
        })
    }
}

/// See [`ThemeDoc::text_sample`].
#[derive(Debug, PartialEq)]
pub struct TextSample {
    pub fg: Option<u8>,
    pub bg: Option<u8>,
    pub attrs: TextAttrs,
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

/// Parses a typed `true` or `false`.
pub fn parse_bool(text: &str) -> Result<Value, String> {
    match text.trim() {
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        other => Err(format!("{other:?} is not true or false")),
    }
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
    fn every_text_colour_is_followed_by_its_attribute_switches() {
        let git = module_specs().iter().find(|s| s.name == "git").unwrap();
        let keys: Vec<&str> = git.props.iter().map(|p| p.key.as_str()).collect();
        let at = keys.iter().position(|k| *k == "clean_fg").unwrap();
        assert_eq!(
            keys[at..at + 5],
            [
                "clean_fg",
                "clean_bold",
                "clean_italic",
                "clean_underline",
                "clean_bg"
            ]
        );
        let bold = &git.props[at + 1];
        assert_eq!(bold.kind, PropKind::Bool);
        assert!(bold.help.contains("clean_fg"), "{}", bold.help);

        let readonly = module_specs()
            .iter()
            .find(|s| s.name == "readonly")
            .unwrap();
        let keys: Vec<&str> = readonly.props.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys[..5], ["fg", "bold", "italic", "underline", "bg"]);
    }

    #[test]
    fn the_documented_attributes_are_the_ones_the_loader_reads() {
        let options: Value = serde_json::from_str(THEME_OPTIONS).unwrap();
        let documented: Vec<&str> = options["_text_attributes"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .filter(|key| !key.starts_with('_'))
            .collect();
        assert_eq!(documented, TEXT_ATTRIBUTES);
    }

    #[test]
    fn attribute_switches_only_take_booleans() {
        let mut doc = doc();
        let git = ThemeEntry::Module("git".into());
        doc.set(&git, "clean_bold", Some(json!(true))).unwrap();
        assert!(doc.set(&git, "clean_italic", Some(json!("yes"))).is_err());
        assert_eq!(doc.root()["modules"]["git"], json!({ "clean_bold": true }));
        assert_eq!(parse_bool(" false ").unwrap(), json!(false));
        assert!(parse_bool("yes").is_err());
    }

    #[test]
    fn a_switch_samples_its_text_colour_on_its_background() {
        let doc = ThemeDoc::load(
            r#"{
                "defaults": { "fg": 15, "bg": 0 },
                "modules": {
                    "git": {
                        "notstaged_fg": 229, "notstaged_bg": 166,
                        "notstaged_bold": true, "notstaged_italic": true, "clean_underline": true
                    },
                    "pr": { "draft_bg": 239, "status_success_fg": 142, "status_success_underline": true },
                    "cwd": { "bg_colors": [166, 172] },
                    "readonly": { "fg": 229, "bg": 124 }
                }
            }"#,
        )
        .unwrap();
        let sample = |module: &str, switch: &str| {
            doc.text_sample(&ThemeEntry::Module(module.into()), switch)
        };
        let attrs = |bold, italic, underline| TextAttrs {
            bold,
            italic,
            underline,
        };

        // Every switch of a colour samples all of them together.
        let notstaged = TextSample {
            fg: Some(229),
            bg: Some(166),
            attrs: attrs(true, true, false),
        };
        assert_eq!(sample("git", "notstaged_bold").as_ref(), Some(&notstaged));
        assert_eq!(sample("git", "notstaged_underline"), Some(notstaged));
        assert_eq!(
            sample("readonly", "italic"),
            Some(TextSample {
                fg: Some(229),
                bg: Some(124),
                attrs: TextAttrs::NONE,
            })
        );
        // A colour with no `_bg` partner sits on the module's first background.
        assert_eq!(
            sample("pr", "status_success_bold"),
            Some(TextSample {
                fg: Some(142),
                bg: Some(239),
                attrs: attrs(false, false, true),
            })
        );
        assert_eq!(sample("cwd", "path_bold").unwrap().bg, Some(166));
        // Unset colours fall back to the defaults.
        assert_eq!(
            sample("git", "untracked_bold"),
            Some(TextSample {
                fg: Some(15),
                bg: Some(0),
                attrs: TextAttrs::NONE,
            })
        );

        assert_eq!(sample("git", "notstaged_fg"), None);
        assert_eq!(sample("git", "branch_icon"), None);
    }

    #[test]
    fn the_starter_theme_is_valid() {
        let doc = ThemeDoc::new_from_starter();
        assert!(doc.is_dirty());
    }

    #[test]
    fn bundled_themes_only_set_documented_properties() {
        for (file, text) in crate::themes::BUNDLED_THEMES {
            let doc = ThemeDoc::load(text).unwrap();
            for entry in doc.entries() {
                // Only the library's ExitCode module reads this; no widget shows it.
                if entry == ThemeEntry::Module("exit_code".into()) {
                    continue;
                }
                for (spec, value) in doc.props(&entry) {
                    assert!(
                        value.is_none() || !spec.help.is_empty(),
                        "themes/{file} sets {}.{}, which theme-options.json does not describe",
                        entry.label(),
                        spec.key
                    );
                }
            }
        }
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
        assert_eq!(color_value(4, true), json!("blue"));
        assert_eq!(color_value(4, false), json!(4));
        assert_eq!(color_value(99, true), json!(99));
    }
}
