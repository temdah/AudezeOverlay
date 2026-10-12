//! Blur behind the overlay through `ext-background-effect`. The compositor
//! decides how strong the blur is, so only on/off carries over; the region is
//! the window's rounded rectangle so the blur does not show past its corners.
use super::State;
use smithay_client_toolkit::{
    compositor::{CompositorState, Region},
    reexports::{
        client::{
            globals::GlobalList, protocol::wl_surface::WlSurface, Dispatch, Proxy, QueueHandle,
        },
        protocols::ext::background_effect::v1::client::{
            ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1,
            ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
        },
    },
};

pub(super) struct BackgroundEffect {
    manager: ExtBackgroundEffectManagerV1,
}

/// The blur object of one surface and the region last sent to it.
pub(super) struct SurfaceBlur {
    effect: ExtBackgroundEffectSurfaceV1,
    applied: Option<Shape>,
}

/// What the compositor is asked to blur, in logical pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Shape {
    pub width: i32,
    pub height: i32,
    pub corner_radius: i32,
}

impl BackgroundEffect {
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<State>) -> Option<Self> {
        Some(Self {
            manager: globals.bind(qh, 1..=1, ()).ok()?,
        })
    }

    pub(super) fn attach(&self, surface: &WlSurface, qh: &QueueHandle<State>) -> SurfaceBlur {
        SurfaceBlur {
            effect: self.manager.get_background_effect(surface, qh, ()),
            applied: None,
        }
    }
}

impl SurfaceBlur {
    /// Blurs `shape`, or nothing; takes effect with the next commit. Repeating
    /// the last request sends nothing.
    pub(super) fn apply(&mut self, shape: Option<Shape>, compositor: &CompositorState) {
        if self.applied == shape {
            return;
        }
        match shape.and_then(|shape| Some((shape, Region::new(compositor).ok()?))) {
            Some((shape, region)) => {
                for (x, y, w, h) in rounded_rectangle(shape) {
                    region.add(x, y, w, h);
                }
                self.effect.set_blur_region(Some(region.wl_region()));
            }
            None => self.effect.set_blur_region(None),
        }
        self.applied = shape;
    }
}

impl Drop for SurfaceBlur {
    fn drop(&mut self) {
        self.effect.destroy();
    }
}

/// A rounded rectangle as rows of rectangles `(x, y, width, height)`: one block
/// for the straight middle and a one-pixel row for each pixel of the corners.
fn rounded_rectangle(shape: Shape) -> Vec<(i32, i32, i32, i32)> {
    let Shape {
        width,
        height,
        corner_radius,
    } = shape;
    let radius = corner_radius.clamp(0, width.min(height) / 2);
    let mut rects = vec![(0, radius, width, height - 2 * radius)];
    for row in 0..radius {
        // Distance from the corner circle's centre to the middle of this row.
        let dy = (radius - row) as f32 - 0.5;
        let reach = ((radius * radius) as f32 - dy * dy).max(0.).sqrt();
        let inset = (radius as f32 - reach).ceil() as i32;
        rects.push((inset, row, width - 2 * inset, 1));
        rects.push((inset, height - 1 - row, width - 2 * inset, 1));
    }
    rects.retain(|(_, _, w, h)| *w > 0 && *h > 0);
    rects
}

macro_rules! no_events {
    ($($interface:ty),*) => {$(
        impl Dispatch<$interface, ()> for State {
            fn event(
                _: &mut Self,
                _: &$interface,
                _: <$interface as Proxy>::Event,
                _: &(),
                _: &smithay_client_toolkit::reexports::client::Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}
// The compositor's capability flags are not needed: asking for blur it does not
// support is not an error, the region is just not applied.
no_events!(ExtBackgroundEffectManagerV1, ExtBackgroundEffectSurfaceV1);

#[cfg(test)]
mod tests {
    use super::*;

    fn covered(rects: &[(i32, i32, i32, i32)], x: i32, y: i32) -> bool {
        rects
            .iter()
            .any(|(rx, ry, w, h)| x >= *rx && x < rx + w && y >= *ry && y < ry + h)
    }

    #[test]
    fn corners_are_cut_and_the_rest_is_covered() {
        let rects = rounded_rectangle(Shape {
            width: 100,
            height: 60,
            corner_radius: 8,
        });
        assert!(!covered(&rects, 0, 0));
        assert!(!covered(&rects, 99, 59));
        assert!(covered(&rects, 50, 0));
        assert!(covered(&rects, 0, 30));
        assert!(covered(&rects, 8, 8));
    }

    #[test]
    fn a_zero_radius_is_the_whole_rectangle() {
        let rects = rounded_rectangle(Shape {
            width: 10,
            height: 10,
            corner_radius: 0,
        });
        assert_eq!(rects, vec![(0, 0, 10, 10)]);
    }
}
