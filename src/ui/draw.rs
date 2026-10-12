//! CPU rasterization at the final display resolution, with independent alpha.
use fontdue::{Font, FontSettings, Metrics};
use std::{collections::HashMap, path::PathBuf};
use tiny_skia::*;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Box2 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Box2 {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn inset(self, n: f32) -> Self {
        Self::new(self.x + n, self.y + n, self.w - 2. * n, self.h - 2. * n)
    }
}
pub struct Painter {
    pub fonts: [Font; 2],
    glyphs: HashMap<(char, u16, bool), (Metrics, Vec<u8>)>,
    pub logo: Vec<Path>,
}
pub struct Canvas<'a> {
    pub pix: Pixmap,
    pub scale: f32,
    pub painter: &'a mut Painter,
}

/// Bilinear glyph coverage retains fractional movement instead of rounding each
/// animation frame to a whole pixel. Outside the cached glyph is transparent.
fn glyph_coverage(bitmap: &[u8], width: usize, height: usize, x: f32, y: f32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let sample = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            0.
        } else {
            bitmap[y as usize * width + x as usize] as f32
        }
    };
    let a = sample(ix, iy) * (1. - fx) + sample(ix + 1, iy) * fx;
    let b = sample(ix, iy + 1) * (1. - fx) + sample(ix + 1, iy + 1) * fx;
    a * (1. - fy) + b * fy
}
pub fn asset(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [
        exe.join("assets").join(name),
        PathBuf::from("assets/local").join(name),
    ]
    .into_iter()
    .find(|p| p.exists())
}
#[derive(Clone, Copy)]
enum FontWeight {
    Regular,
    Semibold,
}
/// The platform's UI font for `weight`: Segoe UI on Windows.
#[cfg(windows)]
fn system_font(weight: FontWeight) -> Option<PathBuf> {
    let file = match weight {
        FontWeight::Regular => "segoeui.ttf",
        FontWeight::Semibold => "seguisb.ttf",
    };
    Some(PathBuf::from(r"C:\Windows\Fonts").join(file))
}
/// The user's default sans-serif as named by fontconfig, then common font files.
/// Run once at startup; no font is bundled with the application.
#[cfg(not(windows))]
fn system_font(weight: FontWeight) -> Option<PathBuf> {
    let (pattern, fallbacks) = match weight {
        FontWeight::Regular => (
            "sans-serif:weight=regular",
            [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ],
        ),
        FontWeight::Semibold => (
            "sans-serif:weight=demibold",
            [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/noto/NotoSans-Bold.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            ],
        ),
    };
    std::process::Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|file| usable_font_file(file.trim()))
        .or_else(|| fallbacks.iter().find_map(|path| usable_font_file(path)))
}
/// An existing absolute TrueType or OpenType file; fontdue reads nothing else.
#[cfg(not(windows))]
fn usable_font_file(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    (path.is_absolute() && matches!(extension.as_str(), "ttf" | "otf") && path.is_file())
        .then_some(path)
}
const NO_UI_FONT: &str = "No usable UI font was found. Install a sans-serif TrueType font \
(for example DejaVu Sans or Noto Sans) and make sure fontconfig can find it.";
impl Painter {
    #[cfg(any(windows, test))]
    pub fn new() -> Self {
        Self::try_new().unwrap_or_else(|error| panic!("{error}"))
    }
    /// Loads the UI fonts, or says why that is impossible.
    pub fn try_new() -> Result<Self, String> {
        let load = |name: &str, weight: FontWeight| -> Result<Font, String> {
            if let Some(font) = asset(name)
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| Font::from_bytes(b, FontSettings::default()).ok())
            {
                return Ok(font);
            }
            let path = system_font(weight).ok_or(NO_UI_FONT)?;
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("Could not read the UI font {}: {e}", path.display()))?;
            Font::from_bytes(bytes, FontSettings::default())
                .map_err(|e| format!("The UI font {} is not usable: {e}", path.display()))
        };
        // The installed Demi Bold file is a demo font whose punctuation draws a
        // watermark. Use the complete Medium face for a consistent, valid glyph set.
        let fonts = [
            load("connary fagen - visby cf medium.otf", FontWeight::Regular)?,
            load("connary fagen - visby cf medium.otf", FontWeight::Semibold)?,
        ];
        let mut logo = vec![];
        {
            // Keep the standalone executable and its tray icon complete.
            let xml = asset("logo-wide.svg")
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_else(|| include_str!("../../assets/logo-wide.svg").into());
            for part in xml.split(" d=\"").skip(1) {
                if let Some(d) = part.split('"').next() {
                    let mut pb = PathBuilder::new();
                    let (mut x, mut y) = (0., 0.);
                    for seg in svgtypes::PathParser::from(d).flatten() {
                        use svgtypes::PathSegment::*;
                        match seg {
                            MoveTo { abs, x: a, y: b } => {
                                x = if abs { a } else { x + a };
                                y = if abs { b } else { y + b };
                                pb.move_to(x as f32, y as f32)
                            }
                            LineTo { abs, x: a, y: b } => {
                                x = if abs { a } else { x + a };
                                y = if abs { b } else { y + b };
                                pb.line_to(x as f32, y as f32)
                            }
                            HorizontalLineTo { abs, x: a } => {
                                x = if abs { a } else { x + a };
                                pb.line_to(x as f32, y as f32)
                            }
                            VerticalLineTo { abs, y: b } => {
                                y = if abs { b } else { y + b };
                                pb.line_to(x as f32, y as f32)
                            }
                            CurveTo {
                                abs,
                                x1,
                                y1,
                                x2,
                                y2,
                                x: a,
                                y: b,
                            } => {
                                let (ox, oy) = if abs { (0., 0.) } else { (x, y) };
                                pb.cubic_to(
                                    (x1 + ox) as f32,
                                    (y1 + oy) as f32,
                                    (x2 + ox) as f32,
                                    (y2 + oy) as f32,
                                    (a + ox) as f32,
                                    (b + oy) as f32,
                                );
                                x = a + ox;
                                y = b + oy;
                            }
                            ClosePath { .. } => pb.close(),
                            _ => (),
                        }
                    }
                    if let Some(p) = pb.finish() {
                        logo.push(p);
                    }
                }
            }
        }
        Ok(Self {
            fonts,
            glyphs: HashMap::new(),
            logo,
        })
    }
}
impl<'a> Canvas<'a> {
    pub fn new(painter: &'a mut Painter, w: u32, h: u32, scale: f32) -> Self {
        Self {
            pix: Pixmap::new(w, h).unwrap(),
            scale,
            painter,
        }
    }
    pub fn path(&mut self, path: &Path, color: u32, alpha: f32, stroke: Option<f32>) {
        let mut p = Paint::default();
        p.set_color_rgba8(
            (color >> 16) as u8,
            (color >> 8) as u8,
            color as u8,
            (alpha.clamp(0., 1.) * 255.).round() as u8,
        );
        let t = Transform::from_scale(self.scale, self.scale);
        if let Some(w) = stroke {
            self.pix.stroke_path(
                path,
                &p,
                &Stroke {
                    width: w,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                },
                t,
                None,
            )
        } else {
            self.pix.fill_path(path, &p, FillRule::Winding, t, None)
        }
    }
    pub fn rect(&mut self, r: Box2, color: u32, alpha: f32, radius: f32) {
        if r.w <= 0. || r.h <= 0. {
            return;
        }
        let n = radius.min(r.w / 2.).min(r.h / 2.);
        let (x, y, w, h) = (r.x, r.y, r.w, r.h);
        // Circular quarter arcs, matching the native rounded window region.
        let k = n * 0.552_284_8;
        let mut p = PathBuilder::new();
        p.move_to(x + n, y);
        p.line_to(x + w - n, y);
        p.cubic_to(x + w - n + k, y, x + w, y + n - k, x + w, y + n);
        p.line_to(x + w, y + h - n);
        p.cubic_to(x + w, y + h - n + k, x + w - n + k, y + h, x + w - n, y + h);
        p.line_to(x + n, y + h);
        p.cubic_to(x + n - k, y + h, x, y + h - n + k, x, y + h - n);
        p.line_to(x, y + n);
        p.cubic_to(x, y + n - k, x + n - k, y, x + n, y);
        p.close();
        self.path(&p.finish().unwrap(), color, alpha, None)
    }
    /// Reveal a fixed rounded button through a straight moving fill boundary.
    /// The small scratch surface is bounded by the button, not the window.
    pub fn reveal_rect(&mut self, r: Box2, fill: Box2, color: u32, radius: f32) {
        if fill.w >= r.w {
            self.rect(r, color, 1., radius);
            return;
        }
        if fill.w <= 0. {
            return;
        }
        let scale = self.scale;
        let mut local = Canvas::new(
            self.painter,
            (r.w * scale).ceil() as u32,
            (r.h * scale).ceil() as u32,
            scale,
        );
        local.rect(Box2::new(0., 0., r.w, r.h), color, 1., radius);
        let width = local.pix.width() as usize;
        let left = (fill.x - r.x) * scale;
        let right = left + fill.w * scale;
        for (i, pixel) in local.pix.pixels_mut().iter_mut().enumerate() {
            let x = (i % width) as f32 + 0.5;
            if x < left || x > right {
                *pixel = tiny_skia::PremultipliedColorU8::TRANSPARENT;
            }
        }
        self.pix.draw_pixmap(
            (r.x * scale).round() as i32,
            (r.y * scale).round() as i32,
            local.pix.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    /// Project one cached glyph around a vertical digit drum. Only this small
    /// glyph is resampled; no scratch surface or extra animation timer is needed.
    pub fn drum_digit(&mut self, digit: u8, bounds: Box2, size: f32, offset: f32) {
        if offset.abs() >= 1. || bounds.w <= 0. {
            return;
        }
        let angle = offset * std::f32::consts::FRAC_PI_2;
        let squash = angle.cos();
        if squash < 0.01 {
            return;
        }
        let px = (size * self.scale).round().max(1.) as u16;
        let zero = self.painter.fonts[1].metrics('0', px as f32);
        let half_ink = zero.height as f32 * 0.5;
        let half_window = (bounds.h * self.scale * 0.5 - 1.).max(half_ink);
        // This radius fits the full rotating glyph, even between edge-on and
        // face-on. Its edge compresses into the drum instead of hitting a mask.
        let radius = (half_window * half_window - half_ink * half_ink)
            .sqrt()
            .min(half_ink * 1.4);
        let center_y = (bounds.y + bounds.h * 0.5) * self.scale + angle.sin() * radius;
        let ch = char::from(b'0' + digit);
        let (metrics, bitmap) = self
            .painter
            .glyphs
            .entry((ch, px, true))
            .or_insert_with(|| self.painter.fonts[1].rasterize(ch, px as f32));
        // The leading mark unfolds into the space created by a carry; it must
        // never overlap the adjacent drum while that space is still opening.
        let squeeze = ((bounds.w * self.scale) / (metrics.advance_width + 1.)).min(1.);
        if squeeze < 0.01 {
            return;
        }
        let left = (bounds.x + bounds.w * 0.5) * self.scale
            + (metrics.xmin as f32 - metrics.advance_width * 0.5) * squeeze;
        let top = center_y
            + (half_ink + zero.ymin as f32 - metrics.height as f32 - metrics.ymin as f32) * squash;
        let width = self.pix.width() as i32;
        let height = self.pix.height() as i32;
        // A short softness at the horizon avoids a one-pixel line popping off.
        let visibility = (squash * 3.).min(1.);
        let pixels = self.pix.data_mut();
        for y in (top.floor() as i32).max(0)
            ..((top + metrics.height as f32 * squash).ceil() as i32).min(height)
        {
            let source_y = (y as f32 + 0.5 - top) / squash - 0.5;
            for x in (left.floor() as i32).max(0)
                ..((left + metrics.width as f32 * squeeze).ceil() as i32).min(width)
            {
                if !bounds.contains(x as f32 / self.scale, y as f32 / self.scale) {
                    continue;
                }
                let a = (glyph_coverage(
                    bitmap,
                    metrics.width,
                    metrics.height,
                    (x as f32 + 0.5 - left) / squeeze - 0.5,
                    source_y,
                ) * visibility)
                    .round() as u32;
                if a == 0 {
                    continue;
                }
                let at = ((y * width + x) * 4) as usize;
                for channel in &mut pixels[at..at + 3] {
                    *channel = ((243 * a + *channel as u32 * (255 - a) + 127) / 255) as u8;
                }
                pixels[at + 3] =
                    (a + (pixels[at + 3] as u32 * (255 - a) + 127) / 255).min(255) as u8;
            }
        }
    }

    pub fn line(&mut self, pts: &[(f32, f32)], color: u32, alpha: f32, width: f32) {
        if pts.len() < 2 {
            return;
        }
        let mut p = PathBuilder::new();
        p.move_to(pts[0].0, pts[0].1);
        for &(x, y) in &pts[1..] {
            p.line_to(x, y)
        }
        self.path(&p.finish().unwrap(), color, alpha, Some(width));
    }
    pub fn circle(&mut self, x: f32, y: f32, r: f32, color: u32, alpha: f32) {
        if let Some(p) = PathBuilder::from_circle(x, y, r) {
            self.path(&p, color, alpha, None)
        }
    }
    // Immediate-mode drawing keeps geometry and paint parameters explicit.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(
        &mut self,
        x: f32,
        y: f32,
        r: f32,
        from: f32,
        to: f32,
        color: u32,
        alpha: f32,
        width: f32,
    ) {
        let steps = ((to - from).abs() / 3.).ceil().max(1.) as usize;
        let pts: Vec<_> = (0..=steps)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / steps as f32).to_radians();
                (x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.line(&pts, color, alpha, width)
    }
    pub fn wrapped_text(&mut self, text: &str, r: Box2, size: f32, color: u32) {
        let mut line = String::new();
        let mut y = r.y;
        for word in text.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            let width: f32 = candidate
                .chars()
                .map(|ch| {
                    self.painter.fonts[0]
                        .metrics(ch, size * self.scale)
                        .advance_width
                        / self.scale
                })
                .sum();
            if width > r.w && !line.is_empty() {
                self.text(
                    &line,
                    Box2::new(r.x, y, r.w, size + 4.),
                    size,
                    color,
                    false,
                    false,
                );
                y += size + 4.;
                line = word.into();
                if y + size > r.y + r.h {
                    return;
                }
            } else {
                line = candidate;
            }
        }
        self.text(
            &line,
            Box2::new(r.x, y, r.w, size + 4.),
            size,
            color,
            false,
            false,
        );
    }
    pub fn text(&mut self, text: &str, r: Box2, size: f32, color: u32, bold: bool, center: bool) {
        self.text_alpha(text, r, size, color, bold, center, 1.);
    }
    pub fn fitted_text(
        &mut self,
        text: &str,
        r: Box2,
        size: f32,
        color: u32,
        bold: bool,
        center: bool,
    ) {
        let mut size = size.min((r.h - 2.).max(1.));
        while size > 11. {
            let px = (size * self.scale).round().max(1.);
            let width: f32 = text
                .chars()
                .map(|ch| {
                    self.painter.fonts[usize::from(bold)]
                        .metrics(ch, px)
                        .advance_width
                        / self.scale
                })
                .sum();
            if width <= r.w - 1. {
                break;
            }
            size -= 0.5;
        }
        self.text_clipped(text, r, size, color, bold, center, r);
    }
    // Immediate-mode drawing keeps geometry and paint parameters explicit.
    #[allow(clippy::too_many_arguments)]
    pub fn text_alpha(
        &mut self,
        text: &str,
        r: Box2,
        size: f32,
        color: u32,
        bold: bool,
        center: bool,
        opacity: f32,
    ) {
        self.text_region(text, r, size, color, bold, center, opacity, None);
    }
    // Immediate-mode drawing keeps geometry and paint parameters explicit.
    #[allow(clippy::too_many_arguments)]
    pub fn text_clipped(
        &mut self,
        text: &str,
        r: Box2,
        size: f32,
        color: u32,
        bold: bool,
        center: bool,
        clip: Box2,
    ) {
        self.text_region(text, r, size, color, bold, center, 1., Some(clip));
    }
    // Immediate-mode drawing keeps geometry and paint parameters explicit.
    #[allow(clippy::too_many_arguments)]
    fn text_region(
        &mut self,
        text: &str,
        r: Box2,
        size: f32,
        color: u32,
        bold: bool,
        center: bool,
        opacity: f32,
        clip: Option<Box2>,
    ) {
        if r.w <= 0. || text.is_empty() {
            return;
        }
        let px = (size * self.scale).round().max(1.) as u16;
        let f = &self.painter.fonts[usize::from(bold)];
        let max = r.w * self.scale;
        let mut chars = String::new();
        let mut total = 0.;
        for c in text.chars() {
            let w = f.metrics(c, px as f32).advance_width;
            if total + w > max {
                while !chars.is_empty() && total + f.metrics('…', px as f32).advance_width > max {
                    let c = chars.pop().unwrap();
                    total -= f.metrics(c, px as f32).advance_width;
                }
                chars.push('…');
                total += f.metrics('…', px as f32).advance_width;
                break;
            }
            chars.push(c);
            total += w;
        }
        let mut pen = r.x * self.scale + if center { (max - total) / 2. } else { 0. };
        let baseline = r.y * self.scale
            + f.horizontal_line_metrics(px as f32)
                .map(|m| m.ascent)
                .unwrap_or(px as f32);
        for c in chars.chars() {
            let (m, b) =
                self.painter.glyphs.entry((c, px, bold)).or_insert_with(|| {
                    self.painter.fonts[usize::from(bold)].rasterize(c, px as f32)
                });
            let left = pen.round() as i32 + m.xmin;
            let top = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            let width = self.pix.width() as i32;
            let height = self.pix.height() as i32;
            for yy in 0..m.height {
                let y = top + yy as i32;
                if y < 0 || y >= height {
                    continue;
                }
                for xx in 0..m.width {
                    let x = left + xx as i32;
                    if x < 0 || x >= width {
                        continue;
                    }
                    if clip.is_some_and(|bounds| {
                        !bounds.contains(x as f32 / self.scale, y as f32 / self.scale)
                    }) {
                        continue;
                    }
                    let a = (b[yy * m.width + xx] as f32 * opacity.clamp(0., 1.)).round() as u32;
                    if a == 0 {
                        continue;
                    }
                    let at = ((y * width + x) * 4) as usize;
                    let dst = &mut self.pix.data_mut()[at..at + 4];
                    for (i, chan) in [(color >> 16) & 255, (color >> 8) & 255, color & 255]
                        .into_iter()
                        .enumerate()
                    {
                        dst[i] = ((chan * a + dst[i] as u32 * (255 - a) + 127) / 255) as u8;
                    }
                    dst[3] = (a + (dst[3] as u32 * (255 - a) + 127) / 255).min(255) as u8;
                }
            }
            pen += m.advance_width;
        }
    }
    pub fn logo_mark(&mut self, x: f32, y: f32) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(245, 245, 245, 255);
        let transform = Transform::from_scale(self.scale, self.scale)
            .post_translate(x * self.scale, y * self.scale);
        for path in self
            .painter
            .logo
            .iter()
            .filter(|p| p.bounds().right() < 30.)
        {
            self.pix
                .fill_path(path, &paint, FillRule::Winding, transform, None);
        }
    }
    pub fn logo(&mut self, x: f32, y: f32, width: f32) {
        if self.painter.logo.is_empty() {
            self.text(
                "AUDEZE",
                Box2::new(x, y, width, 30.),
                22.,
                0xffffff,
                true,
                false,
            );
            return;
        }
        let mut p = Paint::default();
        p.set_color_rgba8(245, 245, 245, 255);
        let scale = width / 150.;
        let t = Transform::from_scale(scale * self.scale, scale * self.scale)
            .post_translate(x * self.scale, y * self.scale);
        for path in &self.painter.logo {
            self.pix.fill_path(path, &p, FillRule::Winding, t, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_glyph_sampling_moves_coverage_without_whole_pixel_jumps() {
        let glyph = [0, 255, 0];
        let a = glyph_coverage(&glyph, 1, 3, 0., 1.25);
        let b = glyph_coverage(&glyph, 1, 3, 0., 1.5);
        let c = glyph_coverage(&glyph, 1, 3, 0., 1.75);
        assert!(a > b && b > c && c > 0.);
        assert_eq!(a + c, 255.);
        assert_eq!(glyph_coverage(&glyph, 1, 3, -2., 1.), 0.);
    }

    #[test]
    fn rotating_digits_fit_inside_the_drum_without_touching_a_mask_edge() {
        let mut painter = Painter::new();
        for digit in 0..=9 {
            for offset in [-0.95, -0.5, -0.1, 0., 0.1, 0.5, 0.95] {
                let mut c = Canvas::new(&mut painter, 64, 64, 1.);
                c.drum_digit(digit, Box2::new(10., 10., 44., 40.), 30., offset);
                for y in [10, 49] {
                    for x in 10..54 {
                        assert_eq!(
                            c.pix.pixel(x, y).unwrap().alpha(),
                            0,
                            "digit {digit}, offset {offset} hits the rectangular edge"
                        );
                    }
                }
            }
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn only_existing_absolute_truetype_or_opentype_files_are_used() {
        let dir = std::env::temp_dir().join(format!("audeze-font-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ttf = dir.join("Sans.TTF");
        let bitmap = dir.join("Sans.pcf");
        std::fs::write(&ttf, b"").unwrap();
        std::fs::write(&bitmap, b"").unwrap();
        assert_eq!(usable_font_file(ttf.to_str().unwrap()), Some(ttf.clone()));
        assert_eq!(
            usable_font_file(bitmap.to_str().unwrap()),
            None,
            "not fontdue-readable"
        );
        assert_eq!(
            usable_font_file(dir.join("missing.ttf").to_str().unwrap()),
            None
        );
        assert_eq!(
            usable_font_file("Sans.ttf"),
            None,
            "relative paths are never trusted"
        );
        assert_eq!(usable_font_file(""), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
