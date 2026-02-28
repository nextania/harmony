pub mod limiter;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use tokio::sync::mpsc;

use crate::media::audio::limiter::Limiter;
use crate::media::codec;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAME_SIZE: usize = 960;
const MAX_PACKET: usize = 4000;

struct TrackPlayback {
    decoder: opus::Decoder,
    ring_buf: VecDeque<f32>,
    volume: f32,
}

impl TrackPlayback {
    fn new() -> Result<Self> {
        let decoder =
            opus::Decoder::new(SAMPLE_RATE, opus::Channels::Stereo).context("opus decoder init")?;
        Ok(Self {
            decoder,
            ring_buf: VecDeque::with_capacity(SAMPLE_RATE as usize),
            volume: 1.0,
        })
    }
}

type MixBuf = Arc<Mutex<VecDeque<f32>>>;

fn new_mix_buf() -> MixBuf {
    Arc::new(Mutex::new(VecDeque::with_capacity(SAMPLE_RATE as usize)))
}

pub struct AudioPipeline {
    tracks: HashMap<String, TrackPlayback>,
    mix_buf: MixBuf,
    limiter: Limiter,
    playback_stream: Option<Stream>,

    capture_stream: Option<Stream>,
    capture_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

impl AudioPipeline {
    pub fn new() -> Result<Self> {
        Ok(Self {
            tracks: HashMap::new(),
            mix_buf: new_mix_buf(),
            limiter: Limiter::new(),
            playback_stream: None,
            capture_stream: None,
            capture_tx: None,
        })
    }

    pub fn add_track(&mut self, track_id: String) -> Result<()> {
        if self.tracks.contains_key(&track_id) {
            return Ok(());
        }
        self.tracks.insert(track_id, TrackPlayback::new()?);
        Ok(())
    }

    pub fn remove_track(&mut self, track_id: &str) {
        self.tracks.remove(track_id);
    }

    pub fn set_volume(&mut self, track_id: &str, volume: f32) {
        if let Some(track) = self.tracks.get_mut(track_id) {
            track.volume = volume.clamp(0.0, 1.0);
        }
    }

    pub fn get_volume(&self, track_id: &str) -> Option<f32> {
        self.tracks.get(track_id).map(|t| t.volume)
    }

    pub fn list_tracks(&self) -> Vec<String> {
        self.tracks.keys().cloned().collect()
    }

    pub fn start_playback(&mut self) -> Result<()> {
        if self.playback_stream.is_some() {
            return Ok(()); // already running
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no audio output device available")?;

        let config = StreamConfig {
            channels: CHANNELS,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };

        let mix = Arc::clone(&self.mix_buf);

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buf = mix.lock().unwrap();
                    for sample in data.iter_mut() {
                        *sample = buf.pop_front().unwrap_or(0.0);
                    }
                },
                |err| {
                    tracing::error!("audio output stream error: {err}");
                },
                None,
            )
            .context("failed to build audio output stream")?;

        stream.play().context("failed to start audio playback")?;
        self.playback_stream = Some(stream);
        Ok(())
    }

    pub fn stop_playback(&mut self) {
        self.playback_stream = None;
    }

    pub fn feed_packet(&mut self, track_id: &str, data: &[u8]) -> Result<()> {
        let track = self.tracks.get_mut(track_id).context("unknown track id")?;

        let mut pcm = vec![0f32; FRAME_SIZE * CHANNELS as usize];
        let decoded = track
            .decoder
            .decode_float(data, &mut pcm, false)
            .map_err(|e| anyhow::anyhow!("opus decode error: {e}"))?;

        let total_samples = decoded * CHANNELS as usize;
        track.ring_buf.extend(&pcm[..total_samples]);

        self.mix_and_limit();

        Ok(())
    }

    fn mix_and_limit(&mut self) {
        let max_available = self
            .tracks
            .values()
            .map(|t| t.ring_buf.len())
            .max()
            .unwrap_or(0);

        if max_available == 0 {
            return;
        }

        let frame_samples = (max_available / CHANNELS as usize) * CHANNELS as usize;
        if frame_samples == 0 {
            return;
        }

        let mut mixed = vec![0.0f32; frame_samples];
        for track in self.tracks.values_mut() {
            let avail = track.ring_buf.len().min(frame_samples);
            for i in 0..avail {
                mixed[i] += track.ring_buf[i] * track.volume;
            }
            track.ring_buf.drain(..avail);
        }

        self.limiter.process(&mut mixed);

        let mut buf = self.mix_buf.lock().unwrap();
        buf.extend(mixed);
    }

    pub fn start_capture(&mut self) -> Result<Option<mpsc::UnboundedReceiver<Vec<u8>>>> {
        if self.capture_stream.is_some() {
            return Ok(None);
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no audio input device available")?;

        let config = StreamConfig {
            channels: CHANNELS,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };

        let (tx, rx) = mpsc::unbounded_channel();
        self.capture_tx = Some(tx.clone());

        let sample_buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(FRAME_SIZE)));
        let encoder = Arc::new(Mutex::new(
            opus::Encoder::new(SAMPLE_RATE, opus::Channels::Stereo, opus::Application::Voip)
                .context("opus encoder init")?,
        ));

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buf = sample_buf.lock().unwrap();
                    buf.extend_from_slice(data);

                    while buf.len() >= FRAME_SIZE {
                        let frame: Vec<f32> = buf.drain(..FRAME_SIZE).collect();
                        let mut out = vec![0u8; MAX_PACKET];
                        let mut enc = encoder.lock().unwrap();
                        match enc.encode_float(&frame, &mut out) {
                            Ok(len) => {
                                out.truncate(len);
                                let packet = codec::prepend_codec_byte(codec::AUDIO_OPUS, &out);
                                let _ = tx.send(packet);
                            }
                            Err(e) => {
                                tracing::warn!("opus encode error: {e}");
                            }
                        }
                    }
                },
                |err| {
                    tracing::error!("audio input stream error: {err}");
                },
                None,
            )
            .context("failed to build audio input stream")?;

        stream.play().context("failed to start audio capture")?;
        self.capture_stream = Some(stream);

        Ok(Some(rx))
    }

    pub fn stop_capture(&mut self) {
        self.capture_stream = None;
        self.capture_tx = None;
    }

    pub fn is_capturing(&self) -> bool {
        self.capture_stream.is_some()
    }

    pub fn is_playing(&self) -> bool {
        self.playback_stream.is_some()
    }
}
