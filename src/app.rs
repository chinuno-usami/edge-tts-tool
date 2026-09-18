use crate::audio::{list_output_devices, AudioController, AudioDeviceInfo, AudioEvent};
use crate::config::AppConfig;
use crate::edge_tts::{
    fetch_voices_list, get_preset_voices, synthesize, SpeakOptions, Voice,
};
use crate::hotkey::{AppHotkeyManager, HotkeyConfig};
use crossbeam_channel::{unbounded, Receiver, Sender};
use egui::{Align, Color32, Layout, RichText, ScrollArea, Vec2};
use std::sync::Arc;
use tokio::runtime::Runtime;

enum AsyncMessage {
    VoicesLoaded(Vec<Voice>),
    VoicesLoadFailed(String),
    SynthesisSuccess {
        audio: Vec<u8>,
        auto_play: bool,
    },
    SynthesisFailed(String),
}

pub struct TtsApp {
    config: AppConfig,
    text: String,

    // Voices
    voices: Vec<Voice>,
    selected_voice_name: String,
    voice_filter: String,
    is_loading_voices: bool,

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
    last_audio: Option<Vec<u8>>,

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
}

impl TtsApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        let (async_tx, async_rx) = unbounded();
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to initialize tokio runtime"),
        );

        let mut hotkey_manager = AppHotkeyManager::new();
        if let Err(e) = hotkey_manager.register(&config.hotkey) {
            log::warn!("Initial hotkey registration notice: {}", e);
        }

        let preset_voices = get_preset_voices();
        let selected_voice_name = if preset_voices.iter().any(|v| v.short_name == config.voice) {
            config.voice.clone()
        } else {
            preset_voices
                .first()
                .map(|v| v.short_name.clone())
                .unwrap_or_else(|| "zh-CN-XiaoxiaoNeural".to_string())
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
            filtered_voices: Vec::new(),
            selected_display: String::new(),
            text_len: config.last_text.chars().count(),
            output_devices: devices,
            audio_controller: AudioController::new(),
            status_text: "就绪".to_string(),
            status_color: Color32::from_rgb(100, 180, 100),
            is_playing: false,
            is_synthesizing: false,
            last_audio: None,
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
        };

        // Populate the cached dropdown data for the preset voices.
        app.recompute_voice_cache();

        // Fetch latest online voices in the background
        app.refresh_voices_async();

        app
    }

    fn refresh_devices(&mut self) {
        self.output_devices = list_output_devices();
        self.status_text = format!("音频输出设备列表已刷新 (共 {} 个设备)", self.output_devices.len());
        self.status_color = Color32::LIGHT_BLUE;
    }

    // Rebuild the cached voice-dropdown list. Called only when the filter text or the
    // voice list actually changes, so the per-frame ui() path no longer does a full
    // to_lowercase + filter + display_name(format!) + collect over hundreds of voices.
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
            .unwrap_or_else(|| self.selected_voice_name.clone());
    }

    fn refresh_voices_async(&self) {
        let tx = self.async_tx.clone();
        self.runtime.spawn(async move {
            match fetch_voices_list().await {
                Ok(voices) => {
                    let _ = tx.send(AsyncMessage::VoicesLoaded(voices));
                }
                Err(e) => {
                    let _ = tx.send(AsyncMessage::VoicesLoadFailed(e.to_string()));
                }
            }
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

        self.is_synthesizing = true;
        self.status_text = "正在调用 Edge-TTS 生成音频...".to_string();
        self.status_color = Color32::LIGHT_BLUE;

        let options = SpeakOptions {
            voice: self.selected_voice_name.clone(),
            rate: self.rate,
            pitch: self.pitch,
            volume: self.volume,
        };

        let text = self.text.clone();
        let tx = self.async_tx.clone();

        self.runtime.spawn(async move {
            match synthesize(&text, &options).await {
                Ok(audio) => {
                    let _ = tx.send(AsyncMessage::SynthesisSuccess { audio, auto_play });
                }
                Err(e) => {
                    let _ = tx.send(AsyncMessage::SynthesisFailed(e.to_string()));
                }
            }
        });
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
        self.config.voice = self.selected_voice_name.clone();
        self.config.output_device = self.selected_device_name.clone();
        self.config.rate = self.rate;
        self.config.pitch = self.pitch;
        self.config.volume = self.volume;
        self.config.last_text = self.text.clone();
        self.config.save();
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
                AsyncMessage::VoicesLoaded(fetched_voices) => {
                    self.is_loading_voices = false;
                    if !fetched_voices.is_empty() {
                        self.voices = fetched_voices;
                        self.recompute_voice_cache();
                        self.status_text = format!("已加载 Edge 云端语音 (共 {} 个)", self.voices.len());
                        self.status_color = Color32::LIGHT_BLUE;
                    }
                }
                AsyncMessage::VoicesLoadFailed(err) => {
                    self.is_loading_voices = false;
                    log::warn!("Failed to load online voices: {}", err);
                }
                AsyncMessage::SynthesisSuccess { audio, auto_play } => {
                    self.is_synthesizing = false;
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
                AsyncMessage::SynthesisFailed(err) => {
                    self.is_synthesizing = false;
                    self.status_text = format!("合成失败: {}", err);
                    self.status_color = Color32::RED;
                }
            }
        }

        // 5. Render Central GUI Panel
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

            // --- Top Header ---
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🔊 Edge TTS 语音合成").strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let hotkey_str = self.config.hotkey.display_string();
                    if ui
                        .button(RichText::new(format!("⌨ 快捷键: {}", hotkey_str)).size(12.0))
                        .on_hover_text("点击配置全局显示/隐藏快捷键")
                        .clicked()
                    {
                        self.show_hotkey_dialog = !self.show_hotkey_dialog;
                    }

                    // Status Badge
                    ui.label(RichText::new(&self.status_text).color(self.status_color).size(13.0));
                });
            });

            ui.separator();

            // --- Audio Device Selection Bar ---
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🎧 输出声卡 / 设备:").strong());

                    let current_device_label = self
                        .selected_device_name
                        .as_deref()
                        .unwrap_or("默认音频输出设备 (System Default)");

                    let mut selected_dev_to_set = None;
                    egui::ComboBox::from_id_salt("audio_device_combo")
                        .width(360.0)
                        .selected_text(current_device_label)
                        .show_ui(ui, |ui| {
                            let is_default_selected = self.selected_device_name.is_none();
                            if ui
                                .selectable_label(is_default_selected, "默认音频输出设备 (System Default)")
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

                                let is_selected = self.selected_device_name.as_deref() == Some(&dev.name);
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
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🗣 语音角色:").strong());

                    // Quick filter text input
                    let filter_changed = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.voice_filter)
                                .hint_text("🔍 过滤角色 (如 晓晓, 云希, en-US)...")
                                .desired_width(180.0),
                        )
                        .changed();
                    if filter_changed {
                        self.recompute_voice_cache();
                    }

                    let mut selected_voice_to_set = None;
                    egui::ComboBox::from_id_salt("voice_combo")
                        .width(260.0)
                        .selected_text(self.selected_display.as_str())
                        .show_ui(ui, |ui| {
                            for (short_name, display_name) in self.filtered_voices.iter() {
                                let is_selected = self.selected_voice_name == *short_name;
                                if ui.selectable_label(is_selected, display_name).clicked() {
                                    selected_voice_to_set = Some(short_name.clone());
                                }
                            }
                        });

                    if let Some(new_voice) = selected_voice_to_set {
                        self.selected_voice_name = new_voice;
                        // Sync the cached selected label so the collapsed combo shows
                        // the newly picked voice (previously computed per-frame).
                        self.recompute_voice_cache();
                        self.persist_config();
                    }

                    if ui
                        .button("☁ 刷新云端语音")
                        .on_hover_text("从 Microsoft Edge 服务器拉取最新完整语音列表")
                        .clicked()
                    {
                        self.is_loading_voices = true;
                        self.status_text = "正在获取微软云端全部语音列表...".to_string();
                        self.status_color = Color32::LIGHT_BLUE;
                        self.refresh_voices_async();
                    }
                });

                ui.add_space(4.0);

                // Sliders: Rate, Pitch, Volume
                ui.horizontal(|ui| {
                    ui.label("语速:");
                    if ui
                        .add(egui::Slider::new(&mut self.rate, -50..=100).suffix("%"))
                        .changed()
                    {
                        self.persist_config();
                    }

                    ui.add_space(10.0);
                    ui.label("音调:");
                    if ui
                        .add(egui::Slider::new(&mut self.pitch, -50..=50).suffix("Hz"))
                        .changed()
                    {
                        self.persist_config();
                    }

                    ui.add_space(10.0);
                    ui.label("音量:");
                    if ui
                        .add(egui::Slider::new(&mut self.volume, 0..=100).suffix("%"))
                        .changed()
                    {
                        self.persist_config();
                        self.audio_controller.set_volume((self.volume as f32) / 100.0);
                    }

                    if ui.button("重置参数").clicked() {
                        self.rate = 0;
                        self.pitch = 0;
                        self.volume = 100;
                        self.persist_config();
                        self.audio_controller.set_volume(1.0);
                    }
                });
            });

            // --- Text Input Area ---
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

            ScrollArea::vertical()
                .max_height(240.0)
                .min_scrolled_height(160.0)
                .show(ui, |ui| {
                    let edit = egui::TextEdit::multiline(&mut self.text)
                        .hint_text("在此处输入或粘贴文字... 支持中英文及长文本。")
                        .desired_rows(8)
                        .desired_width(f32::INFINITY);
                    if ui.add(edit).changed() {
                        self.text_len = self.text.chars().count();
                        self.persist_config();
                    }
                });

            ui.add_space(6.0);

            // --- Action Buttons ---
            ui.horizontal(|ui| {
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
                    .clicked()
                {
                    self.start_synthesis(true);
                }

                if self.is_playing {
                    if ui
                        .add(egui::Button::new(RichText::new("⏹ 停止播放").size(16.0).color(Color32::LIGHT_RED)))
                        .clicked()
                    {
                        self.stop_playback();
                    }
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

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(RichText::new("🗕 最小化隐藏").size(13.0))
                        .on_hover_text("最小化隐藏窗口，可通过快捷键随时唤出")
                        .clicked()
                    {
                        self.is_minimized = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
            });

            // --- Hotkey Configuration Modal Window ---
            if self.show_hotkey_dialog {
                egui::Window::new("⚙ 配置全局快捷键")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                        ui.label("设置按下后快速 显示 / 最小化隐藏 本窗口的全局快捷键：");

                        ui.horizontal(|ui| {
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
        });

        // Request repaint to keep UI and audio events responsive
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
