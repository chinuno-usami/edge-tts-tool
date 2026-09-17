pub mod client;
pub mod types;
pub mod voices;

pub use client::{build_ssml, escape_xml, generate_sec_ms_gec, synthesize};
pub use types::{SpeakOptions, Voice};
pub use voices::{fetch_voices_list, get_preset_voices};
