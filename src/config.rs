use crate::hotkey::HotkeyConfig;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn default_edge_voice() -> String {
    "zh-CN-XiaoxiaoNeural".to_string()
}

fn default_openai_base_url() -> String {
    "https://api.openai.com".to_string()
}

fn default_openai_model() -> String {
    "tts-1".to_string()
}

fn default_openai_voice() -> String {
    "alloy".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TtsSource {
    #[default]
    Edge,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAiConfig {
    #[serde(default = "default_openai_base_url")]
    pub base_url: String,
    /// Optional; empty means no Authorization header (e.g. local mlx-audio).
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_openai_model")]
    pub model: String,
    /// Optional; VoiceDesign models may leave this empty and use `instruct`.
    #[serde(default = "default_openai_voice")]
    pub voice: String,
    /// Natural-language voice/style description; sent as JSON `instruct` when non-empty.
    #[serde(default)]
    pub instruct: String,
    /// Server-side path to reference audio for voice cloning (Qwen3-TTS Base / mlx-audio).
    /// Sent as JSON `ref_audio` when non-empty; the TTS server must be able to read this path.
    /// Not validated on the client (client and server may be different machines).
    #[serde(default)]
    pub ref_audio: String,
    /// Exact transcript of `ref_audio`; sent as JSON `ref_text` when non-empty.
    #[serde(default)]
    pub ref_text: String,
    /// When true, try GET /v1/audio/voices?model=...; default off.
    #[serde(default)]
    pub fetch_remote_voices: bool,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: default_openai_base_url(),
            api_key: String::new(),
            model: default_openai_model(),
            voice: default_openai_voice(),
            instruct: String::new(),
            ref_audio: String::new(),
            ref_text: String::new(),
            fetch_remote_voices: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub voice: String,
    pub output_device: Option<String>,
    pub rate: i32,
    pub pitch: i32,
    pub volume: i32,
    pub hotkey: HotkeyConfig,
    pub last_text: String,

    #[serde(default)]
    pub source: TtsSource,
    #[serde(default)]
    pub openai: OpenAiConfig,
    /// Last Edge voice; restored when switching back to Edge.
    #[serde(default = "default_edge_voice")]
    pub edge_voice: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        let edge_voice = default_edge_voice();
        Self {
            voice: edge_voice.clone(),
            output_device: None,
            rate: 0,
            pitch: 0,
            volume: 100,
            hotkey: HotkeyConfig::default(),
            last_text: "你好，欢迎使用 TTS 语音合成工具！这是一个基于 Rust 开发的跨平台语音合成桌面应用。"
                .to_string(),
            source: TtsSource::Edge,
            openai: OpenAiConfig::default(),
            edge_voice,
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

    /// Deserialize + apply migrations. Prefer this over raw `from_str` for persisted configs.
    pub fn from_json_value(value: serde_json::Value) -> Option<Self> {
        let mut cfg = serde_json::from_value::<AppConfig>(value.clone()).ok()?;
        Self::apply_migrations(&value, &mut cfg);
        Some(cfg)
    }

    fn apply_migrations(value: &serde_json::Value, cfg: &mut AppConfig) {
        // Old configs only had `voice`; migrate once when `edge_voice` is absent.
        if value.get("edge_voice").is_none() {
            cfg.edge_voice = cfg.voice.clone();
        }
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(cfg) = Self::from_json_value(value) {
                            return cfg;
                        }
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
