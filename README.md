# Rudder

Rudder is a terminal UI and service manager for development projects. Point it
at a directory containing code, and it detects what kind of project it is,
derives the right dev services/commands, and lets you start, stop, and watch
them (with live logs) from a single Ratatui TUI. It also has a small CLI for
scripting (`up` / `down` / `services` / `init`).

## Problem

A typical workday involves several dev servers, databases, and background
tasks, each with its own start command, working directory, log stream, and
failure mode. Remembering and babysitting all of them across terminals is
tedious and error-prone. Rudder centralizes detection, startup, logging, and
shutdown in one place.

## Features

- Auto-detection of project type and dev command (Node, Rust, Go, Python,
  Java/Spring, Ruby, Elixir, PHP, .NET, Docker Compose services, Make, and
  more — see below).
- `rudder.toml` config for explicit service definitions (overrides
  auto-detection).
- Ratatui TUI with file browser, service list, live log pane, and command
  mode.
- CLI commands: `init`, `up`, `down`, `services`.
- Per-service log capture (stdout + stderr) with ANSI stripping and a
  500-line rolling buffer.
- Localhost URL detection from log output (shown next to the service).
- Port-conflict warning when logs mention `EADDRINUSE` / "port already in
  use".
- Process-group startup/shutdown on Unix (SIGTERM, then SIGKILL after 3s).
- PID files (`.rudder/pids/`) so `rudder down` in another terminal can stop
  what `rudder up` started, with stale-PID cleanup.
- Session log in `~/.rudder/logs/`, exportable via `:export-log`.

## Supported project / service types

`detect_primary` (first match wins, in this order):

| Marker | Command |
|---|---|
| `package.json` with `dev`/`develop` script | `<pm> run dev` (`<pm>` = bun/pnpm/yarn/npm based on lockfile; `npm start` for `react-scripts`, `ng serve` for Angular) |
| `package.json` with `start` script | `<pm> start` |
| `Cargo.toml` | `cargo run` |
| `go.mod` | `go run .` |
| `pom.xml` | `mvn spring-boot:run` |
| `build.gradle` / `build.gradle.kts` | `gradle bootRun` |
| `manage.py` | `python manage.py runserver` |
| `pyproject.toml` | `uv run` |
| `requirements.txt` | `python main.py` |
| `Gemfile` | `bundle exec rails server` |
| `mix.exs` | `mix phx.server` |
| `composer.json` (+ `artisan`) | `php artisan serve` (else `php -S localhost:8000`) |
| `Program.cs` / `Program.fs` | `dotnet run` |
| `main.py` | `uvicorn main:app --reload` (FastAPI), `flask run` (Flask), else `python main.py` |
| `index.js` / `index.ts` | `node index.js` |
| `server.js` | `node server.js` |
| `Makefile` | `make dev` / `make run` / `make start` / `make` |

Plus:

- **Docker Compose** (`docker-compose.yml/.yaml`, `compose.yml/.yaml`):
  parsed as YAML; known service keys (e.g. `postgres` → `Database`,
  `redis` → `Redis`, plus mongo, kafka, elasticsearch, grafana, nginx, …)
  each become `<docker compose|docker-compose> up <service>`.
  Only known names are detected; unknown compose services are ignored.
- **Tooling**: `Dockerfile` → `docker build -t app .`;
  `justfile` → `just`; `Taskfile.yml/.yaml` → `task dev`;
  `*.tf` → `terraform plan`; `flake.nix` → `nix develop`.
- **Subdirectories** (one level): each immediate subdir is scanned the same
  way; detected services get `dir = "<subdir>"`. Skipped: `node_modules`,
  `.git`, `target`, `dist`, `build`, `.next`, `.svelte-kit`, `.cache`,
  `vendor`, `__pycache__`, dot-directories.

## How detection works

- `detect_services_for(dir)`: if `rudder.toml` (or `.rudder.toml`) exists
  and defines services, it is the **sole** source of truth. Otherwise:
  primary + compose + subdirs + tooling, deduplicated by service ID
  (`<dir>::<name>`).
- `detect_services_for_init(dir)`: same as above but *also* merges in config
  services, so `rudder init` shows everything (used to generate config).
- Only one primary service per directory: a directory with both
  `package.json` and `Cargo.toml` detects only the Node service (known
  limitation).
- Lockfiles pick the Node package manager (`bun.lockb` → bun,
  `pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn, else npm).

## Starting / stopping services

- Commands are tokenized with `shlex` (POSIX-like splitting). Shell syntax
  (pipes, redirects, env assignments, `&&`) is **not** supported — wrap in
  `sh -c "..."` if needed.
- Pre-flight checks: working directory must exist; the executable must be
  in `PATH` (`which`) unless it looks like a path (contains `.` or `/`).
  Failures produce `Status::Failed` with a message in the service log.
- Spawn: `stdin` null, `stdout`/`stderr` piped, `TERM=dumb`, new process
  group on Unix (`process_group(0)`).
- `stop()` (blocking): kills the process group, reaps the child, marks
  `Stopped`. `stop_background()` (used by the TUI toggle): marks
  `Stopping`, kills the group on a background thread; the next `refresh()`
  reaps it. `start()` refuses while `Running`/`Starting`/`Stopping`.
- `refresh()` (called on every TUI tick / `up` poll): trims logs, scans for
  URLs/port warnings, and reaps exited children — non-zero exit →
  `Failed("exit code N")`, clean exit → `Stopped`, exit while stopping →
  `Stopped`.
- Shutdown escalation (Unix): SIGTERM (`kill -15 -<pgid>`), wait up to 3s,
  then SIGKILL (`kill -9`). Windows: `child.kill()` / `taskkill /F /PID`.

## Logs

- Each service keeps an in-memory log (`Waiting to start...` initially,
  `[sys] Started` on restart, ANSI codes stripped). Trimmed to the newest
  500 lines on each `refresh()`.
- Session log: `~/.rudder/logs/<epoch>.log`, with `latest.log` alongside
  (symlink on Unix, copy on Windows). TUI events (`[sys]`, `[launch]`,
  `[err]`) go here.
- `:export-log` (or `:L`) copies the session log to `./rudder-session.log`.
- `F12` opens the session log in the system viewer (`open` / `xdg-open` /
  Windows `start`).

## TUI

Layout: header (`Rudder`, mode, focused pane, path) · left file-browser pane
· right services list + log pane · bottom command bar.

- `Tab` switches panes. Browser: arrows/`j`/`k`, `Enter`/`l` enters a
  directory, `h`/`Backspace` goes up (services are stopped on directory
  change). Services: arrows/`j`/`k`, `Enter` toggles start/stop,
  `PgUp`/`PgDn` (or `Shift+Up/Down`) scrolls the log.
- `:` enters command mode, `Esc` leaves it, `Enter` runs the command,
  `Ctrl+C`/`Ctrl+D` quits.
- Commands: `start [name|all]`, `stop [name|all]`, `restart [name|all]`,
  `cd <path>`, `services`/`ls`, `init`, `export-log`/`L`, `logs`/`open-log`,
  `quit`/`q`. Bare `start`/`stop`/`restart` act on the selected service.
- Status dots: `✓` running, `◐` starting, `↓` stopping, `○` stopped,
  `✕` failed.

## Configuration (`rudder.toml`)

```toml
[[service]]
name = "Frontend"
cmd = "npm run dev"
dir = "frontend"          # optional, relative to the project root
url = "http://localhost:5173"  # optional, shown until log detection overrides
```

Generate it with `rudder init` (writes `rudder.toml`) or
`rudder init --stdout` (prints it). When a config exists, auto-detection is
skipped for normal operation.

## Installation

Requires a recent stable Rust toolchain (edition 2024).

```sh
cargo install --path .
# or
cargo build --release
./target/release/rudder
```

## Building from source

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets
```

## Basic usage

```sh
cd my-project
rudder services   # what would run here?
rudder init       # write rudder.toml (optional)
rudder            # open the TUI
rudder up         # run all services in the foreground (Ctrl+C stops)
rudder down       # stop services started by `rudder up` (another terminal)
```

## Example workflow

```sh
cd test-projects/monorepo
rudder services
# Services in .../monorepo:
#   ○  Frontend  (npm run dev)  [frontend]
#   ○  Backend  (node server.js)  [backend]
rudder up
# Starting Frontend...
#   Frontend started (pid 12345)
# ...
```

## Testing

- `cargo test` — 9 unit tests in `src/service.rs` covering start/stop
  lifecycle, non-blocking stop + reap, clean-exit vs failure status,
  stale-URL clearing on restart, double-start refusal, and real process-group
  kill (Unix).
- `test-projects/` fixtures: `node-app`, `rust-app`, `go-app`, `py-app`,
  `docker-app`, `spring-app`, `monorepo` (config-driven),
  `broken-project`. Build output inside fixtures is not committed; run
  `cargo build` / `npm install` inside a fixture as needed.
- `FAILURE_MATRIX.md` records 15 manual failure scenarios (detection,
  crashes, missing executables, stale PIDs, cross-terminal up/down, …).
  Cases 3 (Node+Rust same root), 8 (port conflict end-to-end), 10 (Ctrl+C
  mid-startup), 14 (heavy log output), 15 (rapid start/stop) are untested or
  partially verified — see that file.

## Current limitations

- One primary service per directory (Node wins over Rust/Go/etc. at the
  same level).
- Subdirectory scan is one level deep; unknown Docker Compose service names
  are ignored.
- No shell syntax in `cmd` (use `sh -c` explicitly).
- Config file disables auto-detection entirely when present.
- PID files are PID-only: OS PID reuse between `up` and `down` could in
  theory signal the wrong process.
- Log reader threads push without backpressure; trimming happens in
  `refresh()`, so a very chatty service can grow memory between ticks.
- Unix process management is well-tested; Windows paths (`taskkill`,
  `tasklist`, `cmd start`) compile but have not been verified on Windows.
- No Linux/macOS CI is configured yet.

## Project structure

```text
src/
  main.rs      CLI (init/up/down/services) + TUI entry + panic hook
  app.rs       TUI state, navigation, service cache across directories
  service.rs   Service lifecycle, process groups, PID files, URL/port scan
  detect.rs    project/compose/tooling/subdir detection + config parsing
  browser.rs   file browser pane
  commands.rs  TUI command-mode commands
  ui.rs        Ratatui rendering
  input.rs     key handling
  logger.rs    session log (~/.rudder/logs/)
test-projects/  detection/lifecycle fixtures (node, rust, go, python,
                docker, spring, monorepo, broken)
FAILURE_MATRIX.md  manual failure-scenario test log
```

## License

MIT — see [LICENSE](LICENSE).
