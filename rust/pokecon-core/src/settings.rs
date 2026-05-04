use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error("Config directory not found")]
    ConfigDirNotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SerialSettings {
    pub port: String,
    pub baud_rate: u32,
    pub timeout_ms: u64,
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud_rate: 9600,
            timeout_ms: 1000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CameraSettings {
    pub device_index: i32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            device_index: 0,
            width: 1280,
            height: 720,
            fps: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct NotifySettings {
    pub discord_webhook_url: Option<String>,
    pub line_access_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub serial: SerialSettings,
    pub camera: CameraSettings,
    pub notify: NotifySettings,
    pub script_dir: PathBuf,
    pub profile_dir: PathBuf,
    pub auto_connect: bool,
    pub log_level: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            serial: SerialSettings::default(),
            camera: CameraSettings::default(),
            notify: NotifySettings::default(),
            script_dir: PathBuf::from("scripts"),
            profile_dir: PathBuf::from("profiles"),
            auto_connect: false,
            log_level: "info".to_string(),
        }
    }
}

impl Settings {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, SettingsError> {
        let content = std::fs::read_to_string(path)?;
        let settings: Self = toml::from_str(&content)?;
        Ok(settings)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), SettingsError> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn config_path() -> Result<PathBuf, SettingsError> {
        let dir = dirs::config_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .ok_or(SettingsError::ConfigDirNotFound)?;
        Ok(dir.join("pokecon").join("settings.toml"))
    }

    pub fn load_default() -> Result<Self, SettingsError> {
        let path = Self::config_path()?;
        if path.exists() {
            Self::load(&path)
        } else {
            let settings = Self::default();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            settings.save(&path)?;
            Ok(settings)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_roundtrip() {
        let mut file = NamedTempFile::new().unwrap();
        let original = Settings {
            serial: SerialSettings {
                port: "/dev/ttyUSB0".to_string(),
                baud_rate: 115200,
                timeout_ms: 500,
            },
            camera: CameraSettings {
                device_index: 1,
                width: 1920,
                height: 1080,
                fps: 60,
            },
            notify: NotifySettings {
                discord_webhook_url: Some("https://discord.com/api/webhooks/xxx".to_string()),
                line_access_token: None,
            },
            script_dir: PathBuf::from("custom_scripts"),
            profile_dir: PathBuf::from("custom_profiles"),
            auto_connect: true,
            log_level: "debug".to_string(),
        };

        let toml_str = toml::to_string_pretty(&original).unwrap();
        file.write_all(toml_str.as_bytes()).unwrap();

        let loaded = Settings::load(file.path()).unwrap();
        assert_eq!(original, loaded);
    }

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.serial.baud_rate, 9600);
        assert_eq!(settings.camera.width, 1280);
        assert!(!settings.auto_connect);
        assert_eq!(settings.log_level, "info");
    }

    #[test]
    fn test_load_nonexistent() {
        let result = Settings::load("/nonexistent/path/settings.toml");
        assert!(result.is_err());
    }
}
