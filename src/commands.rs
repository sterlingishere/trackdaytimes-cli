//! The read commands: each fetches one API answer (or every page of it with `--all`) and prints its rows.

use std::io::Write;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::Paging;
use crate::client::{Client, DEFAULT_API};
use crate::config::Config;
use crate::output::{
    self, Col, Format, Kind, car_name, col, computed, driver_names, gap, ident, lap,
};

/// A list answer: its rows (at `rows`), and how many there are in all (at `total`, when the endpoint pages).
struct List<'a> {
    path: String,
    query: Vec<(&'a str, String)>,
    rows: &'static str,
    total: Option<&'static str>,
}

/// The answer and its rows; with `--all`, every page's rows (and the answer is the first page's).
fn fetch(client: &Client, list: List, paging: Option<&Paging>) -> Result<(Value, Vec<Value>)> {
    let mut query = list.query.clone();
    let all = paging.is_some_and(|p| p.all);
    if let Some(p) = paging {
        let limit = p.limit.or(all.then_some(200));
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        if let Some(offset) = p.offset {
            query.push(("offset", offset.to_string()));
        }
    }
    let first = client.get(&list.path, &query)?;
    let mut rows = rows_at(&first, list.rows);
    if all && let Some(total_at) = list.total {
        let total = first.get(total_at).and_then(Value::as_u64).unwrap_or(0);
        let mut offset = rows.len() as u64;
        while offset < total && !rows.is_empty() {
            let mut page_query = query.clone();
            page_query.retain(|(k, _)| *k != "offset");
            page_query.push(("offset", offset.to_string()));
            let page = client.get(&list.path, &page_query)?;
            let more = rows_at(&page, list.rows);
            if more.is_empty() {
                break;
            }
            offset += more.len() as u64;
            rows.extend(more);
            eprint!("\r{offset} of {total}…");
        }
        if total > 0 {
            eprintln!("\r{} rows.        ", rows.len());
        }
    }
    Ok((first, rows))
}

fn rows_at(value: &Value, pointer: &str) -> Vec<Value> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Prints rows, or with `--format json` the API's answer (every page's rows, after `--all`).
fn show(
    out: &mut dyn Write,
    format: Format,
    whole: &Value,
    rows: &[Value],
    cols: &[Col],
    all: bool,
) -> Result<()> {
    if format == Format::Json {
        let value = if all {
            Value::Array(rows.to_vec())
        } else {
            whole.clone()
        };
        writeln!(out, "{}", serde_json::to_string_pretty(&value)?)?;
        return Ok(());
    }
    output::write_rows(out, rows, cols, format)
}

/// Says how many more there are when a page was cut short (tables only).
fn more_note(format: Format, whole: &Value, total_at: &str, shown: usize, paging: &Paging) {
    if format != Format::Table || paging.all {
        return;
    }
    let total = whole.get(total_at).and_then(Value::as_u64).unwrap_or(0) as usize;
    let offset = paging.offset.unwrap_or(0) as usize;
    if total > offset + shown {
        eprintln!(
            "Showing {}–{} of {total}. Use --all for every row, or --limit and --offset.",
            offset + 1,
            offset + shown
        );
    }
}

fn opt(value: Option<String>) -> String {
    value.unwrap_or_default()
}

/// A path segment, escaped.
fn seg(text: &str) -> String {
    let mut out = String::new();
    for b in text.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---- account -----------------------------------------------------------------------------------------------

pub fn login(api_url: &str, flag_key: Option<String>) -> Result<()> {
    let key = match flag_key.filter(|k| !k.trim().is_empty()) {
        Some(key) => key,
        None => {
            eprintln!(
                "Make a key at {DEFAULT_API}/settings#api-keys (Settings, API key), then paste it here."
            );
            rpassword::prompt_password("API key: ").context("Couldn't read the key.")?
        }
    };
    let key = key.trim().to_string();
    if !key.starts_with("tdtk_") {
        bail!("That doesn't look like a TrackDayTimes API key: they start with tdtk_.");
    }
    let client = Client::new(api_url, Some(key.clone()));
    let me = client.get("key", &[])?;
    let path = Config { api_key: Some(key) }.save()?;
    println!(
        "Signed in as {} (@{}). Saved to {}.",
        me.pointer("/person/displayName")
            .and_then(Value::as_str)
            .unwrap_or("?"),
        me.pointer("/person/handle")
            .and_then(Value::as_str)
            .unwrap_or("?"),
        path.display()
    );
    Ok(())
}

pub fn logout() -> Result<()> {
    if Config::clear()? {
        println!(
            "Forgot the saved key. (It still works until you revoke it in Settings, API key.)"
        );
    } else {
        println!("No key was saved.");
    }
    Ok(())
}

pub fn whoami(client: &Client, out: &mut dyn Write, format: Format) -> Result<()> {
    let me = client.get("key", &[])?;
    if format == Format::Table {
        writeln!(
            out,
            "{} (@{}), key {}…, {} uncached reads a minute",
            me.pointer("/person/displayName")
                .and_then(Value::as_str)
                .unwrap_or("?"),
            me.pointer("/person/handle")
                .and_then(Value::as_str)
                .unwrap_or("?"),
            me.pointer("/apiKey/prefix")
                .and_then(Value::as_str)
                .unwrap_or("?"),
            me.pointer("/limits/readsPerMinute")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )?;
        return Ok(());
    }
    writeln!(out, "{}", serde_json::to_string_pretty(&me)?)?;
    Ok(())
}

// ---- tracks and rankings -------------------------------------------------------------------------------------

pub fn tracks(client: &Client, out: &mut dyn Write, format: Format) -> Result<()> {
    let list = List {
        path: "tracks".into(),
        query: vec![],
        rows: "/tracks",
        total: None,
    };
    let (whole, rows) = fetch(client, list, None)?;
    let cols = vec![
        ident("Track", "id", "/id"),
        col("Name", "name", "/name"),
        col("Location", "location", "/location"),
        col("Opened", "opened", "/opened"),
    ];
    show(out, format, &whole, &rows, &cols, false)
}

pub fn track(client: &Client, out: &mut dyn Write, format: Format, track: &str) -> Result<()> {
    let list = List {
        path: format!("tracks/{}", seg(track)),
        query: vec![],
        rows: "/track/layouts",
        total: None,
    };
    let (whole, rows) = fetch(client, list, None)?;
    if format == Format::Table {
        writeln!(
            out,
            "{} — {}\n",
            whole
                .pointer("/track/name")
                .and_then(Value::as_str)
                .unwrap_or(track),
            whole
                .pointer("/track/location")
                .and_then(Value::as_str)
                .unwrap_or("")
        )?;
    }
    let cols = vec![
        ident("Layout", "slug", "/slug"),
        ident("Id", "id", "/id"),
        col("Name", "name", "/name"),
        col("Km", "length_km", "/lengthKm"),
        col("Turns", "turns", "/turns"),
        col("Historic", "historic", "/historic"),
        lap("Record", "record_ms", "/stats/fastest/bestLapMs"),
    ];
    show(out, format, &whole, &rows, &cols, false)
}

pub fn rankings(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    track: &str,
    layout: &str,
    category: Option<String>,
    paging: &Paging,
) -> Result<()> {
    let list = List {
        path: format!("tracks/{}/layouts/{}/rankings", seg(track), seg(layout)),
        query: vec![("category", opt(category))],
        rows: "/rows",
        total: Some("total"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    let cols = vec![
        col("#", "rank", "/rank"),
        lap("Best", "best_lap_ms", "/bestLapMs"),
        gap("Gap", "gap_ms", "/gapMs"),
        computed("Driver", "drivers", driver_names),
        computed("Car", "car", |r| car_name(&r["car"])),
        col("Team", "team", "/team/name"),
        col("Date", "date", "/date"),
        ident("Event", "event_slug", "/eventSlug"),
    ];
    show(out, format, &whole, &rows, &cols, paging.all)?;
    more_note(format, &whole, "total", rows.len(), paging);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn top(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    what: &str,
    category: Option<String>,
    track: Option<String>,
    sort: Option<String>,
    paging: &Paging,
) -> Result<()> {
    let list = List {
        path: format!("rankings/{what}"),
        query: vec![
            ("category", opt(category)),
            ("track", opt(track)),
            ("sort", opt(sort)),
        ],
        rows: "/rows",
        total: Some("total"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    let mut cols = vec![col("#", "rank", "/rank")];
    if what == "cars" {
        cols.push(computed("Model", "model", |r| car_name(&r["model"])));
        cols.push(ident("Slug", "model_slug", "/model/slug"));
    } else {
        cols.push(col("Driver", "driver", "/person/displayName"));
        cols.push(ident("Handle", "handle", "/person/handle"));
        cols.push(col("Country", "country", "/person/country"));
        cols.push(col("Team", "team", "/team/name"));
    }
    cols.push(col("Elo", "elo", "/elo"));
    cols.push(col("Events", "events", "/events"));
    cols.push(col("Laps", "laps", "/laps"));
    show(out, format, &whole, &rows, &cols, paging.all)?;
    more_note(format, &whole, "total", rows.len(), paging);
    Ok(())
}

// ---- events --------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn events(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    upcoming: bool,
    track: Option<String>,
    layout: Option<String>,
    from: Option<String>,
    to: Option<String>,
    paging: &Paging,
) -> Result<()> {
    let list = List {
        path: "events".into(),
        query: vec![
            ("upcoming", upcoming.to_string()),
            ("track", opt(track)),
            ("layout", opt(layout)),
            ("from", opt(from)),
            ("to", opt(to)),
        ],
        rows: "/events",
        total: Some("total"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    let cols = vec![
        col("Date", "date", "/date"),
        ident("Event", "slug", "/slug"),
        col("Name", "name", "/name"),
        col("Track", "track", "/track/shortName"),
        col("Layout", "layout", "/layout/short"),
        col("Cars", "cars", "/carCount"),
    ];
    show(out, format, &whole, &rows, &cols, paging.all)?;
    more_note(format, &whole, "total", rows.len(), paging);
    Ok(())
}

pub fn event(client: &Client, out: &mut dyn Write, format: Format, event: &str) -> Result<()> {
    let list = List {
        path: format!("events/{}", seg(event)),
        query: vec![],
        rows: "/leaderboard",
        total: None,
    };
    let (whole, rows) = fetch(client, list, None)?;
    if format == Format::Table {
        writeln!(
            out,
            "{} — {}, {} {}\n",
            whole
                .pointer("/event/name")
                .and_then(Value::as_str)
                .unwrap_or(event),
            whole
                .pointer("/event/dateLabel")
                .and_then(Value::as_str)
                .unwrap_or(""),
            whole
                .pointer("/event/track/shortName")
                .and_then(Value::as_str)
                .unwrap_or(""),
            whole
                .pointer("/event/layout/name")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )?;
    }
    let cols = vec![
        col("#", "rank", "/rank"),
        lap("Best", "best_lap_ms", "/bestLapMs"),
        gap("Gap", "gap_ms", "/gapMs"),
        col("Laps", "lap_count", "/lapCount"),
        computed("Driver", "drivers", driver_names),
        computed("Car", "car", |r| car_name(&r["car"])),
        col("No.", "number", "/car/number"),
        col("Team", "team", "/team/name"),
        ident("Car slug", "car_slug", "/car/slug"),
    ];
    show(out, format, &whole, &rows, &cols, false)
}

pub fn laps(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    event: &str,
    car: Option<&str>,
) -> Result<()> {
    let (event_value, mut rows) = crate::export::event_laps(client, event)?;
    if let Some(car) = car {
        let car = car.trim().to_ascii_lowercase();
        rows.retain(|r| {
            r.get("carSlug").and_then(Value::as_str) == Some(car.as_str())
                || r.get("carId").and_then(Value::as_str) == Some(car.as_str())
        });
        if rows.is_empty() {
            bail!(
                "No laps for a car called {car} at this event. `tdt event {event}` lists its cars."
            );
        }
    }
    if format == Format::Table {
        writeln!(
            out,
            "{} — {}\n",
            event_value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(event),
            event_value
                .get("date")
                .and_then(Value::as_str)
                .unwrap_or("")
        )?;
    }
    let cols = vec![
        col("Car", "car", "/carName"),
        col("No.", "number", "/carNumber"),
        col("Driver", "driver", "/driverName"),
        col("Session", "session", "/sessionName"),
        col("Lap", "lap_number", "/lapNumber"),
        lap("Time", "time_ms", "/timeMs"),
        col("Timed by", "source", "/source"),
    ];
    let whole = json!({ "event": event_value, "laps": rows });
    show(out, format, &whole, &rows, &cols, false)
}

// ---- drivers, cars, teams ----------------------------------------------------------------------------------------

fn results_cols() -> Vec<Col> {
    vec![
        col("Date", "date", "/event/date"),
        col("Event", "event", "/event/name"),
        col("Track", "track", "/track/shortName"),
        col("Layout", "layout", "/layout/short"),
        col("Pos", "position", "/position/rank"),
        lap("Best", "best_lap_ms", "/bestLapMs"),
        col("Laps", "lap_count", "/lapCount"),
        computed("Car", "car", |r| car_name(&r["car"])),
        computed("Driver", "drivers", driver_names),
        ident("Event slug", "event_slug", "/event/slug"),
    ]
}

pub fn drivers(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    paging: &Paging,
) -> Result<()> {
    let list = List {
        path: "drivers".into(),
        query: vec![],
        rows: "/rows",
        total: Some("total"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    let cols = vec![
        ident("Handle", "handle", "/person/handle"),
        col("Name", "name", "/person/displayName"),
        col("Country", "country", "/person/country"),
        col("Events", "events", "/stats/events"),
        col("Laps", "laps", "/stats/laps"),
        col("Elo", "elo", "/stats/elo"),
        col("Rank", "rank", "/rank/rank"),
    ];
    show(out, format, &whole, &rows, &cols, paging.all)?;
    more_note(format, &whole, "total", rows.len(), paging);
    Ok(())
}

pub fn driver(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    handle: &str,
    paging: &Paging,
) -> Result<()> {
    let handle = handle.trim().trim_start_matches('@');
    let list = List {
        path: format!("drivers/{}", seg(handle)),
        query: vec![],
        rows: "/results",
        total: Some("resultsTotal"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    if format == Format::Table {
        writeln!(
            out,
            "{} (@{}) {} · {} events, {} laps, Elo {}{}\n",
            whole
                .pointer("/person/displayName")
                .and_then(Value::as_str)
                .unwrap_or(handle),
            whole
                .pointer("/person/handle")
                .and_then(Value::as_str)
                .unwrap_or(handle),
            whole
                .pointer("/person/country")
                .and_then(Value::as_str)
                .unwrap_or(""),
            whole
                .pointer("/stats/events")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            whole
                .pointer("/stats/laps")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            whole
                .pointer("/stats/elo")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            whole
                .pointer("/team/name")
                .and_then(Value::as_str)
                .map(|t| format!(" · {t}"))
                .unwrap_or_default(),
        )?;
    }
    show(out, format, &whole, &rows, &results_cols(), paging.all)?;
    more_note(format, &whole, "resultsTotal", rows.len(), paging);
    Ok(())
}

pub fn models(client: &Client, out: &mut dyn Write, format: Format, paging: &Paging) -> Result<()> {
    let list = List {
        path: "models".into(),
        query: vec![],
        rows: "/rows",
        total: Some("total"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    let cols = vec![
        ident("Model", "slug", "/slug"),
        col("Make", "make", "/make"),
        col("Name", "model", "/model"),
        col("Years", "years", "/years"),
        col("Race car", "racecar", "/racecar"),
        col("Cars", "cars", "/carCount"),
        col("Elo", "elo", "/stats/elo"),
    ];
    show(out, format, &whole, &rows, &cols, paging.all)?;
    more_note(format, &whole, "total", rows.len(), paging);
    Ok(())
}

pub fn model(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    slug: &str,
    paging: &Paging,
) -> Result<()> {
    let list = List {
        path: format!("models/{}", seg(slug)),
        query: vec![],
        rows: "/cars",
        total: Some("carsTotal"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    if format == Format::Table {
        writeln!(
            out,
            "{} {} {}\n",
            whole
                .pointer("/model/make")
                .and_then(Value::as_str)
                .unwrap_or(""),
            whole
                .pointer("/model/model")
                .and_then(Value::as_str)
                .unwrap_or(slug),
            whole
                .pointer("/model/years")
                .and_then(Value::as_str)
                .map(|y| format!("({y})"))
                .unwrap_or_default(),
        )?;
    }
    let cols = vec![
        ident("Car", "car_slug", "/car/slug"),
        col("No.", "number", "/car/number"),
        col("Owner", "owner", "/owner/displayName"),
        computed("Drivers", "drivers", driver_names),
        col("Team", "team", "/team/name"),
        col("Events", "events", "/events"),
        lap("Best", "best_lap_ms", "/bestLapMs"),
    ];
    show(out, format, &whole, &rows, &cols, paging.all)?;
    more_note(format, &whole, "carsTotal", rows.len(), paging);
    Ok(())
}

pub fn car(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    slug: &str,
    paging: &Paging,
) -> Result<()> {
    let list = List {
        path: format!("cars/{}", seg(slug)),
        query: vec![],
        rows: "/history",
        total: Some("historyTotal"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    if format == Format::Table {
        writeln!(
            out,
            "{}{}\n",
            car_name(&whole["car"]).as_str().unwrap_or(slug),
            whole
                .pointer("/owner/displayName")
                .and_then(Value::as_str)
                .map(|o| format!(" · {o}"))
                .unwrap_or_default(),
        )?;
    }
    show(out, format, &whole, &rows, &results_cols(), paging.all)?;
    more_note(format, &whole, "historyTotal", rows.len(), paging);
    Ok(())
}

pub fn teams(client: &Client, out: &mut dyn Write, format: Format, paging: &Paging) -> Result<()> {
    let list = List {
        path: "teams".into(),
        query: vec![],
        rows: "/teams",
        total: Some("total"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    let cols = vec![
        ident("Team", "slug", "/team/slug"),
        col("Name", "name", "/team/name"),
        col("Country", "country", "/team/country"),
        col("Events", "events", "/events"),
        col("Cars", "cars", "/cars"),
        col("Drivers", "drivers", "/drivers"),
        col("Last", "last_date", "/lastDate"),
    ];
    show(out, format, &whole, &rows, &cols, paging.all)?;
    more_note(format, &whole, "total", rows.len(), paging);
    Ok(())
}

pub fn team(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    slug: &str,
    paging: &Paging,
) -> Result<()> {
    let list = List {
        path: format!("teams/{}", seg(slug)),
        query: vec![],
        rows: "/results",
        total: Some("resultsTotal"),
    };
    let (whole, rows) = fetch(client, list, Some(paging))?;
    if format == Format::Table {
        writeln!(
            out,
            "{} · {} events\n",
            whole
                .pointer("/team/name")
                .and_then(Value::as_str)
                .unwrap_or(slug),
            whole.get("events").and_then(Value::as_i64).unwrap_or(0),
        )?;
    }
    show(out, format, &whole, &rows, &results_cols(), paging.all)?;
    more_note(format, &whole, "resultsTotal", rows.len(), paging);
    Ok(())
}

// ---- search and raw --------------------------------------------------------------------------------------------

pub fn search(
    client: &Client,
    out: &mut dyn Write,
    format: Format,
    query: &str,
    kind: Option<String>,
) -> Result<()> {
    if query.trim().is_empty() {
        bail!("Say what to look for, like `tdt search porsche gt3`.");
    }
    let whole = client.get("search", &[("q", query.to_string()), ("kind", opt(kind))])?;
    let rows: Vec<Value> = whole
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|s| {
            s.get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let site = client.site().to_string();
    let cols = vec![
        col("Kind", "kind", "/kind"),
        col("Name", "name", "/label"),
        Col {
            kind: Kind::Id,
            ..computed("Link", "url", move |r| {
                r.get("href")
                    .and_then(Value::as_str)
                    .map(|h| json!(format!("{site}{h}")))
                    .unwrap_or(Value::Null)
            })
        },
    ];
    show(out, format, &whole, &rows, &cols, false)
}

pub fn api(client: &Client, out: &mut dyn Write, path: &str) -> Result<()> {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (k.to_string(), v.to_string())
        })
        .collect();
    let path = path.trim_start_matches('/').trim_start_matches("api/v1/");
    let query: Vec<(&str, String)> = pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let value = client.get(path, &query)?;
    writeln!(out, "{}", serde_json::to_string_pretty(&value)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_escaped() {
        assert_eq!(seg("max.verstappen"), "max.verstappen");
        assert_eq!(seg(" a b/c "), "a%20b%2Fc");
    }
}
