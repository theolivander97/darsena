//! Captura de una ventana con Windows Graphics Capture (WGC).
//!
//! Prototipo F0: cada fotograma se copia a una textura de staging y se lee en CPU (BGRA).
//! Más adelante se pasará la textura D3D11 directamente al codificador sin bajar a CPU.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

/// Fotograma BGRA tal cual lo entrega la GPU (sin relleno de filas).
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Tamaño de la ventana a capturar (para fijar la resolución del codificador).
pub fn window_size(hwnd: isize) -> Result<(u32, u32)> {
    let item = item_for_window(hwnd)?;
    let s = item.Size()?;
    Ok((s.Width.max(0) as u32, s.Height.max(0) as u32))
}

fn item_for_window(hwnd: isize) -> Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(HWND(hwnd as *mut _)) }
        .context("no se puede capturar esa ventana (¿se cerró o está minimizada?)")
}

/// Bucle de captura: bloquea hasta que `stop` sea true. Envía como máximo `fps` fotogramas/s.
pub fn run(hwnd: isize, fps: u32, stop: Arc<AtomicBool>, tx: SyncSender<RawFrame>) -> Result<()> {
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let item = item_for_window(hwnd)?;

    // Dispositivo D3D11 propio para WGC.
    let mut device: Option<ID3D11Device> = None;
    let mut ctx: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )?;
    }
    let device = device.context("sin dispositivo D3D11")?;
    let ctx = ctx.context("sin contexto D3D11")?;
    let dxgi: IDXGIDevice = device.cast()?;
    let winrt_dev: IDirect3DDevice =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;

    let mut size = item.Size()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt_dev,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )?;
    let session = pool.CreateCaptureSession(&item)?;
    let _ = session.SetIsBorderRequired(false); // Windows 11; en 10 falla y se ignora
    session.StartCapture()?;

    let period = Duration::from_micros(1_000_000 / fps.max(1) as u64);
    let mut next = Instant::now();
    let mut staging: Option<(ID3D11Texture2D, u32, u32)> = None;

    while !stop.load(Ordering::Relaxed) {
        // Nos quedamos con el fotograma más reciente.
        let mut latest = None;
        while let Ok(f) = pool.TryGetNextFrame() {
            latest = Some(f);
        }
        let Some(frame) = latest else {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        if Instant::now() < next {
            continue; // demasiado pronto: descartamos para respetar los fps
        }
        next = Instant::now() + period;

        let content = frame.ContentSize()?;
        if content.Width != size.Width || content.Height != size.Height {
            size = content;
            pool.Recreate(
                &winrt_dev,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                size,
            )?;
        }

        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };

        if staging.as_ref().map(|(_, w, h)| (*w, *h)) != Some((desc.Width, desc.Height)) {
            let mut d = desc;
            d.Usage = D3D11_USAGE_STAGING;
            d.BindFlags = 0;
            d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            d.MiscFlags = 0;
            let mut t: Option<ID3D11Texture2D> = None;
            unsafe { device.CreateTexture2D(&d, None, Some(&mut t))? };
            staging = Some((t.context("sin textura staging")?, desc.Width, desc.Height));
        }
        let (stex, w, h) = staging.as_ref().unwrap();
        unsafe {
            ctx.CopyResource(stex, &tex);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(stex, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let row = (*w * 4) as usize;
            let mut bgra = vec![0u8; row * *h as usize];
            for y in 0..*h as usize {
                let src = (map.pData as *const u8).add(y * map.RowPitch as usize);
                std::ptr::copy_nonoverlapping(src, bgra.as_mut_ptr().add(y * row), row);
            }
            ctx.Unmap(stex, 0);
            // Si el consumidor va lento, descartamos (latencia antes que calidad).
            let _ = tx.try_send(RawFrame { width: *w, height: *h, bgra });
        }
    }

    let _ = session.Close();
    let _ = pool.Close();
    Ok(())
}
