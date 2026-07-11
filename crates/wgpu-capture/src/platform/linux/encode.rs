use std::fs::File;
use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Instant;

use cros_codecs::{
    BlockingMode, Fourcc, FrameLayout, PlaneLayout, Resolution,
    encoder::{
        FrameMetadata, RateControl, Tunings, VideoEncoder,
        av1::EncoderConfig as Av1Config,
        h264::EncoderConfig as H264Config,
        stateless::{
            av1::StatelessEncoder as Av1StatelessEncoder,
            h264::StatelessEncoder as H264StatelessEncoder,
        },
    },
    utils::DmabufFrame,
    video_frame::generic_dma_video_frame::GenericDmaVideoFrame,
};
use libva::{
    BufferType, Context, Display, Picture, PictureNew, ProcColorProperties,
    ProcPipelineParameterBuffer, Surface, VA_RT_FORMAT_RGB32, VAEntrypoint, VAProfile,
};
use nix::sys::mman::{MapFlags, ProtFlags};

use crate::{CaptureFrame, Codec, EncodeConfig, EncodeOutput, EncodeSession, Result};
use tracing::debug;

enum EncodeCommand {
    Frame {
        encode_fd: OwnedFd,
        width: u32,
        height: u32,
        stride: u32,
        fourcc: drm_fourcc::DrmFourcc,
    },
    SetBitrate(u32),
    RequestKeyframe,
    Finish,
}

pub(crate) struct VaapiEncoder {
    tx: SyncSender<EncodeCommand>,
    encoder_thread: Option<thread::JoinHandle<Result<()>>>,
    last_keyframe_request: Option<Instant>,
}

impl VaapiEncoder {
    pub(crate) fn new(config: EncodeConfig) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<EncodeCommand>(4);

        let width = config.width;
        let height = config.height;
        let fps = config.fps;
        let bitrate = config.bitrate_bps;
        let codec = config.codec;
        let output = config.output;

        let encoder_thread = thread::Builder::new()
            .name("wgpu-capture-vaapi-enc".to_owned())
            .spawn(move || {
                let result = encoder_thread(rx, width, height, fps, bitrate, codec, output);
                if let Err(e) = &result {
                    debug!("encoder thread exiting with error: {e}");
                }
                result
            })?;

        Ok(VaapiEncoder {
            tx,
            encoder_thread: Some(encoder_thread),
            last_keyframe_request: None,
        })
    }
}

impl EncodeSession for VaapiEncoder {
    fn submit_frame(&mut self, frame: &CaptureFrame) -> Result<()> {
        let linux_frame = &frame.0.0;
        let encode_fd = nix::unistd::dup(&linux_frame.encode_fd)?;
        self.tx
            .send(EncodeCommand::Frame {
                encode_fd,
                width: linux_frame.width,
                height: linux_frame.height,
                stride: linux_frame.stride,
                fourcc: linux_frame.fourcc,
            })
            .map_err(|_| crate::Error::Thread)?;
        Ok(())
    }

    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<()> {
        self.tx
            .send(EncodeCommand::SetBitrate(bitrate_bps))
            .map_err(|_| crate::Error::Thread)?;
        Ok(())
    }

    fn request_keyframe(&mut self) -> Result<()> {
        let now = Instant::now();
        if let Some(last) = self.last_keyframe_request {
            if now.duration_since(last) < std::time::Duration::from_secs(1) {
                return Ok(());
            }
        }
        self.last_keyframe_request = Some(now);

        self.tx
            .send(EncodeCommand::RequestKeyframe)
            .map_err(|_| crate::Error::Thread)?;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        self.tx.send(EncodeCommand::Finish).ok();
        if let Some(handle) = self.encoder_thread.take() {
            handle.join().map_err(|_| crate::Error::Thread)??;
        }
        Ok(())
    }
}

fn encoder_thread(
    rx: std::sync::mpsc::Receiver<EncodeCommand>,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    codec: Codec,
    output: EncodeOutput,
) -> Result<()> {
    let display = Display::open().ok_or_else(|| crate::Error::VaapiDeviceMissing)?;

    // frames must be aligned for the encoder to accept them
    let aligned_width = (width + CODEC_SIZE_ALIGNMENT - 1) & !(CODEC_SIZE_ALIGNMENT - 1);
    let aligned_height = (height + CODEC_SIZE_ALIGNMENT - 1) & !(CODEC_SIZE_ALIGNMENT - 1);

    let mut vpp: Option<VaapiVpp> =
        match VaapiVpp::new(Arc::clone(&display), aligned_width, aligned_height) {
            Ok(v) => {
                debug!("VA-API VPP color converter ready");
                Some(v)
            }
            Err(e) => {
                debug!("VA-API VPP unavailable ({e}); non-NV12 frames will be skipped");
                None
            }
        };
    let fourcc = Fourcc::from(b"NV12");
    let resolution = Resolution {
        width: aligned_width,
        height: aligned_height,
    };
    let low_power = check_low_power(&*display, &codec);
    let callback: Arc<dyn Fn(Vec<u8>) + Send + Sync> = output.0.clone();

    let gop_limit = (fps.max(1) * 2).clamp(1, u16::MAX as u32) as u16;
    let pred_structure = cros_codecs::encoder::PredictionStructure::LowDelay { limit: gop_limit };

    let mut encoder: Box<dyn VideoEncoder<GenericDmaVideoFrame>> = match codec {
        Codec::H264 => {
            let config = H264Config {
                resolution,
                pred_structure,
                initial_tunings: Tunings {
                    rate_control: RateControl::ConstantBitrate(bitrate as u64),
                    framerate: fps,
                    ..Default::default()
                },
                ..Default::default()
            };

            let enc: H264StatelessEncoder<
                GenericDmaVideoFrame,
                cros_codecs::backend::vaapi::encoder::VaapiBackend<
                    GenericDmaVideoFrame,
                    libva::Surface<GenericDmaVideoFrame>,
                >,
            > = H264StatelessEncoder::new_vaapi(
                display.clone(),
                config,
                fourcc,
                resolution,
                low_power,
                BlockingMode::Blocking,
            )?;

            Box::new(enc)
        }
        Codec::AV1 => {
            let qp = bitrate_to_qp(bitrate);
            let config = Av1Config {
                resolution,
                pred_structure,
                initial_tunings: Tunings {
                    rate_control: RateControl::ConstantQuality(qp),
                    framerate: fps,
                    ..Default::default()
                },
                ..Default::default()
            };

            let enc: Av1StatelessEncoder<
                GenericDmaVideoFrame,
                cros_codecs::backend::vaapi::encoder::VaapiBackend<
                    GenericDmaVideoFrame,
                    libva::Surface<GenericDmaVideoFrame>,
                >,
            > = Av1StatelessEncoder::new_vaapi(
                display.clone(),
                config,
                fourcc,
                resolution,
                low_power,
                BlockingMode::Blocking,
            )?;

            Box::new(enc)
        }
    };

    let mut force_keyframe = false;

    // on high refresh rate displays the compositor will give us frames
    // much faster than we need them, so we drop frames on the an interval
    // determined by the desired frame rate accordingly
    let target_fps = fps.max(1);
    let frame_interval = std::time::Duration::from_secs_f64(1.0 / target_fps as f64);
    let mut stream_start: Option<Instant> = None;
    let mut next_frame_deadline: Option<Instant> = None;
    let mut last_timestamp: Option<u64> = None;

    loop {
        drain_output(&mut *encoder, &callback)?;

        match rx.recv() {
            Ok(EncodeCommand::Frame {
                encode_fd,
                width: fw,
                height: fh,
                stride,
                fourcc: frame_fourcc,
            }) => {
                let now = Instant::now();

                if next_frame_deadline.is_some_and(|deadline| now < deadline) {
                    continue;
                }

                next_frame_deadline = Some(match next_frame_deadline {
                    Some(prev) if now.duration_since(prev) < frame_interval => {
                        prev + frame_interval
                    }
                    _ => now + frame_interval,
                });

                let start = *stream_start.get_or_insert(now);
                let mut frame_timestamp =
                    (now.duration_since(start).as_secs_f64() * target_fps as f64).round() as u64;
                if let Some(last) = last_timestamp
                    && frame_timestamp <= last
                {
                    frame_timestamp = last + 1;
                }
                last_timestamp = Some(frame_timestamp);

                let is_native_nv12 = frame_fourcc == drm_fourcc::DrmFourcc::Nv12
                    && fw == aligned_width
                    && fh == aligned_height;

                let (dma_frame, frame_layout) = if is_native_nv12 {
                    let layout = FrameLayout {
                        format: (fourcc, 0u64),
                        size: Resolution {
                            width: aligned_width,
                            height: aligned_height,
                        },
                        planes: vec![
                            PlaneLayout {
                                buffer_index: 0,
                                offset: 0,
                                stride: stride as usize,
                            },
                            PlaneLayout {
                                buffer_index: 0,
                                offset: (stride * fh) as usize,
                                stride: stride as usize,
                            },
                        ],
                    };
                    let frame =
                        GenericDmaVideoFrame::new(vec![File::from(encode_fd)], layout.clone())
                            .map_err(|e| {
                                crate::Error::Import(format!("GenericDmaVideoFrame::new: {e}"))
                            })?;
                    (frame, layout)
                } else {
                    match vpp.as_mut() {
                        Some(v) => v.convert(encode_fd, frame_fourcc, fw, fh, stride)?,
                        None => {
                            debug!(
                                "encoder: skipping frame with unsupported format \
                                 {frame_fourcc:?} (NV12 required, no VPP available)"
                            );
                            continue;
                        }
                    }
                };
                let meta = FrameMetadata {
                    timestamp: frame_timestamp,
                    layout: frame_layout,
                    force_keyframe,
                    force_idr: force_keyframe,
                };

                encoder.encode(meta, dma_frame)?;

                force_keyframe = false;
            }
            Ok(EncodeCommand::SetBitrate(new_bitrate)) => {
                if codec == Codec::AV1 {
                    let qp = bitrate_to_qp(new_bitrate);
                    encoder
                        .tune(Tunings {
                            rate_control: RateControl::ConstantQuality(qp),
                            framerate: fps,
                            ..Default::default()
                        })
                        .ok();
                } else {
                    encoder
                        .tune(Tunings {
                            rate_control: RateControl::ConstantBitrate(new_bitrate as u64),
                            framerate: fps,
                            ..Default::default()
                        })
                        .ok();
                }
            }
            Ok(EncodeCommand::RequestKeyframe) => {
                force_keyframe = true;
            }
            Ok(EncodeCommand::Finish) | Err(_) => break,
        }
    }

    encoder.drain()?;
    drain_output(&mut *encoder, &callback)?;

    Ok(())
}

fn drain_output(
    encoder: &mut dyn VideoEncoder<GenericDmaVideoFrame>,
    callback: &Arc<dyn Fn(Vec<u8>) + Send + Sync>,
) -> Result<()> {
    loop {
        match encoder.poll()? {
            Some(coded) => {
                callback(coded.bitstream);
            }
            None => return Ok(()),
        }
    }
}

enum SrcSurface {
    DmaBuf(Surface<DmabufFrame>),
    VaAllocated(Surface<()>),
}

impl SrcSurface {
    fn id(&self) -> libva::VASurfaceID {
        match self {
            SrcSurface::DmaBuf(s) => s.id(),
            SrcSurface::VaAllocated(s) => s.id(),
        }
    }
}

/// Converts any RGB DMA-buf to NV12.
struct VaapiVpp {
    display: Arc<Display>,
    context: Rc<Context>,
    dst_pool: Vec<Rc<Surface<()>>>,
    pool_idx: usize,
    aligned_width: u32,
    aligned_height: u32,
}

const VPP_POOL_SIZE: usize = 4;
const SURFACE_PITCH_ALIGNMENT: u32 = 128;
const CODEC_SIZE_ALIGNMENT: u32 = 16;

impl VaapiVpp {
    fn new(display: Arc<Display>, width: u32, height: u32) -> Result<Self> {
        let config = display.create_config(
            vec![],
            VAProfile::VAProfileNone,
            VAEntrypoint::VAEntrypointVideoProc,
        )?;
        let dst_surfaces = display.create_surfaces(
            libva::VA_RT_FORMAT_YUV420,
            Some(u32::from_le_bytes(*b"NV12")),
            width,
            height,
            None,
            vec![(); VPP_POOL_SIZE],
        )?;
        let dst_pool: Vec<Rc<Surface<()>>> = dst_surfaces.into_iter().map(Rc::new).collect();
        let context = display.create_context::<()>(&config, width, height, None, true)?;

        Ok(Self {
            display,
            context,
            dst_pool,
            pool_idx: 0,
            aligned_width: width,
            aligned_height: height,
        })
    }

    fn convert(
        &mut self,
        src_fd: OwnedFd,
        src_drm_fourcc: drm_fourcc::DrmFourcc,
        width: u32,
        height: u32,
        stride: u32,
    ) -> Result<(GenericDmaVideoFrame, FrameLayout)> {
        let va_src_fourcc = match src_drm_fourcc {
            drm_fourcc::DrmFourcc::Xrgb8888 => u32::from_le_bytes(*b"BGRX"),
            drm_fourcc::DrmFourcc::Argb8888 => u32::from_le_bytes(*b"BGRA"),
            drm_fourcc::DrmFourcc::Xbgr8888 => u32::from_le_bytes(*b"RGBX"),
            drm_fourcc::DrmFourcc::Abgr8888 => u32::from_le_bytes(*b"RGBA"),
            _ => u32::from_le_bytes(*b"BGRX"),
        };

        let src_surface = if stride % SURFACE_PITCH_ALIGNMENT == 0 {
            debug!("VPP convert: zero-copy path (stride={stride}, {width}x{height})");
            let src_cros_fourcc = Fourcc::from(&va_src_fourcc.to_le_bytes());
            let src_layout = FrameLayout {
                format: (src_cros_fourcc, 0u64),
                size: Resolution { width, height },
                planes: vec![PlaneLayout {
                    buffer_index: 0,
                    offset: 0,
                    stride: stride as usize,
                }],
            };
            let src_frame = DmabufFrame {
                fds: vec![src_fd],
                layout: src_layout,
            };
            let mut src_surfaces = self.display.create_surfaces(
                VA_RT_FORMAT_RGB32,
                Some(va_src_fourcc),
                width,
                height,
                None,
                vec![src_frame],
            )?;
            SrcSurface::DmaBuf(src_surfaces.remove(0))
        } else {
            debug!(
                "VPP convert: aligned-upload path (stride={stride} not aligned to {SURFACE_PITCH_ALIGNMENT})"
            );
            let mut src_surfaces = self.display.create_surfaces(
                VA_RT_FORMAT_RGB32,
                Some(va_src_fourcc),
                width,
                height,
                None,
                vec![(); 1],
            )?;
            let surface = src_surfaces.remove(0);

            let image_fmts = self.display.query_image_formats()?;
            let image_fmt = image_fmts
                .into_iter()
                .find(|f| f.fourcc == va_src_fourcc)
                .ok_or_else(|| {
                    crate::Error::Import(format!(
                        "no VAImageFormat for fourcc 0x{va_src_fourcc:08x}"
                    ))
                })?;

            let mut image =
                libva::Image::create_from(&surface, image_fmt, (width, height), (width, height))?;
            let va_image = *image.image();
            let dest_pitch = va_image.pitches[0] as usize;
            let dest_offset = va_image.offsets[0] as usize;
            let dest = image.as_mut();
            let row_bytes = (width as usize) * 4;

            let src_size = (stride as usize) * (height as usize);
            let src_len = std::num::NonZeroUsize::new(src_size)
                .ok_or_else(|| crate::Error::Import("zero-size source dma-buf".into()))?;
            let src_ptr = unsafe {
                nix::sys::mman::mmap(
                    None,
                    src_len,
                    ProtFlags::PROT_READ,
                    MapFlags::MAP_SHARED,
                    &src_fd,
                    0,
                )
            }?;

            {
                let src_bytes =
                    unsafe { std::slice::from_raw_parts(src_ptr.as_ptr() as *const u8, src_size) };
                for row in 0..(height as usize) {
                    let src_row = row * (stride as usize);
                    let dst_row = dest_offset + row * dest_pitch;
                    dest[dst_row..dst_row + row_bytes]
                        .copy_from_slice(&src_bytes[src_row..src_row + row_bytes]);
                }
            }

            drop(image);
            drop(src_fd);

            unsafe {
                nix::sys::mman::munmap(src_ptr, src_size)?;
            }

            SrcSurface::VaAllocated(surface)
        };

        let used_idx = self.pool_idx;
        self.pool_idx = (self.pool_idx + 1) % VPP_POOL_SIZE;
        let dst = Rc::clone(&self.dst_pool[used_idx]);

        let ppb = {
            ProcPipelineParameterBuffer::new(
                src_surface.id(),
                None, // whole source
                0,
                None, // whole destination
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                0,
                None,
                0,
                None,
                0,
                0,
                ProcColorProperties::default(),
                ProcColorProperties::default(),
                0,
                None,
            )
        };

        let buf = self
            .context
            .create_buffer(BufferType::ProcPipelineParameter(ppb))?;
        let mut pic = Picture::<PictureNew, Rc<Surface<()>>>::new(0, Rc::clone(&self.context), dst);
        pic.add_buffer(buf);
        pic.begin::<()>()?
            .render()?
            .end()?
            .sync::<()>()
            .map_err(|(e, _)| e)?;

        let prime = self.dst_pool[used_idx].export_prime()?;
        if prime.objects.is_empty() || prime.layers.is_empty() {
            return Err(crate::Error::Import(
                "export_prime returned empty objects or layers".into(),
            ));
        }

        let layer = &prime.layers[0];
        let y_stride = layer.pitch[0] as usize;
        let uv_offset = layer.offset[1] as usize;
        let uv_stride = layer.pitch[1] as usize;
        let modifier = prime.objects[0].drm_format_modifier;

        let nv12_layout = FrameLayout {
            format: (Fourcc::from(b"NV12"), modifier),
            size: Resolution {
                width: self.aligned_width,
                height: self.aligned_height,
            },
            planes: vec![
                PlaneLayout {
                    buffer_index: 0,
                    offset: 0,
                    stride: y_stride,
                },
                PlaneLayout {
                    buffer_index: 0,
                    offset: uv_offset,
                    stride: uv_stride,
                },
            ],
        };

        let exported_file = File::from(prime.objects.into_iter().next().unwrap().fd);
        let dma_frame = GenericDmaVideoFrame::new(vec![exported_file], nv12_layout.clone())
            .map_err(|e| crate::Error::Import(format!("VPP dst GenericDmaVideoFrame::new: {e}")))?;

        Ok((dma_frame, nv12_layout))
    }
}

/// Check whether the low-power encoding entrypoint is available.
fn check_low_power(display: &Display, codec: &Codec) -> bool {
    let va_profile = match codec {
        Codec::H264 => VAProfile::VAProfileH264Main,
        Codec::AV1 => VAProfile::VAProfileAV1Profile0,
    };

    match display.query_config_entrypoints(va_profile) {
        Ok(entrypoints) => entrypoints.contains(&VAEntrypoint::VAEntrypointEncSliceLP),
        Err(_) => false,
    }
}

/// Map a target bitrate (bps) to an AV1 CQP quality value.
///
/// The VA-API backend only supports CQP, so we approximate.
fn bitrate_to_qp(bitrate_bps: u32) -> u32 {
    // TODO: tune this if necessary
    let mbps = (bitrate_bps as f64 / 1_000_000.0).clamp(0.5, 12.0);
    let qp = 50.0 - (mbps - 0.5) / 11.5 * 42.0;
    qp.round() as u32
}
