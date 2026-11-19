//! Where the API key is kept: `config.json` in the user's config folder, readable by them alone.
//! `TDT_API_KEY` (or `--key`) overrides it without touching the file.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Config {
    /// The folder: `$XDG_CONFIG_HOME/trackdaytimes`, else `~/.config/trackdaytimes` (macOS and Linux), or
    /// `%APPDATA%\trackdaytimes` (Windows).
    pub fn dir() -> Result<PathBuf> {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(xdg).join("trackdaytimes"));
        }
        if cfg!(windows)
            && let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty())
        {
            return Ok(PathBuf::from(appdata).join("trackdaytimes"));
        }
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("Couldn't find your home folder. Set TDT_API_KEY instead."))?;
        Ok(PathBuf::from(home).join(".config").join("trackdaytimes"))
    }

    pub fn path() -> Result<PathBuf> {
        Ok(Config::dir()?.join("config.json"))
    }

    /// The saved settings, with `TDT_API_KEY` on top.
    pub fn load() -> Result<Config> {
        let mut config = match fs::read_to_string(Config::path()?) {
            Ok(text) => serde_json::from_str(&text)
                .context("Your config file is damaged. Run `tdt login` again.")?,
            Err(_) => Config::default(),
        };
        if let Ok(key) = std::env::var("TDT_API_KEY")
            && !key.trim().is_empty()
        {
            config.api_key = Some(key.trim().to_string());
        }
        Ok(config)
    }

    pub fn save(&self) -> Result<PathBuf> {
        let dir = Config::dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("Couldn't make {}", dir.display()))?;
        let path = dir.join("config.json");
        fs::write(&path, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("Couldn't write {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(path)
    }

    /// Forgets the saved key. True when there was one.
    pub fn clear() -> Result<bool> {
        let path = Config::path()?;
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Couldn't remove {}", path.display()))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
