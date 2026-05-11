use std::mem::{ManuallyDrop, MaybeUninit};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Instant;

use tracing::debug;
use windows::Win32::Media::MediaFoundation::eAVEncCommonRateControlMode_CBR;
use windows::{
    Win32::{
        Foundation::{E_NOTIMPL, VARIANT_BOOL},
        Graphics::Direct3D11::{
            D3D11_BIND_DECODER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
            D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
            D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
            D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
            ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
            ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
            ID3D11VideoProcessorOutputView,
        },
        Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC},
        Media::MediaFoundation::{
            CODECAPI_AVEncCommonLowLatency, CODECAPI_AVEncCommonMaxBitRate,
            CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode,
            CODECAPI_AVEncCommonRealTime, CODECAPI_AVEncMPVDefaultBPictureCount,
            CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncVideoForceKeyFrame,
            CODECAPI_AVEncVideoOutputFrameRate, ICodecAPI, IMFActivate, IMFDXGIDeviceManager,
            IMFMediaType, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_LOW_LATENCY,
            MF_MT_AVG_BITRATE, MF_MT_COMPRESSED, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
            MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_VERSION,
            MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer, MFCreateMediaType,
            MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFSTARTUP_FULL, MFShutdown,
            MFStartup, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER,
            MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_INFO, MFT_REGISTER_TYPE_INFO, MFTEnumEx,
            MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_NV12,
            MFVideoInterlace_Progressive,
        },
        System::Com::CoTaskMemFree,
        System::Variant::{
            VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_UI4, VT_UI8,
        },
    },
    core::{GUID, IUnknown, Interface},
};

use crate::platform::windows::com_initialized;
use crate::platform::windows::directx::get_shared_device;
use crate::{CaptureFrame, Codec, EncodeConfig, EncodeOutput, EncodeSession, Result};

enum EncodeCommand {
    Frame(CaptureFrame, i64 /* pts in 100-ns units */),
    SetBitrate(u32),
    RequestKeyframe,
    Finish,
}

pub(crate) struct MfEncoder {
    tx: SyncSender<EncodeCommand>,
    writer_thread: Option<thread::JoinHandle<Result<()>>>,
    frame_count: u64,
    frame_duration_hns: i64,
    last_keyframe_request: Option<Instant>,
}

impl MfEncoder {
    pub(crate) fn new(config: EncodeConfig) -> Result<Self> {
        let frame_duration_hns = 10_000_000i64 / config.fps as i64;

        let (tx, rx) = mpsc::sync_channel::<EncodeCommand>(4);

        let width = config.width;
        let height = config.height;
        let fps = config.fps;
        let bitrate = config.bitrate_bps;
        let codec = config.codec;
        let output = config.output;

        let writer_thread = thread::Builder::new()
            .name("wgpu-capture-mf-writer".to_owned())
            .spawn(move || encode_thread(rx, width, height, fps, bitrate, codec, output))?;

        Ok(MfEncoder {
            tx,
            writer_thread: Some(writer_thread),
            frame_count: 0,
            frame_duration_hns,
            last_keyframe_request: None,
        })
    }
}

impl EncodeSession for MfEncoder {
    fn submit_frame(&mut self, frame: &CaptureFrame) -> Result<()> {
        let pts = self.frame_count as i64 * self.frame_duration_hns;
        self.frame_count += 1;

        match self.tx.try_send(EncodeCommand::Frame(frame.clone(), pts)) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(_)) => {}
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                if let Some(thread_handle) = self.writer_thread.take() {
                    if let Ok(Err(e)) = thread_handle.join() {
                        return Err(e);
                    }
                }
                return Err(crate::Error::Thread);
            }
        }
        Ok(())
    }

    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<()> {
        self.tx
            .send(EncodeCommand::SetBitrate(bitrate_bps))
            .map_err(|_| crate::Error::Thread)
    }

    fn request_keyframe(&mut self) -> Result<()> {
        // 1-second cooldown
        let now = Instant::now();
        if let Some(last) = self.last_keyframe_request {
            if now.duration_since(last) < std::time::Duration::from_secs(1) {
                return Ok(());
            }
        }
        self.last_keyframe_request = Some(now);
        self.tx
            .send(EncodeCommand::RequestKeyframe)
            .map_err(|_| crate::Error::Thread)
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        self.tx.send(EncodeCommand::Finish).ok();
        if let Some(handle) = self.writer_thread.take() {
            handle.join().map_err(|_| crate::Error::Thread)??;
        }
        Ok(())
    }
}

pub struct MediaFoundation;

impl MediaFoundation {
    pub fn new() -> Result<Self> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        }
        Ok(MediaFoundation)
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        unsafe {
            MFShutdown().ok();
        }
    }
}

fn encode_thread(
    rx: std::sync::mpsc::Receiver<EncodeCommand>,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    codec: Codec,
    output: EncodeOutput,
) -> Result<()> {
    unsafe {
        com_initialized();
        let _mf = MediaFoundation::new()?;
        let shared = get_shared_device()?;
        let d3d_device = &shared.device;
        let d3d_ctx = shared.ctx.clone();
        let vp_converter = VideoProcessor::new(d3d_device, d3d_ctx, width, height)?;

        // required for GPU-accelerated MFTs
        let mut reset_token: u32 = 0;
        let mut mgr: Option<IMFDXGIDeviceManager> = None;
        MFCreateDXGIDeviceManager(&mut reset_token, &mut mgr)?;
        let mgr = mgr.unwrap();
        mgr.ResetDevice(d3d_device, reset_token)?;

        let transform = find_encoder_mft(codec, width, height, &mgr, fps)?;
        let codec_api: ICodecAPI = transform.cast()?;

        let gop_size = fps / 2;
        configure_encoder(&codec_api, bitrate, fps, gop_size)?;

        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .ok();
        transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

        let stream_info = transform.GetOutputStreamInfo(0)?;
        let mft_owns_samples = (stream_info.dwFlags & 256) != 0;

        let frame_duration_hns = 10_000_000i64 / fps as i64;

        let callback = &output.0;
        let mut pending_keyframe = false;
        let mut pending_frames: u32 = 0;

        debug!("encoder thread started ({codec:?}), bitrate={bitrate} bps");

        loop {
            if pending_keyframe {
                force_keyframe(&codec_api);
                pending_keyframe = false;
            }
            match rx.recv() {
                Ok(EncodeCommand::Frame(capture_frame, pts)) => {
                    if pending_frames >= MAX_PENDING_FRAMES {
                        // drop frame if encoder is behind
                        continue;
                    }
                    let bgra_texture = &capture_frame.0.0.texture;
                    let nv12_texture = {
                        let _lock = shared.ctx_lock.lock().unwrap();
                        let tex = vp_converter.convert(d3d_device, bgra_texture)?;
                        shared.ctx.Flush();
                        tex
                    };

                    let sample = make_sample_from_texture(nv12_texture, pts, frame_duration_hns)?;

                    transform.ProcessInput(0, &sample, 0)?;
                    pending_frames += 1;

                    drop(capture_frame);

                    let drained =
                        drain_output(&transform, &stream_info, mft_owns_samples, callback)?;
                    pending_frames = pending_frames.saturating_sub(drained);
                }

                Ok(EncodeCommand::SetBitrate(new_bps)) => {
                    let max_bps = (new_bps as f64 * 1.15) as u32;
                    set_codec_u32(&codec_api, &CODECAPI_AVEncCommonMeanBitRate, new_bps);
                    set_codec_u32(&codec_api, &CODECAPI_AVEncCommonMaxBitRate, max_bps);
                }

                Ok(EncodeCommand::RequestKeyframe) => {
                    pending_keyframe = true;
                }

                Ok(EncodeCommand::Finish) | Err(_) => {
                    transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
                    drain_output(&transform, &stream_info, mft_owns_samples, callback)?;
                    break;
                }
            }
        }

        Ok(())
    }
}

const ENC_POOL_SIZE: usize = 4;

struct VideoProcessor {
    dev_ctx: ID3D11DeviceContext,
    ctx: ID3D11VideoContext,
    vp: ID3D11VideoProcessor,
    vpe: ID3D11VideoProcessorEnumerator,
    vp_tex: ID3D11Texture2D,
    out_view: ID3D11VideoProcessorOutputView,
    enc_pool: Vec<ID3D11Texture2D>,
    enc_index: std::cell::Cell<usize>,
}

impl VideoProcessor {
    fn new(
        device: &ID3D11Device,
        ctx: ID3D11DeviceContext,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        unsafe {
            let vdev: ID3D11VideoDevice = device.cast()?;
            let dev_ctx = ctx.clone();
            let vctx: ID3D11VideoContext = ctx.cast()?;

            let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputWidth: width,
                InputHeight: height,
                OutputWidth: width,
                OutputHeight: height,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
                ..Default::default()
            };
            let vpe = vdev.CreateVideoProcessorEnumerator(&content_desc)?;
            let vp = vdev.CreateVideoProcessor(&vpe, 0)?;

            let vp_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: Default::default(),
                MiscFlags: 0,
            };
            let mut vp_opt: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&vp_desc, None, Some(&mut vp_opt))?;
            let vp_tex = vp_opt.unwrap();

            let out_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: Default::default(),
                },
            };
            let mut out_view_opt: Option<ID3D11VideoProcessorOutputView> = None;
            vdev.CreateVideoProcessorOutputView(&vp_tex, &vpe, &out_desc, Some(&mut out_view_opt))?;
            let out_view = out_view_opt.unwrap();

            let enc_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_DECODER.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: Default::default(),
                MiscFlags: 0,
            };
            let mut enc_pool = Vec::with_capacity(ENC_POOL_SIZE);
            for _ in 0..ENC_POOL_SIZE {
                let mut tex: Option<ID3D11Texture2D> = None;
                device.CreateTexture2D(&enc_desc, None, Some(&mut tex))?;
                enc_pool.push(tex.unwrap());
            }

            Ok(VideoProcessor {
                dev_ctx,
                ctx: vctx,
                vp,
                vpe,
                vp_tex,
                out_view,
                enc_pool,
                enc_index: std::cell::Cell::new(0),
            })
        }
    }

    fn convert(&self, device: &ID3D11Device, src: &ID3D11Texture2D) -> Result<&ID3D11Texture2D> {
        unsafe {
            let vdev: ID3D11VideoDevice = device.cast()?;

            let in_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: Default::default(),
                },
            };
            let mut in_view_opt: Option<ID3D11VideoProcessorInputView> = None;
            vdev.CreateVideoProcessorInputView(src, &self.vpe, &in_desc, Some(&mut in_view_opt))?;
            let in_view = in_view_opt.unwrap();

            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                OutputIndex: 0,
                InputFrameOrField: 0,
                PastFrames: 0,
                FutureFrames: 0,
                ppPastSurfaces: std::ptr::null_mut(),
                pInputSurface: ManuallyDrop::new(Some(in_view)),
                ppFutureSurfaces: std::ptr::null_mut(),
                ppPastSurfacesRight: std::ptr::null_mut(),
                pInputSurfaceRight: ManuallyDrop::new(None),
                ppFutureSurfacesRight: std::ptr::null_mut(),
            };

            self.ctx.VideoProcessorBlt(
                &self.vp,
                &self.out_view,
                0,
                std::slice::from_ref(&stream),
            )?;

            stream.pInputSurface.take();
            stream.pInputSurfaceRight.take();

            let idx = self.enc_index.get();
            let dst = &self.enc_pool[idx];
            self.dev_ctx.CopyResource(dst, &self.vp_tex);
            self.enc_index.set((idx + 1) % ENC_POOL_SIZE);

            Ok(dst)
        }
    }
}

/// Map our public Codec to an MF output subtype GUID.
fn codec_output_subtype(codec: Codec) -> GUID {
    match codec {
        Codec::H264 => MFVideoFormat_H264,
        Codec::AV1 => MFVideoFormat_AV1,
    }
}

/// Enumerate hardware encoder MFTs and activate the best match.
fn find_encoder_mft(
    codec: Codec,
    width: u32,
    height: u32,
    mgr: &IMFDXGIDeviceManager,
    fps: u32,
) -> Result<IMFTransform> {
    let input_info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output_info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: codec_output_subtype(codec),
    };

    // hardware sync MFTs
    let result = try_enum_mfts(
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SYNCMFT.0),
        &input_info,
        &output_info,
        width,
        height,
        mgr,
        fps,
        codec,
    );
    if let Ok(t) = result {
        return Ok(t);
    }

    // fall back to any sync MFT (software)
    try_enum_mfts(
        MFT_ENUM_FLAG_SYNCMFT,
        &input_info,
        &output_info,
        width,
        height,
        mgr,
        fps,
        codec,
    )
}

fn try_enum_mfts(
    flags: MFT_ENUM_FLAG,
    input_info: &MFT_REGISTER_TYPE_INFO,
    output_info: &MFT_REGISTER_TYPE_INFO,
    width: u32,
    height: u32,
    mgr: &IMFDXGIDeviceManager,
    fps: u32,
    codec: Codec,
) -> Result<IMFTransform> {
    let mut activate_array: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count: u32 = 0;

    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(input_info),
            Some(output_info),
            &mut activate_array,
            &mut count,
        )?;
    }

    if count == 0 || activate_array.is_null() {
        return Err(crate::Error::UnsupportedCodec(codec));
    }

    for i in 0..count {
        let activate: IMFActivate = match unsafe { (*activate_array.add(i as usize)).take() } {
            Some(a) => a,
            None => continue,
        };

        let transform: IMFTransform = match unsafe { activate.ActivateObject() } {
            Ok(t) => t,
            Err(e) => {
                debug!("ActivateObject: {e}");
                continue;
            }
        };

        // IMPORTANT: set MF_LOW_LATENCY on the MFT to minimise internal buffering
        let attrs = unsafe { transform.GetAttributes() }?;
        (unsafe { attrs.SetUINT32(&MF_LOW_LATENCY, 1) })?;

        share_d3d11_with_mft(&transform, mgr).ok();

        let partial_in = build_partial_input_media_type()?;
        unsafe {
            transform.SetInputType(0, &partial_in, 0).ok();
        }

        if let Err(e) = negotiate_output_type(&transform, codec, width, height, fps) {
            debug!("negotiate_output_type: {e}");
            continue;
        }

        let in_type = build_input_media_type(width, height, fps)?;
        if let Err(e) = unsafe { transform.SetInputType(0, &in_type, 0) } {
            debug!("SetInputType: {e}");
            continue;
        }

        unsafe { CoTaskMemFree(Some(activate_array as *const _)) };
        return Ok(transform);
    }

    unsafe { CoTaskMemFree(Some(activate_array as *const _)) };
    Err(crate::Error::UnsupportedCodec(codec))
}

fn share_d3d11_with_mft(transform: &IMFTransform, mgr: &IMFDXGIDeviceManager) -> Result<bool> {
    unsafe {
        let mgr_unknown: IUnknown = mgr.cast()?;
        let ptr = Interface::as_raw(&mgr_unknown) as usize;
        let hr = transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, ptr);
        if let Err(ref e) = hr {
            if e.code() == E_NOTIMPL {
                return Ok(false);
            }
        }
        hr?;
        Ok(true)
    }
}

fn negotiate_output_type(
    transform: &IMFTransform,
    codec: Codec,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<()> {
    unsafe {
        let target_subtype = codec_output_subtype(codec);

        for idx in 0u32.. {
            let mt = match transform.GetOutputAvailableType(0, idx) {
                Ok(m) => m,
                Err(_) => break,
            };

            let subtype = match mt.GetGUID(&MF_MT_SUBTYPE) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if subtype != target_subtype {
                continue;
            }

            if let Ok(frame_size) = mt.GetUINT64(&MF_MT_FRAME_SIZE) {
                let w = (frame_size >> 32) as u32;
                let h = frame_size as u32;
                if w != width || h != height {
                    continue;
                }
            }

            let cloned = MFCreateMediaType()?;
            mt.CopyAllItems(&cloned).ok();

            cloned
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_u32(width, height))
                .ok();
            cloned.SetUINT64(&MF_MT_FRAME_RATE, pack_u32(fps, 1)).ok();
            cloned
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .ok();
            cloned.SetUINT32(&MF_MT_COMPRESSED, 1).ok();
            cloned.SetUINT32(&MF_MT_AVG_BITRATE, 8_000_000).ok();

            if let Err(e) = transform.SetOutputType(0, &cloned, 0) {
                debug!("SetOutputType: {e}");
                continue;
            }
            return Ok(());
        }
        Err(crate::Error::UnsupportedCodec(codec))
    }
}

fn configure_encoder(
    codec_api: &ICodecAPI,
    bitrate_bps: u32,
    fps: u32,
    gop_size: u32,
) -> Result<()> {
    unsafe {
        // Rate control
        set_codec_u32(
            codec_api,
            &CODECAPI_AVEncCommonRateControlMode,
            eAVEncCommonRateControlMode_CBR.0 as u32,
        );
        // Target bitrate
        set_codec_u32(codec_api, &CODECAPI_AVEncCommonMeanBitRate, bitrate_bps);
        // Max bitrate
        let max_bps = (bitrate_bps as f64 * 1.15) as u32;
        set_codec_u32(codec_api, &CODECAPI_AVEncCommonMaxBitRate, max_bps);
        // GOP size (keyframe interval in frames)
        set_codec_u32(codec_api, &CODECAPI_AVEncMPVGOPSize, gop_size);
        // Disable B-frames
        set_codec_u32(codec_api, &CODECAPI_AVEncMPVDefaultBPictureCount, 0);
        // Low latency mode
        set_codec_bool(codec_api, &CODECAPI_AVEncCommonLowLatency, true);
        // Real-time encoding mode
        set_codec_bool(codec_api, &CODECAPI_AVEncCommonRealTime, true);
        // Output frame rate
        let rate = pack_u32(fps, 1);
        let var = make_variant_u64(rate);
        codec_api
            .SetValue(&CODECAPI_AVEncVideoOutputFrameRate, &var)
            .ok();

        Ok(())
    }
}

/// Build a VARIANT holding a VT_UI4 value.
fn make_variant_u32(value: u32) -> VARIANT {
    unsafe {
        let inner = VARIANT_0_0 {
            vt: VT_UI4,
            Anonymous: {
                let mut a: MaybeUninit<VARIANT_0_0_0> = MaybeUninit::zeroed();
                (*a.as_mut_ptr()).ulVal = value;
                a.assume_init()
            },
            ..Default::default()
        };
        VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(inner),
            },
        }
    }
}

/// Build a VARIANT holding a VT_UI8 (u64) value.
fn make_variant_u64(value: u64) -> VARIANT {
    unsafe {
        let inner = VARIANT_0_0 {
            vt: VT_UI8,
            Anonymous: {
                let mut a: MaybeUninit<VARIANT_0_0_0> = MaybeUninit::zeroed();
                (*a.as_mut_ptr()).ullVal = value;
                a.assume_init()
            },
            ..Default::default()
        };
        VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(inner),
            },
        }
    }
}

/// Build a VARIANT holding a VT_BOOL value.
fn make_variant_bool(value: bool) -> VARIANT {
    unsafe {
        let bv = if value {
            VARIANT_BOOL(-1)
        } else {
            VARIANT_BOOL(0)
        };
        let inner = VARIANT_0_0 {
            vt: VT_BOOL,
            Anonymous: {
                let mut a: MaybeUninit<VARIANT_0_0_0> = MaybeUninit::zeroed();
                (*a.as_mut_ptr()).boolVal = bv;
                a.assume_init()
            },
            ..Default::default()
        };
        VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(inner),
            },
        }
    }
}

/// Set a UINT32 codec property.
fn set_codec_u32(api: &ICodecAPI, key: &GUID, value: u32) {
    let var = make_variant_u32(value);
    unsafe {
        api.SetValue(key, &var).ok();
    }
}

/// Set a BOOL codec property.
fn set_codec_bool(api: &ICodecAPI, key: &GUID, value: bool) {
    let var = make_variant_bool(value);
    unsafe {
        api.SetValue(key, &var).ok();
    }
}

fn force_keyframe(codec_api: &ICodecAPI) {
    let var = make_variant_bool(true);
    unsafe {
        codec_api
            .SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &var)
            .ok();
    }
}

fn build_partial_input_media_type() -> Result<IMFMediaType> {
    unsafe {
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        Ok(mt)
    }
}

fn build_input_media_type(width: u32, height: u32, fps: u32) -> Result<IMFMediaType> {
    unsafe {
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32(width, height))?;
        mt.SetUINT64(&MF_MT_FRAME_RATE, pack_u32(fps, 1))?;
        mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        Ok(mt)
    }
}

fn make_sample_from_texture(
    texture: &ID3D11Texture2D,
    pts_hns: i64,
    duration_hns: i64,
) -> Result<IMFSample> {
    unsafe {
        let texture_as_unknown: IUnknown = texture.cast()?;
        let buf = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &texture_as_unknown, 0, false)?;

        {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            buf.SetCurrentLength(desc.Width * desc.Height * 3 / 2)?;
        }

        let sample = MFCreateSample()?;
        sample.AddBuffer(&buf)?;
        sample.SetSampleTime(pts_hns)?;
        sample.SetSampleDuration(duration_hns)?;
        Ok(sample)
    }
}

const MAX_PENDING_FRAMES: u32 = 8;

fn drain_output(
    transform: &IMFTransform,
    stream_info: &MFT_OUTPUT_STREAM_INFO,
    mft_owns_samples: bool,
    callback: &std::sync::Arc<dyn Fn(Vec<u8>) + Send + Sync>,
) -> Result<u32> {
    let mut drained = 0u32;
    unsafe {
        loop {
            let mut out_bufs = [build_output_buffer(stream_info, mft_owns_samples)];
            let mut status: u32 = 0;

            match transform.ProcessOutput(0, &mut out_bufs, &mut status) {
                Ok(()) => {
                    drained += 1;
                    if let Some(sample) = out_bufs[0].pSample.as_ref() {
                        if let Ok(data) = extract_encoded_data(sample) {
                            if !data.is_empty() {
                                callback(data);
                            }
                        }
                    }
                    out_bufs[0].pSample.take();
                    out_bufs[0].pEvents.take();
                }
                Err(e) if e == MF_E_TRANSFORM_NEED_MORE_INPUT.into() => {
                    out_bufs[0].pSample.take();
                    out_bufs[0].pEvents.take();
                    break;
                }
                Err(e) => {
                    out_bufs[0].pSample.take();
                    out_bufs[0].pEvents.take();
                    return Err(e.into());
                }
            }
        }
        Ok(drained)
    }
}

fn build_output_buffer(
    stream_info: &MFT_OUTPUT_STREAM_INFO,
    mft_owns_samples: bool,
) -> MFT_OUTPUT_DATA_BUFFER {
    unsafe {
        let sample = if mft_owns_samples {
            None
        } else {
            match MFCreateSample() {
                Ok(s) => {
                    if let Ok(buf) = MFCreateMemoryBuffer(stream_info.cbSize) {
                        s.AddBuffer(&buf).ok();
                    }
                    Some(s)
                }
                Err(_) => None,
            }
        };

        MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
            pSample: ManuallyDrop::new(sample),
        }
    }
}

/// Extract encoded bytes from an output IMFSample.
fn extract_encoded_data(sample: &IMFSample) -> Result<Vec<u8>> {
    unsafe {
        let buf = sample.GetBufferByIndex(0)?;
        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut cur_len: u32 = 0;
        buf.Lock(&mut data_ptr, None, Some(&mut cur_len))?;
        let data = std::slice::from_raw_parts(data_ptr, cur_len as usize).to_vec();
        buf.Unlock()?;
        Ok(data)
    }
}

fn pack_u32(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}
