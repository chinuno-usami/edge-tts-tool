use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ShortName")]
    pub short_name: String,
    #[serde(rename = "Gender")]
    pub gender: String,
    #[serde(rename = "Locale")]
    pub locale: String,
    #[serde(rename = "FriendlyName", default)]
    pub friendly_name: String,
}

impl Voice {
    pub fn new(short_name: &str, friendly_name: &str, locale: &str, gender: &str) -> Self {
        Self {
            name: format!("Microsoft Server Speech Text to Speech Voice ({locale}, {short_name})"),
            short_name: short_name.to_string(),
            gender: gender.to_string(),
            locale: locale.to_string(),
            friendly_name: friendly_name.to_string(),
        }
    }

    pub fn display_name(&self) -> String {
        if !self.friendly_name.is_empty() {
            format!("{} ({})", self.friendly_name, self.locale)
        } else {
            format!("{} ({})", self.short_name, self.locale)
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpeakOptions {
    pub voice: String,
    pub rate: i32,    // e.g. 0 is normal, -50% to +100%
    pub pitch: i32,   // e.g. 0 is normal, -50Hz to +50Hz
    pub volume: i32,  // 0 to 100%
}

impl Default for SpeakOptions {
    fn default() -> Self {
        Self {
            voice: "zh-CN-XiaoxiaoNeural".to_string(),
            rate: 0,
            pitch: 0,
            volume: 100,
        }
    }
}
