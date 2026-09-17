use super::types::Voice;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use std::time::Duration;

pub const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
pub const VOICES_URL: &str = "https://speech.platform.bing.com/consumer/speech/synthesize/readaloud/voices/list?trustedclienttoken=6A5AA1D4EAFF4E9FB37E23D68491D6F4";

/// Return a curated list of popular voices across Chinese and English
pub fn get_preset_voices() -> Vec<Voice> {
    vec![
        // Chinese Mainland (zh-CN)
        Voice::new("zh-CN-XiaoxiaoNeural", "微软 晓晓 (女声, 温暖自然)", "zh-CN", "Female"),
        Voice::new("zh-CN-YunxiNeural", "微软 云希 (男声, 沉稳生动)", "zh-CN", "Male"),
        Voice::new("zh-CN-YunjianNeural", "微软 云健 (男声, 影视解说/旁白)", "zh-CN", "Male"),
        Voice::new("zh-CN-XiaoyiNeural", "微软 晓伊 (女声, 甜美播报)", "zh-CN", "Female"),
        Voice::new("zh-CN-YunyangNeural", "微软 云扬 (男声, 专业新闻)", "zh-CN", "Male"),
        Voice::new("zh-CN-liaoning-XiaobeiNeural", "微软 东北话 晓北 (女声, 幽默东北口音)", "zh-CN-liaoning", "Female"),
        Voice::new("zh-CN-shaanxi-XiaoniNeural", "微软 陕西话 晓妮 (女声, 亲切方言)", "zh-CN-shaanxi", "Female"),

        // Chinese Taiwan (zh-TW)
        Voice::new("zh-TW-HsiaoChenNeural", "微软 曉臻 (女声, 台湾普通话)", "zh-TW", "Female"),
        Voice::new("zh-TW-YunJheNeural", "微软 雲哲 (男声, 台湾普通话)", "zh-TW", "Male"),

        // Chinese Hong Kong (zh-HK)
        Voice::new("zh-HK-HiuMaanNeural", "微软 曉曼 (女声, 粤语)", "zh-HK", "Female"),
        Voice::new("zh-HK-WanLungNeural", "微软 雲龍 (男声, 粤语)", "zh-HK", "Male"),

        // English (US)
        Voice::new("en-US-JennyNeural", "Microsoft Jenny (Female, Natural)", "en-US", "Female"),
        Voice::new("en-US-GuyNeural", "Microsoft Guy (Male, Natural)", "en-US", "Male"),
        Voice::new("en-US-AnaNeural", "Microsoft Ana (Female, Child)", "en-US", "Female"),
        Voice::new("en-US-ChristopherNeural", "Microsoft Christopher (Male)", "en-US", "Male"),
        Voice::new("en-US-EricNeural", "Microsoft Eric (Male)", "en-US", "Male"),

        // English (UK)
        Voice::new("en-GB-SoniaNeural", "Microsoft Sonia (Female, British)", "en-GB", "Female"),
        Voice::new("en-GB-RyanNeural", "Microsoft Ryan (Male, British)", "en-GB", "Male"),

        // Japanese
        Voice::new("ja-JP-NanamiNeural", "Microsoft 七海 (女声, 日本語)", "ja-JP", "Female"),
        Voice::new("ja-JP-KeitaNeural", "Microsoft 圭太 (男声, 日本語)", "ja-JP", "Male"),
    ]
}

/// Fetches the full voice list from Microsoft's Edge-TTS server
pub async fn fetch_voices_list() -> Result<Vec<Voice>, Box<dyn std::error::Error + Send + Sync>> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0",
        ),
    );
    headers.insert("Authority", HeaderValue::from_static("speech.platform.bing.com"));
    headers.insert("Sec-CH-UA", HeaderValue::from_static(r#""Microsoft Edge";v="130", "Chromium";v="130""#));
    headers.insert("Sec-CH-UA-Mobile", HeaderValue::from_static("?0"));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .default_headers(headers)
        .build()?;

    let response = client.get(VOICES_URL).send().await?;
    if !response.status().is_success() {
        return Err(format!("Server returned HTTP {}", response.status()).into());
    }

    let voices: Vec<Voice> = response.json().await?;
    Ok(voices)
}
