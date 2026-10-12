//! What the application logic needs from the platform's window system.
//!
//! The logic decides *what* should happen (show the panel, start the hold timer,
//! ask which screen to use); a shell does it with the native API. Everything the
//! logic can ask for is a method here, so the logic never sees a window handle,
//! a surface or a message loop, and can be tested with a fake shell.
//!
//! Menus and confirmations are modal. File selection can finish immediately or
//! deliver a later result on the UI loop, keeping portal waits off that loop.
use std::any::Any;
#[cfg(any(windows, test))]
use std::path::PathBuf;

pub enum FileSelection {
    #[cfg(any(windows, test))]
    Ready(Option<PathBuf>),
    #[cfg(any(target_os = "linux", test))]
    Pending,
}

/// A rectangle in physical (device) pixels. Edges are exclusive on the right and
/// bottom, so `width` is `right - left`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// A screen the overlay may be placed on.
#[derive(Clone, Debug)]
pub struct Monitor {
    /// Stable identifier saved in the preferences.
    pub key: String,
    pub label: String,
    /// The area not covered by the taskbar or panels, in the same coordinates as
    /// the window position.
    pub work: Rect,
}

/// The timers the application logic can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Timer {
    /// Drives animation frames; runs only while something moves.
    Animation,
    /// How long a passive popup stays visible.
    Hold,
    /// Delay before a hovered card swap commits.
    Reorder,
    /// Retries tray registration while the desktop shell starts.
    TrayRetry,
    /// Steps the opt-in native self-test.
    #[cfg(windows)]
    SelfTest,
}

/// The pointer shape the logic wants over the current target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Hand,
    /// Over a card that can be dragged.
    Move,
    /// Over a card's resize handle.
    ResizeDiagonal,
}

/// A window edge or corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// What a point on the window does when pressed and dragged: the logic decides,
/// and the shell moves or resizes the window accordingly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Clicks pass through to whatever is underneath.
    Transparent,
    /// An ordinary control or card.
    Client,
    /// Dragging moves the window.
    Caption,
    /// Dragging resizes the window from this edge.
    Edge(Edge),
}

/// Where a menu was asked for. On Windows, menus opened from the notification
/// icon need the window brought to the front to be dismissed correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuOrigin {
    Panel,
    #[cfg(windows)]
    TrayIcon,
}

/// A menu described without any native handles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        /// Non-zero; returned by `popup_menu` when chosen.
        id: u32,
        label: String,
        checked: bool,
        enabled: bool,
    },
    Separator,
    Submenu {
        label: String,
        entries: Vec<MenuEntry>,
    },
}
impl MenuEntry {
    pub fn item(id: u32, label: impl Into<String>) -> Self {
        Self::item_if(true, id, label)
    }
    pub fn item_if(enabled: bool, id: u32, label: impl Into<String>) -> Self {
        Self::Item {
            id,
            label: label.into(),
            checked: false,
            enabled,
        }
    }
    pub fn checkable(id: u32, label: impl Into<String>, checked: bool) -> Self {
        Self::Item {
            id,
            label: label.into(),
            checked,
            enabled: true,
        }
    }
    /// A greyed-out line that cannot be chosen.
    pub fn disabled(label: impl Into<String>) -> Self {
        Self::item_if(false, 0, label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buttons {
    YesNoCancel,
    YesNo,
    OkCancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Yes,
    No,
    Ok,
    Cancel,
}

pub trait Shell: Any {
    /// Lets platform-specific code (and only that) reach its own shell type.
    #[cfg(any(windows, test))]
    fn as_any(&self) -> &dyn Any;
    #[cfg(windows)]
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // ---- Timers and redraw requests.
    /// Starts or restarts a timer. False if the system refused it.
    fn start_timer(&mut self, timer: Timer, ms: u32) -> bool;
    fn stop_timer(&mut self, timer: Timer);
    /// Asks for a frame later, from the event loop, never re-entrantly.
    fn request_redraw(&self);

    // ---- Pointer capture: drags keep receiving events outside the window.
    fn capture_pointer(&mut self);
    fn release_pointer(&mut self);

    // ---- Visibility, geometry and appearance.
    fn is_visible(&self) -> bool;
    /// Display scale: 1.0 at the platform's reference density.
    fn scale(&self) -> f32;
    /// Size of the drawable area in physical pixels.
    fn client_size(&self) -> Option<(i32, i32)>;
    fn window_rect(&self) -> Rect;
    /// Moves and sizes the window above others without activating it. Does
    /// nothing if it is already there.
    fn place(&mut self, target: Rect);
    /// Moves the window without changing its size or stacking.
    fn move_to(&mut self, x: i32, y: i32);
    /// Work area of the screen that holds most of the window.
    fn window_work_area(&self) -> Rect;
    fn set_cursor(&mut self, cursor: Cursor);
    /// Passive popups never take focus and let clicks through; the open panel does.
    fn set_input_mode(&mut self, interactive: bool);
    fn show_window(&mut self, activate: bool);
    fn hide_window(&mut self);
    fn focus(&mut self);
    /// Desktop blur behind the window; 0 turns it off.
    fn set_blur(&mut self, strength: u8);
    /// Blur on at the default strength, or off.
    fn set_blur_enabled(&mut self, enabled: bool) {
        self.set_blur(if enabled { 50 } else { 0 });
    }
    /// Rounds the window's corners with the given diameter.
    fn clip_rounded(&mut self, diameter: i32);
    /// Closes the window after the current event has finished.
    fn quit_later(&self);
    /// Closes the window, which ends the application.
    fn quit(&mut self);

    // ---- Frames: a pixel buffer is stored once, then shown at a given opacity.
    /// Keeps `pixels` as the current frame. False if no surface could be made.
    fn store_frame(&mut self, pixels: &tiny_skia::Pixmap) -> bool;
    /// Shows the stored frame with `alpha` (255 is opaque).
    fn present(&self, alpha: u8);

    // ---- Screens.
    fn monitors(&self) -> Vec<Monitor>;
    /// Work area of the screen nearest `point`, or of the screen the user is
    /// working on when there is no saved position.
    fn work_area_near(&self, point: Option<(i32, i32)>) -> Rect;

    // ---- Modal interaction. Each call returns once the user has answered.
    /// Shows a menu at the pointer and returns the chosen item's id.
    fn popup_menu(&mut self, entries: &[MenuEntry], origin: MenuOrigin) -> Option<u32>;
    fn confirm(&mut self, title: &str, message: &str, buttons: Buttons) -> Choice;
    /// Asks for a profile file to open or to write. `Ok(None)` means cancelled.
    fn choose_file(&mut self, save: bool) -> Result<FileSelection, String>;

    // ---- Global shortcut. Only one binding is active at a time. `modifiers` and
    // `key` are stored in the preferences: bit 1 Alt, 2 Ctrl, 4 Shift, and a key
    // code in the preference format (Windows virtual-key numbering); a shell on
    // another platform translates it to its own key names.
    fn register_hotkey(&mut self, modifiers: u32, key: u32) -> bool;
    fn unregister_hotkey(&mut self);
    /// Switches to a new binding without a gap: the previous one keeps working
    /// if the new one is taken.
    fn rebind_hotkey(&mut self, modifiers: u32, key: u32) -> bool;

    // ---- Notification-area icon.
    fn tray_add(&mut self) -> bool;
    fn tray_remove(&mut self);

    // ---- Start with the user's session.
    fn set_autostart(&mut self, enabled: bool) -> Result<(), String>;

    /// Releases what the shell registered with the system.
    fn shutdown(&mut self);
}
