use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Double,
}

impl Default for MouseButton {
    fn default() -> Self {
        Self::Left
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PositionMode {
    FollowCursor,
    Fixed,
}

impl Default for PositionMode {
    fn default() -> Self {
        Self::FollowCursor
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickConfig {
    pub interval_ms: u64,
    pub button: MouseButton,
    pub position_mode: PositionMode,
    pub fixed_x: i32,
    pub fixed_y: i32,
    pub click_limit: Option<u64>,
    pub hotkey_code: u16,
    pub capture_hotkey_code: u16,
}

impl Default for ClickConfig {
    fn default() -> Self {
        Self {
            interval_ms: 1000,
            button: MouseButton::Left,
            position_mode: PositionMode::FollowCursor,
            fixed_x: 960,
            fixed_y: 540,
            click_limit: None,
            hotkey_code: 64,         // KEY_F6
            capture_hotkey_code: 65, // KEY_F7
        }
    }
}

impl ClickConfig {
    fn config_path() -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join(".config")
            .join("vibeclicker")
            .join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
}


pub const HOTKEY_OPTIONS: &[(u16, &str)] = &[
    (59, "F1"),
    (60, "F2"),
    (61, "F3"),
    (62, "F4"),
    (63, "F5"),
    (64, "F6"),
    (65, "F7"),
    (66, "F8"),
    (67, "F9"),
    (68, "F10"),
    (87, "F11"),
    (88, "F12"),
];
