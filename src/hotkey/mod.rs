use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            ctrl: true,
            alt: false,
            shift: true,
            meta: false,
            key: "KeyT".to_string(),
        }
    }
}

impl HotkeyConfig {
    pub fn display_string(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.meta {
            parts.push("Win");
        }

        let key_display = match self.key.as_str() {
            k if k.starts_with("Key") => &k[3..],
            "Space" => "Space",
            "F1" => "F1",
            "F2" => "F2",
            "F3" => "F3",
            "F4" => "F4",
            "F5" => "F5",
            "F6" => "F6",
            "F7" => "F7",
            "F8" => "F8",
            "F9" => "F9",
            "F10" => "F10",
            "F11" => "F11",
            "F12" => "F12",
            other => other,
        };
        parts.push(key_display);

        parts.join(" + ")
    }

    pub fn to_hotkey(&self) -> Option<HotKey> {
        let mut modifiers = Modifiers::empty();
        if self.ctrl {
            modifiers |= Modifiers::CONTROL;
        }
        if self.alt {
            modifiers |= Modifiers::ALT;
        }
        if self.shift {
            modifiers |= Modifiers::SHIFT;
        }
        if self.meta {
            modifiers |= Modifiers::SUPER;
        }

        let code = parse_key_code(&self.key)?;
        Some(HotKey::new(
            if modifiers.is_empty() {
                None
            } else {
                Some(modifiers)
            },
            code,
        ))
    }
}

pub fn parse_key_code(key_str: &str) -> Option<Code> {
    match key_str {
        "KeyA" => Some(Code::KeyA),
        "KeyB" => Some(Code::KeyB),
        "KeyC" => Some(Code::KeyC),
        "KeyD" => Some(Code::KeyD),
        "KeyE" => Some(Code::KeyE),
        "KeyF" => Some(Code::KeyF),
        "KeyG" => Some(Code::KeyG),
        "KeyH" => Some(Code::KeyH),
        "KeyI" => Some(Code::KeyI),
        "KeyJ" => Some(Code::KeyJ),
        "KeyK" => Some(Code::KeyK),
        "KeyL" => Some(Code::KeyL),
        "KeyM" => Some(Code::KeyM),
        "KeyN" => Some(Code::KeyN),
        "KeyO" => Some(Code::KeyO),
        "KeyP" => Some(Code::KeyP),
        "KeyQ" => Some(Code::KeyQ),
        "KeyR" => Some(Code::KeyR),
        "KeyS" => Some(Code::KeyS),
        "KeyT" => Some(Code::KeyT),
        "KeyU" => Some(Code::KeyU),
        "KeyV" => Some(Code::KeyV),
        "KeyW" => Some(Code::KeyW),
        "KeyX" => Some(Code::KeyX),
        "KeyY" => Some(Code::KeyY),
        "KeyZ" => Some(Code::KeyZ),
        "Space" => Some(Code::Space),
        "F1" => Some(Code::F1),
        "F2" => Some(Code::F2),
        "F3" => Some(Code::F3),
        "F4" => Some(Code::F4),
        "F5" => Some(Code::F5),
        "F6" => Some(Code::F6),
        "F7" => Some(Code::F7),
        "F8" => Some(Code::F8),
        "F9" => Some(Code::F9),
        "F10" => Some(Code::F10),
        "F11" => Some(Code::F11),
        "F12" => Some(Code::F12),
        _ => None,
    }
}

pub struct AppHotkeyManager {
    manager: Option<GlobalHotKeyManager>,
    active_hotkey: Option<HotKey>,
}

impl AppHotkeyManager {
    pub fn new() -> Self {
        let manager = GlobalHotKeyManager::new().ok();
        Self {
            manager,
            active_hotkey: None,
        }
    }

    pub fn register(&mut self, config: &HotkeyConfig) -> Result<(), String> {
        let manager = self
            .manager
            .as_mut()
            .ok_or_else(|| "Global hotkey manager is unavailable".to_string())?;

        // Unregister previous hotkey if registered
        if let Some(old_hotkey) = self.active_hotkey.take() {
            let _ = manager.unregister(old_hotkey);
        }

        let new_hotkey = config
            .to_hotkey()
            .ok_or_else(|| "Invalid hotkey configuration".to_string())?;

        manager
            .register(new_hotkey)
            .map_err(|e| format!("Failed to register hotkey: {}", e))?;

        self.active_hotkey = Some(new_hotkey);
        Ok(())
    }

    pub fn poll_event(&self) -> bool {
        let receiver = GlobalHotKeyEvent::receiver();
        let mut triggered = false;

        while let Ok(event) = receiver.try_recv() {
            if let Some(ref hotkey) = self.active_hotkey {
                if event.id == hotkey.id() && event.state == HotKeyState::Pressed {
                    triggered = true;
                }
            }
        }

        triggered
    }
}
