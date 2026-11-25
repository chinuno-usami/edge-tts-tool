use crate::config::OpenAiConfig;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// Normalize base URL and append `/v1/audio/speech` (or `/audio/speech` if already under `/v1`).
pub fn join_speech_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        format!("{}/audio/speech", trimmed)
    } else {
        format!("{}/v1/audio/speech", trimmed)
    }
}

/// Map UI rate% (-50..=100) to OpenAI `speed` (clamped 0.25..=4.0).
pub fn rate_to_speed(rate: i32) -> f32 {
    ((1.0 + rate as f32 / 100.0).clamp(0.25, 4.0) * 100.0).round() / 100.0
}

/// Classic OpenAI Speech models (`tts-1` / `tts-1-hd`, or empty model).
/// These endpoints reject unknown extension fields such as `instruct` / sampling params.
pub fn model_is_classic_openai_speech(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    matches!(m.as_str(), "" | "tts-1" | "tts-1-hd")
}

/// Classic OpenAI Speech models reject unknown fields such as `instruct`.
pub fn model_supports_instruct(model: &str) -> bool {
    !model_is_classic_openai_speech(model)
}

/// mlx-audio / Qwen3-TTS style sampling (`temperature` / `top_p`).
/// Currently the same exclusion set as classic Speech; kept as a separate predicate
/// so instruct support and sampling support can diverge later.
pub fn model_supports_sampling_params(model: &str) -> bool {
    !model_is_classic_openai_speech(model)
}

/// Classic Speech models require a non-empty `voice`; VoiceDesign-style models may omit it.
pub fn model_requires_voice(model: &str) -> bool {
    model_is_classic_openai_speech(model) && !model.trim().is_empty()
}

/// mlx-audio / Qwen3-TTS Base voice-clone extensions (`ref_audio` / `ref_text`).
/// VoiceDesign models support `instruct` but not clone refs.
pub fn model_supports_voice_clone_refs(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if !model_supports_instruct(&m) {
        return false;
    }
    // Prefer explicit Base markers; exclude VoiceDesign-style models.
    if m.contains("voicedesign") || m.contains("voice-design") || m.contains("voice_design") {
        return false;
    }
    m.contains("base") || m.contains("voice-clone") || m.contains("voice_clone")
}

/// Qwen3-TTS VoiceDesign / Base style models should not send leftover classic voices
/// (e.g. `alloy` left over from switching away from tts-1).
pub fn model_should_omit_classic_voice(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if !model_supports_instruct(&m) {
        return false;
    }
    m.contains("voicedesign")
        || m.contains("voice-design")
        || m.contains("voice_design")
        || model_supports_voice_clone_refs(&m)
}

#[derive(Debug, Serialize)]
pub struct OpenAiSpeechRequest<'a> {
    pub model: &'a str,
    pub input: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruct: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_audio: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    pub response_format: &'a str,
    pub speed: f32,
}

impl<'a> OpenAiSpeechRequest<'a> {
    pub fn from_parts(text: &'a str, rate: i32, cfg: &'a OpenAiConfig) -> Self {
        let model = cfg.model.trim();
        let supports_instruct = model_supports_instruct(model);
        let supports_clone = model_supports_voice_clone_refs(model);
        let ref_audio = {
            let p = cfg.ref_audio.trim();
            if p.is_empty() || !supports_clone {
                None
            } else {
                Some(p)
            }
        };
        let ref_text = {
            let t = cfg.ref_text.trim();
            if t.is_empty() || !supports_clone {
                None
            } else {
                Some(t)
            }
        };
        // Qwen3-TTS Base: `voice` conflicts with ref_audio/ref_text cloning.
        // VoiceDesign / instruct-first models: omit leftover classic voices (e.g. alloy).
        let using_clone_refs = ref_audio.is_some() || ref_text.is_some();
        let voice = {
            let v = cfg.voice.trim();
            if v.is_empty()
                || using_clone_refs
                || model_should_omit_classic_voice(model)
            {
                None
            } else {
                Some(v)
            }
        };
        let instruct = {
            let i = cfg.instruct.trim();
            if i.is_empty() || !supports_instruct {
                None
            } else {
                Some(i)
            }
        };
        // Classic OpenAI Speech models reject unknown sampling fields.
        let temperature = if model_supports_sampling_params(model) {
            cfg.temperature
                .map(|t| t.clamp(OpenAiConfig::TEMPERATURE_MIN, OpenAiConfig::TEMPERATURE_MAX))
        } else {
            None
        };
        let top_p = if model_supports_sampling_params(model) {
            cfg.top_p
                .map(|p| p.clamp(OpenAiConfig::TOP_P_MIN, OpenAiConfig::TOP_P_MAX))
        } else {
            None
        };
        Self {
            model,
            input: text,
            voice,
            instruct,
            ref_audio,
            ref_text,
            temperature,
            top_p,
            response_format: "mp3",
            speed: rate_to_speed(rate),
        }
    }
}

fn parse_error_body(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(msg) = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .filter(|s| !s.is_empty())
        {
            return format!("HTTP {}: {}", status, msg);
        }
        if let Some(detail) = v.get("detail") {
            let detail_str = match detail {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if !detail_str.is_empty() {
                return format!("HTTP {}: {}", status, detail_str);
            }
        }
    }
    let prefix: String = body.chars().take(200).collect();
    if prefix.is_empty() {
        format!("HTTP {}", status)
    } else {
        format!("HTTP {}: {}", status, prefix)
    }
}

/// Synthesize text via OpenAI-compatible `POST /v1/audio/speech`.
pub async fn synthesize(
    text: &str,
    rate: i32,
    cfg: &OpenAiConfig,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    if model_requires_voice(&cfg.model) && cfg.voice.trim().is_empty() {
        return Err("经典 TTS 模型（如 tts-1）需要填写 voice".into());
    }

    // ref_audio is a server-side path; do not validate existence on the client.

    let url = join_speech_url(&cfg.base_url);
    let body = OpenAiSpeechRequest::from_parts(text, rate, cfg);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    let mut request = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body);

    let api_key = cfg.api_key.trim();
    if !api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = request.send().await.map_err(|e| {
        format!("OpenAI TTS 请求失败: {}", e)
    })?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        return Err(parse_error_body(status, &err_body).into());
    }

    let audio = response.bytes().await?.to_vec();
    if audio.is_empty() {
        return Err("OpenAI TTS 返回空音频".into());
    }

    let looks_mp3 = audio.starts_with(b"ID3")
        || (audio.len() >= 2 && audio[0] == 0xFF && (audio[1] & 0xE0) == 0xE0);
    if !looks_mp3 {
        log::warn!(
            "OpenAI TTS response does not look like MP3 ({} bytes); playing/export may still work",
            audio.len()
        );
    }

    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_speech_url() {
        assert_eq!(
            join_speech_url("https://api.openai.com"),
            "https://api.openai.com/v1/audio/speech"
        );
        assert_eq!(
            join_speech_url("https://api.openai.com/"),
            "https://api.openai.com/v1/audio/speech"
        );
        assert_eq!(
            join_speech_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/audio/speech"
        );
        assert_eq!(
            join_speech_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/audio/speech"
        );
        assert_eq!(
            join_speech_url("http://127.0.0.1:8081"),
            "http://127.0.0.1:8081/v1/audio/speech"
        );
        assert_eq!(
            join_speech_url("https://my-gateway.example.com/openai"),
            "https://my-gateway.example.com/openai/v1/audio/speech"
        );
    }

    #[test]
    fn test_model_requires_voice() {
        assert!(model_requires_voice("tts-1"));
        assert!(model_requires_voice("TTS-1-HD"));
        assert!(!model_requires_voice(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16"
        ));
        assert!(!model_requires_voice(""));
    }

    #[test]
    fn test_model_supports_voice_clone_refs() {
        assert!(model_supports_voice_clone_refs(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16"
        ));
        assert!(!model_supports_voice_clone_refs(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16"
        ));
        assert!(!model_supports_voice_clone_refs("tts-1"));
        assert!(!model_supports_voice_clone_refs("tts-1-hd"));
        assert!(!model_supports_voice_clone_refs(""));
    }

    #[test]
    fn test_model_should_omit_classic_voice() {
        assert!(model_should_omit_classic_voice(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16"
        ));
        assert!(model_should_omit_classic_voice(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16"
        ));
        assert!(!model_should_omit_classic_voice("tts-1"));
        assert!(!model_should_omit_classic_voice("custom-gateway-model"));
    }

    #[test]
    fn test_rate_to_speed() {
        assert!((rate_to_speed(0) - 1.0).abs() < f32::EPSILON);
        assert!((rate_to_speed(100) - 2.0).abs() < f32::EPSILON);
        assert!((rate_to_speed(-50) - 0.5).abs() < f32::EPSILON);
        assert!((rate_to_speed(-100) - 0.25).abs() < f32::EPSILON);
        assert!((rate_to_speed(500) - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_request_json_without_instruct() {
        let cfg = OpenAiConfig {
            model: "tts-1".to_string(),
            voice: "alloy".to_string(),
            instruct: String::new(),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("hello", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "tts-1");
        assert_eq!(json["input"], "hello");
        assert_eq!(json["voice"], "alloy");
        assert_eq!(json["response_format"], "mp3");
        assert!((json["speed"].as_f64().unwrap() - 1.0).abs() < 1e-6);
        assert!(json.get("instruct").is_none());
    }

    #[test]
    fn test_request_json_with_instruct_and_empty_voice() {
        let cfg = OpenAiConfig {
            model: "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16".to_string(),
            voice: String::new(),
            instruct: "温柔女声，语速适中，带一点微笑感".to_string(),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("你好", 20, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["instruct"], "温柔女声，语速适中，带一点微笑感");
        assert!(json.get("voice").is_none());
        assert!(json.get("ref_audio").is_none());
        assert!(json.get("ref_text").is_none());
        assert!((json["speed"].as_f64().unwrap() - 1.2).abs() < 1e-6);
    }

    #[test]
    fn test_request_json_voicedesign_omits_residual_classic_voice() {
        let cfg = OpenAiConfig {
            model: "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16".to_string(),
            voice: "alloy".to_string(),
            instruct: "温柔女声".to_string(),
            ref_audio: "/tmp/should-not-send.wav".to_string(),
            ref_text: "ignored for VoiceDesign".to_string(),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("你好", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("voice").is_none(), "residual classic voice must be omitted");
        assert!(json.get("ref_audio").is_none(), "VoiceDesign must not send clone refs");
        assert!(json.get("ref_text").is_none());
        assert_eq!(json["instruct"], "温柔女声");
    }

    #[test]
    fn test_request_json_omits_instruct_for_classic_openai_models() {
        let cfg = OpenAiConfig {
            model: "tts-1".to_string(),
            voice: "alloy".to_string(),
            instruct: "should not be sent".to_string(),
            temperature: Some(0.8),
            top_p: Some(0.95),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("hello", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("instruct").is_none());
        assert!(json.get("temperature").is_none());
        assert!(json.get("top_p").is_none());
        assert_eq!(json["voice"], "alloy");
    }

    #[test]
    fn test_request_json_with_temperature_and_top_p() {
        let cfg = OpenAiConfig {
            model: "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16".to_string(),
            voice: String::new(),
            temperature: Some(0.8),
            top_p: Some(0.95),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("你好", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert!((json["temperature"].as_f64().unwrap() - 0.8).abs() < 1e-6);
        assert!((json["top_p"].as_f64().unwrap() - 0.95).abs() < 1e-6);
    }

    #[test]
    fn test_request_json_omits_unset_temperature_and_top_p() {
        let cfg = OpenAiConfig {
            model: "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16".to_string(),
            instruct: "温柔女声".to_string(),
            temperature: None,
            top_p: None,
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("你好", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("temperature").is_none());
        assert!(json.get("top_p").is_none());
    }

    #[test]
    fn test_request_json_clamps_temperature_and_top_p() {
        let cfg = OpenAiConfig {
            model: "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16".to_string(),
            voice: String::new(),
            temperature: Some(5.0),
            top_p: Some(-0.1),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("你好", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert!((json["temperature"].as_f64().unwrap() - 2.0).abs() < 1e-6);
        assert!((json["top_p"].as_f64().unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_request_json_with_voice_clone_refs_omits_voice() {
        let cfg = OpenAiConfig {
            model: "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16".to_string(),
            voice: "should-be-omitted".to_string(),
            ref_audio: "/tmp/ref.wav".to_string(),
            ref_text: "This is what my voice sounds like.".to_string(),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("你好", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["ref_audio"], "/tmp/ref.wav");
        assert_eq!(json["ref_text"], "This is what my voice sounds like.");
        assert!(json.get("voice").is_none());
    }

    #[test]
    fn test_request_json_omits_clone_refs_for_classic_openai_models() {
        let cfg = OpenAiConfig {
            model: "tts-1".to_string(),
            voice: "alloy".to_string(),
            ref_audio: "/tmp/ref.wav".to_string(),
            ref_text: "ignored".to_string(),
            ..OpenAiConfig::default()
        };
        let req = OpenAiSpeechRequest::from_parts("hello", 0, &cfg);
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("ref_audio").is_none());
        assert!(json.get("ref_text").is_none());
        assert_eq!(json["voice"], "alloy");
    }

    #[tokio::test]
    async fn test_empty_text_returns_empty() {
        let cfg = OpenAiConfig::default();
        let audio = synthesize("   ", 0, &cfg).await.unwrap();
        assert!(audio.is_empty());
    }

    #[tokio::test]
    async fn test_classic_model_rejects_empty_voice() {
        let cfg = OpenAiConfig {
            model: "tts-1".to_string(),
            voice: String::new(),
            ..OpenAiConfig::default()
        };
        let err = synthesize("hello", 0, &cfg).await.unwrap_err();
        assert!(
            err.to_string().contains("voice"),
            "expected voice requirement error, got: {}",
            err
        );
    }
}
