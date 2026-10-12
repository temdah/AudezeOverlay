//! Menus and confirmations drawn by the application and shown as a popup of the
//! overlay window, answered before the call returns (the same contract as the
//! native menus and message boxes on Windows).
//!
//! A modal runs its own little Wayland client on a second event queue of the
//! same connection, with its own seat, pointer and keyboard objects. That lets
//! it wait for the answer inside a call from the application logic without
//! touching the main window's event handling, which stays paused meanwhile.
use crate::ui::{
    draw::{Box2, Canvas, Painter},
    shell::{Buttons, Choice, MenuEntry},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        Connection, QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::LayerSurface,
        xdg::{
            popup::{ConfigureKind, Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowHandler},
            XdgPositioner, XdgShell,
        },
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_positioner::{
    Anchor, ConstraintAdjustment, Gravity,
};

const BTN_LEFT: u32 = 0x110;
const BACKGROUND: u32 = 0x1c1c1c;
const BORDER: u32 = 0x3d3d3d;
const HOVER: u32 = 0x3a3a3a;
const TEXT: u32 = 0xf0f0f0;
const DISABLED: u32 = 0x808080;
const ACCENT: u32 = 0xebebeb;

pub(super) enum Content {
    Menu(Vec<MenuEntry>),
    Dialog {
        title: String,
        message: String,
        buttons: Buttons,
    },
}

pub(super) enum Outcome {
    Menu(Option<u32>),
    Dialog(Choice),
}

/// Where and how a modal is shown, taken from the overlay window it belongs to.
pub(super) struct Placement {
    /// Anchor point in the overlay's logical pixels.
    pub anchor: (i32, i32),
    /// Serial of the input event that caused the modal; the compositor needs it
    /// to give the popup the pointer and keyboard.
    pub serial: u32,
    pub scale: i32,
    /// Size of the overlay, to centre dialogs in it.
    pub parent_size: (i32, i32),
}

/// One line of a menu or one button of a dialog, in popup-local logical pixels.
struct Hit {
    area: Box2,
    enabled: bool,
    answer: Answer,
}
#[derive(Clone, Copy)]
enum Answer {
    Id(u32),
    Choice(Choice),
    None,
}

struct Modal {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    popup: Popup,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    hits: Vec<Hit>,
    hover: Option<usize>,
    frame: Option<Vec<u8>>,
    size: (u32, u32),
    scale: i32,
    drawn: bool,
    outcome: Option<Outcome>,
    is_menu: bool,
    painter: Box<Painter>,
    content: Content,
}

/// Shows `content` and returns the answer, or `None` if the popup was dismissed.
pub(super) fn run(
    connection: &Connection,
    parent: &LayerSurface,
    painter: Box<Painter>,
    content: Content,
    placement: Placement,
) -> (Option<Outcome>, Box<Painter>) {
    match try_run(connection, parent, painter, content, placement) {
        Ok(result) => result,
        Err(painter) => (None, painter),
    }
}

fn try_run(
    connection: &Connection,
    parent: &LayerSurface,
    mut painter: Box<Painter>,
    content: Content,
    placement: Placement,
) -> Result<(Option<Outcome>, Box<Painter>), Box<Painter>> {
    let scale = placement.scale.max(1);
    let layout = layout(&mut painter, &content);
    let (width, height) = (layout.size.0.ceil() as i32, layout.size.1.ceil() as i32);

    let Ok((globals, mut queue)) = registry_queue_init::<Modal>(connection) else {
        return Err(painter);
    };
    let qh = queue.handle();
    let (Ok(compositor), Ok(xdg_shell), Ok(shm)) = (
        CompositorState::bind(&globals, &qh),
        XdgShell::bind(&globals, &qh),
        Shm::bind(&globals, &qh),
    ) else {
        return Err(painter);
    };
    let Ok(pool) = SlotPool::new(
        (width * height * 4 * scale * scale).max(4096) as usize,
        &shm,
    ) else {
        return Err(painter);
    };
    let Ok(positioner) = XdgPositioner::new(&xdg_shell) else {
        return Err(painter);
    };
    positioner.set_size(width, height);
    let (ax, ay) = match &content {
        Content::Menu(_) => placement.anchor,
        // Dialogs open in the middle of the overlay.
        Content::Dialog { .. } => (
            (placement.parent_size.0 - width) / 2,
            (placement.parent_size.1 - height) / 2,
        ),
    };
    positioner.set_anchor_rect(ax, ay, 1, 1);
    positioner.set_anchor(Anchor::TopLeft);
    positioner.set_gravity(Gravity::BottomRight);
    positioner.set_constraint_adjustment(
        ConstraintAdjustment::FlipX
            | ConstraintAdjustment::FlipY
            | ConstraintAdjustment::SlideX
            | ConstraintAdjustment::SlideY,
    );

    let surface = compositor.create_surface(&qh);
    let Ok(popup) = Popup::from_surface(None, &positioner, &qh, surface, &xdg_shell) else {
        return Err(painter);
    };
    parent.get_popup(popup.xdg_popup());

    let is_menu = matches!(content, Content::Menu(_));
    let mut modal = Modal {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        popup,
        keyboard: None,
        pointer: None,
        hits: layout.hits,
        hover: None,
        frame: None,
        size: (width as u32, height as u32),
        scale,
        drawn: false,
        outcome: None,
        is_menu,
        painter,
        content,
    };
    modal.render();

    // Learn the seat, then ask the compositor to give the popup the input.
    let _ = queue.roundtrip(&mut modal);
    let _ = queue.roundtrip(&mut modal);
    if let Some(seat) = modal.seat_state.seats().next() {
        modal.popup.xdg_popup().grab(&seat, placement.serial);
    }
    modal.popup.wl_surface().commit();

    while modal.outcome.is_none() {
        if queue.blocking_dispatch(&mut modal).is_err() {
            break;
        }
    }
    let outcome = modal.outcome.take();
    Ok((outcome, modal.painter))
}

/// The measured rows or buttons of a modal, before it is drawn.
struct Layout {
    size: (f32, f32),
    hits: Vec<Hit>,
}

/// `Canvas::text` starts drawing at the top of its box, so a label that should be
/// centred vertically in `area` gets a box moved up by half the text's height.
fn centered(painter: &Painter, area: Box2, size: f32, bold: bool, scale: f32) -> Box2 {
    let px = (size * scale).round().max(1.);
    let ascent = painter.fonts[usize::from(bold)]
        .horizontal_line_metrics(px)
        .map_or(px, |m| m.ascent)
        / scale;
    let capital_height = size * 0.72;
    Box2::new(
        area.x,
        area.y + (area.h + capital_height) / 2. - ascent,
        area.w,
        area.h,
    )
}

fn text_width(painter: &Painter, text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| painter.fonts[0].metrics(c, size).advance_width)
        .sum()
}

/// Splits a message into lines no wider than `width`.
fn wrap(painter: &Painter, text: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = vec![];
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && text_width(painter, &candidate, size) > width {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

fn layout(painter: &mut Painter, content: &Content) -> Layout {
    match content {
        Content::Menu(entries) => {
            let rows = flatten(entries);
            let widest = rows
                .iter()
                .map(|r| text_width(painter, &r.label, 15.) + if r.indent { 14. } else { 0. })
                .fold(120., f32::max);
            let width = widest + 2. * 14. + 18.;
            let mut y = 6.;
            let mut hits = vec![];
            for row in &rows {
                let h = if row.separator { 9. } else { 30. };
                hits.push(Hit {
                    area: Box2::new(4., y, width - 8., h),
                    enabled: row.enabled && !row.separator && !row.header,
                    answer: if row.enabled && !row.separator && !row.header {
                        Answer::Id(row.id)
                    } else {
                        Answer::None
                    },
                });
                y += h;
            }
            Layout {
                size: (width, y + 6.),
                hits,
            }
        }
        Content::Dialog {
            message, buttons, ..
        } => {
            let width = 380.;
            let lines = wrap(painter, message, 15., width - 40.);
            let height = 20. + 24. + 12. + lines.len() as f32 * 22. + 20. + 34. + 18.;
            let choices: &[(Choice, &str)] = match buttons {
                Buttons::YesNoCancel => &[
                    (Choice::Yes, "Yes"),
                    (Choice::No, "No"),
                    (Choice::Cancel, "Cancel"),
                ],
                Buttons::YesNo => &[(Choice::Yes, "Yes"), (Choice::No, "No")],
                Buttons::OkCancel => &[(Choice::Ok, "OK"), (Choice::Cancel, "Cancel")],
            };
            let mut x = width - 20.;
            let mut hits = vec![];
            for (choice, _) in choices.iter().rev() {
                x -= 92.;
                hits.push(Hit {
                    area: Box2::new(x, height - 18. - 34., 88., 34.),
                    enabled: true,
                    answer: Answer::Choice(*choice),
                });
            }
            hits.reverse();
            Layout {
                size: (width, height),
                hits,
            }
        }
    }
}

struct Row {
    id: u32,
    label: String,
    checked: bool,
    enabled: bool,
    separator: bool,
    header: bool,
    indent: bool,
}

/// Submenus are shown inline: the submenu's name as a heading, then its items.
fn flatten(entries: &[MenuEntry]) -> Vec<Row> {
    let mut rows = vec![];
    for entry in entries {
        match entry {
            MenuEntry::Item {
                id,
                label,
                checked,
                enabled,
            } => rows.push(Row {
                id: *id,
                label: label.clone(),
                checked: *checked,
                enabled: *enabled,
                separator: false,
                header: false,
                indent: false,
            }),
            MenuEntry::Separator => rows.push(Row {
                id: 0,
                label: String::new(),
                checked: false,
                enabled: false,
                separator: true,
                header: false,
                indent: false,
            }),
            MenuEntry::Submenu { label, entries } => {
                rows.push(Row {
                    id: 0,
                    label: label.clone(),
                    checked: false,
                    enabled: false,
                    separator: false,
                    header: true,
                    indent: false,
                });
                for mut row in flatten(entries) {
                    row.indent = true;
                    rows.push(row);
                }
            }
        }
    }
    rows
}

impl Modal {
    /// Draws the modal into a premultiplied BGRA frame.
    fn render(&mut self) {
        let scale = self.scale as f32;
        let (w, h) = self.size;
        let mut canvas = Canvas::new(
            &mut self.painter,
            w * self.scale as u32,
            h * self.scale as u32,
            scale,
        );
        let whole = Box2::new(0., 0., w as f32, h as f32);
        canvas.rect(whole, BORDER, 1., 10.);
        canvas.rect(whole.inset(1.), BACKGROUND, 1., 9.);
        match &self.content {
            Content::Menu(entries) => {
                let rows = flatten(entries);
                for (index, (row, hit)) in rows.iter().zip(&self.hits).enumerate() {
                    if row.separator {
                        let y = hit.area.y + hit.area.h / 2.;
                        canvas.rect(
                            Box2::new(hit.area.x + 6., y, hit.area.w - 12., 1.),
                            BORDER,
                            1.,
                            0.,
                        );
                        continue;
                    }
                    if self.hover == Some(index) && hit.enabled {
                        canvas.rect(hit.area, HOVER, 1., 6.);
                    }
                    let color = if row.enabled || row.header {
                        TEXT
                    } else {
                        DISABLED
                    };
                    let indent = if row.indent { 14. } else { 0. };
                    if row.checked {
                        let (cx, cy) = (hit.area.x + 13., hit.area.y + hit.area.h / 2.);
                        canvas.line(
                            &[(cx - 4., cy), (cx - 1., cy + 3.), (cx + 4., cy - 4.)],
                            ACCENT,
                            1.,
                            2.,
                        );
                    }
                    let label = Box2::new(
                        hit.area.x + 26. + indent,
                        hit.area.y,
                        hit.area.w - 30. - indent,
                        hit.area.h,
                    );
                    let label = centered(canvas.painter, label, 15., row.header, scale);
                    canvas.text(&row.label, label, 15., color, row.header, false);
                }
            }
            Content::Dialog {
                title,
                message,
                buttons,
            } => {
                canvas.text(
                    title,
                    Box2::new(20., 18., w as f32 - 40., 26.),
                    17.,
                    TEXT,
                    true,
                    false,
                );
                let lines = wrap(canvas.painter, message, 15., w as f32 - 40.);
                for (i, line) in lines.iter().enumerate() {
                    canvas.text(
                        line,
                        Box2::new(20., 56. + i as f32 * 22., w as f32 - 40., 22.),
                        15.,
                        0xcfcfcf,
                        false,
                        false,
                    );
                }
                let labels: &[&str] = match buttons {
                    Buttons::YesNoCancel => &["Yes", "No", "Cancel"],
                    Buttons::YesNo => &["Yes", "No"],
                    Buttons::OkCancel => &["OK", "Cancel"],
                };
                for (index, (hit, label)) in self.hits.iter().zip(labels).enumerate() {
                    let primary = index == 0;
                    let hovered = self.hover == Some(index);
                    let fill = if primary {
                        ACCENT
                    } else if hovered {
                        HOVER
                    } else {
                        0x2b2b2b
                    };
                    canvas.rect(hit.area, fill, 1., 7.);
                    let color = if primary { 0x141414 } else { TEXT };
                    let area = centered(canvas.painter, hit.area, 15., true, scale);
                    canvas.text(label, area, 15., color, true, true);
                }
            }
        }
        let mut bgra = Vec::with_capacity(canvas.pix.data().len());
        for px in canvas.pix.data().as_chunks::<4>().0 {
            bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        self.frame = Some(bgra);
        self.present();
    }

    fn present(&mut self) {
        if !self.drawn {
            return;
        }
        let Some(frame) = &self.frame else { return };
        let (w, h) = (
            (self.size.0 as i32) * self.scale,
            (self.size.1 as i32) * self.scale,
        );
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        canvas[..frame.len()].copy_from_slice(frame);
        let surface = self.popup.wl_surface();
        surface.set_buffer_scale(self.scale);
        surface.damage_buffer(0, 0, w, h);
        if buffer.attach_to(surface).is_ok() {
            surface.commit();
        }
    }

    fn hit_at(&self, x: f64, y: f64) -> Option<usize> {
        self.hits
            .iter()
            .position(|h| h.area.contains(x as f32, y as f32))
    }

    fn answer(&mut self, index: usize) {
        let hit = &self.hits[index];
        if !hit.enabled {
            return;
        }
        self.outcome = Some(match hit.answer {
            Answer::Id(id) => Outcome::Menu(Some(id)),
            Answer::Choice(choice) => Outcome::Dialog(choice),
            Answer::None => return,
        });
    }

    fn cancel(&mut self) {
        self.outcome = Some(if self.is_menu {
            Outcome::Menu(None)
        } else {
            Outcome::Dialog(Choice::Cancel)
        });
    }

    /// Moves the highlight to the next enabled entry, for keyboard use.
    fn step(&mut self, forward: bool) {
        let count = self.hits.len();
        if count == 0 {
            return;
        }
        let mut index = self.hover.unwrap_or(if forward { count - 1 } else { 0 });
        for _ in 0..count {
            index = if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            };
            if self.hits[index].enabled {
                self.hover = Some(index);
                self.render();
                return;
            }
        }
    }
}

// SCTK's generated dispatching needs these handlers to exist, although a popup
// never creates a window or reacts to screens changing.
impl WindowHandler for Modal {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {}
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        _: WindowConfigure,
        _: u32,
    ) {
    }
}

impl OutputHandler for Modal {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl CompositorHandler for Modal {
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

impl PopupHandler for Modal {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Popup,
        configure: PopupConfigure,
    ) {
        if matches!(
            configure.kind,
            ConfigureKind::Initial | ConfigureKind::Reactive | ConfigureKind::Reposition { .. }
        ) {
            self.drawn = true;
            self.present();
        }
    }
    /// The compositor dismissed the popup, for example after a click elsewhere.
    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Popup) {
        self.cancel();
    }
}

impl SeatHandler for Modal {
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
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Modal {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.popup.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let hover = self.hit_at(event.position.0, event.position.1);
                    if hover != self.hover {
                        self.hover = hover;
                        self.render();
                    }
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    if let Some(index) = self.hit_at(event.position.0, event.position.1) {
                        self.answer(index)
                    }
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for Modal {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        match event.keysym {
            Keysym::Escape => self.cancel(),
            Keysym::Down => self.step(true),
            Keysym::Up => self.step(false),
            Keysym::Return | Keysym::KP_Enter => {
                if let Some(index) = self.hover {
                    self.answer(index);
                } else if !self.is_menu {
                    // A dialog's first button is its default.
                    self.answer(0);
                }
            }
            _ => {}
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
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
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

impl ShmHandler for Modal {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Modal);
impl ProvidesRegistryState for Modal {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(Modal);
