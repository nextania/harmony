pub(crate) mod capture;
pub(crate) mod directx;
pub(crate) mod encode;
pub(crate) mod import_dx12;
pub(crate) mod import_vk;

use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use tracing::error;
use windows::Win32::{
    Foundation::{RPC_E_CHANGED_MODE, S_FALSE},
    Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D},
    System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
};

use crate::Result;

pub(crate) const CAPTURE_POOL_SIZE: usize = 4;

pub(crate) struct FramePoolSlot {
    pub texture: ID3D11Texture2D,
    pub device: ID3D11Device,
    pub width: u32,
    pub height: u32,
}

pub(crate) struct WindowsFrame {
    inner: Option<FramePoolSlot>,
    pool: Arc<ArrayQueue<FramePoolSlot>>,
}

impl WindowsFrame {
    pub(crate) fn from_pool(data: FramePoolSlot, pool: Arc<ArrayQueue<FramePoolSlot>>) -> Self {
        WindowsFrame {
            inner: Some(data),
            pool,
        }
    }
}

impl Drop for WindowsFrame {
    fn drop(&mut self) {
        let data = match self.inner.take() {
            Some(d) => d,
            None => return,
        };

        self.pool.push(data).ok();
    }
}

impl std::ops::Deref for WindowsFrame {
    type Target = FramePoolSlot;
    fn deref(&self) -> &FramePoolSlot {
        self.inner.as_ref().expect("WindowsFrame used after drop")
    }
}

/// Ensures COM (MTA) is initialised on the current thread.
pub(crate) fn com_initialized() {
    COM.with(|_| {})
}

thread_local! {
    static COM: ComRuntime = ComRuntime::new().expect("Failed to initialise COM runtime");
}

struct ComRuntime;

impl ComRuntime {
    fn new() -> Result<Self> {
        unsafe {
            // use MTA because Media Foundation prefers it
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() && hr != S_FALSE {
                if hr == RPC_E_CHANGED_MODE {
                    error!(
                        "COM already initialised as STA on this thread; \
                        wgpu-capture requires MTA. Call create_capturer from \
                        a background thread instead.",
                    );
                    return Err(crate::Error::Com);
                }
                error!("CoInitializeEx failed: {hr:?}");
                return Err(crate::Error::Com);
            }
        }
        Ok(Self)
    }
}

impl Drop for ComRuntime {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
