//! Log-line parsing — "JSON → columns" (M1.2).
//!
//! Multi-pod/multi-container log streaming with a regex filter lands the raw line; this
//! module turns **structured** (JSON) log lines into typed columns so a frontend can show
//! a real table instead of a monochrome string. It is renderer-agnostic: the view-model
//! owns *meaning* (which columns exist and their typed values); the frontend owns
//! *geometry* (column widths, truncation).
//!
//! The schema is **inferred** from the first JSON line's keys (stable, first-seen order).
//! Non-JSON lines are returned as a single `_raw` column. This is deliberately cheap —
//! the full OpenAPI/CRD schema validation lands in Phase 2 with the lens engine.

use serde_json::Value;
use std::collections::BTreeMap;

/// A single typed log cell.
#[derive(Debug, Clone, PartialEq)]
pub enum LogCell {
    Text(String),
    Number(i64),
    Float(f64),
    Bool(bool),
    Null,
}

/// One parsed log row: the raw line plus a typed column map (empty for non-JSON lines).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLogLine {
    /// The original (unmodified) line.
    pub raw: String,
    /// Typed columns for JSON lines; empty for plain-text lines.
    pub columns: BTreeMap<String, LogCell>,
}

/// Parse a JSON log line into typed columns. Returns `None` for non-JSON lines (the
/// caller keeps the raw line for a `_raw` column).
pub fn parse_json_line(line: &str) -> Option<BTreeMap<String, LogCell>> {
    let value: Value = serde_json::from_str(line).ok()?;
    let obj = value.as_object()?;
    let mut columns = BTreeMap::new();
    for (key, val) in obj {
        columns.insert(key.clone(), json_to_cell(val));
    }
    Some(columns)
}

/// The inferred column schema for a set of parsed lines (first-seen key order is stable
/// because `BTreeMap` sorts keys).
pub fn infer_columns(parsed: &[ParsedLogLine]) -> Vec<String> {
    let mut keys = BTreeMap::new();
    for line in parsed {
        for key in line.columns.keys() {
            keys.entry(key.clone()).or_insert(());
        }
    }
    keys.into_keys().collect()
}

fn json_to_cell(value: &Value) -> LogCell {
    match value {
        Value::String(s) => LogCell::Text(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                LogCell::Number(i)
            } else {
                LogCell::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::Bool(b) => LogCell::Bool(*b),
        Value::Null => LogCell::Null,
        // Nested objects/arrays collapse to their JSON representation as a text cell.
        other => LogCell::Text(other.to_string()),
    }
}

/// Parse a stream of log lines: JSON lines become typed columns, plain lines keep only
/// the raw text.
pub fn parse_log_stream(lines: impl IntoIterator<Item = String>) -> Vec<ParsedLogLine> {
    lines
        .into_iter()
        .map(|raw| {
            let columns = parse_json_line(&raw).unwrap_or_default();
            ParsedLogLine { raw, columns }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_line_parses_typed_columns() {
        let parsed =
            parse_json_line(r#"{"level":"info","ts":123,"ratio":0.5,"ok":true,"meta":null}"#)
                .unwrap();
        assert_eq!(parsed["level"], LogCell::Text("info".into()));
        assert_eq!(parsed["ts"], LogCell::Number(123));
        assert_eq!(parsed["ratio"], LogCell::Float(0.5));
        assert_eq!(parsed["ok"], LogCell::Bool(true));
        assert_eq!(parsed["meta"], LogCell::Null);
    }

    #[test]
    fn non_json_line_returns_none() {
        assert!(parse_json_line("plain text, not json").is_none());
        assert!(parse_json_line("{not json}").is_none());
    }

    #[test]
    fn infer_columns_union_across_lines() {
        let lines = parse_log_stream(vec![
            r#"{"a":1,"b":"x"}"#.to_string(),
            r#"{"b":"y","c":true}"#.to_string(),
            "plain line".to_string(),
        ]);
        let cols = infer_columns(&lines);
        assert_eq!(cols, vec!["a", "b", "c"]);
    }

    #[test]
    fn nested_json_collapses_to_text() {
        let parsed = parse_json_line(r#"{"obj":{"x":1}}"#).unwrap();
        assert!(matches!(parsed["obj"], LogCell::Text(_)));
    }
}
