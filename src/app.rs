use crate::browser::FileBrowser;
use crate::commands;
use crate::detect;
use crate::logger::Logger;
use crate::service::{Service, Status};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(PartialEq)]
pub enum Mode {
    Normal,
    Command,
}

#[derive(PartialEq)]
pub enum Pane {
    Browser,
    Services,
}

pub struct App {
    pub mode: Mode,
    pub should_quit: bool,
    pub services: Vec<Service>,
    pub command: String,
    pub browser: FileBrowser,
    pub focused_pane: Pane,
    pub service_selected: usize,
    pub logger: Logger,
    pub log_scroll: i32,
    service_cache: HashMap<PathBuf, HashMap<String, (Status, Vec<String>)>>,
}

impl App {
    pub fn new() -> Self {
        let browser = FileBrowser::new();
        let services = detect::detect_services_for(&browser.current_dir);
        Self {
            mode: Mode::Normal,
            should_quit: false,
            services,
            command: String::new(),
            browser,
            focused_pane: Pane::Browser,
            service_selected: 0,
            logger: Logger::new(),
            log_scroll: 0,
            service_cache: HashMap::new(),
        }
    }

    pub fn log(&mut self, msg: &str) {
        self.logger.log(msg);
    }

    fn save_current_services(&mut self) {
        let key = self.browser.current_dir.clone();
        let mut map = HashMap::new();
        for s in &self.services {
            let log = match s.log.lock() {
                Ok(g) => g.clone(),
                Err(e) => e.into_inner().clone(),
            };
            map.insert(s.name.clone(), (s.status.clone(), log));
        }
        self.service_cache.insert(key, map);
    }

    fn restore_services(&mut self) {
        let key = self.browser.current_dir.clone();
        let fresh = detect::detect_services_for(&self.browser.current_dir);
        self.services = match self.service_cache.remove(&key) {
            Some(cached) => fresh
                .into_iter()
                .map(|mut s| {
                    if let Some((status, log)) = cached.get(&s.name) {
                        let st = status.clone();
                        s.status = st;
                        if let Ok(mut l) = s.log.lock() {
                            *l = log.clone();
                        }
                    }
                    s
                })
                .collect(),
            None => fresh,
        };
    }

    pub fn previous(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        match self.focused_pane {
            Pane::Browser => self.browser.previous(),
            Pane::Services => {
                if self.service_selected > 0 {
                    self.service_selected -= 1;
                    self.log_scroll = 0;
                }
            }
        }
    }

    pub fn next(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        match self.focused_pane {
            Pane::Browser => self.browser.next(),
            Pane::Services => {
                if self.service_selected + 1 < self.services.len() {
                    self.service_selected += 1;
                    self.log_scroll = 0;
                }
            }
        }
    }

    pub fn log_scroll_up(&mut self, by: i32) {
        if self.focused_pane != Pane::Services {
            return;
        }
        self.log_scroll += by;
    }

    pub fn log_scroll_down(&mut self, by: i32) {
        if self.focused_pane != Pane::Services {
            return;
        }
        self.log_scroll = (self.log_scroll - by).max(0);
    }

    pub fn confirm(&mut self) {
        match self.mode {
            Mode::Normal => match self.focused_pane {
                Pane::Browser => {
                    self.save_current_services();
                    if self.browser.enter_selected() {
                        self.log(&format!("[sys] Entered directory: {}", self.browser.current_dir.display()));
                        for s in &mut self.services {
                            s.stop();
                        }
                        self.restore_services();
                        self.service_selected = 0;
                    }
                }
                Pane::Services => {
                    let action = {
                        let s = self.services.get(self.service_selected);
                        match s {
                            Some(s) if s.status == Status::Running || s.status == Status::Starting => Some("stop"),
                            Some(s) if s.status == Status::Stopping => None,
                            Some(_) => Some("start"),
                            None => None,
                        }
                    };
                    match action {
                        Some("stop") => {
                            let name = self.services[self.service_selected].name.clone();
                            self.services[self.service_selected].stop_background();
                            self.log(&format!("[sys] Stopping service: {}", name));
                        }
                        Some("start") => {
                            let name = self.services[self.service_selected].name.clone();
                            self.services[self.service_selected].start(&self.browser.current_dir);
                            self.log(&format!("[launch] Started service: {}", name));
                        }
                        _ => {}
                    }
                }
            },
            Mode::Command => {
                let cmd = std::mem::take(&mut self.command);
                self.mode = Mode::Normal;
                commands::execute(self, &cmd);
            }
        }
    }

    pub fn go_up(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        if self.focused_pane != Pane::Browser {
            return;
        }
        self.save_current_services();
        if self.browser.go_to_parent() {
            self.log(&format!("[sys] Moved up to: {}", self.browser.current_dir.display()));
            for s in &mut self.services {
                s.stop();
            }
            self.restore_services();
            self.service_selected = 0;
        }
    }

    pub fn switch_pane(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        self.focused_pane = match self.focused_pane {
            Pane::Browser => Pane::Services,
            Pane::Services => Pane::Browser,
        };
    }

    pub fn change_dir(&mut self, path: &str) {
        let p = std::path::Path::new(path);
        let target = if p.is_relative() {
            self.browser.current_dir.join(p)
        } else {
            p.to_path_buf()
        };
        if target.is_dir() {
            self.log(&format!("[sys] Changed directory to: {}", target.display()));
            self.save_current_services();
            for s in &mut self.services {
                s.stop();
            }
            self.browser.change_dir(&target);
            self.restore_services();
            self.service_selected = 0;
        }
    }

    pub fn switch_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Normal => Mode::Command,
            Mode::Command => {
                self.command.clear();
                Mode::Normal
            }
        };
    }

    pub fn push_char(&mut self, c: char) {
        if self.mode == Mode::Command {
            self.command.push(c);
        }
    }

    pub fn pop_char(&mut self) {
        if self.mode == Mode::Command {
            self.command.pop();
        }
    }

    pub fn open_log(&self) {
        let path = crate::logger::Logger::latest_log_path();
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener)
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    pub fn restore_or_detect(&mut self) {
        let fresh = detect::detect_services_for(&self.browser.current_dir);
        self.services = match self.service_cache.remove(&self.browser.current_dir) {
            Some(cached) => fresh
                .into_iter()
                .map(|mut s| {
                    if let Some((status, log)) = cached.get(&s.name) {
                        let st = status.clone();
                        s.status = st;
                        if let Ok(mut l) = s.log.lock() {
                            *l = log.clone();
                        }
                    }
                    s
                })
                .collect(),
            None => fresh,
        };
    }

    pub fn refresh_services(&mut self) {
        let mut events: Vec<(String, Status)> = Vec::new();
        for service in &mut self.services {
            let before = service.status.clone();
            service.refresh();
            if service.status != before {
                events.push((service.name.clone(), service.status.clone()));
            }
        }
        for (name, status) in events {
            match status {
                Status::Running => self.log(&format!("[sys] Running: {}", name)),
                Status::Stopped => self.log(&format!("[sys] Stopped: {}", name)),
                Status::Failed(e) => self.log(&format!("[err] {} exited ({})", name, e)),
                Status::Starting => {},
                Status::Stopping => self.log(&format!("[sys] Stopping: {}", name)),
            }
        }
    }
}
