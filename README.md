# TrackDayTimes CLI

Lap times, results and rankings from [TrackDayTimes](https://trackdaytimes.com), the track-day and race results
site for UAE circuits (Yas Marina, Dubai Autodrome and more), in your terminal.

```text
$ tdt event yas-marina-gp-circuit-2025-12-05
2025 Abu Dhabi Grand Prix – Formula 1 — 5 Dec 2025, Yas Marina GP Circuit

#  Best      Gap     Laps  Driver          Car            No.  Team
─  ────────  ──────  ────  ──────────────  ─────────────  ───  ────────
1  1:22.207  —       131   Max Verstappen  Red Bull RB21  1    Red Bull
2  1:22.408  +0.201  128   Lando Norris    McLaren MCL39  4    McLaren
3  1:22.437  +0.230  103   Oscar Piastri   McLaren MCL39  81   McLaren
…
```

Everything the website shows is here: tracks and layouts, lap-time rankings, events and their results, every lap
of every car, drivers, car models, teams and search. You can also export whole tables as CSV or JSON for your
own analysis.

## Get an API key

The CLI needs a free TrackDayTimes account and an API key:

1. Sign in at [trackdaytimes.com](https://trackdaytimes.com). If you don't have an account, signing in with your
   email makes one.
2. Open **Settings → API key** ([trackdaytimes.com/settings#api-keys](https://trackdaytimes.com/settings#api-keys))
   and press **Create key**.
3. Press **Copy**. The key starts with `tdtk_`, and you can show or copy it there again at any time.

A key can read public data only, the same data the website shows. It can't see or change anything about your
account. Each account has one key; **Revoke** switches it off at once, and you can then make a new one.

## Install

**Download:** get the file for your computer from the latest
[release](https://github.com/sterlingishere/trackdaytimes-cli/releases/latest):

| Computer | File |
| --- | --- |
| Mac with Apple silicon (M1 and later) | `tdt-…-aarch64-apple-darwin.tar.gz` |
| Mac with an Intel chip | `tdt-…-x86_64-apple-darwin.tar.gz` |
| Linux (64-bit) | `tdt-…-x86_64-unknown-linux-gnu.tar.gz` |
| Windows (64-bit) | `tdt-…-x86_64-pc-windows-msvc.zip` |

Unpack it and put `tdt` (`tdt.exe` on Windows) in a folder on your `PATH`, such as `/usr/local/bin`. The Mac
builds aren't signed, so macOS may refuse to open one at first; allow it once with:

```sh
xattr -d com.apple.quarantine /usr/local/bin/tdt
```

**Or build it** with [Rust](https://rustup.rs) installed:

```sh
cargo install --git https://github.com/sterlingishere/trackdaytimes-cli
```

Then sign in with your key:

```sh
tdt login
```

It asks for the key without showing it on screen, checks it, and saves it to `~/.config/trackdaytimes/config.json`
(`%APPDATA%\trackdaytimes\config.json` on Windows), readable only by you. If you'd rather not save it, set
`TDT_API_KEY` in your environment instead, or pass `--key` to any command.

## Commands

| Command | What it shows |
| --- | --- |
| `tdt tracks` | The circuits |
| `tdt track yas-marina` | A circuit's layouts and their lap records |
| `tdt rankings yas-marina north-circuit [--category road\|race]` | A layout's lap-time ranking, fastest first |
| `tdt top cars\|drivers [--category …] [--track …] [--sort elo\|events\|laps]` | Elo rankings of car models or drivers |
| `tdt events [--upcoming] [--track …] [--from 2025-01-01] [--to …]` | Events, newest first |
| `tdt event <event-slug>` | An event's results, one row per car |
| `tdt laps <event-slug> [--car <car-slug>]` | Every lap at an event, or one car's |
| `tdt drivers` / `tdt driver max.verstappen` | Drivers, or one driver's profile and results |
| `tdt models` / `tdt model porsche-911-gt3-rs-992` | Car models, or one model's cars |
| `tdt car <car-slug>` | One car's results |
| `tdt teams` / `tdt team red-bull` | Racing teams, or one team's results |
| `tdt search porsche gt3 [--kind drivers]` | Search everything |
| `tdt export events\|results\|laps [filters] --out file.csv` | Whole tables, for analysis |
| `tdt api <path>` | Any `/api/v1` read, as raw JSON |
| `tdt whoami` / `tdt logout` | Whose key is in use / forget the saved key |

`tdt <command> --help` explains each command's options. Lists show the first page; add `--all` for every row, or
`--limit` and `--offset` to page yourself.

### Output formats

Every command prints a table by default. Add:

- `--csv` for CSV with a header row,
- `--format jsonl` for one JSON object per line,
- `--json` for the API's full answer as it came.

Tables show lap times as `1:22.207`. CSV and JSON keep them as integer milliseconds (`82207`), so they sort and
add up correctly.

```sh
tdt rankings yas-marina north-circuit --category road --all --csv > north-road.csv
tdt driver max.verstappen --json | jq '.results[0]'
```

### Exports

`tdt export` downloads whole tables with one row per event, per result (car at an event), or per lap:

```sh
tdt export events --out events.csv
tdt export results --track yas-marina --from 2025-01-01 --out yas-results.csv
tdt export laps --event yas-marina-gp-circuit-2025-12-05 --out abu-dhabi-gp-laps.jsonl
```

Filters: `--event <slug>` (repeat it for more), `--track <id>`, `--layout <id>`, `--from` and `--to` (dates as
`YYYY-MM-DD`). The format follows the file's extension (`.csv`, `.jsonl` or `.json`), or set it with
`--as csv|jsonl|json`. Without `--out` it prints to the screen.

Results and laps are fetched one event at a time, four at once. Exporting every lap TrackDayTimes holds (over a
million) takes a few minutes.

**Results columns:** `event_slug, event_date, event_name, track_id, layout_id, layout_name, rank, total, entry_id,
car_id, car_slug, car_make, car_model, car_model_slug, car_year, car_number, racecar, driver_id, driver_handle,
driver_name, driver_country, all_drivers, team_name, best_lap_ms, gap_ms, lap_count, best_lap_number,
best_session_key, best_lap_source, tag, url_path`

**Laps columns:** `event_slug, event_date, track_id, layout_id, entry_id, car_id, car_slug, car_make, car_model,
car_number, session_key, session_name, session_kind, lap_number, time_ms, driver_id, driver_name, source`

`source` is `phone` or `logger` when a member's own device timed the lap (the TrackDayTimes iPhone app, or a lap
file they uploaded), and empty for the organiser's official timing. `tag` is `WR` (the layout's lap record), `PB`
(the driver's best on that layout) or `Best` (they've been faster there since).

## Use your key with MCP

TrackDayTimes also runs an [MCP server](https://trackdaytimes.com/mcp) at `https://mcp.trackdaytimes.com`, so AI
assistants (Claude, ChatGPT and others) can look up lap times, events, drivers and rankings for you. It works without
a key. Add yours for higher limits, either as a header:

```text
Authorization: Bearer tdtk_your_key
```

or, if your app only lets you enter a URL, at the end of the server address:

```text
https://mcp.trackdaytimes.com/?key=tdtk_your_key
```

A key that's wrong or revoked is refused with "This API key isn't valid", so you know to fix it.

## Use the API directly

Everything the CLI does is plain HTTPS and JSON, so you can call it from any language. Send your key in the
`Authorization` header:

```sh
curl -H "Authorization: Bearer $TDT_API_KEY" https://trackdaytimes.com/api/v1/export/events
```

Responses are camelCase JSON, lap times are integer milliseconds (`bestLapMs`, `timeMs`), and every object with a
page on the site carries `href`, its path there. Lists take `limit` and `offset` and say their `total`.

**Needs a key:**

| Path | What it returns |
| --- | --- |
| `/api/v1/key` | Whose key it is and its limit |
| `/api/v1/export/events` | Every event, oldest first |
| `/api/v1/export/events/{slug}/results` | An event's results, one row per car |
| `/api/v1/export/events/{slug}/laps` | Every lap at an event |

**Also readable with a key:** `/api/v1/tracks`, `/api/v1/tracks/{track}`,
`/api/v1/tracks/{track}/layouts/{layout}/rankings`, `/api/v1/events`, `/api/v1/events/{slug}`,
`/api/v1/events/{slug}/cars/{car}`, `/api/v1/cars/{slug}`, `/api/v1/models`, `/api/v1/models/{slug}`,
`/api/v1/rankings/cars`, `/api/v1/rankings/drivers`, `/api/v1/drivers`, `/api/v1/drivers/{handle}`,
`/api/v1/teams`, `/api/v1/teams/{slug}`, `/api/v1/organizations`, `/api/v1/search?q=…`.

To see any answer in full, run `tdt api <path> | jq`, or add `--json` to any command. Errors come back as
`{"error": {"code", "message"}}` with the matching status: 401 for a missing or invalid key, 404 for an event, driver,
car or track that doesn't exist, and 429 with `Retry-After` when you're going too fast.

## Limits

Each key can make 3,000 requests a minute that aren't already cached (most reads are cached, and cached reads don't
count). If you go over, the API answers "slow down" and the CLI waits and tries again by itself. The CLI and MCP
share your key's limit. Please don't share
your key: anything done with it is tied to your account, and a key that's misused can be switched off.

## The data and how you may use it

The CLI's code is open source under the [MIT licence](LICENSE).

**The data is not.** Lap times, results, rankings and everything else the API returns are © TrackDayTimes and
licensed under [Creative Commons Attribution-NonCommercial 4.0](DATA-LICENSE.md) (CC BY-NC 4.0):

- **You may** use it for personal projects, research, analysis, teaching and journalism, and share what you make
  from it, as long as you credit TrackDayTimes with a link to [trackdaytimes.com](https://trackdaytimes.com).
- **You may not** use it commercially (in a paid product or service, or to sell it on) without written permission.
  Ask through the [contact page](https://trackdaytimes.com/support) if you'd like to.

Results come from organisers' timing sheets and from members. Names a driver has chosen to hide show as "Hidden",
in the CLI as on the website. If something looks wrong, report it on the website so it can be put right.

## Building from source

```sh
git clone https://github.com/sterlingishere/trackdaytimes-cli
cd trackdaytimes-cli
cargo build --release   # the binary is target/release/tdt
cargo test
```

`--api-url` (or `TDT_API_URL`) points the CLI at another copy of the site, such as
`https://dev.trackdaytimes.com`. A key works only on the site it was made on.
