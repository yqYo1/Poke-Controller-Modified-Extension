use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error("Profile not found: {0}")]
    NotFound(String),
    #[error("Profile already exists: {0}")]
    AlreadyExists(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Profile {
    pub name: String,
    pub description: Option<String>,
    pub serial_port: Option<String>,
    pub camera_index: Option<i32>,
    pub settings: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ProfileManager {
    profile_dir: PathBuf,
    profiles: HashMap<String, Profile>,
    active: Option<String>,
}

impl ProfileManager {
    pub fn new<P: AsRef<Path>>(profile_dir: P) -> Result<Self, ProfileError> {
        let path = profile_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;

        let mut manager = Self {
            profile_dir: path,
            profiles: HashMap::new(),
            active: None,
        };
        manager.load_all()?;
        Ok(manager)
    }

    pub fn load_all(&mut self) -> Result<(), ProfileError> {
        self.profiles.clear();
        if !self.profile_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.profile_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let content = std::fs::read_to_string(&path)?;
                let profile: Profile = toml::from_str(&content)?;
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&profile.name)
                    .to_string();
                self.profiles.insert(name, profile);
            }
        }
        Ok(())
    }

    pub fn save(&self, name: &str) -> Result<(), ProfileError> {
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| ProfileError::NotFound(name.to_string()))?;
        let path = self.profile_dir.join(format!("{}.toml", name));
        let content = toml::to_string_pretty(profile)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn create(&mut self, profile: Profile) -> Result<(), ProfileError> {
        if self.profiles.contains_key(&profile.name) {
            return Err(ProfileError::AlreadyExists(profile.name.clone()));
        }
        let name = profile.name.clone();
        self.profiles.insert(name.clone(), profile);
        self.save(&name)?;
        Ok(())
    }

    pub fn update(&mut self, profile: Profile) -> Result<(), ProfileError> {
        let name = profile.name.clone();
        if !self.profiles.contains_key(&name) {
            return Err(ProfileError::NotFound(name));
        }
        self.profiles.insert(profile.name.clone(), profile.clone());
        self.save(&profile.name)?;
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<(), ProfileError> {
        if self.profiles.remove(name).is_none() {
            return Err(ProfileError::NotFound(name.to_string()));
        }
        let path = self.profile_dir.join(format!("{}.toml", name));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        if self.active.as_deref() == Some(name) {
            self.active = None;
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }

    pub fn list(&self) -> Vec<&Profile> {
        self.profiles.values().collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }

    pub fn activate(&mut self, name: &str) -> Result<(), ProfileError> {
        if !self.profiles.contains_key(name) {
            return Err(ProfileError::NotFound(name.to_string()));
        }
        self.active = Some(name.to_string());
        Ok(())
    }

    pub fn active(&self) -> Option<&Profile> {
        self.active
            .as_ref()
            .and_then(|name| self.profiles.get(name))
    }

    pub fn active_name(&self) -> Option<&str> {
        self.active.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_and_load() {
        let dir = TempDir::new().unwrap();
        let mut manager = ProfileManager::new(dir.path()).unwrap();

        let profile = Profile {
            name: "test".to_string(),
            description: Some("Test profile".to_string()),
            serial_port: Some("/dev/ttyUSB0".to_string()),
            camera_index: Some(0),
            settings: [("key".to_string(), "value".to_string())]
                .into_iter()
                .collect(),
        };

        manager.create(profile.clone()).unwrap();
        assert_eq!(manager.names(), vec!["test"]);

        let manager2 = ProfileManager::new(dir.path()).unwrap();
        assert_eq!(manager2.names(), vec!["test"]);
        assert_eq!(manager2.get("test"), Some(&profile));
    }

    #[test]
    fn test_activate() {
        let dir = TempDir::new().unwrap();
        let mut manager = ProfileManager::new(dir.path()).unwrap();

        let profile = Profile {
            name: "active".to_string(),
            ..Default::default()
        };
        manager.create(profile).unwrap();
        manager.activate("active").unwrap();

        assert_eq!(manager.active_name(), Some("active"));
        assert!(manager.active().is_some());
    }

    #[test]
    fn test_delete() {
        let dir = TempDir::new().unwrap();
        let mut manager = ProfileManager::new(dir.path()).unwrap();

        let profile = Profile {
            name: "delete_me".to_string(),
            ..Default::default()
        };
        manager.create(profile).unwrap();
        manager.activate("delete_me").unwrap();

        manager.delete("delete_me").unwrap();
        assert!(manager.get("delete_me").is_none());
        assert!(manager.active().is_none());
    }
}
