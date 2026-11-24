pub mod client;
pub mod voices;

pub use client::{
    join_speech_url, model_requires_voice, model_should_omit_classic_voice,
    model_supports_instruct, model_supports_voice_clone_refs, rate_to_speed, synthesize,
    OpenAiSpeechRequest,
};
pub use voices::{fetch_voices_list, get_preset_voices};
