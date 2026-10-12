//! Regenerate the checked-in Windows icon from the same vector used in the tray.
#![allow(dead_code)]
#[path = "../ui/draw.rs"]
mod draw;
use tiny_skia::*;

fn main() {
    let painter = draw::Painter::try_new().expect("A usable font is required for generating icons");
    let mut images = Vec::new();
    for size in [16u32, 24, 32, 48, 64, 128, 256] {
        let mut pix = Pixmap::new(size, size).unwrap();
        let mut paint = Paint::default();
        paint.set_color_rgba8(245, 245, 245, 255);
        for path in painter.logo.iter().filter(|p| p.bounds().right() < 30.) {
            pix.fill_path(
                path,
                &paint,
                FillRule::Winding,
                Transform::from_row(
                    size as f32 / 32.,
                    0.,
                    0.,
                    size as f32 / 32.,
                    size as f32 * 1.5 / 32.,
                    size as f32 / 8.,
                ),
                None,
            );
        }
        images.push((size, pix.encode_png().unwrap()));
    }
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = (6 + images.len() * 16) as u32;
    for (size, data) in &images {
        ico.extend_from_slice(&[*size as u8, *size as u8, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        ico.extend(data);
    }
    std::fs::write("assets/audeze.ico", ico).expect("save Windows icon");
}
