use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn ts_short() -> String {
    let total = epoch_secs();
    let h = (total % 86400) / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

pub struct Logger {
    file: fs::File,
    path: PathBuf,
}

fn log_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(home).join(".rudder").join("logs");
    let _ = fs::create_dir_all(&dir);
    dir
}

impl Logger {
    pub fn new() -> Self {
        let dir = log_dir();
        let path = dir.join(format!("{}.log", epoch_secs()));
        let file = fs::File::create(&path).unwrap_or_else(|_| {
            eprintln!("[rudder] Failed to create log file: {}", path.display());
            std::process::exit(1);
        });
        let latest = dir.join("latest.log");
        let _ = std::fs::remove_file(&latest);
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(&path, &latest);
        #[cfg(windows)]
        let _ = std::fs::copy(&path, &latest);
        #[cfg(not(any(unix, windows)))]
        let _ = std::fs::copy(&path, &latest);
        Self { file, path }
    }

    pub fn log(&mut self, msg: &str) {
        let line = format!("{} {}\n", ts_short(), msg);
        let _ = self.file.write_all(line.as_bytes());
        let _ = self.file.flush();
    }

    pub fn latest_log_path() -> PathBuf {
        log_dir().join("latest.log")
    }

    pub fn export_to(&mut self, dest: &std::path::Path) -> Result<(), String> {
        let _ = self.file.flush();
        fs::copy(&self.path, dest).map_err(|e| format!("Failed to export log: {}", e))?;
        Ok(())
    }
}
