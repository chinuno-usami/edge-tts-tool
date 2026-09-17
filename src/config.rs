use crate::hotkey::HotkeyConfig;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub voice: String,
    pub output_device: Option<String>,
    pub rate: i32,
    pub pitch: i32,
    pub volume: i32,
    pub hotkey: HotkeyConfig,
    pub last_text: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            voice: "zh-CN-XiaoxiaoNeural".to_string(),
            output_device: None,
            rate: 0,
            pitch: 0,
            volume: 100,
            hotkey: HotkeyConfig::default(),
            last_text: "你好，欢迎使用 Edge TTS 语音合成工具！这是一个基于 Rust 开发的跨平台语音合成桌面应用。".to_string(),
        }
    }
}

impl AppConfig {
    fn config_path() -> Option<PathBuf> {
        if let Some(proj_dirs) = ProjectDirs::from("com", "tts", "edge-tts-tool") {
            let config_dir = proj_dirs.config_dir();
            let _ = fs::create_dir_all(config_dir);
            Some(config_dir.join("config.json"))
        } else {
            Some(PathBuf::from("tts_config.json"))
        }
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
                        return cfg;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path() {
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = fs::write(path, json);
            }
        }
    }
}
