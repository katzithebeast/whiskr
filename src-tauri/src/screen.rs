//! /screen in the chat: one picture of the monitor the chat is on.
//! Plain GDI (BitBlt of that monitor), scaled down to a sensible size, PNG.

use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Longest side of the picture (models scale bigger ones down anyway).
const MAX_SIDE: u32 = 1600;

/// PNG of the monitor `near` is on (0 = the one with the mouse).
pub fn capture(near: isize) -> Result<Vec<u8>, String> {
    unsafe {
        // let DWM put a frame on screen without the (just cloaked) chat
        DwmFlush();
        DwmFlush();
        let mon = if near != 0 {
            MonitorFromWindow(near as _, MONITOR_DEFAULTTONEAREST)
        } else {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
        };
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return Err("Snímek: monitor nenalezen".into());
        }
        let r = mi.rcMonitor;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || h <= 0 {
            return Err("Snímek: prázdný monitor".into());
        }
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp as _);
        let ok = BitBlt(mem, 0, 0, w, h, screen, r.left, r.top, SRCCOPY | CAPTUREBLT) != 0;
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h; // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB as _;
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(mem, bmp, 0, h as u32, bgra.as_mut_ptr() as _, &mut bi, DIB_RGB_COLORS);
        SelectObject(mem, old);
        DeleteObject(bmp as _);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        if !ok || lines == 0 {
            return Err("Snímek obrazovky se nepovedl".into());
        }
        let (w, h) = (w as u32, h as u32);
        let (rgb, ow, oh) = downscale(&bgra, w, h);
        encode_png(&rgb, ow, oh)
    }
}

/// BGRA → RGB, box-filtered down so the longest side is ≤ MAX_SIDE.
fn downscale(bgra: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let scale = (w.max(h) as f64 / MAX_SIDE as f64).max(1.0);
    let ow = ((w as f64 / scale).round() as u32).max(1);
    let oh = ((h as f64 / scale).round() as u32).max(1);
    let mut out = vec![0u8; (ow * oh * 3) as usize];
    for oy in 0..oh {
        let y0 = (oy as f64 * scale) as u32;
        let y1 = (((oy + 1) as f64 * scale) as u32).clamp(y0 + 1, h);
        for ox in 0..ow {
            let x0 = (ox as f64 * scale) as u32;
            let x1 = (((ox + 1) as f64 * scale) as u32).clamp(x0 + 1, w);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in y0..y1 {
                let row = (y * w) as usize * 4;
                for x in x0..x1 {
                    let i = row + x as usize * 4;
                    b += bgra[i] as u32;
                    g += bgra[i + 1] as u32;
                    r += bgra[i + 2] as u32;
                    n += 1;
                }
            }
            let o = ((oy * ow + ox) * 3) as usize;
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
        }
    }
    (out, ow, oh)
}

fn encode_png(rgb: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(|e| format!("PNG: {e}"))?;
        wr.write_image_data(rgb).map_err(|e| format!("PNG: {e}"))?;
    }
    Ok(out)
}
