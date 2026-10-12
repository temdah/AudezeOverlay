//! The Win32 side of the overlay window: every platform effect the application
//! logic asks for goes through `WinShell`, so that logic never touches a window
//! handle, a GDI object or a message loop directly.
//!
//! The method set is deliberately the interface a shell for another platform
//! has to provide: timers, pointer capture, visibility and geometry, a retained
//! frame, menus, confirmations, a file chooser, the global shortcut, the tray
//! icon and autostart.
use super::*;
use std::any::Any;

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Self {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
}

/// The native timer id for a logical timer.
fn timer_id(timer: Timer) -> usize {
    match timer {
        Timer::Animation => TICK,
        Timer::Hold => HOLD,
        Timer::Reorder => REORDER,
        Timer::TrayRetry => TRAY_RETRY,
        Timer::SelfTest => 3,
    }
}

pub(super) struct WinShell {
    pub(super) hwnd: HWND,
    bitmap: Option<Bitmap>,
    pub(super) tray_icon: HICON,
    /// Message the desktop shell broadcasts when its taskbar is (re)created.
    pub(super) taskbar_message: u32,
    pub(super) hotkey_id: i32,
}

impl WinShell {
    pub(super) fn new(taskbar_message: u32) -> Self {
        Self {
            hwnd: HWND::default(),
            bitmap: None,
            tray_icon: HICON::default(),
            taskbar_message,
            hotkey_id: 1,
        }
    }

    /// Identity of the retained surface, for checks that it is reused.
    pub(super) fn frame_surface(&self) -> Option<isize> {
        self.bitmap.as_ref().map(|b| b.bitmap.0 as isize)
    }

    fn tray(&self, op: NOTIFY_ICON_MESSAGE) -> bool {
        let mut n = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: self.tray_icon,
            ..Default::default()
        };
        let tip = wide("Audeze Overlay · Direct USB");
        n.szTip[..tip.len()].copy_from_slice(&tip);
        unsafe { Shell_NotifyIconW(op, &n).as_bool() }
    }
}

impl Shell for WinShell {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    // ---- Timers and redraw requests.
    fn start_timer(&mut self, timer: Timer, ms: u32) -> bool {
        unsafe { SetTimer(Some(self.hwnd), timer_id(timer), ms, None) != 0 }
    }
    fn stop_timer(&mut self, timer: Timer) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), timer_id(timer));
        }
    }
    /// Asks for a frame later, from the event loop, never re-entrantly.
    fn request_redraw(&self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), DRAW, WPARAM(0), LPARAM(0));
        }
    }

    // ---- Pointer capture: drags keep receiving events outside the window.
    fn capture_pointer(&mut self) {
        unsafe {
            SetCapture(self.hwnd);
        }
    }
    fn release_pointer(&mut self) {
        unsafe {
            let _ = ReleaseCapture();
        }
    }

    // ---- Visibility, geometry and appearance.
    fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }
    /// Display scale: 1.0 at 96 DPI.
    fn scale(&self) -> f32 {
        unsafe { GetDpiForWindow(self.hwnd).max(96) as f32 / 96. }
    }
    fn client_size(&self) -> Option<(i32, i32)> {
        let mut r = RECT::default();
        unsafe { GetClientRect(self.hwnd, &mut r) }
            .ok()
            .map(|_| (r.right - r.left, r.bottom - r.top))
    }
    fn window_rect(&self) -> Rect {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut r);
        }
        r.into()
    }
    /// Moves and sizes the window above others without activating it. Does
    /// nothing if it is already there.
    fn place(&mut self, target: Rect) {
        if self.window_rect() != target {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    target.left,
                    target.top,
                    target.width(),
                    target.height(),
                    SWP_NOACTIVATE,
                );
            }
        }
    }
    fn move_to(&mut self, x: i32, y: i32) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    fn window_work_area(&self) -> Rect {
        unsafe {
            let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(monitor, &mut info);
            info.rcWork.into()
        }
    }
    fn set_cursor(&mut self, cursor: Cursor) {
        let id = match cursor {
            Cursor::Arrow => IDC_ARROW,
            Cursor::Hand => IDC_HAND,
            Cursor::Move => IDC_SIZEALL,
            Cursor::ResizeDiagonal => IDC_SIZENWSE,
        };
        unsafe {
            if let Ok(c) = LoadCursorW(None, id) {
                SetCursor(Some(c));
            }
        }
    }
    /// Passive popups never take focus and let clicks through; the open panel does.
    fn set_input_mode(&mut self, interactive: bool) {
        let style = WS_EX_TOPMOST
            | WS_EX_TOOLWINDOW
            | WS_EX_LAYERED
            | if interactive {
                WINDOW_EX_STYLE(0)
            } else {
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT
            };
        unsafe {
            if GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32 != style.0 {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style.0 as isize);
            }
        }
    }
    fn show_window(&mut self, activate: bool) {
        unsafe {
            let _ = ShowWindow(
                self.hwnd,
                if activate { SW_SHOW } else { SW_SHOWNOACTIVATE },
            );
        }
    }
    fn hide_window(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
    fn focus(&mut self) {
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
    }
    /// Desktop blur behind the window; 0 turns it off.
    fn set_blur(&mut self, strength: u8) {
        unsafe {
            let _ = crate::platform::blur::apply_strength(self.hwnd, strength);
        }
    }
    /// Blur on at the default strength, or off.
    fn set_blur_enabled(&mut self, enabled: bool) {
        unsafe {
            let _ = crate::platform::blur::apply(self.hwnd, enabled);
        }
    }
    /// Rounds the window's corners with the given diameter.
    fn clip_rounded(&mut self, diameter: i32) {
        unsafe {
            let mut r = RECT::default();
            if GetClientRect(self.hwnd, &mut r).is_err() || r.right <= 0 || r.bottom <= 0 {
                return;
            }
            let region = CreateRoundRectRgn(0, 0, r.right + 1, r.bottom + 1, diameter, diameter);
            if !region.is_invalid() && SetWindowRgn(self.hwnd, Some(region), true) == 0 {
                // On success Windows owns the region; on failure it is still ours.
                let _ = DeleteObject(region.into());
            }
        }
    }
    /// Closes the window after the current event has finished.
    fn quit_later(&self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_APP + 7, WPARAM(0), LPARAM(0));
        }
    }
    /// Closes the window, which ends the application.
    fn quit(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    // ---- Frames: a pixel buffer is stored once, then shown at a given opacity.
    /// Keeps `pixels` as the current frame. False if no surface could be made.
    fn store_frame(&mut self, pixels: &tiny_skia::Pixmap) -> bool {
        let (w, h) = (pixels.width(), pixels.height());
        if !self
            .bitmap
            .as_ref()
            .is_some_and(|b| b.size.cx == w as i32 && b.size.cy == h as i32)
        {
            let bitmap = unsafe { Bitmap::new(w, h) };
            let Some(bitmap) = bitmap else {
                return false;
            };
            self.bitmap = Some(bitmap);
        }
        if let Some(bitmap) = &mut self.bitmap {
            bitmap.write(pixels.data());
        }
        true
    }
    /// Shows the stored frame with `alpha` (255 is opaque).
    fn present(&self, alpha: u8) {
        if let Some(b) = &self.bitmap {
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: alpha,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            unsafe {
                let _ = UpdateLayeredWindow(
                    self.hwnd,
                    None,
                    None,
                    Some(&b.size),
                    Some(b.dc),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
            }
        }
    }
    // ---- Screens.
    fn monitors(&self) -> Vec<Monitor> {
        unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
            let list = &mut *(list.0 as *mut Vec<Monitor>);
            let mut i = MONITORINFOEXW::default();
            i.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(h, &mut i.monitorInfo).as_bool() {
                let key = String::from_utf16_lossy(
                    &i.szDevice[..i.szDevice.iter().position(|n| *n == 0).unwrap_or(32)],
                );
                list.push(Monitor {
                    key,
                    label: format!("Screen {}", list.len() + 1),
                    work: i.monitorInfo.rcWork.into(),
                });
            }
            BOOL(1)
        }
        let mut monitors = vec![];
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(collect),
                LPARAM(&mut monitors as *mut _ as isize),
            );
        }
        monitors
    }
    /// Work area of the screen nearest `point`, or of the screen the user is
    /// working on when there is no saved position.
    fn work_area_near(&self, point: Option<(i32, i32)>) -> Rect {
        unsafe {
            let monitor = if let Some((x, y)) = point {
                MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST)
            } else {
                MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST)
            };
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(monitor, &mut info);
            info.rcWork.into()
        }
    }

    // ---- Modal interaction. Each call returns once the user has answered.
    /// Shows a menu at the pointer and returns the chosen item's id.
    fn popup_menu(&mut self, entries: &[MenuEntry], origin: MenuOrigin) -> Option<u32> {
        unsafe {
            let menu = build_menu(entries)?;
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);
            if origin == MenuOrigin::TrayIcon {
                let _ = SetForegroundWindow(self.hwnd);
            }
            let id = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY,
                point.x,
                point.y,
                None,
                self.hwnd,
                None,
            )
            .0;
            let _ = DestroyMenu(menu);
            if origin == MenuOrigin::TrayIcon {
                // Lets the menu loop finish before the next message is handled.
                let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            }
            (id > 0).then_some(id as u32)
        }
    }
    fn confirm(&mut self, title: &str, message: &str, buttons: Buttons) -> Choice {
        let flags = match buttons {
            Buttons::YesNoCancel => MB_YESNOCANCEL,
            Buttons::YesNo => MB_YESNO,
            Buttons::OkCancel => MB_OKCANCEL,
        } | MB_ICONQUESTION;
        let answer = unsafe {
            MessageBoxW(
                Some(self.hwnd),
                PCWSTR(wide(message).as_ptr()),
                PCWSTR(wide(title).as_ptr()),
                flags,
            )
        };
        match answer {
            IDYES => Choice::Yes,
            IDNO => Choice::No,
            IDOK => Choice::Ok,
            _ => Choice::Cancel,
        }
    }
    /// Asks for a profile file to open or to write. `Ok(None)` means cancelled.
    fn choose_file(&mut self, save: bool) -> std::result::Result<FileSelection, String> {
        crate::platform::profile_dialog::choose(self.hwnd, save).map(FileSelection::Ready)
    }

    // ---- Global shortcut. Only one binding is active at a time.
    fn register_hotkey(&mut self, modifiers: u32, key: u32) -> bool {
        unsafe {
            RegisterHotKey(
                Some(self.hwnd),
                self.hotkey_id,
                HOT_KEY_MODIFIERS(modifiers) | MOD_NOREPEAT,
                key,
            )
            .is_ok()
        }
    }
    fn unregister_hotkey(&mut self) {
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), self.hotkey_id);
        }
    }
    /// Switches to a new binding without a gap: the previous one keeps working
    /// if the new one is taken.
    fn rebind_hotkey(&mut self, modifiers: u32, key: u32) -> bool {
        let candidate = if self.hotkey_id >= 100 {
            self.hotkey_id - 100
        } else {
            self.hotkey_id + 100
        };
        unsafe {
            if RegisterHotKey(
                Some(self.hwnd),
                candidate,
                HOT_KEY_MODIFIERS(modifiers) | MOD_NOREPEAT,
                key,
            )
            .is_ok()
            {
                let _ = UnregisterHotKey(Some(self.hwnd), self.hotkey_id);
                self.hotkey_id = candidate;
                true
            } else {
                false
            }
        }
    }

    // ---- Notification-area icon.
    fn tray_add(&mut self) -> bool {
        self.tray(NIM_ADD)
    }
    fn tray_remove(&mut self) {
        self.tray(NIM_DELETE);
    }
    // ---- Start with the user's session.
    fn set_autostart(&mut self, enabled: bool) -> std::result::Result<(), String> {
        crate::platform::startup::set_enabled(enabled)
    }

    /// Releases what the shell registered with the system.
    fn shutdown(&mut self) {
        self.tray_remove();
        unsafe {
            if !self.tray_icon.is_invalid() {
                let _ = DestroyIcon(self.tray_icon);
            }
        }
        self.unregister_hotkey();
    }
}

/// The Windows shell behind the application, for the code that is Windows-only.
pub(super) fn win(app: &App) -> &WinShell {
    app.shell
        .as_any()
        .downcast_ref::<WinShell>()
        .expect("the application runs on the Windows shell")
}
pub(super) fn win_mut(app: &mut App) -> &mut WinShell {
    app.shell
        .as_any_mut()
        .downcast_mut::<WinShell>()
        .expect("the application runs on the Windows shell")
}

/// Builds a native menu. `&` introduces a mnemonic in Windows menus, so labels
/// are escaped to show literally.
unsafe fn build_menu(entries: &[MenuEntry]) -> Option<HMENU> {
    let menu = CreatePopupMenu().ok()?;
    for entry in entries {
        match entry {
            MenuEntry::Item {
                id,
                label,
                checked,
                enabled,
            } => {
                let flags = MF_STRING
                    | if *checked { MF_CHECKED } else { MF_UNCHECKED }
                    | if *enabled { MF_ENABLED } else { MF_GRAYED };
                let _ = AppendMenuW(
                    menu,
                    flags,
                    *id as usize,
                    PCWSTR(wide(&label.replace('&', "&&")).as_ptr()),
                );
            }
            MenuEntry::Separator => {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            MenuEntry::Submenu { label, entries } => {
                let Some(child) = build_menu(entries) else {
                    continue;
                };
                if AppendMenuW(
                    menu,
                    MF_POPUP | MF_STRING,
                    child.0 as usize,
                    PCWSTR(wide(&label.replace('&', "&&")).as_ptr()),
                )
                .is_err()
                {
                    let _ = DestroyMenu(child);
                }
            }
        }
    }
    Some(menu)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menus are described once, platform-independently; this checks that the
    /// description becomes the native menu the application always showed.
    #[test]
    fn a_menu_description_keeps_labels_states_nesting_and_literal_ampersands() {
        let entries = vec![
            MenuEntry::item(1, "Fish & chips"),
            MenuEntry::checkable(2, "Checked", true),
            MenuEntry::item_if(false, 3, "Unavailable"),
            MenuEntry::Separator,
            MenuEntry::Submenu {
                label: "Profile".into(),
                entries: vec![MenuEntry::item(10, "Inner"), MenuEntry::disabled("Gone")],
            },
            MenuEntry::disabled("Nothing here"),
        ];
        unsafe {
            let menu = build_menu(&entries).expect("native menu");
            assert_eq!(GetMenuItemCount(Some(menu)), 6);
            let state = |position: u32| GetMenuState(menu, position, MF_BYPOSITION);
            let text = |position: i32| {
                let mut buffer = [0u16; 64];
                let length =
                    GetMenuStringW(menu, position as u32, Some(&mut buffer), MF_BYPOSITION);
                String::from_utf16_lossy(&buffer[..length as usize])
            };
            // "&" starts a mnemonic in Windows menus, so a literal one is doubled.
            assert_eq!(text(0), "Fish && chips");
            assert_eq!(state(1) & MF_CHECKED.0, MF_CHECKED.0);
            assert_eq!(state(0) & MF_CHECKED.0, 0);
            assert_ne!(state(2) & MF_GRAYED.0, 0, "disabled entries are greyed");
            assert_eq!(state(0) & MF_GRAYED.0, 0);
            assert_eq!(state(3) & MF_SEPARATOR.0, MF_SEPARATOR.0);
            assert_eq!(state(4) & MF_POPUP.0, MF_POPUP.0, "a submenu");
            assert_eq!(state(4) >> 8, 2, "with its two entries");
            assert_ne!(state(5) & MF_GRAYED.0, 0);
            let item = GetMenuItemID(menu, 0);
            assert_eq!(item, 1);
            let _ = DestroyMenu(menu);
        }
    }
}
