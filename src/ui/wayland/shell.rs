//! The Wayland implementation of `Shell`: one layer-shell surface on the overlay
//! layer, a shared-memory frame, calloop timers and a ping for redraw requests.
//!
//! Everything the application logic asks for is a request on `Core`, which the
//! event handlers in the parent module share. Nothing here waits for the
//! compositor: requests are queued, and the next configure or frame follows.
use super::{
    blur::{BackgroundEffect, Shape, SurfaceBlur},
    modal::{self, Content, Outcome, Placement},
    scaling::{Scaling, SurfaceScaling},
    State,
};
use crate::ui::{app::TrayCommand, draw::Painter, shell::*};
use smithay_client_toolkit::{
    compositor::{CompositorState, Region},
    reexports::{
        calloop::{
            channel::Sender,
            ping::Ping,
            timer::{TimeoutAction, Timer as CalloopTimer},
            LoopHandle, RegistrationToken,
        },
        client::{protocol::wl_output::WlOutput, protocol::wl_shm, Connection, QueueHandle},
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
        WaylandSurface,
    },
    shm::slot::SlotPool,
};
#[cfg(test)]
use std::any::Any;
use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, time::Duration};

/// A screen as the compositor describes it, in logical (unscaled) coordinates.
pub(super) struct Output {
    pub handle: WlOutput,
    pub name: String,
    pub origin: (i32, i32),
    pub size: (i32, i32),
    pub scale: i32,
}

/// The current frame as premultiplied BGRA, the order Wayland's ARGB8888 has in
/// memory on little-endian machines.
struct Frame {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

pub(super) struct Core {
    pub connection: Connection,
    pub qh: QueueHandle<State>,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub pool: SlotPool,
    pub layer: Option<LayerSurface>,
    pub configured: bool,
    pub quit: bool,
    visible: bool,
    interactive: bool,
    /// Requested size and top-left margin, in logical pixels.
    logical: (u32, u32),
    margin: (i32, i32),
    pub outputs: Vec<Output>,
    /// Index into `outputs` of the screen the window is placed on.
    output: Option<usize>,
    pub scale: f32,
    /// Fractional-scale support, and the objects of the current surface.
    pub scaling: Option<Scaling>,
    /// Blur support, the current surface's blur object, and what was asked for.
    pub background_effect: Option<BackgroundEffect>,
    surface_blur: Option<SurfaceBlur>,
    blur_wanted: bool,
    /// Corner diameter in device pixels, from `clip_rounded`.
    corner_diameter: i32,
    surface_scaling: Option<SurfaceScaling>,
    /// The scale the compositor prefers for the current surface, if it said.
    preferred_scale: Option<f32>,
    frame: Option<Frame>,
    alpha: u8,
    pub cursor: Cursor,
    pub cursor_changed: bool,
    redraw: Ping,
    /// Where the pointer last was over the window (logical pixels) and the
    /// serial of the latest input event, which popups need.
    pub pointer: (f64, f64),
    pub serial: u32,
    /// Fonts for menus and dialogs, loaded the first time one is shown.
    painter: Option<Box<Painter>>,
}

impl Core {
    pub(super) fn new(
        connection: Connection,
        qh: QueueHandle<State>,
        compositor: CompositorState,
        layer_shell: LayerShell,
        pool: SlotPool,
        redraw: Ping,
    ) -> Self {
        Self {
            connection,
            qh,
            compositor,
            layer_shell,
            pool,
            layer: None,
            configured: false,
            quit: false,
            visible: false,
            interactive: false,
            logical: (520, 500),
            margin: (0, 0),
            outputs: Vec::new(),
            output: None,
            scale: 1.,
            scaling: None,
            background_effect: None,
            surface_blur: None,
            blur_wanted: false,
            corner_diameter: 0,
            surface_scaling: None,
            preferred_scale: None,
            frame: None,
            alpha: 255,
            cursor: Cursor::Arrow,
            cursor_changed: false,
            redraw,
            pointer: (0., 0.),
            serial: 0,
            painter: None,
        }
    }

    fn output_rect(&self, index: usize) -> Rect {
        let o = &self.outputs[index];
        let s = self.scale;
        Rect {
            left: (o.origin.0 as f32 * s).round() as i32,
            top: (o.origin.1 as f32 * s).round() as i32,
            right: ((o.origin.0 + o.size.0) as f32 * s).round() as i32,
            bottom: ((o.origin.1 + o.size.1) as f32 * s).round() as i32,
        }
    }

    /// The screen holding a point in device pixels, else the first one.
    fn output_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.outputs.len())
            .find(|i| {
                let r = self.output_rect(*i);
                x >= r.left && x < r.right && y >= r.top && y < r.bottom
            })
            .or(if self.outputs.is_empty() {
                None
            } else {
                Some(0)
            })
    }

    /// The scale of the screen the window is on, rounded up so frames stay sharp.
    fn adopt_output_scale(&mut self) {
        if let Some(preferred) = self.preferred_scale {
            self.scale = preferred;
        } else if let Some(output) = self.output.and_then(|i| self.outputs.get(i)) {
            self.scale = output.scale.max(1) as f32;
        } else if let Some(max) = self.outputs.iter().map(|o| o.scale).max() {
            self.scale = max.max(1) as f32;
        }
    }

    /// Adopts the fractional scale the compositor prefers; true if it differs.
    pub(super) fn set_preferred_scale(&mut self, scale: f32) -> bool {
        let changed = self.preferred_scale != Some(scale);
        self.preferred_scale = Some(scale);
        self.adopt_output_scale();
        changed
    }

    pub(super) fn outputs_changed(&mut self, outputs: Vec<Output>) {
        let previous = self
            .output
            .and_then(|i| self.outputs.get(i))
            .map(|o| o.handle.clone());
        self.output = retained_output(previous.as_ref(), outputs.iter().map(|o| &o.handle));
        let changed = previous.as_ref() != self.output.map(|i| &outputs[i].handle);
        self.outputs = outputs;
        if changed {
            self.destroy_surface();
        }
        self.adopt_output_scale();
        self.ensure_surface();
    }

    pub(super) fn surface_closed(&mut self, layer: &LayerSurface) {
        // Ignore a late close for an old surface after moving monitors. A closed
        // current surface can be recreated on the next output event or show.
        if self
            .layer
            .as_ref()
            .is_some_and(|current| current.wl_surface() == layer.wl_surface())
        {
            self.destroy_surface();
        }
    }

    /// Creates the layer surface if the window is meant to be showing.
    fn ensure_surface(&mut self) {
        if self.layer.is_some() || !self.visible || self.outputs.is_empty() {
            return;
        }
        let surface = self.compositor.create_surface(&self.qh);
        let output = self
            .output
            .and_then(|i| self.outputs.get(i))
            .map(|o| &o.handle);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("audeze-overlay"),
            output,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        // The overlay never pushes other windows aside.
        layer.set_exclusive_zone(-1);
        layer.set_margin(self.margin.1, 0, 0, self.margin.0);
        layer.set_size(self.logical.0, self.logical.1);
        self.surface_scaling = self
            .scaling
            .as_ref()
            .map(|s| s.attach(layer.wl_surface(), &self.qh));
        self.surface_blur = self
            .background_effect
            .as_ref()
            .map(|e| e.attach(layer.wl_surface(), &self.qh));
        self.layer = Some(layer);
        self.configured = false;
        self.apply_input_mode();
    }

    fn destroy_surface(&mut self) {
        self.surface_blur = None;
        self.surface_scaling = None;
        self.preferred_scale = None;
        self.layer = None;
        self.configured = false;
    }

    /// Keyboard focus and click-through follow the interactive flag.
    fn apply_input_mode(&mut self) {
        let Some(layer) = &self.layer else { return };
        if self.interactive {
            layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
            layer.wl_surface().set_input_region(None);
        } else {
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            // An empty input region lets every click through to the game.
            if let Ok(empty) = Region::new(&self.compositor) {
                layer.wl_surface().set_input_region(Some(empty.wl_region()));
            }
        }
        layer.commit();
    }

    fn apply_geometry(&mut self) {
        if let Some(layer) = &self.layer {
            layer.set_margin(self.margin.1, 0, 0, self.margin.0);
            layer.set_size(self.logical.0, self.logical.1);
            layer.commit();
        }
    }

    /// Moves the window by a pointer drag, in logical pixels.
    pub(super) fn nudge(&mut self, dx: i32, dy: i32) {
        self.margin = (self.margin.0 + dx, self.margin.1 + dy);
        self.apply_geometry();
    }

    /// Resizes the window by a pointer drag; edges on the left or top also move it.
    pub(super) fn resize_by(
        &mut self,
        edge: Edge,
        dx: i32,
        dy: i32,
        limits: ((i32, i32), (i32, i32)),
    ) {
        let logical_limit = |(w, h): (i32, i32)| {
            (
                (w as f32 / self.scale).ceil() as i32,
                (h as f32 / self.scale).ceil() as i32,
            )
        };
        let rect = Rect {
            left: self.margin.0,
            top: self.margin.1,
            right: self.margin.0 + self.logical.0 as i32,
            bottom: self.margin.1 + self.logical.1 as i32,
        };
        let resized = resized_rect(
            rect,
            edge,
            (dx, dy),
            (logical_limit(limits.0), logical_limit(limits.1)),
        );
        self.logical = (resized.width() as u32, resized.height() as u32);
        self.margin = (resized.left, resized.top);
        self.apply_geometry();
    }

    /// The compositor decided on a size.
    pub(super) fn configured_size(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.logical = (width, height);
        }
        self.configured = true;
    }

    /// Copies the stored frame into a shared-memory buffer at the current
    /// opacity and shows it.
    pub(super) fn present(&mut self) {
        if !self.configured {
            return;
        }
        let (Some(layer), Some(frame)) = (&self.layer, &self.frame) else {
            return;
        };
        let (width, height) = (frame.width as i32, frame.height as i32);
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(width, height, width * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        // The slot can be rounded up past the frame, so only the frame's bytes are written.
        let canvas = &mut canvas[..frame.bgra.len()];
        if self.alpha == 255 {
            canvas.copy_from_slice(&frame.bgra);
        } else {
            let alpha = u32::from(self.alpha);
            for (dst, src) in canvas.iter_mut().zip(&frame.bgra) {
                *dst = ((u32::from(*src) * alpha + 127) / 255) as u8;
            }
        }
        let surface = layer.wl_surface();
        match &self.surface_scaling {
            // The viewport maps the exact-size buffer onto the logical size.
            Some(scaling) if self.preferred_scale.is_some() => {
                surface.set_buffer_scale(1);
                scaling.set_logical_size(self.logical.0, self.logical.1);
            }
            _ => surface.set_buffer_scale(self.scale as i32),
        }
        if let Some(blur) = &mut self.surface_blur {
            blur.apply(
                self.blur_wanted.then(|| Shape {
                    width: self.logical.0 as i32,
                    height: self.logical.1 as i32,
                    corner_radius: (self.corner_diameter as f32 / 2. / self.scale).round() as i32,
                }),
                &self.compositor,
            );
        }
        surface.damage_buffer(0, 0, width, height);
        if buffer.attach_to(surface).is_ok() {
            layer.commit();
        }
    }

    /// Runs a menu or dialog as a popup of the overlay and waits for the answer.
    /// The borrow on the core is released while it waits.
    fn run_modal(core: &Rc<RefCell<Core>>, content: Content) -> Option<Outcome> {
        let (connection, layer, painter, placement) = {
            let mut core = core.borrow_mut();
            let layer = core.layer.clone()?;
            let painter = match core.painter.take() {
                Some(painter) => painter,
                None => Box::new(Painter::try_new().ok()?),
            };
            let placement = Placement {
                anchor: (core.pointer.0 as i32, core.pointer.1 as i32),
                serial: core.serial,
                scale: core.scale.ceil() as i32,
                parent_size: (core.logical.0 as i32, core.logical.1 as i32),
            };
            (core.connection.clone(), layer, painter, placement)
        };
        let (outcome, painter) = modal::run(&connection, &layer, painter, content, placement);
        core.borrow_mut().painter = Some(painter);
        outcome
    }

    pub(super) fn wake(&self) {
        self.redraw.ping();
    }
}

/// Handed to the application as its `Shell`.
pub(super) struct WlShell {
    core: Rc<RefCell<Core>>,
    handle: LoopHandle<'static, State>,
    timers: RefCell<HashMap<Timer, RegistrationToken>>,
    tray_icons: Vec<ksni::Icon>,
    tray_commands: Sender<TrayCommand>,
    shortcut_pressed: Sender<()>,
    /// Whether the shortcut session was started, and whether the desktop took it.
    shortcut: Option<bool>,
    tray: Option<ksni::blocking::Handle<super::tray::OverlayTray>>,
}

impl WlShell {
    pub(super) fn new(
        core: Rc<RefCell<Core>>,
        handle: LoopHandle<'static, State>,
        tray_icons: Vec<ksni::Icon>,
        tray_commands: Sender<TrayCommand>,
        shortcut_pressed: Sender<()>,
    ) -> Self {
        Self {
            core,
            handle,
            timers: RefCell::new(HashMap::new()),
            tray_icons,
            tray_commands,
            shortcut_pressed,
            shortcut: None,
            tray: None,
        }
    }
}

impl Shell for WlShell {
    #[cfg(test)]
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn start_timer(&mut self, timer: Timer, ms: u32) -> bool {
        self.stop_timer(timer);
        let period = Duration::from_millis(u64::from(ms.max(1)));
        let source = CalloopTimer::from_duration(period);
        match self
            .handle
            .insert_source(source, move |_, _, state: &mut State| {
                state.timer_fired(timer);
                TimeoutAction::ToDuration(period)
            }) {
            Ok(token) => {
                self.timers.borrow_mut().insert(timer, token);
                true
            }
            Err(_) => false,
        }
    }
    fn stop_timer(&mut self, timer: Timer) {
        if let Some(token) = self.timers.borrow_mut().remove(&timer) {
            self.handle.remove(token);
        }
    }
    fn request_redraw(&self) {
        self.core.borrow().wake();
    }

    // The compositor keeps delivering pointer events to the surface that took a
    // button press until the button is released, so there is nothing to capture.
    fn capture_pointer(&mut self) {}
    fn release_pointer(&mut self) {}

    fn is_visible(&self) -> bool {
        self.core.borrow().visible
    }
    fn scale(&self) -> f32 {
        self.core.borrow().scale
    }
    fn client_size(&self) -> Option<(i32, i32)> {
        let core = self.core.borrow();
        Some((
            (core.logical.0 as f32 * core.scale).round() as i32,
            (core.logical.1 as f32 * core.scale).round() as i32,
        ))
    }
    fn window_rect(&self) -> Rect {
        let core = self.core.borrow();
        let origin = core
            .output
            .and_then(|i| core.outputs.get(i))
            .map_or((0, 0), |o| o.origin);
        let left = ((origin.0 + core.margin.0) as f32 * core.scale).round() as i32;
        let top = ((origin.1 + core.margin.1) as f32 * core.scale).round() as i32;
        Rect {
            left,
            top,
            right: left + (core.logical.0 as f32 * core.scale).round() as i32,
            bottom: top + (core.logical.1 as f32 * core.scale).round() as i32,
        }
    }
    fn place(&mut self, target: Rect) {
        let mut core = self.core.borrow_mut();
        let output = core.output_at(target.left, target.top);
        let moved_screens = output != core.output;
        core.output = output;
        core.adopt_output_scale();
        let s = core.scale;
        let origin = output
            .and_then(|i| core.outputs.get(i))
            .map_or((0, 0), |o| o.origin);
        core.margin = (
            (target.left as f32 / s).round() as i32 - origin.0,
            (target.top as f32 / s).round() as i32 - origin.1,
        );
        core.logical = (
            ((target.width() as f32 / s).round() as u32).max(1),
            ((target.height() as f32 / s).round() as u32).max(1),
        );
        if moved_screens && core.layer.is_some() {
            // A surface belongs to one screen; moving to another needs a new one.
            core.destroy_surface();
            core.ensure_surface();
        } else {
            core.apply_geometry();
        }
    }
    fn move_to(&mut self, x: i32, y: i32) {
        let rect = self.window_rect();
        self.place(Rect {
            left: x,
            top: y,
            right: x + rect.width(),
            bottom: y + rect.height(),
        });
    }
    fn window_work_area(&self) -> Rect {
        let core = self.core.borrow();
        core.output
            .filter(|i| *i < core.outputs.len())
            .map_or_else(Rect::default, |i| core.output_rect(i))
    }
    fn set_cursor(&mut self, cursor: Cursor) {
        let mut core = self.core.borrow_mut();
        if core.cursor != cursor {
            core.cursor = cursor;
            core.cursor_changed = true;
        }
    }
    fn set_input_mode(&mut self, interactive: bool) {
        let mut core = self.core.borrow_mut();
        if core.interactive != interactive {
            core.interactive = interactive;
            core.apply_input_mode();
        }
    }
    fn show_window(&mut self, _activate: bool) {
        let mut core = self.core.borrow_mut();
        core.visible = true;
        core.ensure_surface();
        core.apply_input_mode();
        core.present();
    }
    fn hide_window(&mut self) {
        let mut core = self.core.borrow_mut();
        core.visible = false;
        core.destroy_surface();
    }
    // Keyboard focus is requested through the layer's interactivity, so there is
    // nothing to force here.
    fn focus(&mut self) {}
    // Blur and rounded clipping are optional protocol extensions added separately.
    fn set_blur(&mut self, strength: u8) {
        let mut core = self.core.borrow_mut();
        core.blur_wanted = strength > 0;
        core.present();
    }
    fn clip_rounded(&mut self, diameter: i32) {
        self.core.borrow_mut().corner_diameter = diameter;
    }
    fn quit_later(&self) {
        self.core.borrow_mut().quit = true;
        self.core.borrow().wake();
    }
    fn quit(&mut self) {
        self.core.borrow_mut().quit = true;
    }

    fn store_frame(&mut self, pixels: &tiny_skia::Pixmap) -> bool {
        let mut bgra = Vec::with_capacity(pixels.data().len());
        // tiny-skia stores premultiplied RGBA; Wayland expects premultiplied BGRA.
        for px in pixels.data().as_chunks::<4>().0 {
            bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        self.core.borrow_mut().frame = Some(Frame {
            width: pixels.width(),
            height: pixels.height(),
            bgra,
        });
        true
    }
    fn present(&self, alpha: u8) {
        let mut core = self.core.borrow_mut();
        core.alpha = alpha;
        core.present();
    }

    fn monitors(&self) -> Vec<Monitor> {
        let core = self.core.borrow();
        (0..core.outputs.len())
            .map(|i| Monitor {
                key: core.outputs[i].name.clone(),
                label: format!("Screen {}", i + 1),
                work: core.output_rect(i),
            })
            .collect()
    }
    fn work_area_near(&self, point: Option<(i32, i32)>) -> Rect {
        let core = self.core.borrow();
        let index = match point {
            Some((x, y)) => core.output_at(x, y),
            None => {
                core.output
                    .filter(|i| *i < core.outputs.len())
                    .or(if core.outputs.is_empty() {
                        None
                    } else {
                        Some(0)
                    })
            }
        };
        index.map_or_else(Rect::default, |i| core.output_rect(i))
    }

    fn popup_menu(&mut self, entries: &[MenuEntry], _: MenuOrigin) -> Option<u32> {
        // The Linux tray hosts its own menu through StatusNotifierItem.
        match Core::run_modal(&self.core, Content::Menu(entries.to_vec()))? {
            Outcome::Menu(id) => id,
            Outcome::Dialog(_) => None,
        }
    }
    fn confirm(&mut self, title: &str, message: &str, buttons: Buttons) -> Choice {
        let content = Content::Dialog {
            title: title.to_owned(),
            message: message.to_owned(),
            buttons,
        };
        match Core::run_modal(&self.core, content) {
            Some(Outcome::Dialog(choice)) => choice,
            _ => Choice::Cancel,
        }
    }
    fn choose_file(&mut self, save: bool) -> Result<FileSelection, String> {
        use smithay_client_toolkit::reexports::calloop::channel;
        // Exactly one reply, from one outstanding dialog. The application owns
        // the pending action and rejects a second request until this finishes.
        let (sender, receiver) = channel::channel::<Result<Option<PathBuf>, String>>();
        let registration = Rc::new(std::cell::Cell::new(None));
        let cleanup = registration.clone();
        let token = self
            .handle
            .insert_source(receiver, move |event, _, state: &mut State| {
                if let channel::Event::Msg(result) = event {
                    if let Some(token) = cleanup.take() {
                        state.loop_handle.remove(token);
                    }
                    crate::ui::app::profile_file_selected(&mut state.app, result);
                }
            })
            .map_err(|e| format!("Could not start the file dialog: {e}"))?;
        registration.set(Some(token));
        if let Err(error) = std::thread::Builder::new()
            .name("eq-file-dialog".into())
            .spawn(move || {
                let _ = sender.send(super::portal::choose_file(save));
            })
        {
            self.handle.remove(token);
            return Err(format!("Could not start the file dialog: {error}"));
        }
        Ok(FileSelection::Pending)
    }
    fn register_hotkey(&mut self, modifiers: u32, key: u32) -> bool {
        // One portal session lives for the whole run; the desktop owns the binding.
        if self.shortcut.is_none() {
            let trigger = super::shortcut::trigger_for(modifiers, key);
            self.shortcut = Some(super::shortcut::start(
                trigger,
                self.shortcut_pressed.clone(),
            ));
        }
        self.shortcut == Some(true)
    }
    fn unregister_hotkey(&mut self) {}
    // Changing the binding is done in the desktop's own shortcut settings.
    fn rebind_hotkey(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn tray_add(&mut self) -> bool {
        if self.tray.is_none() {
            self.tray = super::tray::spawn(self.tray_icons.clone(), self.tray_commands.clone());
        }
        self.tray.is_some()
    }
    fn tray_remove(&mut self) {
        if let Some(tray) = self.tray.take() {
            tray.shutdown();
        }
    }
    fn set_autostart(&mut self, enabled: bool) -> Result<(), String> {
        super::autostart::set(enabled)
    }
    fn shutdown(&mut self) {
        self.tray_remove();
    }
}

/// Retain monitor identity across enumeration changes, falling back only when
/// that monitor disappeared. Never reuse an index from the previous list.
fn retained_output<'a, T: PartialEq + 'a>(
    previous: Option<&T>,
    outputs: impl Iterator<Item = &'a T>,
) -> Option<usize> {
    let mut first = None;
    for (index, output) in outputs.enumerate() {
        first.get_or_insert(index);
        if previous == Some(output) {
            return Some(index);
        }
    }
    first
}

#[cfg(test)]
mod output_tests {
    use super::retained_output;

    #[test]
    fn monitor_identity_survives_removal_and_reordering() {
        assert_eq!(retained_output(Some(&"B"), ["B", "A"].iter()), Some(0));
        assert_eq!(retained_output(Some(&"B"), ["A"].iter()), Some(0));
        assert_eq!(retained_output(Some(&"B"), [].iter()), None);
        assert_eq!(retained_output(None, ["C"].iter()), Some(0));
    }
}

/// Clamp the dragged edges while retaining the opposite edge as the anchor.
fn resized_rect(
    rect: Rect,
    edge: Edge,
    (dx, dy): (i32, i32),
    (minimum, maximum): ((i32, i32), (i32, i32)),
) -> Rect {
    let left = matches!(edge, Edge::Left | Edge::TopLeft | Edge::BottomLeft);
    let right = matches!(edge, Edge::Right | Edge::TopRight | Edge::BottomRight);
    let top = matches!(edge, Edge::Top | Edge::TopLeft | Edge::TopRight);
    let bottom = matches!(edge, Edge::Bottom | Edge::BottomLeft | Edge::BottomRight);
    let width = if left {
        rect.width().saturating_sub(dx)
    } else if right {
        rect.width().saturating_add(dx)
    } else {
        rect.width()
    }
    .clamp(minimum.0.max(1), maximum.0.max(minimum.0).max(1));
    let height = if top {
        rect.height().saturating_sub(dy)
    } else if bottom {
        rect.height().saturating_add(dy)
    } else {
        rect.height()
    }
    .clamp(minimum.1.max(1), maximum.1.max(minimum.1).max(1));
    let x = if left { rect.right - width } else { rect.left };
    let y = if top { rect.bottom - height } else { rect.top };
    Rect {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;

    #[test]
    fn dragging_stops_at_limits_without_moving_the_opposite_edge() {
        let rect = Rect {
            left: 100,
            top: 50,
            right: 500,
            bottom: 350,
        };
        let limits = ((250, 200), (800, 600));
        let small = resized_rect(rect, Edge::TopLeft, (1000, 1000), limits);
        assert_eq!((small.width(), small.height()), limits.0);
        assert_eq!((small.right, small.bottom), (rect.right, rect.bottom));
        let large = resized_rect(rect, Edge::BottomRight, (10000, 10000), limits);
        assert_eq!((large.width(), large.height()), limits.1);
        assert_eq!((large.left, large.top), (rect.left, rect.top));
    }
}
