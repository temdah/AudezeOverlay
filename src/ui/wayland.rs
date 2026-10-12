//! The Linux entry point: a Wayland event loop around the shared application
//! logic. Compositors with the wlr layer-shell protocol (KDE Plasma, Sway,
//! Hyprland, niri, COSMIC and others) can show the overlay above games; this
//! shell translates their pointer, keyboard, output and timer events into the
//! same input functions the Windows shell uses.
mod autostart;
mod blur;
mod instance;
mod modal;
mod portal;
mod scaling;
mod shell;
mod shortcut;
mod signals;
mod tray;
use super::{app::*, draw::Painter, shell::*};
use audeze_overlay::{config::Config, device, model::SoundState};
use shell::{Core, Output, WlShell};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel, generic::Generic, ping, EventLoop, Interest, LoopHandle, Mode, PostAction,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
            Connection, QueueHandle,
        },
        protocols::wp::{
            cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
            relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{
            cursor_shape::CursorShapeManager, PointerEvent, PointerEventKind, PointerHandler,
        },
        relative_pointer::{RelativeMotionEvent, RelativePointerHandler, RelativePointerState},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const BTN_LEFT: u32 = 0x110;
/// Two presses this close in time and place are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// A pointer drag that moves or resizes the window. Layer surfaces have no
/// compositor-driven move, so the shell does it itself from raw pointer motion.
enum WindowDrag {
    Move,
    Resize(Edge),
}

struct State {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    relative_pointer_state: RelativePointerState,
    shm: Shm,
    loop_handle: LoopHandle<'static, State>,
    core: Rc<RefCell<Core>>,
    app: App,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    relative_pointer: Option<ZwpRelativePointerV1>,
    cursor_manager: Option<CursorShapeManager>,
    cursor_device: Option<WpCursorShapeDeviceV1>,
    enter_serial: u32,
    modifiers: Modifiers,
    /// Whether the overlay window itself has the keyboard focus. A menu or
    /// dialog is a different surface of this client, and the compositor sends
    /// its keystrokes to every keyboard object of the client.
    keyboard_on_window: bool,
    drag: Option<WindowDrag>,
    /// Pointer movement left over after whole logical pixels were applied.
    remainder: (f64, f64),
    last_click: Option<(Instant, (f64, f64))>,
    pointer_position: (f64, f64),
}

pub fn main() {
    if let Err(error) = run() {
        eprintln!("Could not start Audeze Overlay: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(unknown) = args
        .iter()
        .skip(1)
        .find(|a| !matches!(a.as_str(), "--demo" | "--toggle"))
    {
        return Err(format!(
            "unknown option {unknown} (options: --demo, --toggle)"
        ));
    }
    let demo = args.iter().any(|a| a == "--demo");
    // Launching again asks the running instance to show itself; --toggle flips
    // it. Demo runs are independent so they never disturb a real one.
    let request = if args.iter().any(|a| a == "--toggle") {
        instance::Request::Toggle
    } else {
        instance::Request::Open
    };
    let socket = instance::socket_path().filter(|_| !demo);
    let instance = match &socket {
        Some(path) => match instance::acquire(path, request) {
            Ok(instance::Role::Forwarded) => return Ok(()),
            Ok(instance::Role::Primary(instance)) => Some(instance),
            Err(error) => return Err(format!("single-instance startup failed: {error}")),
        },
        None => None,
    };
    let first = !Config::path().exists();
    let config = if demo {
        Config::default()
    } else {
        Config::load()
    };

    let connection = Connection::connect_to_env()
        .map_err(|e| format!("no Wayland compositor to connect to ({e})"))?;
    let (globals, queue) = registry_queue_init::<State>(&connection)
        .map_err(|e| format!("could not read the compositor's globals ({e})"))?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<State> =
        EventLoop::try_new().map_err(|e| format!("no event loop ({e})"))?;
    let handle = event_loop.handle();

    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|e| format!("the compositor has no wl_compositor ({e})"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| {
        "this compositor does not offer wlr-layer-shell, so the overlay cannot be drawn above \
         other windows (GNOME's Mutter does not support it)"
            .to_string()
    })?;
    let shm =
        Shm::bind(&globals, &qh).map_err(|e| format!("the compositor has no wl_shm ({e})"))?;
    let pool = SlotPool::new(520 * 500 * 4, &shm).map_err(|e| format!("shared memory ({e})"))?;

    // A redraw request is a ping to the loop, so it is handled after the event
    // that made it, never inside it.
    let (redraw, ping_source) = ping::make_ping().map_err(|e| format!("ping ({e})"))?;
    let core = Rc::new(RefCell::new(Core::new(
        connection.clone(),
        qh.clone(),
        compositor,
        layer_shell,
        pool,
        redraw,
    )));
    core.borrow_mut().scaling = scaling::Scaling::bind(&globals, &qh);
    core.borrow_mut().background_effect = blur::BackgroundEffect::bind(&globals, &qh);
    let painter = Painter::try_new()?;
    let icons: Vec<ksni::Icon> = [32, 64]
        .iter()
        .filter_map(|size| tray_icon_pixmap(&painter, *size))
        .map(|pixmap| tray::icon_from_pixmap(&pixmap))
        .collect();
    let (tray_sender, tray_receiver) = channel::channel::<TrayCommand>();
    let (shortcut_sender, shortcut_receiver) = channel::channel::<()>();
    let shell = WlShell::new(
        core.clone(),
        handle.clone(),
        icons,
        tray_sender,
        shortcut_sender,
    );

    let latest = Arc::new(Mutex::new(SoundState::default()));
    let launch = Launch {
        demo,
        capture: false,
        smoke: false,
        settings: None,
        note: "".into(),
    };
    let app = App::new(Box::new(shell), config, launch, latest.clone(), painter);

    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        relative_pointer_state: RelativePointerState::bind(&globals, &qh),
        shm,
        loop_handle: handle.clone(),
        core: core.clone(),
        app,
        keyboard: None,
        pointer: None,
        relative_pointer: None,
        cursor_manager: CursorShapeManager::bind(&globals, &qh).ok(),
        cursor_device: None,
        enter_serial: 0,
        modifiers: Modifiers::default(),
        keyboard_on_window: false,
        drag: None,
        remainder: (0., 0.),
        last_click: None,
        pointer_position: (0., 0.),
    };

    handle
        .insert_source(ping_source, |_, _, state: &mut State| {
            draw_requested(&mut state.app)
        })
        .map_err(|e| format!("event loop ({e})"))?;

    if let Some(instance) = &instance {
        let listener = instance
            .listener
            .try_clone()
            .map_err(|e| format!("instance socket ({e})"))?;
        handle
            .insert_source(
                Generic::new(listener, Interest::READ, Mode::Level),
                |_, listener, state: &mut State| {
                    while let Ok((stream, _)) = listener.accept() {
                        match instance::read_request(stream) {
                            Some(instance::Request::Open) => open_requested(&mut state.app),
                            Some(instance::Request::Toggle) => toggle(&mut state.app),
                            None => {}
                        }
                    }
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| format!("event loop ({e})"))?;
    }
    handle
        .insert_source(tray_receiver, |event, _, state: &mut State| {
            if let channel::Event::Msg(command) = event {
                // Leaving while a shortcut is being recorded asks first.
                if confirm_recording(&mut state.app) {
                    tray_command(&mut state.app, command);
                }
            }
        })
        .map_err(|e| format!("event loop ({e})"))?;
    handle
        .insert_source(shortcut_receiver, |event, _, state: &mut State| {
            if let channel::Event::Msg(()) = event {
                hotkey_pressed(&mut state.app, true);
            }
        })
        .map_err(|e| format!("event loop ({e})"))?;
    let pending = Arc::new(signals::Pending::default());
    let received = pending.clone();
    let (worker_ping, receiver) = ping::make_ping().map_err(|e| format!("worker ping ({e})"))?;
    handle
        .insert_source(receiver, move |_, _, state: &mut State| {
            let (status, control) = received.take();
            if status {
                status_changed(&mut state.app);
            }
            if control {
                command_finished(&mut state.app);
            }
        })
        .map_err(|e| format!("event loop ({e})"))?;
    if !demo {
        state.app.controller = Some(device::start(
            move |signal| {
                if pending.push(signal) {
                    worker_ping.ping();
                }
            },
            latest,
        ));
    }

    // Outputs arrive in the first two round trips (wl_output, then xdg-output).
    let mut queue = queue;
    for _ in 0..2 {
        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("talking to the compositor ({e})"))?;
    }
    state.sync_outputs();
    WaylandSource::new(connection, queue)
        .insert(handle.clone())
        .map_err(|e| format!("event loop ({e})"))?;

    if first && !demo {
        save(&mut state.app);
    }
    ensure_tray(&mut state.app);
    let (modifiers, key) = (state.app.config.hotkey_mod, state.app.config.hotkey_vk);
    state.app.hotkey_registered = !demo && state.app.shell.register_hotkey(modifiers, key);
    if !demo && !state.app.hotkey_registered {
        state.app.note = "No global shortcut: bind `audeze-overlay --toggle` instead.".into();
    }
    // Unlike Windows, which starts in the tray after the first run, the panel is
    // always shown at start: a portal shortcut can be granted late or not at all,
    // and the panel must stay reachable until then.
    show(&mut state.app, true);

    let signal = event_loop.get_signal();
    event_loop
        .run(None, &mut state, |state| {
            if state.core.borrow().quit {
                signal.stop();
            }
        })
        .map_err(|e| format!("event loop ({e})"))?;
    state.app.shell.shutdown();
    Ok(())
}

impl State {
    /// Whether `surface` is the overlay window rather than a menu or dialog.
    fn is_window(&self, surface: &wl_surface::WlSurface) -> bool {
        self.core
            .borrow()
            .layer
            .as_ref()
            .is_some_and(|layer| layer.wl_surface() == surface)
    }

    fn timer_fired(&mut self, timer: Timer) {
        timer_fired(&mut self.app, timer);
    }

    /// Mirrors the compositor's screens into the shell and tells the logic.
    fn sync_outputs(&mut self) {
        let outputs: Vec<Output> = self
            .output_state
            .outputs()
            .filter_map(|handle| {
                let info = self.output_state.info(&handle)?;
                let size = info.logical_size.unwrap_or_else(|| {
                    info.modes
                        .iter()
                        .find(|m| m.current)
                        .map_or((1920, 1080), |m| m.dimensions)
                });
                Some(Output {
                    name: info
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("output-{}", info.id)),
                    origin: info.logical_position.unwrap_or(info.location),
                    size,
                    scale: info.scale_factor,
                    handle,
                })
            })
            .collect();
        {
            let mut core = self.core.borrow_mut();
            core.outputs_changed(outputs);
        }
        displays_changed(&mut self.app);
    }

    /// The compositor's preferred fractional scale for the overlay changed.
    fn preferred_scale(&mut self, scale: f32) {
        if self.core.borrow_mut().set_preferred_scale(scale) {
            displays_changed(&mut self.app);
        }
    }

    /// Pointer coordinates (logical surface pixels) in the logic's units.
    fn to_app(&self, position: (f64, f64)) -> (f32, f32) {
        let scale = self.core.borrow().scale;
        let ui = ui_scale(&self.app);
        (
            position.0 as f32 * scale / ui,
            position.1 as f32 * scale / ui,
        )
    }

    fn apply_cursor(&mut self, shape: Shape) {
        if let Some(device) = &self.cursor_device {
            device.set_shape(self.enter_serial, shape);
        }
    }

    /// Chooses the pointer shape for what is under the pointer.
    fn update_cursor(&mut self, region: Region) {
        let shape = match region {
            Region::Edge(Edge::Left) | Region::Edge(Edge::Right) => Shape::EwResize,
            Region::Edge(Edge::Top) | Region::Edge(Edge::Bottom) => Shape::NsResize,
            Region::Edge(Edge::TopLeft) | Region::Edge(Edge::BottomRight) => Shape::NwseResize,
            Region::Edge(Edge::TopRight) | Region::Edge(Edge::BottomLeft) => Shape::NeswResize,
            _ => match self.core.borrow().cursor {
                Cursor::Arrow => Shape::Default,
                Cursor::Hand => Shape::Pointer,
                Cursor::Move => Shape::Move,
                Cursor::ResizeDiagonal => Shape::NwseResize,
            },
        };
        self.apply_cursor(shape);
    }

    fn pointer_pressed(&mut self, position: (f64, f64)) {
        let (x, y) = self.to_app(position);
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(at, from)| {
            now.duration_since(at) < DOUBLE_CLICK
                && (from.0 - position.0).abs() < 5.
                && (from.1 - position.1).abs() < 5.
        });
        self.last_click = if double { None } else { Some((now, position)) };
        match region_at(&self.app, x, y) {
            Region::Caption => self.drag = Some(WindowDrag::Move),
            Region::Edge(edge) => self.drag = Some(WindowDrag::Resize(edge)),
            Region::Client | Region::Transparent => {
                if double {
                    pointer_double_clicked(&mut self.app, x, y);
                } else {
                    pointer_pressed(&mut self.app, x, y);
                }
            }
        }
        self.remainder = (0., 0.);
    }

    fn pointer_released(&mut self) {
        if self.drag.take().is_some() {
            move_or_resize_finished(&mut self.app);
        } else {
            pointer_released(&mut self.app);
        }
    }

    fn key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::Escape => Key::Escape,
            Keysym::a | Keysym::A if self.modifiers.ctrl => Key::SelectAll,
            _ => Key::Other,
        };
        key_pressed(&mut self.app, key);
        if key == Key::Other {
            if event.keysym == Keysym::BackSpace {
                text_typed(&mut self.app, '\u{8}');
            } else if let Some(text) = event.utf8 {
                for ch in text.chars().filter(|c| !c.is_control()) {
                    text_typed(&mut self.app, ch);
                }
            }
        }
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        // The compositor took the surface away, for example with its screen.
        self.core.borrow_mut().surface_closed(layer);
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if !self.is_window(layer.wl_surface()) {
            return;
        }
        self.core
            .borrow_mut()
            .configured_size(configure.new_size.0, configure.new_size.1);
        // Draw at the size the compositor settled on, then show it.
        resized(&mut self.app);
        self.core.borrow_mut().present();
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let callback = Box::new(
                |state: &mut State, _: &wl_keyboard::WlKeyboard, event: KeyEvent| state.key(event),
            );
            self.keyboard = self
                .seat_state
                .get_keyboard_with_repeat(qh, &seat, None, self.loop_handle.clone(), callback)
                .ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(pointer) = self.seat_state.get_pointer(qh, &seat) {
                self.relative_pointer = self
                    .relative_pointer_state
                    .get_relative_pointer(&pointer, qh)
                    .ok();
                self.cursor_device = self
                    .cursor_manager
                    .as_ref()
                    .map(|manager| manager.get_shape_device(&pointer, qh));
                self.pointer = Some(pointer);
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
        if capability == Capability::Pointer {
            self.drag = None;
            pointer_capture_lost(&mut self.app);
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
            if let Some(relative) = self.relative_pointer.take() {
                relative.destroy();
            }
            if let Some(device) = self.cursor_device.take() {
                device.destroy();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.keyboard_on_window = self.is_window(surface);
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.is_window(surface) {
            self.keyboard_on_window = false;
            focus_lost(&mut self.app);
        }
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.core.borrow_mut().serial = serial;
        if self.keyboard_on_window {
            self.key(event);
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if self.keyboard_on_window {
            self.key(event);
        }
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let ours = self
            .core
            .borrow()
            .layer
            .as_ref()
            .map(|layer| layer.wl_surface().clone());
        for event in events {
            if Some(&event.surface) != ours.as_ref() {
                continue;
            }
            self.pointer_position = event.position;
            self.core.borrow_mut().pointer = event.position;
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.enter_serial = serial;
                    self.core.borrow_mut().serial = serial;
                    let (x, y) = self.to_app(event.position);
                    self.update_cursor(region_at(&self.app, x, y));
                }
                PointerEventKind::Leave { .. } => {}
                PointerEventKind::Motion { .. } => {
                    if self.drag.is_none() {
                        let (x, y) = self.to_app(event.position);
                        pointer_moved(&mut self.app, x, y);
                        self.update_cursor(region_at(&self.app, x, y));
                    }
                }
                PointerEventKind::Press { button, serial, .. } if button == BTN_LEFT => {
                    self.core.borrow_mut().serial = serial;
                    self.pointer_pressed(event.position);
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.pointer_released();
                }
                PointerEventKind::Axis { vertical, .. } => {
                    // Wayland scrolls down with positive values; the logic, like
                    // Windows, counts notches away from the user as positive.
                    let delta = if vertical.value120 != 0 {
                        -vertical.value120
                    } else {
                        (-vertical.absolute * 8.) as i32
                    };
                    if delta != 0 {
                        let (x, y) = self.to_app(event.position);
                        wheel(&mut self.app, x, y, delta);
                    }
                }
                _ => {}
            }
        }
    }
}

impl RelativePointerHandler for State {
    fn relative_pointer_motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpRelativePointerV1,
        _: &wl_pointer::WlPointer,
        event: RelativeMotionEvent,
    ) {
        let Some(drag) = &self.drag else { return };
        self.remainder.0 += event.delta.0;
        self.remainder.1 += event.delta.1;
        let (dx, dy) = (
            self.remainder.0.trunc() as i32,
            self.remainder.1.trunc() as i32,
        );
        if dx == 0 && dy == 0 {
            return;
        }
        self.remainder.0 -= f64::from(dx);
        self.remainder.1 -= f64::from(dy);
        match drag {
            WindowDrag::Move => self.core.borrow_mut().nudge(dx, dy),
            WindowDrag::Resize(edge) => {
                let limits = size_limits(&self.app);
                self.core.borrow_mut().resize_by(*edge, dx, dy, limits);
                resized(&mut self.app);
            }
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(State);
impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(State);
