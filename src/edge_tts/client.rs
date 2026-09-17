use super::types::SpeakOptions;
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

pub const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
pub const EDGE_VERSION: &str = "1-143.0.3650.75";
pub const USER_AGENT_STRING: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

/// Calculates the Sec-MS-GEC token based on a 5-minute rolling window
pub fn generate_sec_ms_gec() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Windows File Time epoch: difference between 1601 and 1970 is 11644473600 seconds
    let windows_seconds = (now + 11644473600) / 300 * 300;
    let rounded_ticks = windows_seconds * 10_000_000;
    let str_to_hash = format!("{}{}", rounded_ticks, TRUSTED_CLIENT_TOKEN);

    let mut hasher = Sha256::new();
    hasher.update(str_to_hash.as_bytes());
    let hash = hasher.finalize();

    let mut hex = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write;
        let _ = write!(hex, "{:02X}", byte);
    }
    hex
}

/// Escapes XML special characters for SSML payload
pub fn escape_xml(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(c),
        }
    }
    output
}

/// Builds an SSML document string for Edge-TTS
pub fn build_ssml(text: &str, options: &SpeakOptions) -> String {
    let escaped_text = escape_xml(text);
    let rate_str = if options.rate >= 0 {
        format!("+{}%", options.rate)
    } else {
        format!("{}%", options.rate)
    };
    let pitch_str = if options.pitch >= 0 {
        format!("+{}Hz", options.pitch)
    } else {
        format!("{}Hz", options.pitch)
    };
    let volume_str = format!("{}%", options.volume);

    let lang = if options.voice.starts_with("zh-") {
        "zh-CN"
    } else if options.voice.starts_with("en-") {
        "en-US"
    } else if options.voice.starts_with("ja-") {
        "ja-JP"
    } else {
        "en-US"
    };

    format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='{lang}'>\
            <voice name='{voice}'>\
                <prosody pitch='{pitch_str}' rate='{rate_str}' volume='{volume_str}'>\
                    {escaped_text}\
                </prosody>\
            </voice>\
        </speak>",
        voice = options.voice,
    )
}

/// Synthesize text to MP3 audio bytes using Edge-TTS WebSocket
pub async fn synthesize(
    text: &str,
    options: &SpeakOptions,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let connection_id = uuid::Uuid::new_v4().simple().to_string();
    let token = generate_sec_ms_gec();
    let wss_url = format!(
        "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1?TrustedClientToken={}&Sec-MS-GEC={}&Sec-MS-GEC-Version={}&ConnectionId={}",
        TRUSTED_CLIENT_TOKEN, token, EDGE_VERSION, connection_id
    );

    let mut request = wss_url.into_client_request()?;
    let headers = request.headers_mut();
    headers.insert("User-Agent", USER_AGENT_STRING.parse()?);
    headers.insert(
        "Origin",
        "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold".parse()?,
    );
    headers.insert("Pragma", "no-cache".parse()?);
    headers.insert("Cache-Control", "no-cache".parse()?);
    headers.insert("Sec-MS-GEC", token.parse()?);
    headers.insert("Sec-MS-GEC-Version", EDGE_VERSION.parse()?);

    let (mut ws_stream, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| "Connection timed out")??;

    // 1. Send speech.config message
    let timestamp = chrono::Utc::now()
        .format("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)")
        .to_string();
    let config_msg = format!(
        "X-Timestamp:{}\r\n\
        Content-Type:application/json; charset=utf-8\r\n\
        Path:speech.config\r\n\r\n\
        {{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"true\"}},\"outputFormat\":\"audio-24khz-48kbitrate-mono-mp3\"}}}}}}}}",
        timestamp
    );
    ws_stream.send(Message::Text(config_msg)).await?;

    // 2. Send SSML message
    let ssml = build_ssml(text, options);
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let ssml_msg = format!(
        "X-RequestId:{}\r\n\
        Content-Type:application/ssml+xml\r\n\
        Path:ssml\r\n\r\n\
        {}",
        request_id, ssml
    );
    ws_stream.send(Message::Text(ssml_msg)).await?;

    // 3. Receive audio data chunks until turn.end
    let mut audio_data = Vec::new();
    let timeout_duration = Duration::from_secs(30);

    loop {
        let msg = tokio::time::timeout(timeout_duration, ws_stream.next()).await;
        match msg {
            Ok(Some(Ok(Message::Binary(bin)))) => {
                if bin.len() >= 2 {
                    // 2-byte big-endian header length
                    let header_len = u16::from_be_bytes([bin[0], bin[1]]) as usize;
                    let payload_offset = 2 + header_len;
                    if bin.len() > payload_offset {
                        audio_data.extend_from_slice(&bin[payload_offset..]);
                    }
                }
            }
            Ok(Some(Ok(Message::Text(txt)))) => {
                if txt.contains("Path:turn.end") {
                    break;
                }
            }
            Ok(Some(Ok(Message::Close(_)))) => {
                break;
            }
            Ok(Some(Ok(Message::Ping(p)))) => {
                let _ = ws_stream.send(Message::Pong(p)).await;
            }
            Ok(Some(Err(e))) => {
                return Err(format!("WebSocket error: {}", e).into());
            }
            Ok(None) => {
                break;
            }
            Err(_) => {
                return Err("Timeout waiting for audio stream chunk".into());
            }
            _ => {}
        }
    }

    if audio_data.is_empty() {
        return Err("No audio data received from server".into());
    }

    Ok(audio_data)
}
