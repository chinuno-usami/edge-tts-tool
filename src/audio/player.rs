use cpal::traits::{DeviceTrait, HostTrait};
use crossbeam_channel::{bounded, Receiver, Sender};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::Cursor;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub enum AudioCommand {
    Play {
        bytes: Vec<u8>,
        device_name: Option<String>,
        volume: f32,
    },
    Stop,
    SetVolume(f32),
}

#[derive(Debug, Clone)]
pub enum AudioEvent {
    PlaybackStarted,
    PlaybackFinished,
    PlaybackError(String),
}

pub struct AudioController {
    cmd_tx: Sender<AudioCommand>,
    event_rx: Receiver<AudioEvent>,
}

impl AudioController {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = bounded::<AudioCommand>(16);
        let (event_tx, event_rx) = bounded::<AudioEvent>(16);

        thread::spawn(move || {
            run_audio_thread(cmd_rx, event_tx);
        });

        Self { cmd_tx, event_rx }
    }

    pub fn play(&self, bytes: Vec<u8>, device_name: Option<String>, volume: f32) {
        let _ = self.cmd_tx.send(AudioCommand::Play {
            bytes,
            device_name,
            volume,
        });
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(AudioCommand::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.cmd_tx.send(AudioCommand::SetVolume(volume));
    }

    pub fn try_recv_event(&self) -> Option<AudioEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Default for AudioController {
    fn default() -> Self {
        Self::new()
    }
}

fn run_audio_thread(cmd_rx: Receiver<AudioCommand>, event_tx: Sender<AudioEvent>) {
    let mut current_stream: Option<OutputStream> = None;
    let mut current_handle: Option<OutputStreamHandle> = None;
    let mut current_sink: Option<Sink> = None;
    let mut current_device_name: Option<String> = None;
    let mut is_playing = false;

    loop {
        // Poll for incoming commands with a short timeout to also monitor playback end
        let cmd = match cmd_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(c) => Some(c),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };

        if let Some(cmd) = cmd {
            match cmd {
                AudioCommand::Play {
                    bytes,
                    device_name,
                    volume,
                } => {
                    // Stop current playback if any
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }

                    // Check if output stream needs to be recreated for a different device
                    let need_new_stream = current_stream.is_none()
                        || current_device_name != device_name;

                    if need_new_stream {
                        current_stream = None;
                        current_handle = None;

                        let host = cpal::default_host();
                        let device = if let Some(ref target_name) = device_name {
                            host.output_devices().ok().and_then(|mut devs| {
                                devs.find(|d| d.name().map(|n| n == *target_name).unwrap_or(false))
                            })
                        } else {
                            host.default_output_device()
                        };

                        let stream_result = if let Some(ref dev) = device {
                            OutputStream::try_from_device(dev)
                        } else {
                            OutputStream::try_default()
                        };

                        match stream_result {
                            Ok((stream, handle)) => {
                                current_stream = Some(stream);
                                current_handle = Some(handle);
                                current_device_name = device_name;
                            }
                            Err(e) => {
                                let _ = event_tx.send(AudioEvent::PlaybackError(format!(
                                    "Failed to open audio device: {}",
                                    e
                                )));
                                is_playing = false;
                                continue;
                            }
                        }
                    }

                    if let Some(ref handle) = current_handle {
                        match Sink::try_new(handle) {
                            Ok(sink) => {
                                let cursor = Cursor::new(bytes);
                                match Decoder::new(cursor) {
                                    Ok(source) => {
                                        sink.set_volume(volume.clamp(0.0, 2.0));
                                        sink.append(source);
                                        current_sink = Some(sink);
                                        is_playing = true;
                                        let _ = event_tx.send(AudioEvent::PlaybackStarted);
                                    }
                                    Err(e) => {
                                        let _ = event_tx.send(AudioEvent::PlaybackError(format!(
                                            "Failed to decode audio: {}",
                                            e
                                        )));
                                        is_playing = false;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = event_tx.send(AudioEvent::PlaybackError(format!(
                                    "Failed to create audio sink: {}",
                                    e
                                )));
                                is_playing = false;
                            }
                        }
                    }
                }
                AudioCommand::Stop => {
                    if let Some(sink) = current_sink.take() {
                        sink.stop();
                    }
                    if is_playing {
                        is_playing = false;
                        let _ = event_tx.send(AudioEvent::PlaybackFinished);
                    }
                }
                AudioCommand::SetVolume(vol) => {
                    if let Some(ref sink) = current_sink {
                        sink.set_volume(vol.clamp(0.0, 2.0));
                    }
                }
            }
        }

        // Check if playback has finished naturally
        if is_playing {
            if let Some(ref sink) = current_sink {
                if sink.empty() {
                    is_playing = false;
                    current_sink = None;
                    let _ = event_tx.send(AudioEvent::PlaybackFinished);
                }
            } else {
                is_playing = false;
            }
        }
    }
}
