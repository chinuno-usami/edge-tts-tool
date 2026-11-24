#[cfg(test)]
mod tests {
    use edge_tts_tool::config::{AppConfig, OpenAiConfig, TtsSource};
    use edge_tts_tool::edge_tts::{
        build_ssml, escape_xml, generate_sec_ms_gec, generate_sec_ms_gec_at, SpeakOptions,
    };
    use edge_tts_tool::hotkey::HotkeyConfig;
    use edge_tts_tool::openai_tts;

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
        assert_eq!(
            escaped,
            "Hello &lt;world&gt; &amp; &quot;everyone&quot; &apos;here&apos;"
        );
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
        assert_eq!(cfg.source, deserialized.source);
        assert_eq!(cfg.openai.instruct, deserialized.openai.instruct);
        assert_eq!(cfg.openai.ref_audio, deserialized.openai.ref_audio);
        assert_eq!(cfg.openai.ref_text, deserialized.openai.ref_text);
    }

    #[test]
    fn test_config_old_json_compat() {
        // Pre-OpenAI fields only — must load with Edge defaults.
        let old = r#"{
            "voice": "zh-CN-YunxiNeural",
            "output_device": null,
            "rate": 10,
            "pitch": -2,
            "volume": 90,
            "hotkey": {
                "ctrl": true,
                "alt": false,
                "shift": true,
                "meta": false,
                "key": "KeyT"
            },
            "last_text": "hello"
        }"#;
        let cfg: AppConfig = serde_json::from_str(old).expect("old config must deserialize");
        assert_eq!(cfg.source, TtsSource::Edge);
        assert_eq!(cfg.voice, "zh-CN-YunxiNeural");
        assert_eq!(cfg.openai.model, "tts-1");
        assert!(cfg.openai.instruct.is_empty());
        assert!(cfg.openai.ref_audio.is_empty());
        assert!(cfg.openai.ref_text.is_empty());
        assert!(!cfg.openai.fetch_remote_voices);
        // Raw serde leaves edge_voice at its field default until migration runs.
        assert_eq!(cfg.edge_voice, "zh-CN-XiaoxiaoNeural");

        let migrated = AppConfig::from_json_value(
            serde_json::from_str(old).expect("old json value"),
        )
        .expect("migrated config");
        assert_eq!(migrated.voice, "zh-CN-YunxiNeural");
        assert_eq!(
            migrated.edge_voice, "zh-CN-YunxiNeural",
            "missing edge_voice must copy from voice"
        );

        // Missing instruct inside openai object should default to empty.
        let no_instruct = r#"{
            "voice": "alloy",
            "output_device": null,
            "rate": 0,
            "pitch": 0,
            "volume": 100,
            "hotkey": {
                "ctrl": true,
                "alt": false,
                "shift": true,
                "meta": false,
                "key": "KeyT"
            },
            "last_text": "",
            "source": "open_ai_compatible",
            "openai": {
                "base_url": "http://127.0.0.1:8081",
                "api_key": "",
                "model": "tts-1",
                "voice": ""
            },
            "edge_voice": "zh-CN-XiaoxiaoNeural"
        }"#;
        let cfg2: AppConfig =
            serde_json::from_str(no_instruct).expect("openai without instruct must deserialize");
        assert_eq!(cfg2.source, TtsSource::OpenAiCompatible);
        assert!(cfg2.openai.instruct.is_empty());
        assert_eq!(cfg2.openai.base_url, "http://127.0.0.1:8081");

        let cfg2_migrated = AppConfig::from_json_value(
            serde_json::from_str(no_instruct).expect("no_instruct value"),
        )
        .expect("migrated openai config");
        assert_eq!(cfg2_migrated.edge_voice, "zh-CN-XiaoxiaoNeural");
    }

    #[test]
    fn test_openai_url_and_speed_helpers() {
        assert_eq!(
            openai_tts::join_speech_url("http://127.0.0.1:8081"),
            "http://127.0.0.1:8081/v1/audio/speech"
        );
        assert!((openai_tts::rate_to_speed(0) - 1.0).abs() < f32::EPSILON);
        assert!((openai_tts::rate_to_speed(-50) - 0.5).abs() < f32::EPSILON);
        assert!(openai_tts::model_requires_voice("tts-1"));
        assert!(!openai_tts::model_requires_voice(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16"
        ));
        assert!(!openai_tts::model_supports_instruct("tts-1-hd"));
        assert!(openai_tts::model_supports_voice_clone_refs(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16"
        ));
        assert!(!openai_tts::model_supports_voice_clone_refs(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16"
        ));
        assert!(!openai_tts::model_supports_voice_clone_refs("tts-1"));
        assert!(openai_tts::model_should_omit_classic_voice(
            "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16"
        ));
        assert!(!openai_tts::model_should_omit_classic_voice("tts-1"));
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
                println!(
                    "Successfully synthesized {} bytes of audio!",
                    audio_bytes.len()
                );
                assert!(!audio_bytes.is_empty());
                let is_mp3 = audio_bytes.starts_with(b"ID3")
                    || (audio_bytes.len() >= 2
                        && audio_bytes[0] == 0xFF
                        && (audio_bytes[1] & 0xE0) == 0xE0);
                assert!(is_mp3, "Expected valid MP3 audio header");
            }
            Err(e) => {
                println!(
                    "Notice: network synthesis test skipped or failed (offline / blocked): {}",
                    e
                );
            }
        }
    }

    /// Local mlx-audio acceptance. Run with:
    /// ```ignore
    /// OPENAI_BASE_URL=http://127.0.0.1:8081 \
    /// OPENAI_MODEL=mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16 \
    /// OPENAI_INSTRUCT='温柔女声，语速适中，带一点微笑感' \
    ///   cargo test test_openai_tts_synthesize_mlx_audio -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_openai_tts_synthesize_mlx_audio() {
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8081".to_string());
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| {
            "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16".to_string()
        });
        let instruct = std::env::var("OPENAI_INSTRUCT")
            .unwrap_or_else(|_| "温柔女声，语速适中，带一点微笑感".to_string());

        let cfg = OpenAiConfig {
            base_url,
            api_key: String::new(),
            model,
            voice: String::new(),
            instruct,
            ref_audio: std::env::var("OPENAI_REF_AUDIO").unwrap_or_default(),
            ref_text: std::env::var("OPENAI_REF_TEXT").unwrap_or_default(),
            fetch_remote_voices: false,
        };

        let audio = openai_tts::synthesize("你好，这是音色定制测试。", 0, &cfg)
            .await
            .expect("mlx-audio speech should succeed when service is running");
        assert!(!audio.is_empty(), "expected non-empty audio bytes");

        let is_mp3 = audio.starts_with(b"ID3")
            || (audio.len() >= 2 && audio[0] == 0xFF && (audio[1] & 0xE0) == 0xE0);
        assert!(is_mp3, "expected MP3-like header from mlx-audio");
        println!("mlx-audio synthesis ok: {} bytes", audio.len());
    }
}
