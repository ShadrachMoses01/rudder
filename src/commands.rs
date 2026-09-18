use crate::app::{App, Pane};

/// Resolve service indices from a command argument.
/// `None` means "currently selected service".
/// `Some("all")` means "all services".
/// Otherwise matches by service name or ID.
fn resolve_targets(app: &App, name: Option<&str>) -> Vec<usize> {
    match name {
        Some("all") => (0..app.services.len()).collect(),
        Some(n) => app
            .services
            .iter()
            .enumerate()
            .find(|(_, s)| s.name == n || s.id == n)
            .map(|(i, _)| vec![i])
            .unwrap_or_default(),
        None => {
            if app.service_selected < app.services.len() {
                vec![app.service_selected]
            } else {
                vec![]
            }
        }
    }
}

fn do_start(app: &mut App, name: Option<&str>) {
    let indices = resolve_targets(app, name);
    if indices.is_empty() {
        if let Some(n) = name {
            app.log(&format!("[err] Service not found: {}", n));
        }
        return;
    }
    for &i in &indices {
        let name = app.services[i].name.clone();
        app.log(&format!("[launch] Starting service: {}", name));
        app.services[i].start(&app.browser.current_dir);
    }
    app.focused_pane = Pane::Services;
}

fn do_stop(app: &mut App, name: Option<&str>) {
    let indices = resolve_targets(app, name);
    if indices.is_empty() {
        if let Some(n) = name {
            app.log(&format!("[err] Service not found: {}", n));
        }
        return;
    }
    for &i in &indices {
        let name = app.services[i].name.clone();
        app.log(&format!("[sys] Stopping service: {}", name));
        app.services[i].stop_background();
    }
    app.focused_pane = Pane::Services;
}

fn do_restart(app: &mut App, name: Option<&str>) {
    let indices = resolve_targets(app, name);
    if indices.is_empty() {
        if let Some(n) = name {
            app.log(&format!("[err] Service not found: {}", n));
        }
        return;
    }
    for &i in &indices {
        let name = app.services[i].name.clone();
        app.log(&format!("[sys] Restarting service: {}", name));
        app.services[i].stop();
        app.services[i].start(&app.browser.current_dir);
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

        Some(&"init") => {
            let cwd = app.browser.current_dir.clone();
            let services = crate::detect::detect_services_for_init(&cwd);
            if services.is_empty() {
                app.log("[sys] No services detected; nothing to init");
                return;
            }
            let path = cwd.join("rudder.toml");
            let mut out = String::new();
            for s in &services {
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
            match std::fs::write(&path, &out) {
                Ok(_) => {
                    app.log(&format!("[sys] Config written to {}", path.display()));
                    for s in &mut app.services {
                        s.stop();
                    }
                    app.restore_or_detect();
                    app.service_selected = 0;
                }
                Err(e) => {
                    app.log(&format!("[err] Failed to write config: {}", e));
                }
            }
        }

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
