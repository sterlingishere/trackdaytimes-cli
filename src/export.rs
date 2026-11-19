//! `tdt export`: whole tables, one row per event, result or lap, from the key-only `/api/v1/export` endpoints.
//! Results and laps are fetched one event at a time (four at once), with progress on stderr.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::client::Client;
use crate::output::{self, Col, Format, col, computed, driver_names};

#[derive(clap::Args)]
pub struct ExportArgs {
    /// What to export: events, results (one row per car per event) or laps (one row per lap).
    #[arg(value_parser = ["events", "results", "laps"])]
    pub table: String,
    /// Write here instead of the screen (the format follows the extension: .csv, .jsonl or .json).
    #[arg(long, short = 'o')]
    pub out: Option<String>,
    /// Only these events (slugs; repeat --event for more).
    #[arg(long = "event", short = 'e')]
    pub events: Vec<String>,
    /// Only this track (an id like yas-marina).
    #[arg(long, short = 't')]
    pub track: Option<String>,
    /// Only this layout (an id like yas-north).
    #[arg(long, short = 'l')]
    pub layout: Option<String>,
    /// From this date (YYYY-MM-DD).
    #[arg(long)]
    pub from: Option<String>,
    /// Up to this date (YYYY-MM-DD).
    #[arg(long)]
    pub to: Option<String>,
    /// csv (default), jsonl or json.
    #[arg(long = "as", value_parser = ["csv", "jsonl", "json"])]
    pub as_format: Option<String>,
}

/// How many events are fetched at once.
const WORKERS: usize = 4;

/// Every published event, past and upcoming, oldest first.
pub fn all_events(client: &Client) -> Result<Vec<Value>> {
    let whole = client.get("export/events", &[])?;
    Ok(whole
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// An event's laps with each car's name and number and the named driver's name, from the results of the same event.
pub fn event_laps(client: &Client, slug: &str) -> Result<(Value, Vec<Value>)> {
    let path = slug.trim().to_ascii_lowercase();
    let results = client.get(&format!("export/events/{path}/results"), &[])?;
    let laps = client.get(&format!("export/events/{path}/laps"), &[])?;
    let event = laps.get("event").cloned().unwrap_or(Value::Null);
    let mut cars = std::collections::HashMap::new();
    let mut people = std::collections::HashMap::new();
    for r in results
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = r.pointer("/car/id").and_then(Value::as_str) {
            cars.insert(id.to_string(), r.clone());
        }
        for p in r
            .get("drivers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = p.get("id").and_then(Value::as_str) {
                people.insert(
                    id.to_string(),
                    p.get("displayName").cloned().unwrap_or(Value::Null),
                );
            }
        }
    }
    let rows = laps
        .get("laps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut lap| {
            let car = lap
                .get("carId")
                .and_then(Value::as_str)
                .and_then(|id| cars.get(id))
                .cloned();
            let driver = lap
                .get("driverId")
                .and_then(Value::as_str)
                .and_then(|id| people.get(id))
                .cloned();
            if let Value::Object(o) = &mut lap {
                if let Some(car) = &car {
                    o.insert(
                        "carSlug".into(),
                        car.pointer("/car/slug").cloned().unwrap_or(Value::Null),
                    );
                    o.insert("carName".into(), output::car_name(&car["car"]));
                    o.insert(
                        "carNumber".into(),
                        car.pointer("/car/number").cloned().unwrap_or(Value::Null),
                    );
                    o.insert(
                        "carMake".into(),
                        car.pointer("/car/make").cloned().unwrap_or(Value::Null),
                    );
                    o.insert(
                        "carModel".into(),
                        car.pointer("/car/model").cloned().unwrap_or(Value::Null),
                    );
                }
                // The driver named on the lap, else the car's (when one person drove it).
                let name = driver.or_else(|| {
                    car.as_ref().and_then(|c| {
                        let drivers = c.get("drivers").and_then(Value::as_array)?;
                        (drivers.len() == 1)
                            .then(|| drivers[0].get("displayName").cloned())
                            .flatten()
                    })
                });
                o.insert("driverName".into(), name.unwrap_or(Value::Null));
            }
            lap
        })
        .collect();
    Ok((event, rows))
}

fn event_cols() -> Vec<Col> {
    vec![
        col("", "event_id", "/id"),
        col("", "event_slug", "/slug"),
        col("", "name", "/name"),
        col("", "date", "/date"),
        col("", "end_date", "/endDate"),
        col("", "kind", "/kind"),
        col("", "series", "/series"),
        col("", "organiser", "/organiser"),
        col("", "upcoming", "/upcoming"),
        col("", "track_id", "/trackId"),
        col("", "track_name", "/trackName"),
        col("", "layout_id", "/layoutId"),
        col("", "layout_name", "/layoutName"),
        col("", "layout_slug", "/layoutSlug"),
        col("", "layout_length_km", "/layoutLengthKm"),
        col("", "url_path", "/href"),
    ]
}

/// Results rows carry their event under `/event`.
fn result_cols() -> Vec<Col> {
    vec![
        col("", "event_slug", "/event/slug"),
        col("", "event_date", "/event/date"),
        col("", "event_name", "/event/name"),
        col("", "track_id", "/event/trackId"),
        col("", "layout_id", "/event/layoutId"),
        col("", "layout_name", "/event/layoutName"),
        col("", "rank", "/rank"),
        col("", "total", "/total"),
        col("", "entry_id", "/entryId"),
        col("", "car_id", "/car/id"),
        col("", "car_slug", "/car/slug"),
        col("", "car_make", "/car/make"),
        col("", "car_model", "/car/model"),
        col("", "car_model_slug", "/car/modelSlug"),
        col("", "car_year", "/car/year"),
        col("", "car_number", "/car/number"),
        col("", "racecar", "/car/racecar"),
        col("", "driver_id", "/person/id"),
        col("", "driver_handle", "/person/handle"),
        col("", "driver_name", "/person/displayName"),
        col("", "driver_country", "/person/country"),
        computed("", "all_drivers", driver_names),
        col("", "team_name", "/team/name"),
        col("", "best_lap_ms", "/bestLapMs"),
        col("", "gap_ms", "/gapMs"),
        col("", "lap_count", "/lapCount"),
        col("", "best_lap_number", "/bestLapNumber"),
        col("", "best_session_key", "/bestSessionKey"),
        col("", "best_lap_source", "/bestLapSource"),
        col("", "tag", "/tag"),
        col("", "url_path", "/href"),
    ]
}

fn lap_cols() -> Vec<Col> {
    vec![
        col("", "event_slug", "/event/slug"),
        col("", "event_date", "/event/date"),
        col("", "track_id", "/event/trackId"),
        col("", "layout_id", "/event/layoutId"),
        col("", "entry_id", "/entryId"),
        col("", "car_id", "/carId"),
        col("", "car_slug", "/carSlug"),
        col("", "car_make", "/carMake"),
        col("", "car_model", "/carModel"),
        col("", "car_number", "/carNumber"),
        col("", "session_key", "/sessionKey"),
        col("", "session_name", "/sessionName"),
        col("", "session_kind", "/sessionKind"),
        col("", "lap_number", "/lapNumber"),
        col("", "time_ms", "/timeMs"),
        col("", "driver_id", "/driverId"),
        col("", "driver_name", "/driverName"),
        col("", "source", "/source"),
    ]
}

fn format_of(args: &ExportArgs) -> Format {
    let by_name = |s: &str| match s {
        "jsonl" => Format::Jsonl,
        "json" => Format::Json,
        _ => Format::Csv,
    };
    if let Some(f) = &args.as_format {
        return by_name(f);
    }
    match args
        .out
        .as_deref()
        .and_then(|o| o.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
    {
        Some(ext) if ext == "jsonl" || ext == "json" => by_name(&ext),
        _ => Format::Csv,
    }
}

/// The events the arguments pick (all published ones that have run, unless named).
fn chosen_events(client: &Client, args: &ExportArgs, include_upcoming: bool) -> Result<Vec<Value>> {
    let events = all_events(client)?;
    let wanted: Vec<String> = args
        .events
        .iter()
        .map(|e| e.trim().to_ascii_lowercase())
        .collect();
    let picked: Vec<Value> = events
        .into_iter()
        .filter(|e| {
            let s = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let date = s("date");
            (wanted.is_empty() || wanted.contains(&s("slug")) || wanted.contains(&s("id")))
                && (include_upcoming
                    || !e.get("upcoming").and_then(Value::as_bool).unwrap_or(false))
                && args.track.as_ref().is_none_or(|t| &s("trackId") == t)
                && args.layout.as_ref().is_none_or(|l| &s("layoutId") == l)
                && args
                    .from
                    .as_ref()
                    .is_none_or(|f| date.as_str() >= f.as_str())
                && args.to.as_ref().is_none_or(|t| date.as_str() <= t.as_str())
        })
        .collect();
    if picked.is_empty() {
        bail!(
            "No events match. `tdt events` lists them; `tdt tracks` and `tdt track <id>` show track and layout ids."
        );
    }
    Ok(picked)
}

/// One event's rows, once fetched.
type Slot = Option<Result<Vec<Value>>>;

/// Runs `fetch` for every event, four at a time, in event order.
fn per_event(
    events: &[Value],
    fetch: impl Fn(&Value) -> Result<Vec<Value>> + Sync,
) -> Result<Vec<Value>> {
    let next = Mutex::new(0usize);
    let done = Mutex::new(0usize);
    let results: Mutex<Vec<Slot>> = Mutex::new((0..events.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(events.len()) {
            scope.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().unwrap();
                        let i = *n;
                        *n += 1;
                        i
                    };
                    if i >= events.len() {
                        break;
                    }
                    let got = fetch(&events[i]);
                    results.lock().unwrap()[i] = Some(got);
                    let mut d = done.lock().unwrap();
                    *d += 1;
                    eprint!("\rEvents: {} of {}…", *d, events.len());
                }
            });
        }
    });
    eprintln!();
    let mut rows = Vec::new();
    for (i, got) in results.into_inner().unwrap().into_iter().enumerate() {
        let slug = events[i].get("slug").and_then(Value::as_str).unwrap_or("?");
        rows.extend(
            got.unwrap_or_else(|| Ok(vec![]))
                .with_context(|| format!("while exporting {slug}"))?,
        );
    }
    Ok(rows)
}

pub fn run(client: &Client, args: ExportArgs) -> Result<()> {
    let format = format_of(&args);
    let (rows, cols) = match args.table.as_str() {
        "events" => (chosen_events(client, &args, true)?, event_cols()),
        "results" => {
            let events = chosen_events(client, &args, false)?;
            let rows = per_event(&events, |e| {
                let slug = e.get("slug").and_then(Value::as_str).unwrap_or_default();
                let whole = client.get(&format!("export/events/{slug}/results"), &[])?;
                Ok(whole
                    .get("results")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mut r| {
                        r["event"] = e.clone();
                        r
                    })
                    .collect())
            })?;
            (rows, result_cols())
        }
        _ => {
            let events = chosen_events(client, &args, false)?;
            let rows = per_event(&events, |e| {
                let slug = e.get("slug").and_then(Value::as_str).unwrap_or_default();
                let (_, laps) = event_laps(client, slug)?;
                Ok(laps
                    .into_iter()
                    .map(|mut l| {
                        l["event"] = e.clone();
                        l
                    })
                    .collect())
            })?;
            (rows, lap_cols())
        }
    };
    let mut out: Box<dyn Write> = match &args.out {
        Some(path) => Box::new(BufWriter::new(
            File::create(path).with_context(|| format!("Couldn't write {path}"))?,
        )),
        None => Box::new(BufWriter::new(std::io::stdout().lock())),
    };
    if format == Format::Json {
        let objects: Vec<Value> = rows
            .iter()
            .map(|r| {
                Value::Object(
                    cols.iter()
                        .map(|c| (c.name.to_string(), (c.get)(r)))
                        .collect(),
                )
            })
            .collect();
        writeln!(out, "{}", serde_json::to_string_pretty(&json!(objects))?)?;
    } else {
        output::write_rows(&mut out, &rows, &cols, format)?;
    }
    out.flush()?;
    if let Some(path) = &args.out {
        eprintln!("Wrote {} {} to {path}.", rows.len(), args.table);
    }
    Ok(())
}
