use std::os::fd::{BorrowedFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::PersistMode;
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::screencast::{SelectSourcesOptions, StartCastOptions};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use pipewire::channel;
use pipewire::context::ContextRc;
use pipewire::main_loop::MainLoopRc;
use pipewire::spa::buffer::DataType;
use pipewire::spa::param::ParamType;
use pipewire::spa::param::video::{VideoFormat, VideoInfoRaw};
use pipewire::spa::pod::builder::Builder;
use pipewire::spa::sys::{
    SPA_CHOICE_Enum, SPA_CHOICE_None, SPA_FORMAT_VIDEO_format, SPA_FORMAT_VIDEO_modifier,
    SPA_FORMAT_mediaSubtype, SPA_FORMAT_mediaType, SPA_MEDIA_SUBTYPE_raw, SPA_MEDIA_TYPE_video,
    SPA_PARAM_EnumFormat, SPA_POD_PROP_FLAG_MANDATORY, SPA_TYPE_OBJECT_Format,
    SPA_VIDEO_FORMAT_BGRA, SPA_VIDEO_FORMAT_BGRx,
};
use pipewire::spa::utils::Id;
use pipewire::stream::StreamFlags;
use pipewire::stream::StreamRc;
use std::mem::MaybeUninit;

use crate::platform::PlatformFrame;
use crate::platform::linux::LinuxFrame;
use crate::{CaptureFrame, CaptureTarget, Capturer, Result};
use tracing::debug;

pub(crate) struct LinuxCapturer {
    capture_thread: Option<thread::JoinHandle<()>>,
    stop_tx: Option<std::sync::mpsc::SyncSender<()>>,
    frame_rx: Option<Receiver<CaptureFrame>>,
}

impl LinuxCapturer {
    pub(crate) fn new(target: CaptureTarget) -> Result<Self> {
        if !matches!(target, CaptureTarget::System) {
            return Err(crate::Error::UnsupportedCaptureTarget);
        }
        Ok(LinuxCapturer {
            capture_thread: None,
            stop_tx: None,
            frame_rx: None,
        })
    }
}

impl Capturer for LinuxCapturer {
    fn start(&mut self) -> Result<()> {
        let (frame_tx, frame_rx) = mpsc::sync_channel::<CaptureFrame>(4);
        let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);

        let handle = thread::Builder::new()
            .name("wgpu-capture-pipewire".to_owned())
            .spawn(move || capture_thread(frame_tx, stop_rx))?;

        self.capture_thread = Some(handle);
        self.stop_tx = Some(stop_tx);
        self.frame_rx = Some(frame_rx);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            tx.send(()).ok();
        }
        if let Some(handle) = self.capture_thread.take() {
            handle.join().ok();
        }
    }

    fn next_frame(&mut self) -> Option<CaptureFrame> {
        self.frame_rx.as_ref()?.try_recv().ok()
    }
}

fn capture_thread(frame_tx: SyncSender<CaptureFrame>, stop_rx: std::sync::mpsc::Receiver<()>) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            debug!("tokio runtime build failed: {e}");
            return;
        }
    };

    let (portal_tx, portal_rx) = mpsc::channel::<Result<(OwnedFd, u32)>>();
    let (close_tx, close_rx) = tokio::sync::oneshot::channel::<()>();

    let portal_task = rt.spawn(open_screencast_portal(portal_tx, close_rx));

    let (pw_fd, node_id) = match portal_rx.recv() {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            debug!("screencast portal: {e}");
            return;
        }
        Err(_) => {
            debug!("screencast portal task exited before opening a stream");
            return;
        }
    };

    run_pipewire_loop((pw_fd, node_id), frame_tx, stop_rx);

    close_tx.send(()).ok();
    rt.block_on(async {
        portal_task.await.ok();
    });
}

async fn open_screencast_portal(
    portal_tx: std::sync::mpsc::Sender<Result<(OwnedFd, u32)>>,
    close_rx: tokio::sync::oneshot::Receiver<()>,
) {
    macro_rules! try_or_report {
        ($e:expr) => {
            match $e {
                Ok(v) => v,
                Err(e) => {
                    portal_tx.send(Err(e.into())).ok();
                    return;
                }
            }
        };
    }

    let proxy = try_or_report!(Screencast::new().await);
    let session = try_or_report!(proxy.create_session(CreateSessionOptions::default()).await);
    let options = SelectSourcesOptions::default()
        .set_cursor_mode(CursorMode::Embedded)
        .set_sources(SourceType::Monitor | SourceType::Window)
        .set_multiple(false)
        .set_persist_mode(PersistMode::DoNot);
    try_or_report!(proxy.select_sources(&session, options).await);
    let response = try_or_report!(
        try_or_report!(
            proxy
                .start(&session, None, StartCastOptions::default())
                .await
        )
        .response()
    );
    let node_id = match response.streams().first() {
        Some(stream) => stream.pipe_wire_node_id(),
        None => {
            portal_tx.send(Err(crate::Error::NoSuitableStreams)).ok();
            return;
        }
    };
    let pw_fd = try_or_report!(
        proxy
            .open_pipe_wire_remote(&session, Default::default())
            .await
    );
    let owned = try_or_report!(nix::unistd::dup(&pw_fd));

    if portal_tx.send(Ok((owned, node_id))).is_err() {
        session.close().await.ok();
        return;
    }

    close_rx.await.ok();
    session.close().await.ok();
}

fn run_pipewire_loop(
    (pw_fd, node_id): (OwnedFd, u32),
    frame_tx: SyncSender<CaptureFrame>,
    stop_rx: std::sync::mpsc::Receiver<()>,
) {
    let (sender, receiver) = channel::channel();
    thread::spawn(move || {
        let main_loop = match MainLoopRc::new(None) {
            Ok(ml) => ml,
            Err(e) => {
                debug!("PipeWire MainLoop: {e}");
                return;
            }
        };
        let ml = main_loop.clone();
        let _attached = receiver.attach(main_loop.loop_(), move |_| {
            ml.quit();
        });

        let context = match ContextRc::new(&main_loop, None) {
            Ok(c) => c,
            Err(e) => {
                debug!("PipeWire Context: {e}");
                return;
            }
        };

        let core = match context.connect_fd_rc(pw_fd, None) {
            Ok(c) => c,
            Err(e) => {
                debug!("PipeWire connect_fd: {e}");
                return;
            }
        };

        struct StreamUserData {
            frame_tx: SyncSender<CaptureFrame>,
            video_info: Option<VideoInfoRaw>,
        }

        let frame_tx_clone = frame_tx.clone();

        let stream = match StreamRc::new(
            core,
            "wgpu-capture",
            pipewire::properties::properties! {
                "media.type" => "Video",
                "media.category" => "Capture",
                "media.role" => "Screen",
            },
        ) {
            Ok(s) => s,
            Err(e) => {
                debug!("PipeWire Stream: {e}");
                return;
            }
        };

        let _listener = stream
            .add_local_listener_with_user_data(StreamUserData {
                frame_tx: frame_tx_clone,
                video_info: None,
            })
            .param_changed(|_stream, user_data, id, pod| {
                if id != ParamType::Format.as_raw() {
                    return;
                }
                if let Some(pod) = pod {
                    let mut info = VideoInfoRaw::new();
                    if info.parse(pod).is_ok() {
                        user_data.video_info = Some(info);
                    }
                }
            })
            .process(|stream, user_data| {
                let mut buf = match stream.dequeue_buffer() {
                    Some(b) => b,
                    None => return,
                };
                let datas = buf.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &datas[0];
                if data.type_() != DataType::DmaBuf {
                    return;
                }

                let fd = unsafe { BorrowedFd::borrow_raw(data.as_raw().fd as RawFd) };

                {
                    let mut pfd = [PollFd::new(fd, PollFlags::POLLIN)];
                    poll(&mut pfd, PollTimeout::NONE).ok();
                }

                let video_info = match user_data.video_info {
                    Some(ref info) => info,
                    None => return,
                };
                let size = video_info.size();
                let width = size.width;
                let height = size.height;
                if width == 0 || height == 0 {
                    return;
                }

                const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;
                let modifier = {
                    let m = video_info.modifier();
                    if m == DRM_FORMAT_MOD_INVALID { 0 } else { m }
                };

                let fourcc = spa_video_format_to_drm_fourcc(video_info.format())
                    .unwrap_or(drm_fourcc::DrmFourcc::Argb8888);

                let chunk = data.chunk();
                let bytes_per_pixel: u32 = match fourcc {
                    drm_fourcc::DrmFourcc::Nv12 | drm_fourcc::DrmFourcc::Yuv420 => 1,
                    _ => 4, // all 32-bit packed BGRA/XRGB formats
                };
                let stride = {
                    let s = chunk.stride().unsigned_abs();
                    if s == 0 { width * bytes_per_pixel } else { s }
                };
                let display_fd = match nix::unistd::dup(fd) {
                    Ok(fd) => fd,
                    Err(_) => return,
                };
                let encode_fd = match nix::unistd::dup(fd) {
                    Ok(fd) => fd,
                    Err(_) => return,
                };

                let linux_frame = LinuxFrame {
                    display_fd,
                    encode_fd,
                    width,
                    height,
                    stride,
                    fourcc,
                    modifier,
                };

                let capture_frame = CaptureFrame(Arc::new(PlatformFrame(linux_frame)));
                user_data.frame_tx.try_send(capture_frame).ok();
            })
            .register()
            .expect("register PipeWire listener");

        let mut param_buf = vec![0u8; 1024];
        let param_pod = build_format_params(&mut param_buf);
        let mut pod_arr;
        let params_slice: &mut [&pipewire::spa::pod::Pod] = if let Some(pod) = param_pod {
            pod_arr = [pod];
            &mut pod_arr
        } else {
            &mut []
        };
        stream
            .connect(
                pipewire::spa::utils::Direction::Input,
                Some(node_id),
                StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
                params_slice,
            )
            .expect("stream connect");
        main_loop.run();
    });
    stop_rx.recv().ok();
    sender.send(()).ok();
}

fn build_format_params(buf: &mut Vec<u8>) -> Option<&pipewire::spa::pod::Pod> {
    buf.clear();
    buf.resize(1024, 0);
    let mut b = Builder::new(buf);

    unsafe {
        let mut obj_frame = MaybeUninit::<pipewire::spa::sys::spa_pod_frame>::uninit();
        b.push_object(&mut obj_frame, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat)
            .ok()?;

        // mediaType = Video
        b.add_prop(SPA_FORMAT_mediaType, 0).ok()?;
        b.add_id(Id(SPA_MEDIA_TYPE_video)).ok()?;

        // mediaSubtype = Raw
        b.add_prop(SPA_FORMAT_mediaSubtype, 0).ok()?;
        b.add_id(Id(SPA_MEDIA_SUBTYPE_raw)).ok()?;

        b.add_prop(SPA_FORMAT_VIDEO_format, 0).ok()?;
        let mut fmt_frame = MaybeUninit::<pipewire::spa::sys::spa_pod_frame>::uninit();
        b.push_choice(&mut fmt_frame, SPA_CHOICE_Enum, 0).ok()?;
        b.add_id(Id(SPA_VIDEO_FORMAT_BGRA)).ok()?; // default/preferred choice
        b.add_id(Id(SPA_VIDEO_FORMAT_BGRA)).ok()?;
        b.add_id(Id(SPA_VIDEO_FORMAT_BGRx)).ok()?;
        b.pop(fmt_frame.assume_init_mut());

        // modifier: ONLY LINEAR (0).
        // SPA_CHOICE_None = single fixed value (no alternatives).
        b.add_prop(SPA_FORMAT_VIDEO_modifier, SPA_POD_PROP_FLAG_MANDATORY)
            .ok()?;
        let mut mod_frame = MaybeUninit::<pipewire::spa::sys::spa_pod_frame>::uninit();
        b.push_choice(&mut mod_frame, SPA_CHOICE_None, 0).ok()?;
        b.add_long(0).ok()?; // DRM_FORMAT_MOD_LINEAR = 0
        b.pop(mod_frame.assume_init_mut());

        b.pop(obj_frame.assume_init_mut());
    }

    // The pod starts at the beginning of the buffer.
    // Pod is #[repr(transparent)] over spa_pod, so this cast is safe.
    let pod = unsafe { &*(buf.as_ptr() as *const pipewire::spa::pod::Pod) };
    Some(pod)
}

fn spa_video_format_to_drm_fourcc(fmt: VideoFormat) -> Option<drm_fourcc::DrmFourcc> {
    match fmt {
        VideoFormat::BGRA => Some(drm_fourcc::DrmFourcc::Argb8888),
        VideoFormat::BGRx => Some(drm_fourcc::DrmFourcc::Xrgb8888),
        VideoFormat::RGBA => Some(drm_fourcc::DrmFourcc::Abgr8888),
        VideoFormat::RGBx => Some(drm_fourcc::DrmFourcc::Xbgr8888),
        VideoFormat::NV12 => Some(drm_fourcc::DrmFourcc::Nv12),
        VideoFormat::I420 => Some(drm_fourcc::DrmFourcc::Yuv420),
        _ => None,
    }
}
