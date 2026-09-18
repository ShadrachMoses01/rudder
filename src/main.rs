mod app;
mod browser;
mod commands;
mod detect;
mod input;
mod logger;
mod service;
mod ui;

use app::App;
use clap::{Parser, Subcommand};
use input::Action;
use std::io::Write;

const CLI_POLL_MS: u64 = 500;
const CLI_SHUTDOWN_WAIT_MS: u64 = 100;

#[derive(Parser)]
#[command(name = "rudder", about = "Manage development services")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a rudder.toml config from detected services
    Init {
        /// Print to stdout instead of writing rudder.toml
        #[arg(long)]
        stdout: bool,
    },
    /// Start all services (foreground; Ctrl+C to stop)
    Up,
    /// Stop all services started by `rudder up`
    Down,
    /// List detected services
    Services,
}

fn setup_panic_hook() {
    let log_dir = dirs_or_home();
    std::panic::set_hook(Box::new(move |info| {
        let dir = log_dir.clone();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("panic.log");
        let msg = format!(
            "{} [panic] thread 'main' panicked at {}\n\n",
            super_basic_time(),
            info
        );
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(msg.as_bytes()));
        eprintln!("{}", msg.trim());
    }));
}

fn dirs_or_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".rudder")
        .join("logs")
}

fn super_basic_time() -> String {
    let total = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (total % 86400) / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn generate_config_toml(services: &[service::Service]) -> String {
    let mut out = String::new();
    for s in services {
        out.push_str("[[service]]\n");
        out.push_str(&format!("name = {:?}\n", s.name));
        out.push_str(&format!("cmd = {:?}\n", s.cmd_string()));
        if let Some(ref dir) = s.dir {
            out.push_str(&format!("dir = {:?}\n", dir));
        }
        if let Some(ref url) = s.url {
            out.push_str(&format!("url = {:?}\n", url));
        }
        out.push('\n');
    }
    out
}

fn cmd_init(stdout: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let services = detect::detect_services_for_init(&cwd);

    if services.is_empty() {
        eprintln!("No services detected in {}", cwd.display());
        std::process::exit(1);
    }

    let toml = generate_config_toml(&services);

    if stdout {
        print!("{}", toml);
    } else {
        let path = cwd.join("rudder.toml");
        match std::fs::write(&path, &toml) {
            Ok(_) => {
                println!("Config written to {}", path.display());
            }
            Err(e) => {
                eprintln!("Failed to write config: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn cmd_up() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut services = detect::detect_services_for(&cwd);

    if services.is_empty() {
        eprintln!("No services detected. Run `rudder init` to generate a config.");
        std::process::exit(1);
    }

    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::Relaxed);
    })
    .expect("Failed to set Ctrl+C handler");

    service::clear_pids(&cwd);

    for s in &mut services {
        println!("Starting {}...", s.name);
        s.start(&cwd);
        if let Some(pid) = s.child_pid() {
            service::write_pid(&cwd, &s.name, pid);
            println!("  {} started (pid {})", s.name, pid);
        }
    }

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        for s in &mut services {
            s.refresh();
        }
        let any_alive = services.iter().any(|s| {
            s.status == service::Status::Running
                || s.status == service::Status::Starting
                || s.status == service::Status::Stopping
        });
        if !any_alive {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(CLI_POLL_MS));
    }

    std::thread::sleep(std::time::Duration::from_millis(CLI_SHUTDOWN_WAIT_MS));
    for s in &mut services {
        s.refresh();
    }

    for s in &mut services {
        s.stop();
    }
    service::clear_pids(&cwd);
}

fn cmd_down() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let pids = service::read_pids(&cwd);

    if pids.is_empty() {
        println!("No running services found.");
        return;
    }

    let mut stale = 0;
    let mut stopped = 0;

    for (name, pgid) in &pids {
        print!("Stopping {}... ", name);
        if service::kill_process_group(*pgid) {
            println!("done");
            stopped += 1;
        } else {
            println!("already stopped (stale PID file)");
            stale += 1;
        }
    }

    service::clear_pids(&cwd);

    if stale > 0 {
        println!(
            "Cleaned up {} stale PID entr{}",
            stale,
            if stale == 1 { "y" } else { "ies" }
        );
    }
    if stopped > 0 {
        println!(
            "Stopped {} service{}",
            stopped,
            if stopped == 1 { "" } else { "s" }
        );
    }
}

fn cmd_services() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut services = detect::detect_services_for(&cwd);

    let pids = service::read_pids(&cwd);
    for s in &mut services {
        if let Some((_, pid)) = pids.iter().find(|(name, _)| *name == s.name) {
            if service::process_exists(*pid) {
                s.status = service::Status::Running;
            }
        }
    }

    if services.is_empty() {
        println!("No services detected. Run `rudder init` to generate a config.");
        return;
    }

    println!("Services in {}:\n", cwd.display());
    for s in &services {
        let cmd = s.cmd_string();
        let dir = s.dir.as_deref().unwrap_or(".");
        match s.status {
            service::Status::Running => println!("  ✓  {}  (running: {})", s.name, cmd),
            service::Status::Stopped => println!("  ○  {}  ({})  [{}]", s.name, cmd, dir),
            service::Status::Starting => println!("  ◐  {}  (starting)", s.name),
            service::Status::Stopping => println!("  ↓  {}  (stopping: {})", s.name, cmd),
            service::Status::Failed(ref e) => println!("  ✕  {}  (failed: {})", s.name, e),
        }
    }
}

fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = ratatui::init();
    let mut app = App::new();

    app.log("[sys] Session started");

    while !app.should_quit {
        app.refresh_services();

        terminal.draw(|frame| ui::render(frame, &app))?;

        if let Some(action) = input::read()? {
            match action {
                Action::Quit => app.should_quit = true,
                Action::Up => app.previous(),
                Action::Down => app.next(),
                Action::Confirm => app.confirm(),
                Action::ModeSwitch => app.switch_mode(),
                Action::Tab => app.switch_pane(),
                Action::Char(c) => match c {
                    'h' if app.mode == app::Mode::Normal => app.go_up(),
                    'j' if app.mode == app::Mode::Normal => app.next(),
                    'k' if app.mode == app::Mode::Normal => app.previous(),
                    'l' if app.mode == app::Mode::Normal => app.confirm(),
                    _ => app.push_char(c),
                },
                Action::Backspace => {
                    if app.mode == app::Mode::Normal {
                        app.go_up();
                    } else {
                        app.pop_char();
                    }
                }
                Action::OpenLog => {
                    app.log("[sys] Opening session log (F12)");
                    app.open_log();
                }
                Action::PageUp => app.log_scroll_up(10),
                Action::PageDown => app.log_scroll_down(10),
                Action::None => {}
            }
        }
    }

    app.log("[sys] Shutting down");
    for service in &mut app.services {
        service.stop();
    }

    ratatui::restore();
    app.log("[sys] Session ended");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_panic_hook();
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init { stdout }) => {
            cmd_init(stdout);
        }
        Some(Commands::Up) => {
            cmd_up();
        }
        Some(Commands::Down) => {
            cmd_down();
        }
        Some(Commands::Services) => {
            cmd_services();
        }
        None => {
            run_tui()?;
        }
    }

    Ok(())
}
