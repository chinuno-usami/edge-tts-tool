#[cfg(test)]
mod tests {
    use edge_tts_tool::config::AppConfig;
    use edge_tts_tool::edge_tts::{build_ssml, escape_xml, generate_sec_ms_gec, SpeakOptions};
    use edge_tts_tool::hotkey::HotkeyConfig;

    #[test]
    fn test_sec_ms_gec_format() {
        let token = generate_sec_ms_gec();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
    }

    #[test]
    fn test_escape_xml() {
        let input = "Hello <world> & \"everyone\" 'here'";
        let escaped = escape_xml(input);
        assert_eq!(escaped, "Hello &lt;world&gt; &amp; &quot;everyone&quot; &apos;here&apos;");
    }

    #[test]
    fn test_build_ssml() {
        let options = SpeakOptions {
            voice: "zh-CN-XiaoxiaoNeural".to_string(),
            rate: 20,
            pitch: -5,
            volume: 80,
        };
        let ssml = build_ssml("你好世界", &options);
        assert!(ssml.contains("zh-CN-XiaoxiaoNeural"));
        assert!(ssml.contains("rate='+20%'"));
        assert!(ssml.contains("pitch='-5Hz'"));
        assert!(ssml.contains("volume='80%'"));
        assert!(ssml.contains("你好世界"));
    }

    #[test]
    fn test_hotkey_display() {
        let config = HotkeyConfig {
            ctrl: true,
            alt: false,
            shift: true,
            meta: false,
            key: "KeyT".to_string(),
        };
        assert_eq!(config.display_string(), "Ctrl + Shift + T");

        let config2 = HotkeyConfig {
            ctrl: false,
            alt: true,
            shift: false,
            meta: true,
            key: "F9".to_string(),
        };
        assert_eq!(config2.display_string(), "Alt + Win + F9");
    }

    #[test]
    fn test_config_serde() {
        let cfg = AppConfig::default();
        let json = serde_json::to_string(&cfg).expect("Serialize config");
        let deserialized: AppConfig = serde_json::from_str(&json).expect("Deserialize config");
        assert_eq!(cfg.voice, deserialized.voice);
        assert_eq!(cfg.rate, deserialized.rate);
        assert_eq!(cfg.pitch, deserialized.pitch);
    }

    #[test]
    fn test_list_audio_devices() {
        let devices = edge_tts_tool::audio::list_output_devices();
        println!("Detected {} audio output devices.", devices.len());
        for dev in &devices {
            println!("  - Device: {} (is_default: {})", dev.name, dev.is_default);
        }
    }

    #[tokio::test]
    async fn test_edge_tts_synthesize() {
        let options = SpeakOptions {
            voice: "zh-CN-XiaoxiaoNeural".to_string(),
            rate: 0,
            pitch: 0,
            volume: 100,
        };
        let result = edge_tts_tool::edge_tts::synthesize("测试语音合成", &options).await;
        match result {
            Ok(audio_bytes) => {
                println!("Successfully synthesized {} bytes of audio!", audio_bytes.len());
                assert!(!audio_bytes.is_empty());
                // MP3 file header check (either ID3 tag or 0xFF sync word)
                let is_mp3 = audio_bytes.starts_with(b"ID3")
                    || (audio_bytes.len() >= 2 && audio_bytes[0] == 0xFF && (audio_bytes[1] & 0xE0) == 0xE0);
                assert!(is_mp3, "Expected valid MP3 audio header");
            }
            Err(e) => {
                println!("Notice: network synthesis test skipped or failed (offline / blocked): {}", e);
            }
        }
    }
}
