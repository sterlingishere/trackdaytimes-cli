//! Turning JSON rows into what the user asked for: a table (the default), CSV, or JSON lines.
//!
//! A command describes its columns once ([`Col`]); a table shows lap times as `1:22.207`, CSV and JSON keep the
//! integer milliseconds the API sends.

use std::io::Write;

use anyhow::Result;
use serde_json::{Map, Value};

/// How a command prints its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// A table for reading (lap times as 1:22.207).
    Table,
    /// Comma-separated, one row per line, with a header (lap times in milliseconds).
    Csv,
    /// One JSON object per line (lap times in milliseconds).
    Jsonl,
    /// The API's whole answer, as it came.
    Json,
}

/// What a column holds, for formatting.
#[derive(Clone, Copy)]
pub enum Kind {
    Text,
    /// An identifier or link people copy: shown in full, never cut short.
    Id,
    /// A lap time in milliseconds.
    Lap,
    /// A gap in milliseconds (+0.512 in a table; 0 shows as a dash).
    Gap,
}

/// One column: its table header, its CSV/JSON name, and where its value is.
pub struct Col {
    pub header: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub get: Box<dyn Fn(&Value) -> Value>,
}

/// A column read from a JSON pointer (`/person/displayName`).
pub fn col(header: &'static str, name: &'static str, pointer: &'static str) -> Col {
    Col {
        header,
        name,
        kind: Kind::Text,
        get: Box::new(move |row| row.pointer(pointer).cloned().unwrap_or(Value::Null)),
    }
}

/// An identifier column (a slug, handle or link) read from a JSON pointer: never cut short in a table.
pub fn ident(header: &'static str, name: &'static str, pointer: &'static str) -> Col {
    Col {
        kind: Kind::Id,
        ..col(header, name, pointer)
    }
}

/// A lap-time column read from a JSON pointer.
pub fn lap(header: &'static str, name: &'static str, pointer: &'static str) -> Col {
    Col {
        kind: Kind::Lap,
        ..col(header, name, pointer)
    }
}

/// A gap column read from a JSON pointer.
pub fn gap(header: &'static str, name: &'static str, pointer: &'static str) -> Col {
    Col {
        kind: Kind::Gap,
        ..col(header, name, pointer)
    }
}

/// A column worked out from the row.
pub fn computed(
    header: &'static str,
    name: &'static str,
    get: impl Fn(&Value) -> Value + 'static,
) -> Col {
    Col {
        header,
        name,
        kind: Kind::Text,
        get: Box::new(get),
    }
}

/// `make model` of a car object at `pointer`.
pub fn car_name(car: &Value) -> Value {
    let make = car.get("make").and_then(Value::as_str).unwrap_or("");
    let model = car.get("model").and_then(Value::as_str).unwrap_or("");
    let name = format!("{make} {model}").trim().to_string();
    if name.is_empty() {
        Value::Null
    } else {
        Value::String(name)
    }
}

/// Every driver of a row (`drivers`, else `person`), names joined with " / ".
pub fn driver_names(row: &Value) -> Value {
    let names: Vec<&str> = row
        .get("drivers")
        .and_then(Value::as_array)
        .map(|d| {
            d.iter()
                .filter_map(|p| p.get("displayName").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    if !names.is_empty() {
        return Value::String(names.join(" / "));
    }
    row.pointer("/person/displayName")
        .cloned()
        .unwrap_or(Value::Null)
}

/// `1:22.207` (or `58.112` under a minute).
pub fn format_lap(ms: i64) -> String {
    if ms <= 0 {
        return "—".to_string();
    }
    let minutes = ms / 60_000;
    let seconds = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    if minutes > 0 {
        format!("{minutes}:{seconds:02}.{millis:03}")
    } else {
        format!("{seconds}.{millis:03}")
    }
}

fn format_gap(ms: i64) -> String {
    if ms == 0 {
        "—".to_string()
    } else {
        format!("+{}.{:03}", ms / 1000, ms % 1000)
    }
}

/// A value as table text.
fn cell(value: &Value, kind: Kind) -> String {
    match (kind, value) {
        (_, Value::Null) => String::new(),
        (Kind::Lap, Value::Number(n)) => n.as_i64().map(format_lap).unwrap_or_default(),
        (Kind::Gap, Value::Number(n)) => n.as_i64().map(format_gap).unwrap_or_default(),
        (_, Value::String(s)) => s.clone(),
        (_, Value::Bool(b)) => if *b { "yes" } else { "no" }.to_string(),
        (_, other) => other.to_string(),
    }
}

/// A value as CSV text (numbers as they are, lap times in milliseconds).
fn raw(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// How wide a table cell may be before it is cut short.
const MAX_WIDTH: usize = 42;

fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_WIDTH {
        text.to_string()
    } else {
        let mut cut: String = text.chars().take(MAX_WIDTH - 1).collect();
        cut.push('…');
        cut
    }
}

/// Prints `rows` with `cols` in `format` to `out`. (`Format::Json` is the caller's: it prints the whole answer.)
pub fn write_rows(out: &mut dyn Write, rows: &[Value], cols: &[Col], format: Format) -> Result<()> {
    match format {
        Format::Table | Format::Json => write_table(out, rows, cols)?,
        Format::Csv => {
            let mut csv = csv::Writer::from_writer(out);
            csv.write_record(cols.iter().map(|c| c.name))?;
            for row in rows {
                csv.write_record(cols.iter().map(|c| raw(&(c.get)(row))))?;
            }
            csv.flush()?;
        }
        Format::Jsonl => {
            for row in rows {
                let mut object = Map::new();
                for c in cols {
                    object.insert(c.name.to_string(), (c.get)(row));
                }
                writeln!(out, "{}", Value::Object(object))?;
            }
        }
    }
    Ok(())
}

fn write_table(out: &mut dyn Write, rows: &[Value], cols: &[Col]) -> Result<()> {
    if rows.is_empty() {
        writeln!(out, "Nothing to show.")?;
        return Ok(());
    }
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            cols.iter()
                .map(|c| {
                    let text = cell(&(c.get)(row), c.kind);
                    if matches!(c.kind, Kind::Id) {
                        text
                    } else {
                        clip(&text)
                    }
                })
                .collect()
        })
        .collect();
    let widths: Vec<usize> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| {
            cells
                .iter()
                .map(|r| r[i].chars().count())
                .max()
                .unwrap_or(0)
                .max(c.header.chars().count())
        })
        .collect();
    let line = |values: Vec<&str>| -> String {
        values
            .iter()
            .zip(&widths)
            .map(|(v, w)| format!("{v}{}", " ".repeat(w - v.chars().count())))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    writeln!(out, "{}", line(cols.iter().map(|c| c.header).collect()))?;
    writeln!(
        out,
        "{}",
        widths
            .iter()
            .map(|w| "─".repeat(*w))
            .collect::<Vec<_>>()
            .join("  ")
    )?;
    for row in &cells {
        writeln!(out, "{}", line(row.iter().map(String::as_str).collect()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lap_times_read_like_a_timing_sheet() {
        assert_eq!(format_lap(82_207), "1:22.207");
        assert_eq!(format_lap(58_112), "58.112");
        assert_eq!(format_lap(600_005), "10:00.005");
        assert_eq!(format_gap(512), "+0.512");
        assert_eq!(format_gap(0), "—");
    }

    #[test]
    fn tables_and_csv_agree_on_the_rows() {
        let rows = vec![
            json!({"n": "Max", "t": 82207}),
            json!({"n": "Lando", "t": 82400}),
        ];
        let cols = vec![
            col("Driver", "driver", "/n"),
            lap("Best", "best_lap_ms", "/t"),
        ];
        let mut table = Vec::new();
        write_rows(&mut table, &rows, &cols, Format::Table).unwrap();
        let table = String::from_utf8(table).unwrap();
        assert!(table.contains("Max     1:22.207"));
        let mut csv = Vec::new();
        write_rows(&mut csv, &rows, &cols, Format::Csv).unwrap();
        assert_eq!(
            String::from_utf8(csv).unwrap(),
            "driver,best_lap_ms\nMax,82207\nLando,82400\n"
        );
        let mut jsonl = Vec::new();
        write_rows(&mut jsonl, &rows, &cols, Format::Jsonl).unwrap();
        assert!(
            String::from_utf8(jsonl)
                .unwrap()
                .starts_with(r#"{"driver":"Max","best_lap_ms":82207}"#)
        );
    }

    #[test]
    fn drivers_join_with_a_slash() {
        let row = json!({"drivers": [{"displayName": "A"}, {"displayName": "B"}]});
        assert_eq!(driver_names(&row), json!("A / B"));
        assert_eq!(
            driver_names(&json!({"person": {"displayName": "C"}})),
            json!("C")
        );
    }
}
