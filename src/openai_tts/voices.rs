use crate::config::OpenAiConfig;
use crate::edge_tts::Voice;
use crate::openai_tts::client::join_speech_url;
use serde::Deserialize;
use std::time::Duration;

/// Common OpenAI TTS voice presets.
pub fn get_preset_voices() -> Vec<Voice> {
    vec![
        Voice::new("alloy", "Alloy", "en-US", "Unknown"),
        Voice::new("echo", "Echo", "en-US", "Male"),
        Voice::new("fable", "Fable", "en-US", "Unknown"),
        Voice::new("onyx", "Onyx", "en-US", "Male"),
        Voice::new("nova", "Nova", "en-US", "Female"),
        Voice::new("shimmer", "Shimmer", "en-US", "Female"),
    ]
}

#[derive(Debug, Deserialize)]
struct RemoteVoicesResponse {
    #[serde(default)]
    data: Vec<RemoteVoice>,
}

#[derive(Debug, Deserialize)]
struct RemoteVoice {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    gender: Option<String>,
}

fn join_voices_url(base_url: &str) -> String {
    let speech = join_speech_url(base_url);
    // .../v1/audio/speech -> .../v1/audio/voices
    if let Some(prefix) = speech.strip_suffix("/speech") {
        format!("{}/voices", prefix)
    } else {
        let trimmed = base_url.trim().trim_end_matches('/');
        if trimmed.ends_with("/v1") {
            format!("{}/audio/voices", trimmed)
        } else {
            format!("{}/v1/audio/voices", trimmed)
        }
    }
}

/// Optional remote voice list. VoiceDesign models often return an empty list.
pub async fn fetch_voices_list(
    cfg: &OpenAiConfig,
) -> Result<Vec<Voice>, Box<dyn std::error::Error + Send + Sync>> {
    let mut url = join_voices_url(&cfg.base_url);
    let model = cfg.model.trim();
    if !model.is_empty() {
        let sep = if url.contains('?') { '&' } else { '?' };
        url = format!(
            "{}{}model={}",
            url,
            sep,
            urlencoding_encode(model)
        );
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    let mut request = client.get(&url);
    let api_key = cfg.api_key.trim();
    if !api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(format!("Voices HTTP {}", response.status()).into());
    }

    let parsed: RemoteVoicesResponse = response.json().await?;
    let voices = parsed
        .data
        .into_iter()
        .filter_map(|v| {
            let short = if !v.id.is_empty() {
                v.id
            } else if !v.name.is_empty() {
                v.name.clone()
            } else {
                return None;
            };
            let friendly = if v.name.is_empty() {
                short.clone()
            } else {
                v.name
            };
            let locale = v.locale.unwrap_or_else(|| "und".to_string());
            let gender = v.gender.unwrap_or_else(|| "Unknown".to_string());
            Some(Voice::new(&short, &friendly, &locale, &gender))
        })
        .collect();
    Ok(voices)
}

/// Minimal URL-encoding for query values (no extra dependency).
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0xf) as usize]));
            }
        }
    }
    out
}
