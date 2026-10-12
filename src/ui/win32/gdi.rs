//! GDI surfaces and the notification icon, the Windows-only drawing support.
use super::*;
pub(super) struct Bitmap {
    pub(super) dc: HDC,
    pub(super) bitmap: HBITMAP,
    old: HGDIOBJ,
    pub(super) size: SIZE,
    bits: std::ptr::NonNull<u8>,
}
impl Bitmap {
    pub(super) unsafe fn new(w: u32, h: u32) -> Option<Self> {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        let Some(bits) = std::ptr::NonNull::new(bits.cast::<u8>()) else {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, bitmap.into());
        if old.is_invalid() || old.0 as isize == -1 {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            return None;
        }
        Some(Self {
            dc,
            bitmap,
            old,
            size: SIZE {
                cx: w as i32,
                cy: h as i32,
            },
            bits,
        })
    }

    pub(super) fn write(&mut self, pixels: &[u8]) {
        let len = (self.size.cx as usize) * (self.size.cy as usize) * 4;
        // Both dimensions come from this frame's client rectangle.
        assert_eq!(pixels.len(), len);
        // SAFETY: the owned top-down 32-bit DIB has exactly this byte capacity.
        // Its backing storage outlives this borrow; only the UI thread uses it.
        // Finish GDI's previous use before writing the retained DIB directly.
        let dst = unsafe {
            let _ = GdiFlush();
            std::slice::from_raw_parts_mut(self.bits.as_ptr(), len)
        };
        for (src, dst) in pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(dst.as_chunks_mut::<4>().0.iter_mut())
        {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
    }
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
pub(super) unsafe fn create_tray_icon(painter: &Painter) -> HICON {
    let size = 32;
    let Some(pix) = tray_icon_pixmap(painter, size) else {
        return HICON::default();
    };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let Ok(color) = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
        return HICON::default();
    };
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
    for (s, d) in pix
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<4>().0.iter_mut())
    {
        d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
    }
    let mask_bits = [0u8; 128];
    let mask = CreateBitmap(
        size as i32,
        size as i32,
        1,
        1,
        Some(mask_bits.as_ptr().cast()),
    );
    let icon = CreateIconIndirect(&ICONINFO {
        fIcon: BOOL(1),
        hbmColor: color,
        hbmMask: mask,
        ..Default::default()
    })
    .unwrap_or_default();
    let _ = DeleteObject(color.into());
    let _ = DeleteObject(mask.into());
    if std::env::args().any(|s| s == "--snapshot-preview") {
        let _ = pix.save_png("diagnostics/tray-icon.png");
    }
    icon
}
