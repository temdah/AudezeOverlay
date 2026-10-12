//! Fractional display scales (125 %, 150 %, ...). The compositor tells each
//! surface its preferred scale in 120ths; the frame is rendered at that scale
//! and a viewport maps it onto the surface's logical size, so nothing is
//! resampled. Without these protocols the integer buffer scale is used.
use super::State;
use smithay_client_toolkit::reexports::{
    client::{globals::GlobalList, protocol::wl_surface::WlSurface, Dispatch, Proxy, QueueHandle},
    protocols::wp::{
        fractional_scale::v1::client::{
            wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
            wp_fractional_scale_v1::{self, WpFractionalScaleV1},
        },
        viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
    },
};

/// The compositor's two globals, when it offers both.
pub(super) struct Scaling {
    manager: WpFractionalScaleManagerV1,
    viewporter: WpViewporter,
}

/// The per-surface objects; dropping them releases the protocol objects.
pub(super) struct SurfaceScaling {
    fractional: WpFractionalScaleV1,
    viewport: WpViewport,
}

impl Scaling {
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<State>) -> Option<Self> {
        Some(Self {
            manager: globals.bind(qh, 1..=1, ()).ok()?,
            viewporter: globals.bind(qh, 1..=1, ()).ok()?,
        })
    }

    pub(super) fn attach(&self, surface: &WlSurface, qh: &QueueHandle<State>) -> SurfaceScaling {
        SurfaceScaling {
            fractional: self.manager.get_fractional_scale(surface, qh, ()),
            viewport: self.viewporter.get_viewport(surface, qh, ()),
        }
    }
}

impl SurfaceScaling {
    /// Shows the buffer over the given logical size.
    pub(super) fn set_logical_size(&self, width: u32, height: u32) {
        self.viewport.set_destination(width as i32, height as i32);
    }
}

impl Drop for SurfaceScaling {
    fn drop(&mut self) {
        self.fractional.destroy();
        self.viewport.destroy();
    }
}

/// 120ths, as the protocol sends them, to a scale factor.
pub(super) fn factor(preferred: u32) -> f32 {
    preferred as f32 / 120.
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
no_events!(WpFractionalScaleManagerV1, WpViewporter, WpViewport);

impl Dispatch<WpFractionalScaleV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &smithay_client_toolkit::reexports::client::Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.preferred_scale(factor(scale));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_scale_is_in_120ths() {
        assert_eq!(factor(120), 1.);
        assert_eq!(factor(150), 1.25);
        assert_eq!(factor(180), 1.5);
    }
}
