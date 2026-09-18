use std::fs;
use std::path::{Component, Path, PathBuf};

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    let mut is_absolute = false;
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => {
                result = PathBuf::from(c.as_os_str());
                is_absolute = true;
            }
            Component::CurDir => {}
            Component::Normal(c) => result.push(c),
            Component::ParentDir => {
                if !result.pop() && !is_absolute {
                    result.push("..");
                }
            }
        }
    }
    result
}

#[derive(Clone)]
pub struct FsEntry {
    pub name: String,
    pub is_dir: bool,
}

pub struct FileBrowser {
    pub current_dir: PathBuf,
    pub entries: Vec<FsEntry>,
    pub selected: usize,
    pub error: Option<String>,
}

impl FileBrowser {
    pub fn new() -> Self {
        let raw = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let current_dir = normalize(&raw);
        let mut browser = Self {
            current_dir,
            entries: vec![],
            selected: 0,
            error: None,
        };
        browser.refresh();
        browser
    }

    pub fn refresh(&mut self) {
        let (entries, err) = read_dir_entries(&self.current_dir);
        self.entries = entries;
        self.error = err;
        if !self.entries.is_empty() && self.selected >= self.entries.len() {
            self.selected = self.entries.len() - 1;
        }
    }

    pub fn enter_selected(&mut self) -> bool {
        let entry = match self.entries.get(self.selected) {
            Some(e) => e.clone(),
            None => return false,
        };
        if !entry.is_dir {
            return false;
        }
        if entry.name == ".." {
            self.go_to_parent()
        } else {
            self.current_dir.push(&entry.name);
            self.current_dir = normalize(&self.current_dir);
            self.selected = 0;
            self.refresh();
            true
        }
    }

    pub fn go_to_parent(&mut self) -> bool {
        let dir_name = self
            .current_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string());

        if !self.current_dir.pop() {
            return false;
        }

        self.current_dir = normalize(&self.current_dir);
        self.selected = 0;
        self.refresh();

        if let Some(ref name) = dir_name {
            if let Some(idx) = self
                .entries
                .iter()
                .position(|e| e.name == *name && e.is_dir)
            {
                self.selected = idx;
            }
        }

        true
    }

    pub fn change_dir(&mut self, path: &Path) {
        if path.exists() && path.is_dir() {
            self.current_dir = normalize(path);
            self.selected = 0;
            self.refresh();
        }
    }

    pub fn previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn next(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }
}

fn read_dir_entries(path: &Path) -> (Vec<FsEntry>, Option<String>) {
    let read_dir = match fs::read_dir(path) {
        Ok(r) => r,
        Err(e) => {
            let msg = match e.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    format!("Permission denied: {}", path.display())
                }
                std::io::ErrorKind::NotFound => format!("Path does not exist: {}", path.display()),
                _ => format!("Failed to read directory: {} ({})", path.display(), e),
            };
            return (vec![], Some(msg));
        }
    };

    let mut entries: Vec<FsEntry> = read_dir
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| FsEntry {
            name: e.file_name().to_string_lossy().to_string(),
            is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
        })
        .collect();

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    if path.parent().is_some() {
        entries.insert(
            0,
            FsEntry {
                name: "..".into(),
                is_dir: true,
            },
        );
    }

    (entries, None)
}
