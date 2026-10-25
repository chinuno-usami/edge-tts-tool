#[cfg(test)]
mod tests {
    use edge_tts_tool::config::AppConfig;
    use edge_tts_tool::edge_tts::{build_ssml, escape_xml, generate_sec_ms_gec, generate_sec_ms_gec_at, SpeakOptions};
    use edge_tts_tool::hotkey::HotkeyConfig;

    #[test]
    fn test_sec_ms_gec_format() {
        let token = generate_sec_ms_gec();
        let parts: Vec<&str> = token.split('-').collect();
        assert_eq!(parts.len(), 2, "v2 token must be <cs64>-<sha256>: {}", token);
        assert_eq!(parts[0].len(), 16);
        assert_eq!(parts[1].len(), 64);
        assert!(token
            .chars()
            .all(|c| c == '-' || (c.is_ascii_hexdigit() && !c.is_ascii_lowercase())));
    }

    #[test]
    fn test_sec_ms_gec_known_vectors() {
        // 参考向量来自 edge-tts 7.3.1 的 DRM.generate_sec_ms_gec_v2()
        assert_eq!(
            generate_sec_ms_gec_at(1750000000),
            "B029A7426816F097-4579D112DC47FC3BF432B499C0A282F427C659474AF9BF5FA33CE2C003F9E9C2"
        );
        assert_eq!(
            generate_sec_ms_gec_at(1700000000),
            "2CD23A2CA0B2595A-F3C846C370E05F9CC83ACC5F995C5F5D87A4D0F14CE8936A0F902E64099E38A2"
        );
        assert_eq!(
            generate_sec_ms_gec_at(1234567890),
            "BA405D1577B8E0CE-61F01E88539977A1A1E969572D82EECFA45FB1E5DEBD122D6ACBAF0054F959E5"
        );
        assert_eq!(
            generate_sec_ms_gec_at(1800000000),
            "99E54519B490BE5C-E162DB0977FBDCB9B86DD8148BF1BCDC3C0303176CE39852A42B415935C20654"
        );
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
