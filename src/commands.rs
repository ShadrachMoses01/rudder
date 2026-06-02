use crate::app::{App, Pane};

fn do_start(app: &mut App, name: Option<&str>) {
    match name {
        Some("all") => {
            app.log("[launch] Starting all services");
            for s in &mut app.services {
                s.start(&app.browser.current_dir);
            }
        }
        Some(n) => {
            if app.services.iter().any(|s| s.name == n) {
                app.log(&format!("[launch] Starting service: {}", n));
                if let Some(s) = app.services.iter_mut().find(|s| s.name == n) {
                    s.start(&app.browser.current_dir);
                }
            }
        }
        None => {
            let sname = app.services.get(app.service_selected).map(|s| s.name.clone());
            if let Some(ref name) = sname {
                app.log(&format!("[launch] Starting service: {}", name));
                if let Some(s) = app.services.get_mut(app.service_selected) {
                    s.start(&app.browser.current_dir);
                }
            }
        }
    }
    app.focused_pane = Pane::Services;
}

fn do_stop(app: &mut App, name: Option<&str>) {
    match name {
        Some("all") => {
            app.log("[sys] Stopping all services");
            for s in &mut app.services {
                s.stop();
            }
        }
        Some(n) => {
            if app.services.iter().any(|s| s.name == n) {
                app.log(&format!("[sys] Stopping service: {}", n));
                if let Some(s) = app.services.iter_mut().find(|s| s.name == n) {
                    s.stop();
                }
            }
        }
        None => {
            let sname = app.services.get(app.service_selected).map(|s| s.name.clone());
            if let Some(ref name) = sname {
                app.log(&format!("[sys] Stopping service: {}", name));
                if let Some(s) = app.services.get_mut(app.service_selected) {
                    s.stop();
                }
            }
        }
    }
    app.focused_pane = Pane::Services;
}

fn do_restart(app: &mut App, name: Option<&str>) {
    if name == Some("all") {
        app.log("[sys] Restarting all services");
        for s in &mut app.services {
            s.stop();
            s.start(&app.browser.current_dir);
        }
    } else if let Some(n) = name {
        if app.services.iter().any(|s| s.name == n) {
            app.log(&format!("[sys] Restarting service: {}", n));
            if let Some(s) = app.services.iter_mut().find(|s| s.name == n) {
                s.stop();
                s.start(&app.browser.current_dir);
            }
        }
    } else {
        let sname = app.services.get(app.service_selected).map(|s| s.name.clone());
        if let Some(ref name) = sname {
            app.log(&format!("[sys] Restarting service: {}", name));
            if let Some(s) = app.services.get_mut(app.service_selected) {
                s.stop();
                s.start(&app.browser.current_dir);
            }
        }
    }
    app.focused_pane = Pane::Services;
}

pub fn execute(app: &mut App, input: &str) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return;
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    match parts.first() {
        Some(&"quit") | Some(&"q") => {
            app.log("[sys] Quit requested");
            app.should_quit = true;
        }

        Some(&"cd") => {
            let rest = trimmed[2..].trim();
            if !rest.is_empty() {
                app.log(&format!("[sys] cd: {}", rest));
                app.change_dir(rest);
            }
        }

        Some(&"services") | Some(&"ls") => {
            app.focused_pane = Pane::Services;
        }

        Some(&"start") => do_start(app, parts.get(1).copied()),
        Some(&"stop") => do_stop(app, parts.get(1).copied()),
        Some(&"restart") => do_restart(app, parts.get(1).copied()),

        Some(&"export-log") | Some(&"L") => {
            let dest = std::path::Path::new("rudder-session.log");
            match app.logger.export_to(dest) {
                Ok(_) => {
                    app.log(&format!("[sys] Log exported to {}", dest.display()));
                }
                Err(e) => {
                    app.log(&format!("[err] {}", e));
                }
            }
        }

        Some(&"logs") | Some(&"open-log") => {
            app.log("[sys] Opening session log");
            app.open_log();
        }

        _ => {}
    }
}
