use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Stopped,
    Starting,
    Running,
    Failed(String),
}

pub struct Service {
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub dir: Option<String>,
    pub url: Option<String>,
    pub status: Status,
    pub log: Arc<Mutex<Vec<String>>>,
    child: Option<Child>,
}

fn split_cmd(cmd: &str) -> (String, Vec<String>) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let executable = parts.first().map(|s| s.to_string()).unwrap_or_default();
    let args = parts[1..].iter().map(|s| s.to_string()).collect();
    (executable, args)
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            while let Some(n) = chars.next() {
                if n.is_ascii_alphabetic() || n == '~' {
                    break;
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

impl Service {
    pub fn new(name: &str, cmd: &str) -> Self {
        let (executable, args) = split_cmd(cmd);
        Self {
            name: name.to_string(),
            executable,
            args,
            dir: None,
            url: None,
            status: Status::Stopped,
            log: Arc::new(Mutex::new(vec!["Waiting to start...".into()])),
            child: None,
        }
    }

    pub fn new_with_dir(name: &str, cmd: &str, dir: &str) -> Self {
        let (executable, args) = split_cmd(cmd);
        Self {
            name: name.to_string(),
            executable,
            args,
            dir: Some(dir.to_string()),
            url: None,
            status: Status::Stopped,
            log: Arc::new(Mutex::new(vec!["Waiting to start...".into()])),
            child: None,
        }
    }

    pub fn start(&mut self, cwd: &Path) {
        if self.status == Status::Running || self.status == Status::Starting {
            return;
        }

        let dir = match &self.dir {
            Some(sub) => cwd.join(sub),
            None => cwd.to_path_buf(),
        };

        if !dir.exists() {
            let msg = format!("Working directory does not exist: {}", dir.display());
            if let Ok(mut log) = self.log.lock() {
                log.push(format!("[err] {}", msg));
            }
            self.status = Status::Failed(msg);
            return;
        }

        let exe = &self.executable;
        let has_ext = exe.contains('.') || exe.contains('/');
        if !has_ext {
            let in_path = std::process::Command::new("which")
                .arg(exe)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !in_path {
                let msg = format!("Executable not found: {} (not in PATH)", exe);
                if let Ok(mut log) = self.log.lock() {
                    log.push(format!("[err] {}", msg));
                }
                self.status = Status::Failed(msg);
                return;
            }
        }

        let mut command = Command::new(exe);
        command
            .args(&self.args)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        command.env("TERM", "dumb");
        command.process_group(0);

        match command.spawn() {
            Ok(mut child) => {
                let log_out = self.log.clone();
                let log_err = self.log.clone();

                if let Some(stdout) = child.stdout.take() {
                    thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            let text = match line {
                                Ok(t) => t,
                                Err(_) => break,
                            };
                            if let Ok(mut log) = log_out.lock() {
                                log.push(strip_ansi(&text));
                            }
                        }
                    });
                }

                if let Some(stderr) = child.stderr.take() {
                    thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            let text = match line {
                                Ok(t) => t,
                                Err(_) => break,
                            };
                            if let Ok(mut log) = log_err.lock() {
                                log.push(strip_ansi(&text));
                            }
                        }
                    });
                }

                self.status = Status::Starting;
                self.child = Some(child);
            }
            Err(e) => {
                let msg = match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        format!("Executable not found: {}", exe)
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        format!("Permission denied: {}", exe)
                    }
                    _ => format!("Failed to spawn: {} ({})", exe, e),
                };
                if let Ok(mut log) = self.log.lock() {
                    log.push(format!("[err] {}", msg));
                }
                self.status = Status::Failed(msg);
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pgid = child.id();
            // Kill entire process group (child + all descendants)
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(format!("-{}", pgid))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            // Reap to prevent zombie
            let _ = child.wait();
        }
        self.status = Status::Stopped;
        if let Ok(mut log) = self.log.lock() {
            log.push("[sys] Stopped".into());
        }
    }

    pub fn refresh(&mut self) {
        if let Ok(mut log) = self.log.lock() {
            let excess = log.len().saturating_sub(500);
            if excess > 0 {
                log.drain(0..excess);
            }
            let mut port_warn = false;
            for line in log.iter().rev() {
                if !port_warn
                    && (line.contains("port already in use")
                        || line.contains("EADDRINUSE")
                        || line.contains("Address already in use"))
                {
                    port_warn = true;
                }
                if let Some(start) = line.find("http://localhost:") {
                    let url = line[start..].trim().trim_end_matches('/').to_string();
                    self.url = Some(url);
                    break;
                }
            }
            if port_warn {
                log.push("[warn] Port in use — service may be on a different port".into());
            }
        }
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    let msg = if status.success() {
                        format!("[sys] Process exited (code 0)")
                    } else if let Some(code) = status.code() {
                        format!("[sys] Process exited (code {})", code)
                    } else {
                        format!("[sys] Process killed by signal")
                    };
                    if status.success() && self.status != Status::Starting {
                        self.status = Status::Stopped;
                    } else {
                        let code_desc = status
                            .code()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "signal".into());
                        self.status = Status::Failed(format!("exit code {}", code_desc));
                    }
                    if let Ok(mut log) = self.log.lock() {
                        log.push(msg);
                    }
                }
                Ok(None) => {
                    if self.status == Status::Starting {
                        self.status = Status::Running;
                    }
                }
                Err(e) => {
                    self.child = None;
                    self.status = Status::Failed(format!("process error: {}", e));
                    if let Ok(mut log) = self.log.lock() {
                        log.push(format!("[err] {}", e));
                    }
                }
            }
        }
    }
}
