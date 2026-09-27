//! Writes a config back out the way people tend to write one by hand: every
//! array element on its own line, and short objects such as a segment's
//! options kept on one line.

use serde_json::Value;

const MAX_WIDTH: usize = 100;
const INDENT: usize = 2;

pub fn to_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, 0, 0, &mut out);
    out.push('\n');
    out
}

/// `prefix` is the width already used on the current line (indent plus key).
fn write_value(value: &Value, indent: usize, prefix: usize, out: &mut String) {
    if indent > 0 {
        if let Some(inline) = inline(value) {
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
                write_value(item, indent + INDENT, indent + INDENT, out);
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
                write_value(item, indent + INDENT, indent + INDENT + key.len(), out);
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            push_indent(indent, out);
            out.push('}');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// The one-line form of a value, unless it contains a non-empty array.
fn inline(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) if items.is_empty() => Some("[]".into()),
        Value::Array(_) => None,
        Value::Object(map) if map.is_empty() => Some("{}".into()),
        Value::Object(map) => {
            let fields = map
                .iter()
                .map(|(key, item)| {
                    Some(format!("{}: {}", Value::String(key.clone()), inline(item)?))
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
    fn output_round_trips() {
        let config = serde_json::to_value(crate::config::Config::default()).unwrap();
        let text = to_pretty(&config);
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, config);
    }
}
