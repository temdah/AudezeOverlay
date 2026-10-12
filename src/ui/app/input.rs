//! Pointer, keyboard, timer and status events as the logic sees them: logical
//! coordinates and plain enums, whatever window system delivered them. A shell
//! translates its native events into these calls.
use super::*;

/// A key the logic reacts to while a draft is being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Key {
    Enter,
    Escape,
    /// Ctrl+A.
    SelectAll,
    Other,
}

/// How the notification-area icon was used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(windows)]
pub(in crate::ui) enum TrayEvent {
    /// Context menu requested.
    Menu,
    /// Opened with a double click.
    Open,
}

// ---- Window behavior.

/// What pressing at a point does: pass through, act on a control, move the
/// window, or resize it from an edge. The shell maps this to its native
/// move and resize.
pub(in crate::ui) fn region_at(app: &App, x: f32, y: f32) -> Region {
    if !app.interactive {
        return Region::Transparent;
    }
    let (w, h) = logical_size(app);
    if app.edit && !app.config.locked && app.settings.is_none() {
        let left = x < 8.;
        let right = x > w - 8.;
        let top = y < 8.;
        let bottom = y > h - 8.;
        let edge = match (left, right, top, bottom) {
            (true, _, true, _) => Some(Edge::TopLeft),
            (_, true, true, _) => Some(Edge::TopRight),
            (true, _, _, true) => Some(Edge::BottomLeft),
            (_, true, _, true) => Some(Edge::BottomRight),
            (true, _, _, _) => Some(Edge::Left),
            (_, true, _, _) => Some(Edge::Right),
            (_, _, true, _) => Some(Edge::Top),
            (_, _, _, true) => Some(Edge::Bottom),
            _ => None,
        };
        if let Some(edge) = edge {
            return Region::Edge(edge);
        }
    }
    if y < 68. && hit(app, x, y) == Action::None && !app.config.locked {
        Region::Caption
    } else {
        Region::Client
    }
}

/// Smallest and largest window sizes in device pixels.
pub(in crate::ui) fn size_limits(app: &App) -> ((i32, i32), (i32, i32)) {
    let s = app.shell.scale();
    (
        (
            (app.config.minimum_width() as f32 * s) as i32,
            (app.config.minimum_height() as f32 * s) as i32,
        ),
        ((3200. * s) as i32, (2400. * s) as i32),
    )
}

pub(in crate::ui) fn resized(app: &mut App) {
    clip_window(app);
    request_draw(app);
}

/// The user finished moving or resizing the window: snap it to the screen
/// edges if asked, remember where it is and trim the layout canvas.
pub(in crate::ui) fn move_or_resize_finished(app: &mut App) {
    if app.config.locked {
        return;
    }
    let mut r = app.shell.window_rect();
    if app.config.snap {
        let work = app.shell.window_work_area();
        let w = r.width();
        let h = r.height();
        let mut x = r.left;
        let mut y = r.top;
        if (x - work.left).abs() < 24 {
            x = work.left
        }
        if (r.right - work.right).abs() < 24 {
            x = work.right - w
        }
        if (y - work.top).abs() < 24 {
            y = work.top
        }
        if (r.bottom - work.bottom).abs() < 24 {
            y = work.bottom - h
        }
        app.shell.move_to(x, y);
        r.left = x;
        r.top = y;
    }
    app.config.position = Some((r.left, r.top));
    if app.settings.is_none() {
        let (w, h) = logical_size(app);
        app.config.resize_canvas(w.round() as u32, h.round() as u32);
    }
    save(app);
}

pub(in crate::ui) fn displays_changed(app: &mut App) {
    app.monitors = app.shell.monitors();
    position(app);
    request_draw(app);
}

#[cfg(windows)]
pub(in crate::ui) fn close_requested(app: &mut App) {
    hide(app);
}

/// A draw was requested. While something animates, the animation timer renders
/// the newest state instead of every request.
pub(in crate::ui) fn draw_requested(app: &mut App) {
    if !app.smoke
        && !app.capture
        && app.shell.is_visible()
        && (app.animation_timer
            || app
                .last_frame
                .is_some_and(|t| t.elapsed() < Duration::from_millis(16)))
    {
        // Keep one pending redraw. The existing animation timer will
        // render the newest state, rather than enqueueing intermediate frames.
        start_animation_timer(app);
        if !app.animation_timer {
            // A failed native timer must not strand the pending redraw.
            render(app);
        }
    } else {
        render(app);
    }
}

// ---- Pointer.

pub(in crate::ui) fn pointer_pressed(app: &mut App, x: f32, y: f32) {
    let target = hit(app, x, y);
    act(app, target, x, y);
}

pub(in crate::ui) fn pointer_double_clicked(app: &mut App, x: f32, y: f32) {
    if hit(app, x, y) == Action::Mix {
        set_mix(app, 50);
    }
}

pub(in crate::ui) fn pointer_moved(app: &mut App, x: f32, y: f32) {
    if !matches!(app.drag, Drag::None) {
        drag(app, x, y)
    } else {
        let target = hit(app, x, y);
        if app.hover != target {
            app.hover = target;
            request_draw(app);
        }
        let cursor = if matches!(target, Action::Card(_)) {
            Cursor::Move
        } else if matches!(target, Action::ResizeCard(_)) {
            Cursor::ResizeDiagonal
        } else if target != Action::None {
            Cursor::Hand
        } else {
            Cursor::Arrow
        };
        app.shell.set_cursor(cursor);
    }
}

pub(in crate::ui) fn pointer_released(app: &mut App) {
    if !matches!(app.drag, Drag::None) {
        app.drag_preview = None;
        app.shell.stop_timer(Timer::Reorder);
        app.drag = Drag::None;
        app.shell.release_pointer();
        save(app);
        request_draw(app);
    }
}

/// The window lost pointer capture (for example to another window).
pub(in crate::ui) fn pointer_capture_lost(app: &mut App) {
    if !matches!(app.drag, Drag::None) {
        save(app);
    }
    app.drag = Drag::None;
    app.drag_preview = None;
    app.shell.stop_timer(Timer::Reorder);
    request_draw(app);
}

/// `delta` is in wheel units: 120 per notch, smaller for smooth wheels.
pub(in crate::ui) fn wheel(app: &mut App, x: f32, y: f32, delta: i32) {
    app.wheel += delta;
    let steps = app.wheel / 120;
    app.wheel %= 120;
    if steps != 0 {
        match hit(app, x, y) {
            Action::Slider(key) => {
                let (value, min, max) = dashboard::slider(&app.config, key);
                slider_value(app, key, (value + steps as f32 * 5.).clamp(min, max));
                save(app);
            }
            Action::EqBand(index) => {
                if let Some(d) = app.eq_editor.draft() {
                    preview_eq_gain(app, index, d.value.bands[index].gain + steps as f32);
                }
                request_draw(app);
            }
            Action::Mix => set_mix(app, (app.mix_to as i32 + steps * 5).clamp(0, 100) as u8),
            Action::Level if app.sound.sidetone == Some(true) => send(
                app,
                Command::SidetoneLevel(
                    (app.sound.sidetone_level.unwrap_or(0) as i32 + steps * 3).clamp(0, 100) as u8,
                ),
            ),
            _ => (),
        }
    }
}

// ---- Keyboard.

/// Handles a key while a draft is open or the panel is showing. False if the
/// key means nothing here and the system should handle it. A shortcut being
/// recorded takes keys before this is called.
pub(in crate::ui) fn key_pressed(app: &mut App, key: Key) -> bool {
    if app.eq_editor.name_edit.is_some() {
        match key {
            Key::Enter => commit_eq_name(app),
            Key::Escape => app.eq_editor.name_edit = None,
            Key::SelectAll => app.eq_editor.name_selected = true,
            Key::Other => (),
        }
        request_draw(app);
        true
    } else if app.number_edit.is_some() {
        match key {
            Key::Enter => {
                commit_number(app);
            }
            Key::Escape => {
                app.number_edit = None;
                app.note.clear();
            }
            Key::SelectAll => app.number_edit.as_mut().unwrap().selected = true,
            Key::Other => (),
        }
        request_draw(app);
        true
    } else if key == Key::Escape {
        if app.edit {
            finish_layout(app);
        } else {
            hide(app)
        }
        true
    } else {
        false
    }
}

/// Typed text for an open draft. False if no draft is open.
pub(in crate::ui) fn text_typed(app: &mut App, ch: char) -> bool {
    if app.eq_editor.name_edit.is_some() {
        type_eq_name(app, ch);
    } else if let Some(draft) = app.number_edit.as_mut() {
        draft.type_char(ch);
    } else {
        return false;
    }
    request_draw(app);
    true
}

pub(in crate::ui) fn focus_lost(app: &mut App) {
    commit_eq_name(app);
    if !commit_number(app) {
        app.number_edit = None;
    }
    request_draw(app);
}

/// The global shortcut fired. `ours` is false for a different registration.
pub(in crate::ui) fn hotkey_pressed(app: &mut App, ours: bool) {
    if app.recording {
        app.recorded_key = Some((app.config.hotkey_mod, app.config.hotkey_vk));
        request_draw(app);
    } else if ours {
        toggle(app);
    }
}

// ---- Timers.

pub(in crate::ui) fn timer_fired(app: &mut App, timer: Timer) {
    match timer {
        Timer::TrayRetry => ensure_tray(app),
        Timer::Reorder => {
            app.shell.stop_timer(Timer::Reorder);
            if matches!(app.drag, Drag::Card { resize: false, .. }) {
                let (x, y) = app.drag_point;
                drag(app, x, y);
            }
        }
        Timer::Hold => {
            app.shell.stop_timer(Timer::Hold);
            if app.config.fade_ms == 0 {
                hide(app)
            } else {
                app.fade_out = Some(Instant::now());
                start_animation_timer(app);
            }
        }
        Timer::Animation => animation_tick(app),
        // Only the native self-test uses this timer, and it handles it itself.
        #[cfg(windows)]
        Timer::SelfTest => (),
    }
}

fn animation_tick(app: &mut App) {
    if let Some(t) = app.fade_out {
        let p = t.elapsed().as_secs_f32() * 1000. / app.config.fade_ms.max(1) as f32;
        if p >= 1. {
            hide(app);
            return;
        }
        app.alpha = (255. * (1. - p).powi(2)) as u8;
        app.shell.present(app.alpha);
    } else if let Some(t) = app.fade_in {
        let p = (t.elapsed().as_secs_f32() / 0.12).min(1.);
        app.alpha = (255. * (1. - (1. - p).powi(3))) as u8;
        app.shell.present(app.alpha);
        if app.alpha >= 230 {
            if let Some(delivery) = app.telemetry_at.take() {
                device::trace_present(delivery, app.alpha, app.sound.mix.map(|m| m.1));
            }
        }
        if p >= 1. {
            app.fade_in = None;
        }
    }
    let mix_active = app.mix_started.is_some();
    if let Some(t) = app.mix_started {
        if t.elapsed() > Duration::from_millis(MIX_ANIMATION_MS) {
            app.mix_started = None;
        }
    }
    if mix_active || app.motion_active || app.draw_queued {
        render(app);
    }
    if app.fade_in.is_none()
        && app.fade_out.is_none()
        && app.mix_started.is_none()
        && !app.motion_active
    {
        stop_animation_timer(app);
    }
}

// ---- The headset worker and the rest of the system.

/// The worker published a new sound snapshot.
pub(in crate::ui) fn status_changed(app: &mut App) {
    changed(app);
}

/// A queued command finished; show its failure, if any.
pub(in crate::ui) fn command_finished(app: &mut App) {
    if let Some(c) = &app.controller {
        app.note = c.last_error().unwrap_or_default().into();
        if !app.note.is_empty() {
            app.eq_editor.failed();
        }
    }
    request_draw(app);
}

/// USB topology may have changed; the worker re-enumerates off this thread.
#[cfg(windows)]
pub(in crate::ui) fn devices_changed(app: &mut App) {
    if let Some(controller) = &app.controller {
        controller.devices_changed();
    }
}

#[cfg(windows)]
pub(in crate::ui) fn tray_used(app: &mut App, event: TrayEvent) {
    match event {
        TrayEvent::Menu => menu(app),
        TrayEvent::Open => open_requested(app),
    }
}

/// Another launch of the application, or the tray icon, asked for the panel.
pub(in crate::ui) fn open_requested(app: &mut App) {
    if !confirm_recording(app) {
        return;
    }
    app.settings = None;
    show(app, true)
}

/// The desktop's notification area was recreated; register the icon again.
#[cfg(windows)]
pub(in crate::ui) fn tray_host_restarted(app: &mut App) {
    app.tray_registration = Default::default();
    ensure_tray(app);
}
