use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::{
    Win32::{
        Foundation::{GENERIC_ALL, HMODULE},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
                ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
                IDXGIAdapter, IDXGIResource1,
            },
        },
    },
    core::Interface,
};

use super::FramePoolSlot;
use crate::Result;

static SHARED_D3D11: OnceLock<SharedDevice> = OnceLock::new();

pub(crate) struct SharedDevice {
    pub device: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    pub ctx_lock: Mutex<()>,
}

pub(crate) fn get_shared_device() -> Result<&'static SharedDevice> {
    if let Some(sd) = SHARED_D3D11.get() {
        return Ok(sd);
    }

    let mut device: Option<ID3D11Device> = None;
    let mut ctx: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )?;
    }
    let device = device.unwrap();
    let ctx = ctx.unwrap();
    unsafe {
        let mt: ID3D11Multithread = device.cast()?;
        mt.SetMultithreadProtected(true).ok().ok();
    }

    let sd = SharedDevice {
        device,
        ctx,
        ctx_lock: Mutex::new(()),
    };

    SHARED_D3D11.set(sd).ok();
    Ok(SHARED_D3D11.get().unwrap())
}

pub(crate) fn wrap_as_winrt_device(
    d3d: &ID3D11Device,
) -> Result<windows::Graphics::DirectX::Direct3D11::IDirect3DDevice> {
    let dxgi: IDXGIDevice = d3d.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
    Ok(inspectable.cast()?)
}

pub(crate) fn create_shared_texture(
    d3d_device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<FramePoolSlot> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET).0 as u32,
        CPUAccessFlags: Default::default(),
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED).0 as u32,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    unsafe {
        d3d_device.CreateTexture2D(&desc, None, Some(&mut texture))?;
    }
    Ok(FramePoolSlot {
        texture: texture.unwrap(),
        device: d3d_device.clone(),
        width,
        height,
    })
}

pub(crate) fn create_nt_handle(
    frame: &FramePoolSlot,
) -> Result<windows::Win32::Foundation::HANDLE> {
    let resource: IDXGIResource1 = frame.texture.cast()?;
    unsafe { Ok(resource.CreateSharedHandle(None, GENERIC_ALL.0, None)?) }
}
