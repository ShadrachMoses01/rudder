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
    Stopping,
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
    match shlex::split(cmd) {
        Some(parts) if !parts.is_empty() => {
            let executable = parts[0].clone();
            let args = parts[1..].to_vec();
            (executable, args)
        }
        _ => (String::new(), vec![]),
    }
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

    pub fn child_pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    pub fn cmd_string(&self) -> String {
        let mut cmd = self.executable.clone();
        for arg in &self.args {
            cmd.push(' ');
            if arg.contains(' ') {
                cmd.push_str(&format!("'{}'", arg));
            } else {
                cmd.push_str(arg);
            }
        }
        cmd
    }

    pub fn start(&mut self, cwd: &Path) {
        if self.status == Status::Running
            || self.status == Status::Starting
            || self.status == Status::Stopping
        {
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
        if !has_ext && which::which(exe).is_err() {
            let msg = format!("Executable not found: {} (not in PATH)", exe);
            if let Ok(mut log) = self.log.lock() {
                log.push(format!("[err] {}", msg));
            }
            self.status = Status::Failed(msg);
            return;
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
            kill_process_group(pgid);
            // Reap to prevent zombie
            let _ = child.wait();
        }
        self.status = Status::Stopped;
        if let Ok(mut log) = self.log.lock() {
            log.push("[sys] Stopped".into());
        }
    }

    /// Initiate async shutdown: set Stopping, kill process group in background.
    /// The caller must call `refresh()` to reap the child once it exits.
    pub fn stop_background(&mut self) {
        if let Some(ref child) = self.child {
            let pgid = child.id();
            self.status = Status::Stopping;
            if let Ok(mut log) = self.log.lock() {
                log.push("[sys] Stopping...".into());
            }
            std::thread::spawn(move || {
                kill_process_group(pgid);
            });
        } else {
            self.status = Status::Stopped;
            if let Ok(mut log) = self.log.lock() {
                log.push("[sys] Stopped".into());
            }
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
                    match self.status {
                        Status::Stopping => {
                            self.status = Status::Stopped;
                        }
                        _ if status.success() && self.status != Status::Starting => {
                            self.status = Status::Stopped;
                        }
                        _ => {
                            let code_desc = status
                                .code()
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "signal".into());
                            self.status = Status::Failed(format!("exit code {}", code_desc));
                        }
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

/// Check if a process exists by sending signal 0
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Kill a process group gracefully: SIGTERM → wait 3s → SIGKILL
///
/// Returns `true` if the process was alive and we attempted to kill it.
/// Returns `false` if the process was already gone.
pub fn kill_process_group(pgid: u32) -> bool {
    if !process_exists(pgid) {
        return false;
    }

    let kill = |sig: &str| {
        let _ = std::process::Command::new("kill")
            .arg(sig)
            .arg(format!("-{}", pgid))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    };

    // SIGTERM — graceful shutdown
    kill("-15");

    // Wait up to 3 seconds for graceful exit
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if !process_exists(pgid) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // SIGKILL if still alive
    if process_exists(pgid) {
        kill("-9");
    }
    true
}

fn pid_dir(project_dir: &Path) -> std::path::PathBuf {
    project_dir.join(".rudder").join("pids")
}

/// Write a PID file for a running service
pub fn write_pid(project_dir: &Path, name: &str, pid: u32) {
    let dir = pid_dir(project_dir);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(name), pid.to_string());
}

/// Read all PID files for a project directory
pub fn read_pids(project_dir: &Path) -> Vec<(String, u32)> {
    let dir = pid_dir(project_dir);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let pid: u32 = std::fs::read_to_string(e.path())
                .ok()?
                .trim()
                .parse()
                .ok()?;
            Some((name, pid))
        })
        .collect()
}

/// Remove all PID files for a project directory
pub fn clear_pids(project_dir: &Path) {
    let dir = pid_dir(project_dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: spawn a sleep process and attach it to a Service.
    fn service_with_sleep(dir: &Path) -> Service {
        let mut s = Service::new("test", "sleep 60");
        s.start(dir);
        // give the process a moment to spawn
        std::thread::sleep(std::time::Duration::from_millis(50));
        // after spawn, refresh confirms Running (or at least Starting becomes Running)
        s.refresh();
        s
    }

    #[test]
    fn stop_background_sets_stopping_when_child_exists() {
        let dir = std::env::temp_dir();
        let mut s = service_with_sleep(&dir);
        assert!(matches!(s.status, Status::Running | Status::Starting),
            "expected Running or Starting, got {:?}", s.status);

        s.stop_background();

        assert_eq!(s.status, Status::Stopping,
            "stop_background() should set Stopping");
        // child is NOT taken — it's still held for refresh() to reap
        assert!(s.child.is_some(), "child must remain for refresh()");
    }

    #[test]
    fn stop_background_sets_stopped_when_no_child() {
        let mut s = Service::new("ghost", "echo hi");
        assert_eq!(s.status, Status::Stopped);

        s.stop_background();

        assert_eq!(s.status, Status::Stopped,
            "stop_background() with no child stays Stopped");
    }

    #[test]
    fn refresh_reaps_stopping_child() {
        let dir = std::env::temp_dir();
        let mut s = service_with_sleep(&dir);
        assert!(s.child.is_some());

        s.stop_background();
        assert_eq!(s.status, Status::Stopping);

        // Give the background kill thread time to deliver SIGTERM + 3s wait + SIGKILL
        // and the OS enough time to reap
        std::thread::sleep(std::time::Duration::from_millis(3500));

        s.refresh();

        assert_eq!(s.status, Status::Stopped,
            "refresh() should reap dead child during Stopping");
        assert!(s.child.is_none(), "child should be None after reaping");
    }

    #[test]
    fn start_refused_during_stopping() {
        let dir = std::env::temp_dir();
        let mut s = service_with_sleep(&dir);
        s.stop_background();
        assert_eq!(s.status, Status::Stopping);

        // Attempt to start — should be a no-op
        s.start(&dir);

        assert_eq!(s.status, Status::Stopping,
            "start() must not override Stopping");
        assert!(s.child.is_some(), "child must remain for refresh()");
    }

    #[test]
    fn stop_background_does_not_block_on_kill() {
        let mut s = Service::new("blocker", "sleep 120");
        let dir = std::env::temp_dir();
        s.start(&dir);
        std::thread::sleep(std::time::Duration::from_millis(100));
        s.refresh();
        assert!(s.child.is_some(), "process should have spawned");

        let pgid = s.child.as_ref().map(|c| c.id()).unwrap();
        assert!(process_exists(pgid), "process should be alive before stop");

        let t0 = std::time::Instant::now();
        s.stop_background();
        let elapsed = t0.elapsed();

        // stop_background() must return in well under 500ms.
        // If it blocked on the full SIGTERM → 3s → SIGKILL sequence,
        // elapsed would be >3s.
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "stop_background() blocked for {:?}, expected <500ms",
            elapsed
        );
        assert_eq!(s.status, Status::Stopping);

        // The kill thread runs async — wait for it to finish.
        std::thread::sleep(std::time::Duration::from_millis(3500));
        s.refresh();

        assert_eq!(s.status, Status::Stopped,
            "process should be dead and reaped after background kill");
        assert!(s.child.is_none());
    }

    #[test]
    fn full_lifecycle_start_stopbackground_refresh() {
        let dir = std::env::temp_dir();
        let mut s = service_with_sleep(&dir);
        assert!(matches!(s.status, Status::Running | Status::Starting));

        s.stop_background();
        assert_eq!(s.status, Status::Stopping);
        assert!(s.child.is_some());

        // Wait for kill + let refresh() reap
        std::thread::sleep(std::time::Duration::from_millis(3500));
        s.refresh();

        assert_eq!(s.status, Status::Stopped,
            "full lifecycle should end at Stopped");
        assert!(s.child.is_none(), "child should be reaped");
    }

    #[test]
    fn kill_process_group_kills_real_process() {
        // Spawn sleep as an independent process
        let mut child = Command::new("sleep")
            .arg("90")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("failed to spawn sleep");

        let pgid = child.id();
        assert!(process_exists(pgid), "process should be alive");

        let killed = kill_process_group(pgid);
        assert!(killed, "kill_process_group should return true");

        // Reap to prevent zombie
        let _ = child.wait();

        assert!(!process_exists(pgid), "process should be dead after kill");
    }
}
