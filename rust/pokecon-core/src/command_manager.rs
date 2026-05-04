use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Command not found: {0}")]
    NotFound(String),
    #[error("Command already loaded: {0}")]
    AlreadyLoaded(String),
    #[error("Invalid command path: {0}")]
    InvalidPath(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandInfo {
    pub name: String,
    pub path: PathBuf,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommandManager {
    script_dir: PathBuf,
    commands: HashMap<String, CommandInfo>,
    active: Option<String>,
}

impl CommandManager {
    pub fn new<P: AsRef<Path>>(script_dir: P) -> Result<Self, CommandError> {
        let path = script_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;
        Ok(Self {
            script_dir: path,
            commands: HashMap::new(),
            active: None,
        })
    }

    pub fn scan(&mut self) -> Result<Vec<String>, CommandError> {
        let mut found = Vec::new();
        if !self.script_dir.exists() {
            return Ok(found);
        }

        for entry in std::fs::read_dir(&self.script_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|s| s.to_str());
                if ext == Some("py") || ext == Some("lua") {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if !name.is_empty() {
                        let info = CommandInfo {
                            name: name.clone(),
                            path: path.clone(),
                            description: Self::extract_description(&path),
                        };
                        self.commands.insert(name.clone(), info);
                        found.push(name);
                    }
                }
            }
        }
        Ok(found)
    }

    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<String, CommandError> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(CommandError::InvalidPath(path));
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            return Err(CommandError::InvalidPath(path));
        }

        if self.commands.contains_key(&name) {
            return Err(CommandError::AlreadyLoaded(name));
        }

        let info = CommandInfo {
            name: name.clone(),
            path: path.clone(),
            description: Self::extract_description(&path),
        };
        self.commands.insert(name.clone(), info);
        Ok(name)
    }

    pub fn unload(&mut self, name: &str) -> Result<(), CommandError> {
        if self.commands.remove(name).is_none() {
            return Err(CommandError::NotFound(name.to_string()));
        }
        if self.active.as_deref() == Some(name) {
            self.active = None;
        }
        Ok(())
    }

    pub fn reload(&mut self, name: &str) -> Result<(), CommandError> {
        let info = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::NotFound(name.to_string()))?;
        let path = info.path.clone();
        let description = Self::extract_description(&path);

        let info = CommandInfo {
            name: name.to_string(),
            path,
            description,
        };
        self.commands.insert(name.to_string(), info);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&CommandInfo> {
        self.commands.get(name)
    }

    pub fn list(&self) -> Vec<&CommandInfo> {
        self.commands.values().collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.commands.keys().cloned().collect()
    }

    pub fn set_active(&mut self, name: &str) -> Result<(), CommandError> {
        if !self.commands.contains_key(name) {
            return Err(CommandError::NotFound(name.to_string()));
        }
        self.active = Some(name.to_string());
        Ok(())
    }

    pub fn active(&self) -> Option<&CommandInfo> {
        self.active
            .as_ref()
            .and_then(|name| self.commands.get(name))
    }

    pub fn active_name(&self) -> Option<&str> {
        self.active.as_deref()
    }

    fn extract_description(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        content
            .lines()
            .find(|line| {
                line.trim_start().starts_with("# Description:")
                    || line.trim_start().starts_with("-- Description:")
            })
            .map(|line| line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_scan_scripts() {
        let dir = TempDir::new().unwrap();
        let script_dir = dir.path().join("scripts");
        std::fs::create_dir(&script_dir).unwrap();

        let mut file = std::fs::File::create(script_dir.join("test.py")).unwrap();
        writeln!(file, "# Description: Test script").unwrap();
        writeln!(file, "print('hello')").unwrap();

        let mut manager = CommandManager::new(&script_dir).unwrap();
        let names = manager.scan().unwrap();
        assert_eq!(names, vec!["test"]);
        assert_eq!(
            manager.get("test").unwrap().description,
            Some("Test script".to_string())
        );
    }

    #[test]
    fn test_load_and_unload() {
        let dir = TempDir::new().unwrap();
        let script_dir = dir.path().join("scripts");
        std::fs::create_dir(&script_dir).unwrap();

        let path = script_dir.join("hello.py");
        std::fs::write(&path, "print('hello')").unwrap();

        let mut manager = CommandManager::new(&script_dir).unwrap();
        let name = manager.load(&path).unwrap();
        assert_eq!(name, "hello");
        assert!(manager.get("hello").is_some());

        manager.unload("hello").unwrap();
        assert!(manager.get("hello").is_none());
    }

    #[test]
    fn test_active() {
        let dir = TempDir::new().unwrap();
        let script_dir = dir.path().join("scripts");
        std::fs::create_dir(&script_dir).unwrap();

        let path = script_dir.join("active.py");
        std::fs::write(&path, "# Description: Active script\n").unwrap();

        let mut manager = CommandManager::new(&script_dir).unwrap();
        manager.load(&path).unwrap();
        manager.set_active("active").unwrap();

        assert_eq!(manager.active_name(), Some("active"));
        assert_eq!(
            manager.active().unwrap().description,
            Some("Active script".to_string())
        );
    }

    #[test]
    fn test_reload() {
        let dir = TempDir::new().unwrap();
        let script_dir = dir.path().join("scripts");
        std::fs::create_dir(&script_dir).unwrap();

        let path = script_dir.join("reload.py");
        std::fs::write(&path, "# Description: Old\n").unwrap();

        let mut manager = CommandManager::new(&script_dir).unwrap();
        manager.load(&path).unwrap();

        std::fs::write(&path, "# Description: New\n").unwrap();
        manager.reload("reload").unwrap();

        assert_eq!(
            manager.get("reload").unwrap().description,
            Some("New".to_string())
        );
    }
}
