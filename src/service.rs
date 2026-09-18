use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use regex::Regex;

const MAX_LOG_LINES: usize = 500;
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const SHUTDOWN_POLL: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed(String),
}

pub type ServiceId = String;

fn compute_id(name: &str, dir: Option<&str>) -> ServiceId {
    match dir {
        Some(d) if !d.is_empty() => format!("{}::{}", d, name),
        _ => name.to_string(),
    }
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::\d+)?(?:/\S*)?")
            .expect("Invalid URL regex")
    })
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

/// Parse a command string into (program, args[]).
/// Uses shlex for POSIX-like tokenization. Does NOT support shell syntax
/// (pipes, redirects, env vars, chaining). For shell features, execute
/// via `sh -c "..."` or `cmd /C "..."`.
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

pub struct Service {
    pub id: ServiceId,
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub dir: Option<String>,
    pub url: Option<String>,
    pub status: Status,
    pub log: Arc<Mutex<Vec<String>>>,
    stopping: Arc<AtomicBool>,
    port_warn_emitted: bool,
    child: Option<Child>,
}

impl Service {
    pub fn new(name: &str, cmd: &str) -> Self {
        let (executable, args) = split_cmd(cmd);
        Self {
            id: compute_id(name, None),
            name: name.to_string(),
            executable,
            args,
            dir: None,
            url: None,
            status: Status::Stopped,
            log: Arc::new(Mutex::new(vec!["Waiting to start...".into()])),
            stopping: Arc::new(AtomicBool::new(false)),
            port_warn_emitted: false,
            child: None,
        }
    }

    pub fn new_with_dir(name: &str, cmd: &str, dir: &str) -> Self {
        let (executable, args) = split_cmd(cmd);
        Self {
            id: compute_id(name, Some(dir)),
            name: name.to_string(),
            executable,
            args,
            dir: Some(dir.to_string()),
            url: None,
            status: Status::Stopped,
            log: Arc::new(Mutex::new(vec!["Waiting to start...".into()])),
            stopping: Arc::new(AtomicBool::new(false)),
            port_warn_emitted: false,
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

    /// Regenerate the stable ID based on current name and dir.
    /// Call after modifying `name` or `dir`.
    pub fn update_id(&mut self) {
        self.id = compute_id(&self.name, self.dir.as_deref());
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

        #[cfg(unix)]
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

                self.stopping.store(false, Ordering::Relaxed);
                self.port_warn_emitted = false;
                self.url = None;
                if let Ok(mut log) = self.log.lock() {
                    log.clear();
                    log.push("[sys] Started".into());
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
            #[cfg(unix)]
            kill_process_group(child.id());
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        self.status = Status::Stopped;
        if let Ok(mut log) = self.log.lock() {
            log.push("[sys] Stopped".into());
        }
    }

    pub fn stop_background(&mut self) {
        if let Some(ref child) = self.child {
            self.status = Status::Stopping;
            self.stopping.store(true, Ordering::Relaxed);
            if let Ok(mut log) = self.log.lock() {
                log.push("[sys] Stopping...".into());
            }
            #[cfg(unix)]
            {
                let pgid = child.id();
                thread::spawn(move || {
                    kill_process_group(pgid);
                });
            }
            #[cfg(not(unix))]
            {
                let pid = child.id();
                thread::spawn(move || {
                    let _ = std::process::Command::new("taskkill")
                        .arg("/F")
                        .arg("/PID")
                        .arg(pid.to_string())
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                });
            }
        } else {
            self.status = Status::Stopped;
            if let Ok(mut log) = self.log.lock() {
                log.push("[sys] Stopped".into());
            }
        }
    }

    pub fn refresh(&mut self) {
        if let Ok(mut log) = self.log.lock() {
            let excess = log.len().saturating_sub(MAX_LOG_LINES);
            if excess > 0 {
                log.drain(0..excess);
            }

            for line in log.iter().rev() {
                if !self.port_warn_emitted
                    && (line.contains("port already in use")
                        || line.contains("EADDRINUSE")
                        || line.contains("Address already in use"))
                {
                    self.port_warn_emitted = true;
                }
                if self.url.is_none() {
                    if let Some(caps) = url_regex().captures(line) {
                        if let Some(m) = caps.get(0) {
                            self.url = Some(m.as_str().trim_end_matches('/').to_string());
                            break;
                        }
                    }
                }
            }
            if self.port_warn_emitted {
                let already = log.iter().any(|l| l.contains("[warn] Port in use"));
                if !already {
                    log.push("[warn] Port in use — service may be on a different port".into());
                }
            }
        }

        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    let msg = if status.success() {
                        "[sys] Process exited (code 0)".to_string()
                    } else if let Some(code) = status.code() {
                        format!("[sys] Process exited (code {})", code)
                    } else {
                        "[sys] Process killed by signal".to_string()
                    };
                    match self.status {
                        Status::Stopping => {
                            self.status = Status::Stopped;
                        }
                        _ if self.stopping.load(Ordering::Relaxed) => {
                            self.status = Status::Stopped;
                        }
                        _ if status.success() => {
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

#[cfg(unix)]
pub fn process_exists(pid: u32) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let state = String::from_utf8_lossy(&o.stdout).trim().chars().next();
            state.is_some() && state != Some('Z')
        }
        _ => false,
    }
}

#[cfg(not(unix))]
pub fn process_exists(pid: u32) -> bool {
    // `tasklist` exits 0 even when the filter matches nothing ("INFO: No tasks"),
    // so check stdout for the PID instead of relying on the exit status.
    let out = std::process::Command::new("tasklist")
        .arg("/FI")
        .arg(format!("PID eq {}", pid))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            text.contains(&pid.to_string())
        }
        Err(_) => false,
    }
}

#[cfg(unix)]
pub fn kill_process_group(pgid: u32) -> bool {
    if !process_exists(pgid) {
        return false;
    }

    let kill = |sig: &str| {
        let _ = std::process::Command::new("kill")
            .arg(sig)
            .arg(format!("-{}", pgid))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };

    kill("-15");

    let deadline = std::time::Instant::now() + SHUTDOWN_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if !process_exists(pgid) {
            return true;
        }
        std::thread::sleep(SHUTDOWN_POLL);
    }

    if process_exists(pgid) {
        kill("-9");
        let deadline = std::time::Instant::now() + SHUTDOWN_TIMEOUT;
        while std::time::Instant::now() < deadline && process_exists(pgid) {
            std::thread::sleep(SHUTDOWN_POLL);
        }
        return !process_exists(pgid);
    }
    true
}

fn pid_dir(project_dir: &Path) -> std::path::PathBuf {
    project_dir.join(".rudder").join("pids")
}

pub fn write_pid(project_dir: &Path, name: &str, pid: u32) {
    let dir = pid_dir(project_dir);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(name), pid.to_string());
}

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

pub fn clear_pids(project_dir: &Path) {
    let dir = pid_dir(project_dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service_with_sleep(dir: &Path) -> Service {
        let mut s = Service::new("test", "sleep 60");
        s.start(dir);
        std::thread::sleep(std::time::Duration::from_millis(50));
        s.refresh();
        s
    }

    #[test]
    fn stop_background_sets_stopping_when_child_exists() {
        let dir = std::env::temp_dir();
        let mut s = service_with_sleep(&dir);
        assert!(matches!(s.status, Status::Running | Status::Starting));

        s.stop_background();

        assert_eq!(s.status, Status::Stopping);
        assert!(s.child.is_some());
    }

    #[test]
    fn stop_background_sets_stopped_when_no_child() {
        let mut s = Service::new("ghost", "echo hi");
        assert_eq!(s.status, Status::Stopped);

        s.stop_background();

        assert_eq!(s.status, Status::Stopped);
    }

    #[test]
    fn refresh_reaps_stopping_child() {
        let dir = std::env::temp_dir();
        let mut s = service_with_sleep(&dir);
        assert!(s.child.is_some());

        s.stop_background();
        assert_eq!(s.status, Status::Stopping);

        std::thread::sleep(std::time::Duration::from_millis(3500));

        s.refresh();

        assert_eq!(s.status, Status::Stopped);
        assert!(s.child.is_none());
    }

    #[test]
    fn quick_clean_exit_marks_stopped_not_failed() {
        let dir = std::env::temp_dir();
        let mut s = Service::new("oneshot", "sh -c 'echo done'");
        s.start(&dir);
        std::thread::sleep(std::time::Duration::from_millis(500));
        s.refresh();

        assert_eq!(s.status, Status::Stopped);
        assert!(s.child.is_none());
    }

    #[test]
    fn restart_clears_stale_url_and_log() {
        let dir = std::env::temp_dir();
        let flag = dir.join("rudder-test-stale-flag");
        let _ = std::fs::remove_file(&flag);
        std::fs::write(&flag, "1").unwrap();

        let cmd = format!(
            "sh -c 'if [ -f {} ]; then echo http://localhost:3000; fi; sleep 30'",
            flag.display()
        );
        let mut s = Service::new("stale", &cmd);
        s.start(&dir);
        std::thread::sleep(std::time::Duration::from_millis(500));
        s.refresh();
        assert_eq!(s.url.as_deref(), Some("http://localhost:3000"));

        std::fs::remove_file(&flag).unwrap();
        s.stop();
        s.start(&dir);
        std::thread::sleep(std::time::Duration::from_millis(100));
        s.refresh();

        assert_eq!(s.url, None);
        let log = s.log.lock().unwrap().clone();
        assert!(!log.iter().any(|l| l.contains("localhost:3000")));
        assert!(log.iter().any(|l| l.contains("Started")));
        s.stop();
    }

    #[test]
    fn start_refused_during_stopping() {
        let dir = std::env::temp_dir();
        let mut s = service_with_sleep(&dir);
        s.stop_background();
        assert_eq!(s.status, Status::Stopping);

        s.start(&dir);

        assert_eq!(s.status, Status::Stopping);
        assert!(s.child.is_some());
    }

    #[test]
    fn stop_background_does_not_block_on_kill() {
        let mut s = Service::new("blocker", "sleep 120");
        let dir = std::env::temp_dir();
        s.start(&dir);
        std::thread::sleep(std::time::Duration::from_millis(100));
        s.refresh();
        assert!(s.child.is_some());

        let t0 = std::time::Instant::now();
        s.stop_background();
        let elapsed = t0.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "stop_background() blocked for {:?}, expected <500ms",
            elapsed
        );
        assert_eq!(s.status, Status::Stopping);

        std::thread::sleep(std::time::Duration::from_millis(3500));
        s.refresh();

        assert_eq!(s.status, Status::Stopped);
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

        std::thread::sleep(std::time::Duration::from_millis(3500));
        s.refresh();

        assert_eq!(s.status, Status::Stopped);
        assert!(s.child.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn kill_process_group_kills_real_process() {
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

        let _ = child.wait();

        assert!(!process_exists(pgid), "process should be dead after kill");
    }
}
