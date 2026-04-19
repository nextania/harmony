pub mod limiter;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc as sync_mpsc;
use std::sync::Arc;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use ringbuf::traits::{Consumer, Observer, Producer, RingBuffer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use tokio::sync::mpsc;

use crate::media::audio::limiter::Limiter;
use crate::media::codec;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAME_SIZE: usize = 960;
const MAX_PACKET: usize = 4000;

enum TrackCommand {
    Add {
        id: String,
        consumer: HeapCons<f32>,
        volume: Arc<AtomicU32>,
    },
    Remove {
        id: String,
    },
}

struct TrackPlayback {
    decoder: opus::Decoder,
    producer: HeapProd<f32>,
    volume: Arc<AtomicU32>,
}

impl TrackPlayback {
    fn new() -> Result<(Self, HeapCons<f32>)> {
        let decoder =
            opus::Decoder::new(SAMPLE_RATE, opus::Channels::Stereo).context("opus decoder init")?;
        let (producer, consumer) = HeapRb::new(SAMPLE_RATE as usize).split();
        Ok((Self {
            decoder,
            producer,
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        }, consumer))
    }

    fn reset(&mut self) -> HeapCons<f32> {
        let (producer, consumer) = HeapRb::new(SAMPLE_RATE as usize).split();
        self.producer = producer;
        consumer
    }
}
pub struct AudioPipeline {
    tracks: HashMap<String, TrackPlayback>,
    pending_consumers: HashMap<String, HeapCons<f32>>,
    playback_stream: Option<Stream>,
    track_cmd_tx: Option<sync_mpsc::Sender<TrackCommand>>,

    capture_stream: Option<Stream>,
    capture_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

impl AudioPipeline {
    pub fn new() -> Result<Self> {
        Ok(Self {
            tracks: HashMap::new(),
            pending_consumers: HashMap::new(),
            playback_stream: None,
            track_cmd_tx: None,
            capture_stream: None,
            capture_tx: None,
        })
    }

    pub fn add_track(&mut self, track_id: String) -> Result<()> {
        if self.tracks.contains_key(&track_id) {
            return Ok(());
        }
        let (track, consumer) = TrackPlayback::new()?;
        if let Some(tx) = &self.track_cmd_tx {
            let _ = tx.send(TrackCommand::Add {
                id: track_id.clone(),
                consumer,
                volume: Arc::clone(&track.volume),
            });
        } else {
            self.pending_consumers.insert(track_id.clone(), consumer);
        }
        self.tracks.insert(track_id, track);
        Ok(())
    }

    pub fn remove_track(&mut self, track_id: &str) {
        self.tracks.remove(track_id);
        self.pending_consumers.remove(track_id);
        if let Some(tx) = &self.track_cmd_tx {
            let _ = tx.send(TrackCommand::Remove {
                id: track_id.to_owned(),
            });
        }
    }

    pub fn set_volume(&mut self, track_id: &str, volume: f32) {
        if let Some(track) = self.tracks.get_mut(track_id) {
            track
                .volume
                .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn get_volume(&self, track_id: &str) -> Option<f32> {
        self.tracks
            .get(track_id)
            .map(|t| f32::from_bits(t.volume.load(Ordering::Relaxed)))
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

        let (cmd_tx, cmd_rx) = sync_mpsc::channel::<TrackCommand>();

        // TODO: ?
        let mut cb_tracks: HashMap<String, (HeapCons<f32>, Arc<AtomicU32>)> = HashMap::new();
        for (id, consumer) in self.pending_consumers.drain() {
            let volume = Arc::clone(&self.tracks[&id].volume);
            cb_tracks.insert(id, (consumer, volume));
        }

        let mut limiter = Limiter::new();
        let mut read_buf: Vec<f32> = Vec::new();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            TrackCommand::Add { id, consumer, volume } => {
                                cb_tracks.insert(id, (consumer, volume));
                            }
                            TrackCommand::Remove { id } => {
                                cb_tracks.remove(&id);
                            }
                        }
                    }

                    data.fill(0.0);

                    read_buf.resize(data.len(), 0.0);
                    for (consumer, volume) in cb_tracks.values_mut() {
                        let vol = f32::from_bits(volume.load(Ordering::Relaxed));
                        let n = consumer.pop_slice(&mut read_buf[..data.len()]);
                        for i in 0..n {
                            data[i] += read_buf[i] * vol;
                        }
                    }

                    limiter.process(data);
                },
                |err| {
                    tracing::error!("audio output stream error: {err}");
                },
                None,
            )
            .context("failed to build audio output stream")?;

        stream.play().context("failed to start audio playback")?;
        self.playback_stream = Some(stream);
        self.track_cmd_tx = Some(cmd_tx);
        Ok(())
    }

    pub fn stop_playback(&mut self) {
        self.playback_stream = None;
        self.track_cmd_tx = None;
        let consumers: Vec<(String, HeapCons<f32>)> = self.tracks
            .iter_mut()
            .map(|(id, track)| (id.clone(), track.reset()))
            .collect();
        self.pending_consumers.extend(consumers);
    }

    pub fn feed_packet(&mut self, track_id: &str, data: &[u8]) -> Result<()> {
        let track = self.tracks.get_mut(track_id).context("unknown track id")?;

        let mut pcm = vec![0f32; FRAME_SIZE * CHANNELS as usize];
        let decoded = track
            .decoder
            .decode_float(data, &mut pcm, false)
            .map_err(|e| anyhow::anyhow!("opus decode error: {e}"))?;

        let total_samples = decoded * CHANNELS as usize;
        track.producer.push_slice(&pcm[..total_samples]);

        Ok(())
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

        let mut sample_buf = HeapRb::new(FRAME_SIZE * CHANNELS as usize * 10);
        let mut encoder = 
            opus::Encoder::new(SAMPLE_RATE, opus::Channels::Stereo, opus::Application::Audio)
                .context("opus encoder init")?;

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    sample_buf.push_slice_overwrite(data);
                    let mut frame = [0f32; FRAME_SIZE * CHANNELS as usize];
                    while sample_buf.occupied_len() >= FRAME_SIZE * CHANNELS as usize {
                        sample_buf.pop_slice(&mut frame);
                        let mut out = vec![0u8; MAX_PACKET];
                        match encoder.encode_float(&frame, &mut out) {
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
