use crate::audio::{list_output_devices, AudioController, AudioDeviceInfo, AudioEvent};
use crate::config::{AppConfig, TtsSource};
use crate::edge_tts::{self, SpeakOptions, Voice};
use crate::hotkey::{AppHotkeyManager, HotkeyConfig};
use crate::openai_tts;
use crossbeam_channel::{unbounded, Receiver, Sender};
use egui::{Align, Color32, Layout, RichText, ScrollArea, Vec2};
use std::sync::Arc;
use tokio::runtime::Runtime;

const SPEAK_SHORTCUT_LABEL: &str = if cfg!(target_os = "macos") {
    "Cmd+Enter"
} else {
    "Ctrl+Enter"
};
const MIN_TEXT_HEIGHT: f32 = 120.0;
const COMPACT_WIDTH: f32 = 640.0;

const OPENAI_MODEL_PRESETS: &[&str] = &[
    "tts-1",
    "tts-1-hd",
    "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16",
    "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16",
];

fn main_text_id() -> egui::Id {
    egui::Id::new("main_text_edit")
}

fn voice_filter_id() -> egui::Id {
    egui::Id::new("voice_filter_edit")
}

fn prosody_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
    suffix: &str,
) -> egui::Response {
    ui.label(label);
    ui.add(egui::Slider::new(value, range).suffix(suffix))
}

enum AsyncMessage {
    VoicesLoaded {
        source: TtsSource,
        generation: u64,
        voices: Vec<Voice>,
    },
    VoicesLoadFailed {
        source: TtsSource,
        generation: u64,
        error: String,
    },
    SynthesisSuccess {
        generation: u64,
        audio: Vec<u8>,
        auto_play: bool,
    },
    SynthesisFailed {
        generation: u64,
        error: String,
    },
}

pub struct TtsApp {
    config: AppConfig,
    text: String,

    // Voices
    voices: Vec<Voice>,
    selected_voice_name: String,
    voice_filter: String,
    is_loading_voices: bool,
    /// Bumped on each voice-list refresh to ignore stale async replies.
    voice_fetch_generation: u64,

    // Cached dropdown data (recomputed only when the filter or voice list changes,
    // NOT on every frame — this used to be rebuilt every repaint).
    filtered_voices: Vec<(String, String)>,
    selected_display: String,

    // Cached text length (updated only when the text changes).
    text_len: usize,

    // Audio Output Devices
    output_devices: Vec<AudioDeviceInfo>,
    selected_device_name: Option<String>,
    audio_controller: AudioController,

    // Speech Prosody
    rate: i32,
    pitch: i32,
    volume: i32,

    // Playback & Synthesis Status
    status_text: String,
    status_color: Color32,
    is_playing: bool,
    is_synthesizing: bool,
    /// Bumped on each synthesis start and source switch to ignore stale async results.
    synthesis_generation: u64,
    last_audio: Option<Vec<u8>>,
    // Text cleared by the speak shortcut; restored if that synthesis fails.
    pending_cleared_text: Option<String>,

    // Global Hotkey
    hotkey_manager: AppHotkeyManager,
    is_minimized: bool,
    show_hotkey_dialog: bool,
    temp_hotkey_ctrl: bool,
    temp_hotkey_alt: bool,
    temp_hotkey_shift: bool,
    temp_hotkey_meta: bool,
    temp_hotkey_key: String,

    // Async Channel & Tokio Runtime
    async_tx: Sender<AsyncMessage>,
    async_rx: Receiver<AsyncMessage>,
    runtime: Arc<Runtime>,
    // Used by background tasks to wake the UI; there is no periodic repaint.
    egui_ctx: egui::Context,
}

impl TtsApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let egui_ctx = cc.egui_ctx.clone();
        let config = AppConfig::load();
        let (async_tx, async_rx) = unbounded();
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to initialize tokio runtime"),
        );

        let hotkey_ctx = egui_ctx.clone();
        let mut hotkey_manager = AppHotkeyManager::new(move || hotkey_ctx.request_repaint());
        if let Err(e) = hotkey_manager.register(&config.hotkey) {
            log::warn!("Initial hotkey registration notice: {}", e);
        }

        let (preset_voices, selected_voice_name) = match config.source {
            TtsSource::Edge => {
                let presets = edge_tts::get_preset_voices();
                let name = if presets.iter().any(|v| v.short_name == config.voice) {
                    config.voice.clone()
                } else if presets.iter().any(|v| v.short_name == config.edge_voice) {
                    config.edge_voice.clone()
                } else {
                    presets
                        .first()
                        .map(|v| v.short_name.clone())
                        .unwrap_or_else(|| "zh-CN-XiaoxiaoNeural".to_string())
                };
                (presets, name)
            }
            TtsSource::OpenAiCompatible => {
                let presets = openai_tts::get_preset_voices();
                let name = config.openai.voice.clone();
                (presets, name)
            }
        };

        let devices = list_output_devices();
        let selected_device_name = config.output_device.clone();

        let mut app = Self {
            text: config.last_text.clone(),
            rate: config.rate,
            pitch: config.pitch,
            volume: config.volume,
            selected_voice_name,
            selected_device_name,
            voices: preset_voices,
            voice_filter: String::new(),
            is_loading_voices: false,
            voice_fetch_generation: 0,
            filtered_voices: Vec::new(),
            selected_display: String::new(),
            text_len: config.last_text.chars().count(),
            output_devices: devices,
            audio_controller: {
                let audio_ctx = egui_ctx.clone();
                AudioController::new(move || audio_ctx.request_repaint())
            },
            status_text: "就绪".to_string(),
            status_color: Color32::from_rgb(100, 180, 100),
            is_playing: false,
            is_synthesizing: false,
            synthesis_generation: 0,
            last_audio: None,
            pending_cleared_text: None,
            hotkey_manager,
            is_minimized: false,
            show_hotkey_dialog: false,
            temp_hotkey_ctrl: config.hotkey.ctrl,
            temp_hotkey_alt: config.hotkey.alt,
            temp_hotkey_shift: config.hotkey.shift,
            temp_hotkey_meta: config.hotkey.meta,
            temp_hotkey_key: config.hotkey.key.clone(),
            config,
            async_tx,
            async_rx,
            runtime,
            egui_ctx,
        };

        app.recompute_voice_cache();

        match app.config.source {
            TtsSource::Edge => {
                app.is_loading_voices = true;
                app.refresh_voices_async();
            }
            TtsSource::OpenAiCompatible => {
                if app.config.openai.fetch_remote_voices {
                    app.is_loading_voices = true;
                    app.refresh_voices_async();
                }
            }
        }

        app
    }

    fn refresh_devices(&mut self) {
        self.output_devices = list_output_devices();
        self.status_text = format!(
            "音频输出设备列表已刷新 (共 {} 个设备)",
            self.output_devices.len()
        );
        self.status_color = Color32::LIGHT_BLUE;
    }

    fn recompute_voice_cache(&mut self) {
        let filter_lower = self.voice_filter.to_lowercase();
        self.filtered_voices = self
            .voices
            .iter()
            .filter(|v| {
                filter_lower.is_empty()
                    || v.short_name.to_lowercase().contains(&filter_lower)
                    || v.friendly_name.to_lowercase().contains(&filter_lower)
                    || v.locale.to_lowercase().contains(&filter_lower)
            })
            .map(|v| (v.short_name.clone(), v.display_name()))
            .collect();

        self.selected_display = self
            .voices
            .iter()
            .find(|v| v.short_name == self.selected_voice_name)
            .map(|v| v.display_name())
            .unwrap_or_else(|| {
                if self.selected_voice_name.is_empty() {
                    "（空 / 自定义）".to_string()
                } else {
                    format!("{} (自定义)", self.selected_voice_name)
                }
            });
    }

    fn switch_source(&mut self, new_source: TtsSource) {
        if self.config.source == new_source {
            return;
        }

        // Invalidate in-flight synthesis / voice fetch so late replies cannot update the new source.
        self.synthesis_generation = self.synthesis_generation.wrapping_add(1);
        self.voice_fetch_generation = self.voice_fetch_generation.wrapping_add(1);
        if self.is_synthesizing {
            self.is_synthesizing = false;
            if let Some(cleared) = self.pending_cleared_text.take() {
                self.restore_cleared_text(cleared);
            }
        }
        self.is_loading_voices = false;

        match self.config.source {
            TtsSource::Edge => {
                self.config.edge_voice = self.selected_voice_name.clone();
            }
            TtsSource::OpenAiCompatible => {
                self.config.openai.voice = self.selected_voice_name.clone();
            }
        }

        self.config.source = new_source;
        self.voice_filter.clear();

        match self.config.source {
            TtsSource::Edge => {
                self.voices = edge_tts::get_preset_voices();
                self.selected_voice_name = self.config.edge_voice.clone();
                self.recompute_voice_cache();
                self.persist_config();
                self.is_loading_voices = true;
                self.status_text = "已切换到 Edge-TTS，正在刷新云端语音...".to_string();
                self.status_color = Color32::LIGHT_BLUE;
                self.refresh_voices_async();
            }
            TtsSource::OpenAiCompatible => {
                self.voices = openai_tts::get_preset_voices();
                self.selected_voice_name = self.config.openai.voice.clone();
                self.recompute_voice_cache();
                self.persist_config();
                self.status_text = "已切换到 OpenAI 兼容源".to_string();
                self.status_color = Color32::LIGHT_BLUE;
                if self.config.openai.fetch_remote_voices {
                    self.is_loading_voices = true;
                    self.refresh_voices_async();
                }
            }
        }
    }

    fn refresh_voices_async(&mut self) {
        self.voice_fetch_generation = self.voice_fetch_generation.wrapping_add(1);
        let generation = self.voice_fetch_generation;
        let tx = self.async_tx.clone();
        let ctx = self.egui_ctx.clone();
        let source = self.config.source.clone();
        let openai_cfg = self.config.openai.clone();

        self.runtime.spawn(async move {
            let result = match source {
                TtsSource::Edge => edge_tts::fetch_voices_list().await,
                TtsSource::OpenAiCompatible => {
                    if openai_cfg.fetch_remote_voices {
                        // Propagate errors / empty lists so the UI can distinguish
                        // success, empty remote catalog, and fallback-on-failure.
                        openai_tts::fetch_voices_list(&openai_cfg).await
                    } else {
                        Ok(openai_tts::get_preset_voices())
                    }
                }
            };

            match result {
                Ok(voices) => {
                    let _ = tx.send(AsyncMessage::VoicesLoaded {
                        source,
                        generation,
                        voices,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncMessage::VoicesLoadFailed {
                        source,
                        generation,
                        error: e.to_string(),
                    });
                }
            }
            ctx.request_repaint();
        });
    }

    fn start_synthesis(&mut self, auto_play: bool) {
        if self.text.trim().is_empty() {
            self.status_text = "请输入要朗读的文本内容".to_string();
            self.status_color = Color32::YELLOW;
            return;
        }

        if self.is_synthesizing {
            return;
        }

        if matches!(self.config.source, TtsSource::OpenAiCompatible) {
            if self.config.openai.base_url.trim().is_empty() {
                self.status_text = "请填写 OpenAI 兼容源的 Base URL".to_string();
                self.status_color = Color32::YELLOW;
                return;
            }
            if self.config.openai.model.trim().is_empty() {
                self.status_text = "请填写 OpenAI 兼容源的 Model".to_string();
                self.status_color = Color32::YELLOW;
                return;
            }
            if openai_tts::model_requires_voice(&self.config.openai.model)
                && self.selected_voice_name.trim().is_empty()
            {
                self.status_text =
                    "经典 TTS 模型（如 tts-1）需要选择或填写 Voice".to_string();
                self.status_color = Color32::YELLOW;
                return;
            }
            let ref_audio = self.config.openai.ref_audio.trim();
            if !ref_audio.is_empty()
                && openai_tts::model_supports_voice_clone_refs(&self.config.openai.model)
                && !std::path::Path::new(ref_audio).is_file()
            {
                self.status_text =
                    format!("参考音频文件不存在或不可读: {}", ref_audio);
                self.status_color = Color32::YELLOW;
                return;
            }
        }

        self.synthesis_generation = self.synthesis_generation.wrapping_add(1);
        let generation = self.synthesis_generation;
        self.is_synthesizing = true;
        self.status_text = match self.config.source {
            TtsSource::Edge => "正在调用 Edge-TTS 生成音频...".to_string(),
            TtsSource::OpenAiCompatible => "正在合成音频...".to_string(),
        };
        self.status_color = Color32::LIGHT_BLUE;

        let text = self.text.clone();
        let tx = self.async_tx.clone();
        let ctx = self.egui_ctx.clone();
        let source = self.config.source.clone();
        let rate = self.rate;
        let pitch = self.pitch;
        let volume = self.volume;
        let voice = self.selected_voice_name.clone();
        let mut openai_cfg = self.config.openai.clone();
        openai_cfg.voice = voice.clone();

        self.runtime.spawn(async move {
            let result = match source {
                TtsSource::Edge => {
                    let options = SpeakOptions {
                        voice,
                        rate,
                        pitch,
                        volume,
                    };
                    edge_tts::synthesize(&text, &options).await
                }
                TtsSource::OpenAiCompatible => {
                    openai_tts::synthesize(&text, rate, &openai_cfg).await
                }
            };

            match result {
                Ok(audio) => {
                    let _ = tx.send(AsyncMessage::SynthesisSuccess {
                        generation,
                        audio,
                        auto_play,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncMessage::SynthesisFailed {
                        generation,
                        error: e.to_string(),
                    });
                }
            }
            ctx.request_repaint();
        });
    }

    /// 用当前文本框内容开始 TTS 并清空输入；合成失败时恢复被清空的文本。
    fn speak_and_clear_text(&mut self) {
        if self.is_synthesizing {
            self.status_text = "正在生成上一段音频，请稍候...".to_string();
            self.status_color = Color32::YELLOW;
            return;
        }
        self.start_synthesis(true);
        if !self.is_synthesizing {
            return;
        }
        self.pending_cleared_text = Some(std::mem::take(&mut self.text));
        self.text_len = 0;
        self.persist_config();
    }

    fn restore_cleared_text(&mut self, cleared: String) {
        if self.text.is_empty() {
            self.text = cleared;
        } else {
            self.text = format!("{}\n{}", cleared, self.text);
        }
        self.text_len = self.text.chars().count();
        self.persist_config();
    }

    fn stop_playback(&mut self) {
        self.audio_controller.stop();
        self.is_playing = false;
        self.status_text = "播放已停止".to_string();
        self.status_color = Color32::GRAY;
    }

    fn save_audio_file(&self) {
        if let Some(ref audio) = self.last_audio {
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("tts_output.mp3")
                .add_filter("MP3 Audio (*.mp3)", &["mp3"])
                .save_file()
            {
                match std::fs::write(&path, audio) {
                    Ok(_) => {
                        log::info!("Audio saved to {:?}", path);
                    }
                    Err(e) => {
                        log::error!("Failed to save audio: {}", e);
                    }
                }
            }
        }
    }

    fn persist_config(&mut self) {
        self.config.output_device = self.selected_device_name.clone();
        self.config.rate = self.rate;
        self.config.pitch = self.pitch;
        self.config.volume = self.volume;
        self.config.last_text = self.text.clone();

        match self.config.source {
            TtsSource::Edge => {
                // Keep legacy `voice` Edge-only for backward-compatible configs.
                self.config.voice = self.selected_voice_name.clone();
                self.config.edge_voice = self.selected_voice_name.clone();
            }
            TtsSource::OpenAiCompatible => {
                self.config.openai.voice = self.selected_voice_name.clone();
            }
        }

        self.config.save();
    }

    /// VoiceDesign / Base models should not keep leftover classic voices like `alloy`.
    fn clear_classic_voice_if_needed(&mut self) -> bool {
        if !matches!(self.config.source, TtsSource::OpenAiCompatible) {
            return false;
        }
        if !openai_tts::model_should_omit_classic_voice(&self.config.openai.model) {
            return false;
        }
        if self.selected_voice_name.is_empty() && self.config.openai.voice.is_empty() {
            return false;
        }
        self.selected_voice_name.clear();
        self.config.openai.voice.clear();
        self.recompute_voice_cache();
        true
    }
}

impl eframe::App for TtsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Check window minimized state from viewport if reported
        if let Some(minimized) = ctx.input(|i| i.viewport().minimized) {
            self.is_minimized = minimized;
        }

        // 2. Poll global hotkey event
        if self.hotkey_manager.poll_event() {
            self.is_minimized = !self.is_minimized;
            if self.is_minimized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }

        // 3. Poll audio controller events
        while let Some(event) = self.audio_controller.try_recv_event() {
            match event {
                AudioEvent::PlaybackStarted => {
                    self.is_playing = true;
                    self.status_text = "正在播放音频...".to_string();
                    self.status_color = Color32::from_rgb(100, 220, 100);
                }
                AudioEvent::PlaybackFinished => {
                    self.is_playing = false;
                    self.status_text = "播放已完成".to_string();
                    self.status_color = Color32::LIGHT_GRAY;
                }
                AudioEvent::PlaybackError(err) => {
                    self.is_playing = false;
                    self.status_text = format!("播放错误: {}", err);
                    self.status_color = Color32::RED;
                }
            }
        }

        // 4. Poll async channel messages (voice loading & synthesis)
        while let Ok(msg) = self.async_rx.try_recv() {
            match msg {
                AsyncMessage::VoicesLoaded {
                    source,
                    generation,
                    voices: fetched_voices,
                } => {
                    // Ignore stale replies from a previous TTS source / older refresh.
                    if source != self.config.source
                        || generation != self.voice_fetch_generation
                    {
                        continue;
                    }
                    self.is_loading_voices = false;
                    if !fetched_voices.is_empty() {
                        self.voices = fetched_voices;
                        self.recompute_voice_cache();
                        self.status_text = match self.config.source {
                            TtsSource::Edge => {
                                format!("已加载 Edge 云端语音 (共 {} 个)", self.voices.len())
                            }
                            TtsSource::OpenAiCompatible => {
                                format!("已加载 OpenAI 兼容语音 (共 {} 个)", self.voices.len())
                            }
                        };
                        self.status_color = Color32::LIGHT_BLUE;
                    } else if matches!(self.config.source, TtsSource::OpenAiCompatible) {
                        self.status_text =
                            "远程语音列表为空，可留空 Voice 并使用 Instruct 定制音色".to_string();
                        self.status_color = Color32::LIGHT_BLUE;
                    }
                }
                AsyncMessage::VoicesLoadFailed {
                    source,
                    generation,
                    error: err,
                } => {
                    if source != self.config.source
                        || generation != self.voice_fetch_generation
                    {
                        continue;
                    }
                    self.is_loading_voices = false;
                    log::warn!("Failed to load online voices: {}", err);
                    match self.config.source {
                        TtsSource::OpenAiCompatible => {
                            self.voices = openai_tts::get_preset_voices();
                            self.recompute_voice_cache();
                            self.status_text =
                                format!("远程语音拉取失败，已回退预设: {}", err);
                            self.status_color = Color32::YELLOW;
                        }
                        TtsSource::Edge => {
                            self.status_text =
                                format!("云端语音拉取失败，仍使用本地预设: {}", err);
                            self.status_color = Color32::YELLOW;
                        }
                    }
                }
                AsyncMessage::SynthesisSuccess {
                    generation,
                    audio,
                    auto_play,
                } => {
                    if generation != self.synthesis_generation {
                        continue;
                    }
                    self.is_synthesizing = false;
                    self.pending_cleared_text = None;
                    let size_kb = audio.len() as f64 / 1024.0;
                    self.status_text = format!("音频合成完毕 ({:.1} KB)", size_kb);
                    self.status_color = Color32::from_rgb(100, 220, 100);
                    self.last_audio = Some(audio.clone());

                    if auto_play {
                        let volume = (self.volume as f32) / 100.0;
                        self.audio_controller.play(
                            audio,
                            self.selected_device_name.clone(),
                            volume,
                        );
                    }
                }
                AsyncMessage::SynthesisFailed { generation, error: err } => {
                    if generation != self.synthesis_generation {
                        continue;
                    }
                    self.is_synthesizing = false;
                    if let Some(cleared) = self.pending_cleared_text.take() {
                        self.restore_cleared_text(cleared);
                        self.status_text = format!("合成失败（已恢复输入文本）: {}", err);
                    } else {
                        self.status_text = format!("合成失败: {}", err);
                    }
                    self.status_color = Color32::RED;
                }
            }
        }

        // 5. Ctrl/Cmd+Enter：朗读当前文本并清空输入（在 TextEdit 之前消费，避免插入换行）。
        let shortcut_allowed = !self.show_hotkey_dialog
            && ctx.memory(|m| m.focused()) != Some(voice_filter_id());
        let speak_shortcut = shortcut_allowed
            && ctx.input_mut(|i| {
                !i.modifiers.shift
                    && !i.modifiers.alt
                    && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
            });
        if speak_shortcut {
            self.speak_and_clear_text();
        }

        // 6. Bottom action bar
        egui::TopBottomPanel::bottom("action_bar").show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let play_btn_text = if self.is_synthesizing {
                    "⏳ 正在生成..."
                } else if self.is_playing {
                    "🔄 重新朗读"
                } else {
                    "▶ 朗读 / 播放"
                };

                let play_enabled = !self.is_synthesizing && !self.text.trim().is_empty();
                if ui
                    .add_enabled(
                        play_enabled,
                        egui::Button::new(RichText::new(play_btn_text).size(16.0).strong()),
                    )
                    .on_hover_text(format!(
                        "点击朗读当前文本；{}：朗读并清空输入框",
                        SPEAK_SHORTCUT_LABEL
                    ))
                    .clicked()
                {
                    self.start_synthesis(true);
                }

                if self.is_playing
                    && ui
                        .add(egui::Button::new(
                            RichText::new("⏹ 停止播放").size(16.0).color(Color32::LIGHT_RED),
                        ))
                        .clicked()
                {
                    self.stop_playback();
                }

                let save_enabled = self.last_audio.is_some();
                if ui
                    .add_enabled(
                        save_enabled,
                        egui::Button::new(RichText::new("💾 导出 MP3 文件").size(15.0)),
                    )
                    .on_hover_text("将当前合成的音频保存至本地磁盘")
                    .clicked()
                {
                    self.save_audio_file();
                }

                if ui
                    .button(RichText::new("🗕 最小化隐藏").size(13.0))
                    .on_hover_text("最小化隐藏窗口，可通过快捷键随时唤出")
                    .clicked()
                {
                    self.is_minimized = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            });
            ui.add_space(4.0);
        });

        // 7. Central panel
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            let viewport_height = ui.available_height();

            ScrollArea::vertical()
                .id_salt("main_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let content_top = ui.cursor().top();
                    self.show_settings(ui);
                    self.show_text_input(ui, viewport_height - (ui.cursor().top() - content_top));
                });
        });

        // 8. Hotkey Configuration Modal Window
        if self.show_hotkey_dialog {
            self.show_hotkey_window(ctx);
        }
    }
}

impl TtsApp {
    fn show_settings(&mut self, ui: &mut egui::Ui) {
        // --- Top Header ---
        ui.horizontal_wrapped(|ui| {
            ui.heading(RichText::new("🔊 TTS 语音合成").strong());
            let hotkey_str = self.config.hotkey.display_string();
            if ui
                .button(RichText::new(format!("⌨ 快捷键: {}", hotkey_str)).size(12.0))
                .on_hover_text("点击配置全局显示/隐藏快捷键")
                .clicked()
            {
                self.show_hotkey_dialog = !self.show_hotkey_dialog;
            }
            ui.label(RichText::new(&self.status_text).color(self.status_color).size(13.0));
        });

        ui.separator();

        // --- TTS Source ---
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("📡 TTS 源:").strong());
                let mut source = self.config.source.clone();
                let mut changed = false;
                egui::ComboBox::from_id_salt("tts_source_combo")
                    .selected_text(match source {
                        TtsSource::Edge => "Edge-TTS",
                        TtsSource::OpenAiCompatible => "OpenAI 兼容",
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(&mut source, TtsSource::Edge, "Edge-TTS")
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .selectable_value(
                                &mut source,
                                TtsSource::OpenAiCompatible,
                                "OpenAI 兼容",
                            )
                            .changed()
                        {
                            changed = true;
                        }
                    });
                if changed {
                    self.switch_source(source);
                }
            });
        });

        // --- OpenAI Compatible settings ---
        if matches!(self.config.source, TtsSource::OpenAiCompatible) {
            self.show_openai_settings(ui);
        }

        // --- Audio Device Selection Bar ---
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("🎧 输出声卡 / 设备:").strong());

                let current_device_label = self
                    .selected_device_name
                    .as_deref()
                    .unwrap_or("默认音频输出设备 (System Default)");

                let combo_width = (ui.available_width() - 110.0).clamp(160.0, 360.0);
                let mut selected_dev_to_set = None;
                egui::ComboBox::from_id_salt("audio_device_combo")
                    .width(combo_width)
                    .truncate()
                    .selected_text(current_device_label)
                    .show_ui(ui, |ui| {
                        let is_default_selected = self.selected_device_name.is_none();
                        if ui
                            .selectable_label(
                                is_default_selected,
                                "默认音频输出设备 (System Default)",
                            )
                            .clicked()
                        {
                            selected_dev_to_set = Some(None);
                        }

                        for dev in &self.output_devices {
                            let label = if dev.is_default {
                                format!("{} [系统默认]", dev.name)
                            } else {
                                dev.name.clone()
                            };

                            let is_selected =
                                self.selected_device_name.as_deref() == Some(&dev.name);
                            if ui.selectable_label(is_selected, label).clicked() {
                                selected_dev_to_set = Some(Some(dev.name.clone()));
                            }
                        }
                    });

                if let Some(new_dev) = selected_dev_to_set {
                    self.selected_device_name = new_dev;
                    self.persist_config();
                }

                if ui.button("🔄 刷新设备").clicked() {
                    self.refresh_devices();
                }
            });
        });

        // --- Voice Selection & Parameter Sliders ---
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("🗣 语音角色:").strong());

                let filter_width = (ui.available_width() * 0.35).clamp(120.0, 180.0);
                let filter_changed = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.voice_filter)
                            .id(voice_filter_id())
                            .hint_text("🔍 过滤角色...")
                            .desired_width(filter_width),
                    )
                    .changed();
                if filter_changed {
                    self.recompute_voice_cache();
                }

                let combo_width = (ui.available_width() - 130.0).clamp(160.0, 260.0);
                let mut selected_voice_to_set = None;
                egui::ComboBox::from_id_salt("voice_combo")
                    .width(combo_width)
                    .truncate()
                    .selected_text(self.selected_display.as_str())
                    .show_ui(ui, |ui| {
                        if matches!(self.config.source, TtsSource::OpenAiCompatible) {
                            if ui
                                .selectable_label(self.selected_voice_name.is_empty(), "（留空）")
                                .clicked()
                            {
                                selected_voice_to_set = Some(String::new());
                            }
                        }
                        for (short_name, display_name) in self.filtered_voices.iter() {
                            let is_selected = self.selected_voice_name == *short_name;
                            if ui.selectable_label(is_selected, display_name).clicked() {
                                selected_voice_to_set = Some(short_name.clone());
                            }
                        }
                    });

                if let Some(new_voice) = selected_voice_to_set {
                    self.selected_voice_name = new_voice;
                    self.recompute_voice_cache();
                    self.persist_config();
                }

                if matches!(self.config.source, TtsSource::OpenAiCompatible) {
                    let mut custom = self.selected_voice_name.clone();
                    let custom_width = (ui.available_width() * 0.25).clamp(80.0, 160.0);
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut custom)
                                .hint_text("自定义 voice")
                                .desired_width(custom_width),
                        )
                        .changed()
                    {
                        self.selected_voice_name = custom;
                        self.recompute_voice_cache();
                        self.persist_config();
                    }
                }

                let refresh_label = if matches!(self.config.source, TtsSource::Edge) {
                    "☁ 刷新云端语音"
                } else {
                    "☁ 刷新语音列表"
                };
                let refresh_hover = if matches!(self.config.source, TtsSource::Edge) {
                    "从 Microsoft Edge 服务器拉取最新完整语音列表"
                } else {
                    "加载 OpenAI 预设语音；若开启远程拉取则请求 /v1/audio/voices"
                };
                if ui
                    .button(refresh_label)
                    .on_hover_text(refresh_hover)
                    .clicked()
                {
                    self.is_loading_voices = true;
                    self.status_text = match self.config.source {
                        TtsSource::Edge => "正在获取微软云端全部语音列表...".to_string(),
                        TtsSource::OpenAiCompatible => "正在刷新 OpenAI 兼容语音列表...".to_string(),
                    };
                    self.status_color = Color32::LIGHT_BLUE;
                    self.refresh_voices_async();
                }

                if self.is_loading_voices {
                    ui.spinner();
                }
            });

            ui.add_space(4.0);
            self.show_prosody_controls(ui);
        });
    }

    fn show_openai_settings(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("⚙ OpenAI 兼容配置").strong());

            let mut dirty = false;

            ui.horizontal_wrapped(|ui| {
                ui.label("Base URL:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.config.openai.base_url)
                        .hint_text("http://127.0.0.1:8081 或 https://api.openai.com")
                        .desired_width((ui.available_width() - 20.0).clamp(200.0, 520.0)),
                );
                if resp.changed() || resp.lost_focus() {
                    dirty = true;
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label("API Key:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.config.openai.api_key)
                        .password(true)
                        .hint_text("本地服务可留空")
                        .desired_width((ui.available_width() - 20.0).clamp(200.0, 520.0)),
                );
                // Persist only on blur so keystrokes are not written to disk every frame.
                if resp.lost_focus() {
                    dirty = true;
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label("Model:");
                let model_width = (ui.available_width() - 120.0).clamp(180.0, 420.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.config.openai.model)
                        .hint_text("tts-1 / VoiceDesign 全名")
                        .desired_width(model_width),
                );
                if resp.changed() || resp.lost_focus() {
                    let _ = self.clear_classic_voice_if_needed();
                    dirty = true;
                }

                let mut picked: Option<&str> = None;
                egui::ComboBox::from_id_salt("openai_model_preset")
                    .selected_text("快捷")
                    .show_ui(ui, |ui| {
                        for m in OPENAI_MODEL_PRESETS {
                            if ui
                                .selectable_label(self.config.openai.model == *m, *m)
                                .clicked()
                            {
                                picked = Some(*m);
                            }
                        }
                    });
                if let Some(m) = picked {
                    self.config.openai.model = m.to_string();
                    let _ = self.clear_classic_voice_if_needed();
                    dirty = true;
                }
            });

            ui.label("Instruct（音色描述）:");
            let instruct_resp = ui.add(
                egui::TextEdit::multiline(&mut self.config.openai.instruct)
                    .hint_text("用自然语言描述目标音色，例如：温柔女声，语速适中，带一点微笑感")
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if instruct_resp.changed() || instruct_resp.lost_focus() {
                dirty = true;
            }

            if self.config.openai.model.to_lowercase().contains("voicedesign")
                && self.config.openai.instruct.trim().is_empty()
            {
                ui.label(
                    RichText::new("提示：VoiceDesign 模型建议填写 Instruct 以定制音色")
                        .size(12.0)
                        .color(Color32::YELLOW),
                );
            }

            if openai_tts::model_requires_voice(&self.config.openai.model)
                && self.selected_voice_name.trim().is_empty()
            {
                ui.label(
                    RichText::new("提示：tts-1 / tts-1-hd 等经典模型需要填写 Voice")
                        .size(12.0)
                        .color(Color32::YELLOW),
                );
            }

            if openai_tts::model_supports_voice_clone_refs(&self.config.openai.model) {
                ui.separator();
                ui.label(RichText::new("🎙 语音克隆（Qwen3-TTS Base）").strong());
                ui.label(
                    RichText::new(
                        "mlx-audio 需要能读取本地路径；克隆时请使用 Base 模型，且不要依赖 Voice",
                    )
                    .size(12.0)
                    .color(Color32::GRAY),
                );

                ui.horizontal_wrapped(|ui| {
                    ui.label("Ref Audio:");
                    let path_width = (ui.available_width() - 140.0).clamp(160.0, 420.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.config.openai.ref_audio)
                            .hint_text("/path/to/reference.wav")
                            .desired_width(path_width),
                    );
                    if resp.changed() || resp.lost_focus() {
                        dirty = true;
                    }
                    if ui.button("选择…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "Audio",
                                &["wav", "mp3", "flac", "m4a", "ogg", "aac"],
                            )
                            .pick_file()
                        {
                            self.config.openai.ref_audio = path.display().to_string();
                            dirty = true;
                        }
                    }
                    if ui.button("清除").clicked() {
                        self.config.openai.ref_audio.clear();
                        dirty = true;
                    }
                });

                ui.label("Ref Text（参考音频逐字稿）:");
                let ref_text_resp = ui.add(
                    egui::TextEdit::multiline(&mut self.config.openai.ref_text)
                        .hint_text("与参考音频内容一致的文本，例如：这是我说话的声音。")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                if ref_text_resp.changed() || ref_text_resp.lost_focus() {
                    dirty = true;
                }

                let has_refs = !self.config.openai.ref_audio.trim().is_empty()
                    || !self.config.openai.ref_text.trim().is_empty();
                if has_refs && !self.selected_voice_name.trim().is_empty() {
                    ui.label(
                        RichText::new(
                            "提示：已填写克隆参考时，请求将省略 Voice，避免与 Base 模型冲突",
                        )
                        .size(12.0)
                        .color(Color32::YELLOW),
                    );
                }
                if self.config.openai.model.to_lowercase().contains("base")
                    && self.config.openai.ref_audio.trim().is_empty()
                {
                    ui.label(
                        RichText::new("提示：Base 语音克隆建议选择 Ref Audio，并尽量填写 Ref Text")
                            .size(12.0)
                            .color(Color32::YELLOW),
                    );
                }
            }

            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut self.config.openai.fetch_remote_voices,
                        "尝试拉取远程 voices",
                    )
                    .changed()
                {
                    dirty = true;
                }
            });

            if dirty {
                self.persist_config();
            }
        });
    }

    // Sliders: Rate, Pitch, Volume. Pitch disabled for OpenAI compatible source.
    fn show_prosody_controls(&mut self, ui: &mut egui::Ui) {
        let compact = ui.available_width() < COMPACT_WIDTH;
        if compact {
            ui.spacing_mut().slider_width = (ui.available_width() - 140.0).clamp(80.0, 320.0);
        }

        let openai = matches!(self.config.source, TtsSource::OpenAiCompatible);

        let body = |ui: &mut egui::Ui| {
            let separate = |ui: &mut egui::Ui| {
                if compact {
                    ui.end_row();
                } else {
                    ui.add_space(10.0);
                }
            };
            let rate = prosody_slider(ui, "语速:", &mut self.rate, -50..=100, "%");
            separate(ui);

            ui.label("音调:");
            let pitch = ui
                .add_enabled(
                    !openai,
                    egui::Slider::new(&mut self.pitch, -50..=50).suffix("Hz"),
                )
                .on_disabled_hover_text("当前源请用 Instruct 定制音色，不使用音调滑条");
            separate(ui);

            let volume = prosody_slider(ui, "音量:", &mut self.volume, 0..=100, "%");
            separate(ui);
            if compact {
                ui.label("");
            }
            let reset = ui.button("重置参数").clicked();
            (rate, pitch, volume, reset)
        };

        let (rate, pitch, volume, reset) = if compact {
            egui::Grid::new("prosody_grid")
                .num_columns(2)
                .spacing(Vec2::new(8.0, 6.0))
                .show(ui, body)
                .inner
        } else {
            ui.horizontal(body).inner
        };

        let committed = |r: &egui::Response| r.drag_stopped() || r.lost_focus() || r.clicked();
        if volume.changed() {
            self.audio_controller.set_volume((self.volume as f32) / 100.0);
        }
        if committed(&rate) || committed(&pitch) || committed(&volume) {
            self.persist_config();
        }
        if reset {
            self.rate = 0;
            self.pitch = 0;
            self.volume = 100;
            self.persist_config();
            self.audio_controller.set_volume(1.0);
        }
    }

    fn show_text_input(&mut self, ui: &mut egui::Ui, remaining_height: f32) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("📝 输入要合成的文字:").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("字数: {}", self.text_len)).size(12.0));

                if ui.button("清空").clicked() {
                    self.text.clear();
                    self.text_len = 0;
                    self.persist_config();
                }

                if ui.button("粘贴").clicked() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(paste_text) = clipboard.get_text() {
                            if !paste_text.is_empty() {
                                self.text.push_str(&paste_text);
                                self.text_len = self.text.chars().count();
                                self.persist_config();
                            }
                        }
                    }
                }
            });
        });

        let header_height = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let text_height = (remaining_height - header_height - 8.0).max(MIN_TEXT_HEIGHT);

        ScrollArea::vertical()
            .id_salt("text_scroll")
            .max_height(text_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let edit = egui::TextEdit::multiline(&mut self.text)
                    .id(main_text_id())
                    .hint_text(format!(
                        "在此处输入或粘贴文字... {} 朗读并清空。",
                        SPEAK_SHORTCUT_LABEL
                    ))
                    .min_size(Vec2::new(0.0, text_height))
                    .desired_width(f32::INFINITY);
                if ui.add(edit).changed() {
                    self.text_len = self.text.chars().count();
                    self.persist_config();
                }
            });
    }

    fn show_hotkey_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("⚙ 配置全局快捷键")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                ui.label("设置按下后快速 显示 / 最小化隐藏 本窗口的全局快捷键：");

                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.temp_hotkey_ctrl, "Ctrl");
                    ui.checkbox(&mut self.temp_hotkey_alt, "Alt");
                    ui.checkbox(&mut self.temp_hotkey_shift, "Shift");
                    ui.checkbox(&mut self.temp_hotkey_meta, "Win");

                    ui.label("+ 按键:");
                    egui::ComboBox::from_id_salt("hotkey_key_combo")
                        .selected_text(&self.temp_hotkey_key)
                        .show_ui(ui, |ui| {
                            let keys = [
                                "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG",
                                "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN",
                                "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU",
                                "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ", "Space", "F1",
                                "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10",
                                "F11", "F12",
                            ];
                            for k in keys {
                                ui.selectable_value(&mut self.temp_hotkey_key, k.to_string(), k);
                            }
                        });
                });

                let candidate_config = HotkeyConfig {
                    ctrl: self.temp_hotkey_ctrl,
                    alt: self.temp_hotkey_alt,
                    shift: self.temp_hotkey_shift,
                    meta: self.temp_hotkey_meta,
                    key: self.temp_hotkey_key.clone(),
                };

                ui.label(format!("当前预览: {}", candidate_config.display_string()));

                ui.horizontal(|ui| {
                    if ui.button("保存并生效").clicked() {
                        match self.hotkey_manager.register(&candidate_config) {
                            Ok(_) => {
                                self.config.hotkey = candidate_config;
                                self.persist_config();
                                self.show_hotkey_dialog = false;
                                self.status_text = "全局快捷键更新成功".to_string();
                                self.status_color = Color32::from_rgb(100, 220, 100);
                            }
                            Err(e) => {
                                self.status_text = format!("快捷键注册失败: {}", e);
                                self.status_color = Color32::RED;
                            }
                        }
                    }

                    if ui.button("取消").clicked() {
                        self.temp_hotkey_ctrl = self.config.hotkey.ctrl;
                        self.temp_hotkey_alt = self.config.hotkey.alt;
                        self.temp_hotkey_shift = self.config.hotkey.shift;
                        self.temp_hotkey_meta = self.config.hotkey.meta;
                        self.temp_hotkey_key = self.config.hotkey.key.clone();
                        self.show_hotkey_dialog = false;
                    }
                });
            });
    }
}
