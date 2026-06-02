mod app;
mod browser;
mod commands;
mod detect;
mod input;
mod logger;
mod service;
mod ui;

use app::App;
use input::Action;

fn setup_panic_hook() {
    let log_dir = dirs_or_home();
    std::panic::set_hook(Box::new(move |info| {
        let dir = log_dir.clone();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("panic.log");
        let msg = format!(
            "{} [panic] thread 'main' panicked at {}\n",
            super_basic_time(),
            info
        );
        let _ = std::fs::write(&path, &msg);
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_panic_hook();
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
