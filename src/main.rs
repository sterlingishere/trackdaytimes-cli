//! `tdt`: TrackDayTimes from the terminal. See README.md.

mod client;
mod commands;
mod config;
mod export;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::output::Format;

#[derive(Parser)]
#[command(
    name = "tdt",
    version,
    about = "Lap times, results and rankings from TrackDayTimes (UAE circuits), in your terminal.",
    long_about = "Lap times, results and rankings from TrackDayTimes (UAE circuits), in your terminal.\n\n\
        Every command needs an API key: make one at https://trackdaytimes.com/settings#api-keys and run `tdt login`.\n\
        The data is free for non-commercial use with credit to TrackDayTimes (CC BY-NC 4.0, see DATA-LICENSE.md).",
    after_help = "Examples:\n  tdt tracks\n  tdt rankings yas-marina north-circuit --category road\n  tdt event yas-marina-gp-circuit-2025-12-05\n  tdt laps yas-marina-gp-circuit-2025-12-05 --car red-bull-rb21-max.verstappen\n  tdt driver max.verstappen\n  tdt export laps --track yas-marina --from 2025-01-01 --out laps.csv"
)]
struct Cli {
    /// How to print: table (default), csv, jsonl, or json (the API's answer as it came).
    #[arg(long, short = 'f', global = true, value_enum, default_value_t = Format::Table)]
    format: Format,

    /// Shorthand for --format json.
    #[arg(long, global = true)]
    json: bool,

    /// Shorthand for --format csv.
    #[arg(long, global = true)]
    csv: bool,

    /// The site to read (for testing against dev).
    #[arg(long, global = true, env = "TDT_API_URL", default_value = client::DEFAULT_API, hide_default_value = true)]
    api_url: String,

    /// Use this API key instead of the saved one (TDT_API_KEY works too).
    #[arg(long, global = true, hide_env_values = true)]
    key: Option<String>,

    #[command(subcommand)]
    command: Command,
}

/// Paging for lists.
#[derive(clap::Args, Clone)]
pub struct Paging {
    /// Rows per page.
    #[arg(long, short = 'n')]
    pub limit: Option<u32>,
    /// Rows to skip.
    #[arg(long)]
    pub offset: Option<u32>,
    /// Every page, not just the first.
    #[arg(long, conflicts_with = "offset")]
    pub all: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Save your API key (from trackdaytimes.com, Settings, API key).
    Login,
    /// Forget the saved API key.
    Logout,
    /// Show whose key is in use and its limits.
    Whoami,

    /// List the circuits.
    Tracks,
    /// A circuit and its layouts.
    Track {
        /// Track id, like yas-marina or dubai-autodrome.
        track: String,
    },
    /// Lap-time ranking of a layout: the fastest car at each, best lap first.
    Rankings {
        /// Track id, like yas-marina.
        track: String,
        /// Layout slug, like north-circuit (see `tdt track <track>`).
        layout: String,
        /// overall, road or race.
        #[arg(long, short = 'c')]
        category: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    /// Elo rankings of car models or drivers.
    Top {
        /// cars or drivers.
        #[arg(value_parser = ["cars", "drivers"])]
        what: String,
        /// cars: overall, road or race. drivers: overall, amateur or pro.
        #[arg(long, short = 'c')]
        category: Option<String>,
        /// Only this track (an id like yas-marina).
        #[arg(long, short = 't')]
        track: Option<String>,
        /// elo (default), events or laps.
        #[arg(long)]
        sort: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    /// List events, newest first (past by default).
    Events {
        /// Upcoming events instead of past ones.
        #[arg(long)]
        upcoming: bool,
        /// Only this track (an id like yas-marina).
        #[arg(long, short = 't')]
        track: Option<String>,
        /// Only this layout (an id like yas-north).
        #[arg(long, short = 'l')]
        layout: Option<String>,
        /// From this date (YYYY-MM-DD).
        #[arg(long)]
        from: Option<String>,
        /// Up to this date (YYYY-MM-DD).
        #[arg(long)]
        to: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    /// An event's results: every car, best lap first.
    Event {
        /// Event slug (from `tdt events`).
        event: String,
    },
    /// Every lap at an event, or one car's.
    Laps {
        /// Event slug (from `tdt events`).
        event: String,
        /// Only this car (its slug or id, from `tdt event`).
        #[arg(long)]
        car: Option<String>,
    },
    /// List drivers.
    Drivers {
        #[command(flatten)]
        paging: Paging,
    },
    /// A driver's profile and results.
    Driver {
        /// Their username, like max.verstappen.
        handle: String,
        #[command(flatten)]
        paging: Paging,
    },
    /// List car models.
    Models {
        #[command(flatten)]
        paging: Paging,
    },
    /// A car model and its cars.
    Model {
        /// Model slug, like porsche-911-gt3-rs-992.
        slug: String,
        #[command(flatten)]
        paging: Paging,
    },
    /// A car's results.
    Car {
        /// Car slug, like red-bull-rb21-max.verstappen.
        slug: String,
        #[command(flatten)]
        paging: Paging,
    },
    /// List racing teams.
    Teams {
        #[command(flatten)]
        paging: Paging,
    },
    /// A team's results.
    Team {
        /// Team slug, like red-bull.
        slug: String,
        #[command(flatten)]
        paging: Paging,
    },
    /// Search drivers, cars, events, tracks and teams.
    Search {
        /// What to look for.
        query: Vec<String>,
        /// Only one kind: drivers, cars, events, tracks, teams or organizations.
        #[arg(long, short = 'k')]
        kind: Option<String>,
    },
    /// Download whole tables (events, results or laps) as CSV or JSON lines.
    Export(export::ExportArgs),
    /// GET any /api/v1 path and print the JSON (for anything the commands don't cover).
    Api {
        /// A path like events?upcoming=true or /api/v1/meta.
        path: String,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("tdt: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let format = if cli.json {
        Format::Json
    } else if cli.csv {
        Format::Csv
    } else {
        cli.format
    };
    match cli.command {
        Command::Login => commands::login(&cli.api_url, cli.key),
        Command::Logout => commands::logout(),
        command => {
            let client = client::Client::signed_in(&cli.api_url, cli.key)?;
            let out = &mut std::io::stdout().lock();
            match command {
                Command::Login | Command::Logout => unreachable!(),
                Command::Whoami => commands::whoami(&client, out, format),
                Command::Tracks => commands::tracks(&client, out, format),
                Command::Track { track } => commands::track(&client, out, format, &track),
                Command::Rankings {
                    track,
                    layout,
                    category,
                    paging,
                } => commands::rankings(&client, out, format, &track, &layout, category, &paging),
                Command::Top {
                    what,
                    category,
                    track,
                    sort,
                    paging,
                } => commands::top(&client, out, format, &what, category, track, sort, &paging),
                Command::Events {
                    upcoming,
                    track,
                    layout,
                    from,
                    to,
                    paging,
                } => commands::events(
                    &client, out, format, upcoming, track, layout, from, to, &paging,
                ),
                Command::Event { event } => commands::event(&client, out, format, &event),
                Command::Laps { event, car } => {
                    commands::laps(&client, out, format, &event, car.as_deref())
                }
                Command::Drivers { paging } => commands::drivers(&client, out, format, &paging),
                Command::Driver { handle, paging } => {
                    commands::driver(&client, out, format, &handle, &paging)
                }
                Command::Models { paging } => commands::models(&client, out, format, &paging),
                Command::Model { slug, paging } => {
                    commands::model(&client, out, format, &slug, &paging)
                }
                Command::Car { slug, paging } => {
                    commands::car(&client, out, format, &slug, &paging)
                }
                Command::Teams { paging } => commands::teams(&client, out, format, &paging),
                Command::Team { slug, paging } => {
                    commands::team(&client, out, format, &slug, &paging)
                }
                Command::Search { query, kind } => {
                    commands::search(&client, out, format, &query.join(" "), kind)
                }
                Command::Export(args) => export::run(&client, args),
                Command::Api { path } => commands::api(&client, out, &path),
            }
        }
    }
}
