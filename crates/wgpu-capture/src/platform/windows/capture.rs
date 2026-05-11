use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{ID3D11Resource, ID3D11Texture2D};
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::DirectXPixelFormat,
        SizeInt32,
    },
    Win32::{
        Foundation::POINT,
        Graphics::{
            Dxgi::IDXGISurface,
            Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint},
        },
        System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop,
    },
    core::Interface,
};

use super::{FramePoolSlot, WindowsFrame};
use crate::platform::PlatformFrame;
use crate::platform::windows::directx::{self, create_shared_texture};
use crate::platform::windows::{CAPTURE_POOL_SIZE, com_initialized};
use crate::{CaptureFrame, CaptureTarget, Capturer, Result};

pub(crate) struct WindowsCapturer {
    target: CaptureTarget,
    shared: &'static directx::SharedDevice,
    session: Option<GraphicsCaptureSession>,
    frame_pool_handle: Option<Direct3D11CaptureFramePool>,
    d3d_pool: Arc<crossbeam_queue::ArrayQueue<FramePoolSlot>>,
    frames: Arc<crossbeam_queue::ArrayQueue<WindowsFrame>>,
}

impl WindowsCapturer {
    pub(crate) fn new(target: CaptureTarget) -> Result<Self> {
        if !matches!(target, CaptureTarget::Monitor(_) | CaptureTarget::Window(_)) {
            return Err(crate::Error::UnsupportedCaptureTarget);
        }
        com_initialized();
        let shared = directx::get_shared_device()?;
        Ok(WindowsCapturer {
            target,
            shared,
            session: None,
            frame_pool_handle: None,
            d3d_pool: Arc::new(ArrayQueue::new(CAPTURE_POOL_SIZE)),
            frames: Arc::new(ArrayQueue::new(CAPTURE_POOL_SIZE)),
        })
    }

    fn capture_item(&self) -> Result<GraphicsCaptureItem> {
        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;

        Ok(unsafe {
            match &self.target {
                CaptureTarget::Monitor(_idx) => {
                    // TODO: enumerate monitors by index
                    let hmonitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
                    interop.CreateForMonitor(hmonitor)?
                }
                CaptureTarget::Window(hwnd) => interop.CreateForWindow(HWND(*hwnd as _))?,
            }
        })
    }
}

impl Capturer for WindowsCapturer {
    fn start(&mut self) -> Result<()> {
        let item = self.capture_item()?;

        let size = item.Size()?;
        let (width, height) = (size.Width as u32, size.Height as u32);

        for _ in 0..CAPTURE_POOL_SIZE {
            let data = create_shared_texture(&self.shared.device, width, height)?;
            self.d3d_pool.push(data).ok();
        }

        let winrt_device = directx::wrap_as_winrt_device(&self.shared.device)?;

        let wgc_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            CAPTURE_POOL_SIZE as i32,
            SizeInt32 {
                Width: width as i32,
                Height: height as i32,
            },
        )?;
        let session = wgc_pool.CreateCaptureSession(&item)?;

        let shared = self.shared;
        let d3d_pool = self.d3d_pool.clone();
        let frames = self.frames.clone();

        wgc_pool.FrameArrived(&TypedEventHandler::new(
            move |wgc_pool_ref: windows::core::Ref<'_, Direct3D11CaptureFramePool>, _| {
                let wgc_pool_ref = match wgc_pool_ref.as_ref() {
                    Some(p) => p,
                    None => return Ok(()),
                };
                let frame = match wgc_pool_ref.TryGetNextFrame() {
                    Ok(f) => f,
                    Err(_) => return Ok(()),
                };
                let surface = frame.Surface()?;
                let src_texture: ID3D11Texture2D = unsafe {
                    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
                    let dxgi_surface: IDXGISurface = access.GetInterface()?;
                    dxgi_surface.cast()?
                };
                let dst_data = match d3d_pool.pop() {
                    Some(d) => d,
                    None => return Ok(()),
                };

                let dst_resource: ID3D11Resource = dst_data.texture.cast()?;
                let src_resource: ID3D11Resource = src_texture.cast()?;
                {
                    let _lock = shared.ctx_lock.lock().unwrap();
                    unsafe {
                        shared.ctx.CopyResource(&dst_resource, &src_resource);
                        shared.ctx.Flush();
                    }
                }

                let dst = WindowsFrame::from_pool(dst_data, d3d_pool.clone());
                frames.force_push(dst);
                Ok(())
            },
        ))?;

        session.StartCapture()?;

        self.session = Some(session);
        self.frame_pool_handle = Some(wgc_pool);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(s) = self.session.take() {
            s.Close().ok();
        }
        if let Some(p) = self.frame_pool_handle.take() {
            p.Close().ok();
        }
    }

    fn next_frame(&mut self) -> Option<CaptureFrame> {
        self.frames
            .pop()
            .map(|frame| CaptureFrame(Arc::new(PlatformFrame(frame))))
    }
}
