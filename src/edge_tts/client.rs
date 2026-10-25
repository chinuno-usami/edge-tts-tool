use super::types::SpeakOptions;
use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

pub const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
/// Sec-MS-GEC-Version 形如 `<2>-<Edge 完整版本号>`。
/// 微软自 Edge-TTS 服务升级为 v2 令牌后只接受 `2-` 前缀，`1-` 前缀会直接返回 403。
pub const EDGE_VERSION: &str = "2-143.0.3650.75";
pub const USER_AGENT_STRING: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

// Windows File Time epoch: difference between 1601 and 1970 is 11644473600 seconds
const WIN_EPOCH: u64 = 11644473600;
const SEC_MS_GEC_WINDOW_SECONDS: u64 = 300;

const SHA256_INITIAL: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB,
    0x5BE0CD19,
];
const SHA256_STATE: [u32; 8] = [
    0xFD9FBEA2, 0x91243CE7, 0xBE499094, 0xCD2E1E0A, 0x6C006F49, 0x04BD35AB, 0xFEB0A7E4,
    0x03EFFBD2,
];
const SHA256_SCHEDULE: [u32; 14] = [
    0xEB24BF9E, 0x1558691F, 0xA0C09EE2, 0x0CFA32BA, 0xA622EE8C, 0xD1DD9491, 0xC0DD25A0,
    0xF9E70AB6, 0xF0C31573, 0x7058CE4E, 0x7188E606, 0x00E5AB33, 0xD258B060, 0x72766564,
];
const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4,
    0xAB1C5ED5, 0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE,
    0x9BDC06A7, 0xC19BF174, 0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F,
    0x4A7484AA, 0x5CB0A9DC, 0x76F988DA, 0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7,
    0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967, 0x27B70A85, 0x2E1B2138, 0x4D2C6DFC,
    0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85, 0xA2BFE8A1, 0xA81A664B,
    0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070, 0x19A4C116,
    0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7,
    0xC67178F2,
];
const SHA256_ADJUSTMENT: u32 = 0xC9DC06A7;
const SHA256_SIGMA0_ADJUSTMENT: u32 = 0x059C0B80;
const SHA256_SIGMA1_ADJUSTMENT: u32 = 0x000B92C0;

const CS64_COEFFICIENTS: [(u32, u32); 14] = [
    (0xE82E91B1, 0xB32700D0),
    (0x44DE24C6, 0x88DFF51D),
    (0x3BEB65BC, 0x29514038),
    (0xC89C5933, 0xB328CBDF),
    (0x8D1EC211, 0xD405B4D5),
    (0x314EF5D5, 0x26E55F3C),
    (0xE92491B1, 0x607801C6),
    (0xF0A5B9D5, 0x82B2C1FB),
    (0x06064CF3, 0xFA51A3DA),
    (0xDA79FA60, 0x207CC464),
    (0xBED20560, 0x3C3C74F3),
    (0x89F9C9F8, 0xD2D14B61),
    (0x35FA38C6, 0xBB04A2C1),
    (0xCB49E3C6, 0xC5626167),
];
const CS64_FACTOR: u32 = 0x82893933;
const CS64_ADJUSTMENT0: u32 = 0xF2F90B1A;
const CS64_ADJUSTMENT1: u32 = 0x0B1A0000;

/// SHA-256 small sigma zero, truncating the combined rotations once
fn sigma0(value: u32) -> u32 {
    (value >> 7) ^ (value << 25) ^ (value >> 18) ^ (value << 14) ^ (value >> 3)
}

/// SHA-256 small sigma one, truncating the combined rotations once
fn sigma1(value: u32) -> u32 {
    (value >> 17) ^ (value << 15) ^ (value >> 19) ^ (value << 13) ^ (value >> 10)
}

fn expand_schedule(words: &mut [u32], first_word: usize) {
    for index in first_word..64 {
        let x = words[index - 2];
        let y = words[index - 15];
        let value = sigma1(x)
            .wrapping_add(words[index - 7])
            .wrapping_add(sigma0(y))
            .wrapping_add(words[index - 16]);
        words[index] = value;
    }
}

fn sha256_rounds(state: [u32; 8], words: &[u32], first_round: usize) -> [u32; 8] {
    let mut constants = SHA256_ROUND_CONSTANTS;
    if first_round == 14 {
        constants[14] = SHA256_ADJUSTMENT;
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
    for index in first_round..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = g ^ (e & (f ^ g));
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(constants[index])
            .wrapping_add(words[index]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) | (c & (a | b));
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    [a, b, c, d, e, f, g, h]
}

/// Initial SHA-256 state for the token data (first seven bytes form the salt)
fn sha256_init(data: &[u8]) -> [u32; 8] {
    let mut words = [0u32; 64];
    words[14] = u32::from_be_bytes([0, data[0], data[1], data[2]]);
    words[15] = u32::from_be_bytes([data[3], data[4], data[5], data[6]]);
    for index in 16..30 {
        let mut value = SHA256_SCHEDULE[index - 16];
        if index - 2 >= 14 {
            value = value.wrapping_add(if index == 16 {
                sigma1(words[index - 2]) ^ SHA256_SIGMA1_ADJUSTMENT
            } else {
                sigma1(words[index - 2])
            });
        }
        if index - 7 >= 14 {
            value = value.wrapping_add(words[index - 7]);
        }
        if index - 15 >= 14 {
            value = value.wrapping_add(if index == 29 {
                sigma0(words[index - 15]) ^ SHA256_SIGMA0_ADJUSTMENT
            } else {
                sigma0(words[index - 15])
            });
        }
        words[index] = value;
    }
    words[30] = sigma1(words[28])
        .wrapping_add(words[23])
        .wrapping_add(sigma0(words[15]))
        .wrapping_add(words[14])
        .wrapping_add(SHA256_ADJUSTMENT)
        .wrapping_sub(SHA256_ROUND_CONSTANTS[14]);
    expand_schedule(&mut words, 31);

    let working = sha256_rounds(SHA256_STATE, &words, 14);
    let mut state = [0u32; 8];
    for index in 0..8 {
        state[index] = SHA256_INITIAL[index].wrapping_add(working[index]);
    }
    state
}

/// Computes the SHA-256 digest used by the v2 Sec-MS-GEC token
fn token_sha256_digest(data: &[u8]) -> [u8; 32] {
    let length = 57 + data.len();
    let mut padded = Vec::with_capacity(64);
    padded.extend_from_slice(&data[7..]);
    padded.push(0x80);
    let zeros = (55usize.wrapping_sub(length)) % 64;
    padded.extend(std::iter::repeat(0u8).take(zeros));
    padded.extend_from_slice(&((length as u64) * 8).to_be_bytes());

    let mut state = sha256_init(data);
    for block in padded.chunks(64) {
        let mut words = [0u32; 64];
        for index in 0..16 {
            words[index] = u32::from_be_bytes([
                block[index * 4],
                block[index * 4 + 1],
                block[index * 4 + 2],
                block[index * 4 + 3],
            ]);
        }
        expand_schedule(&mut words, 16);
        let working = sha256_rounds(state, &words, 0);
        for index in 0..8 {
            state[index] = state[index].wrapping_add(working[index]);
        }
    }

    let mut digest = [0u8; 32];
    for index in 0..8 {
        digest[index * 4..index * 4 + 4].copy_from_slice(&state[index].to_be_bytes());
    }
    digest
}

/// Splits data into little-endian 32-bit words, zero-padded to an even count
fn words_le4_even(data: &[u8]) -> Vec<u32> {
    let mut padded = data.to_vec();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let mut words: Vec<u32> = padded
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    if words.len() % 2 == 1 {
        words.push(0);
    }
    words
}

/// Computes the CS64 digest words for the v2 Sec-MS-GEC token
fn cs64_digest(data: &[u8], digest: &[u8; 32]) -> (u32, u32) {
    let key_a = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 1;
    let key_b = u32::from_le_bytes([digest[4], digest[5], digest[6], digest[7]]) | 1;
    let factors = [
        key_a.wrapping_mul(CS64_FACTOR),
        key_b.wrapping_mul(CS64_FACTOR),
    ];

    let mut terms: Vec<u32> = CS64_COEFFICIENTS
        .iter()
        .enumerate()
        .map(|(index, (coefficient, _))| coefficient.wrapping_mul(factors[index % 2]))
        .collect();
    let mut offsets: Vec<u32> = CS64_COEFFICIENTS.iter().map(|(_, offset)| *offset).collect();

    let mut salted = Vec::with_capacity(data.len() + 1);
    salted.push(0u8);
    salted.extend_from_slice(data);
    for (index, word) in words_le4_even(&salted).iter().enumerate() {
        let mut term = word.wrapping_mul(if index % 2 == 0 { key_a } else { key_b });
        let mut offset = word.rotate_left(16).wrapping_mul(if index % 2 == 0 {
            0xEF0569FB
        } else {
            0xC31713DB
        });
        if index == 0 {
            term = term.wrapping_add(CS64_ADJUSTMENT0.wrapping_mul(factors[0]));
            offset = offset.wrapping_add(CS64_ADJUSTMENT1);
        }
        terms.push(term);
        offsets.push(offset);
    }

    let (mut swap_acc, mut swap_last, mut state, mut acc) = (0u32, 0u32, 0u32, 0u32);
    let mut index = 0;
    while index < terms.len() {
        let x = terms[index];
        let y = terms[index + 1];

        let mut t = offsets[index].wrapping_add(x).rotate_left(16);
        let mut q = t.wrapping_mul(0x76F50F34);
        swap_acc = swap_acc.wrapping_add(q.rotate_left(16).wrapping_mul(0xC3EFEA97).wrapping_add(q));

        t = offsets[index + 1].wrapping_add(y).rotate_left(16);
        q = t.wrapping_mul(0x1881CE3C);
        swap_last = q.rotate_left(16).wrapping_mul(0x35BD1EC9).wrapping_add(q);
        swap_acc = swap_acc.wrapping_add(swap_last);

        state = state.wrapping_mul(key_a).wrapping_add(x);
        state = state.rotate_left(16).wrapping_mul(0xCF98B111);
        state = state.rotate_left(16).wrapping_mul(0x87085B9F);
        state = state.rotate_left(16).wrapping_mul(0x12CEB96D);
        state = state.rotate_left(16).wrapping_mul(0x257E1D83);
        acc = acc.wrapping_add(state);

        t = state.wrapping_mul(key_b).wrapping_add(y);
        t = t.rotate_left(16).wrapping_mul(0xA27416F5);
        t = t.rotate_left(16).wrapping_mul(0xD38396FF);
        state = t
            .rotate_left(16)
            .wrapping_mul(0x7C932B89)
            .rotate_left(16)
            .wrapping_mul(0xBFA49F69);
        acc = acc.wrapping_add(state);

        index += 2;
    }

    (swap_last ^ state, swap_acc ^ acc)
}

fn to_hex_upper(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(hex, "{:02X}", byte);
    }
    hex
}

/// Calculates the v2 Sec-MS-GEC token based on a 5-minute rolling window
pub fn generate_sec_ms_gec() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    generate_sec_ms_gec_at(now)
}

/// Calculates the v2 Sec-MS-GEC token for the given Unix timestamp in seconds.
///
/// The token is `<CS64 hex 16>-<SHA256 hex 64>`; the old 64-char SHA256-only
/// token is rejected by the service with HTTP 403.
pub fn generate_sec_ms_gec_at(unix_timestamp: u64) -> String {
    let windows_seconds =
        (unix_timestamp + WIN_EPOCH) / SEC_MS_GEC_WINDOW_SECONDS * SEC_MS_GEC_WINDOW_SECONDS;
    let rounded_ticks = windows_seconds * 10_000_000;
    let str_to_hash = format!("{}{}", rounded_ticks, TRUSTED_CLIENT_TOKEN);
    let data = str_to_hash.as_bytes();

    let digest = token_sha256_digest(data);
    let (swap_last, swap_acc) = cs64_digest(data, &digest);
    format!(
        "{:08X}{:08X}-{}",
        swap_last.swap_bytes(),
        swap_acc.swap_bytes(),
        to_hex_upper(&digest)
    )
}

/// Random 32-hex MUID used in the `muid` cookie (matches Edge browser behavior)
pub fn generate_muid() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    to_hex_upper(&bytes)
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
    headers.insert("Accept-Encoding", "gzip, deflate, br, zstd".parse()?);
    headers.insert("Accept-Language", "en-US,en;q=0.9".parse()?);
    headers.insert("Cookie", format!("muid={};", generate_muid()).parse()?);

    let (mut ws_stream, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| "Connection timed out")?
    .map_err(|e| format!("Edge-TTS 连接失败（请检查网络/代理）: {}", e))?;

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
