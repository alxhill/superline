//! Writes a config or theme back out the way people tend to write one by
//! hand: short objects such as a segment's options kept on one line, and a
//! config's segment lists one element per line.

use serde_json::Value;

const MAX_WIDTH: usize = 100;
const INDENT: usize = 2;

/// Every array element on its own line, as a config's rows and segments are.
pub fn to_pretty(value: &Value) -> String {
    format(value, false)
}

/// Like [`to_pretty`], but short arrays of plain values (a theme's colour
/// lists) stay on one line.
pub fn to_pretty_theme(value: &Value) -> String {
    format(value, true)
}

fn format(value: &Value, inline_arrays: bool) -> String {
    let mut out = String::new();
    write_value(value, 0, 0, inline_arrays, &mut out);
    out.push('\n');
    out
}

/// `prefix` is the width already used on the current line (indent plus key).
fn write_value(value: &Value, indent: usize, prefix: usize, inline_arrays: bool, out: &mut String) {
    if indent > 0 {
        if let Some(inline) = inline(value, inline_arrays) {
            if prefix + inline.len() < MAX_WIDTH {
                out.push_str(&inline);
                return;
            }
        }
    }
    match value {
        Value::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                push_indent(indent + INDENT, out);
                write_value(item, indent + INDENT, indent + INDENT, inline_arrays, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            push_indent(indent, out);
            out.push(']');
        }
        Value::Object(map) if !map.is_empty() => {
            out.push_str("{\n");
            for (i, (key, item)) in map.iter().enumerate() {
                push_indent(indent + INDENT, out);
                let key = format!("{}: ", Value::String(key.clone()));
                out.push_str(&key);
                write_value(
                    item,
                    indent + INDENT,
                    indent + INDENT + key.len(),
                    inline_arrays,
                    out,
                );
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            push_indent(indent, out);
            out.push('}');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// The one-line form of a value. Non-empty arrays only have one when
/// `inline_arrays` is set and they hold plain values.
fn inline(value: &Value, inline_arrays: bool) -> Option<String> {
    match value {
        Value::Array(items) if items.is_empty() => Some("[]".into()),
        Value::Array(items)
            if inline_arrays
                && items
                    .iter()
                    .all(|item| !item.is_array() && !item.is_object()) =>
        {
            let items: Vec<String> = items.iter().map(Value::to_string).collect();
            Some(format!("[{}]", items.join(", ")))
        }
        Value::Array(_) => None,
        Value::Object(map) if map.is_empty() => Some("{}".into()),
        Value::Object(map) => {
            let fields = map
                .iter()
                .map(|(key, item)| {
                    Some(format!(
                        "{}: {}",
                        Value::String(key.clone()),
                        inline(item, inline_arrays)?
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(format!("{{ {} }}", fields.join(", ")))
        }
        scalar => Some(scalar.to_string()),
    }
}

fn push_indent(width: usize, out: &mut String) {
    out.extend(std::iter::repeat_n(' ', width));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn segments_sit_one_per_line_with_short_options_inline() {
        let config = json!({
            "theme": "rainbow",
            "rows": [{
                "left": ["git", { "cwd": { "max_length": 60, "wanted_seg_num": 5 } }],
                "right": []
            }]
        });
        let expected = r#"{
  "theme": "rainbow",
  "rows": [
    {
      "left": [
        "git",
        { "cwd": { "max_length": 60, "wanted_seg_num": 5 } }
      ],
      "right": []
    }
  ]
}
"#;
        assert_eq!(to_pretty(&config), expected);
    }

    #[test]
    fn long_objects_break_across_lines() {
        let long = "x".repeat(120);
        let config = json!({ "rows": [{ "left": [{ "text": long }] }] });
        let text = to_pretty(&config);
        assert!(text.contains("{\n          \"text\": \""), "{text}");
    }

    #[test]
    fn theme_colour_lists_stay_on_one_line() {
        let theme = json!({ "modules": { "cwd": { "bg_colors": [166, "red"] } } });
        let text = to_pretty_theme(&theme);
        assert!(
            text.contains(r#""cwd": { "bg_colors": [166, "red"] }"#),
            "{text}"
        );
    }

    #[test]
    fn output_round_trips() {
        let config = serde_json::to_value(crate::config::Config::default()).unwrap();
        let text = to_pretty(&config);
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, config);
    }
}
