//! The overlay's application logic: state, actions, popups, settings and
//! layout editing. It knows nothing about any window system; everything it needs
//! from the platform goes through a `Shell`.
mod eq_profiles;
mod equalizer;
mod input;
mod layout_editor;
mod notice;
mod render;
mod settings;
#[cfg(test)]
mod tests;
mod tray_registration;
use super::{
    draw::{Box2, Painter},
    shell::*,
    view::{self as dashboard, Action, Hit, View},
};
use audeze_overlay::{
    config::Config,
    device,
    device::command::{Command, Controller},
    layout::{self, Card},
    model::{mix_from_point, reconcile_mix, MixTarget, SoundState},
};
pub(in crate::ui) use eq_profiles::*;
pub(in crate::ui) use equalizer::*;
pub(in crate::ui) use input::*;
pub(in crate::ui) use layout_editor::*;
pub(in crate::ui) use notice::Notice;
pub(in crate::ui) use render::*;
pub(in crate::ui) use settings::*;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub(in crate::ui) const MIX_ANIMATION_MS: u64 = 70;
#[derive(Clone, Copy)]
pub(in crate::ui) enum Drag {
    Setting(&'static str),
    None,
    Mix,
    Level,
    EqBand(usize),
    Card {
        index: usize,
        resize: bool,
        x: f32,
        y: f32,
        original_cards: [Card; 6],
    },
}
pub(in crate::ui) struct App {
    pub(in crate::ui) shell: Box<dyn Shell>,
    pub(in crate::ui) config: Config,
    pub(in crate::ui) eq_editor: audeze_overlay::eq::Editor,
    pending_profile_file: Option<eq_profiles::ProfileFile>,
    pub(in crate::ui) sound: SoundState,
    pub(in crate::ui) latest: Arc<Mutex<SoundState>>,
    pub(in crate::ui) controller: Option<Controller>,
    pub(in crate::ui) painter: Painter,
    pub(in crate::ui) presentation: super::presentation::Presentation,
    pub(in crate::ui) motion_active: bool,
    pub(in crate::ui) raster: Option<tiny_skia::Pixmap>,
    pub(in crate::ui) hits: Vec<Hit>,
    pub(in crate::ui) cards: [Box2; 6],
    pub(in crate::ui) settings: Option<usize>,
    pub(in crate::ui) edit: bool,
    pub(in crate::ui) interactive: bool,
    pub(in crate::ui) hover: Action,
    pub(in crate::ui) note: Notice,
    pub(in crate::ui) recording: bool,
    pub(in crate::ui) recorded_key: Option<(u32, u32)>,
    pub(in crate::ui) number_edit: Option<audeze_overlay::input::NumberDraft>,
    pub(in crate::ui) drag: Drag,
    pub(in crate::ui) reorder_hover: layout::HoverIntent,
    pub(in crate::ui) drag_point: (f32, f32),
    pub(in crate::ui) drag_preview: Option<(usize, Box2)>,
    pub(in crate::ui) demo: bool,
    pub(in crate::ui) capture: bool,
    pub(in crate::ui) hotkey_registered: bool,
    pub(in crate::ui) monitors: Vec<Monitor>,
    pub(in crate::ui) mix_from: f32,
    pub(in crate::ui) mix_to: f32,
    pub(in crate::ui) mix_started: Option<Instant>,
    pub(in crate::ui) pending_mix: Option<MixTarget>,
    pub(in crate::ui) fade_in: Option<Instant>,
    pub(in crate::ui) fade_out: Option<Instant>,
    pub(in crate::ui) alpha: u8,
    pub(in crate::ui) draw_queued: bool,
    pub(in crate::ui) animation_timer: bool,
    pub(in crate::ui) last_frame: Option<Instant>,
    pub(in crate::ui) telemetry_at: Option<device::Delivery>,
    pub(in crate::ui) applied_blur: Option<u8>,
    pub(in crate::ui) notification_options: bool,
    pub(in crate::ui) popup_cards: Option<u8>,
    pub(in crate::ui) tray_registration: tray_registration::TrayRegistration,
    pub(in crate::ui) low_latched: bool,
    pub(in crate::ui) wheel: i32,
    pub(in crate::ui) smoke: bool,
    #[cfg(windows)]
    pub(in crate::ui) smoke_phase: u8,
    #[cfg(windows)]
    pub(in crate::ui) checks: Vec<(String, bool)>,
}

/// How the application was started.
pub(in crate::ui) struct Launch {
    /// Sample data instead of a headset; nothing is written.
    pub demo: bool,
    /// Render one frame to a file instead of running.
    pub capture: bool,
    /// Run the native self-test.
    pub smoke: bool,
    /// Open on this settings page.
    pub settings: Option<usize>,
    pub note: Notice,
}

impl App {
    pub(in crate::ui) fn new(
        shell: Box<dyn Shell>,
        config: Config,
        launch: Launch,
        latest: Arc<Mutex<SoundState>>,
        painter: Painter,
    ) -> Self {
        let Launch {
            demo,
            capture,
            smoke,
            settings,
            note,
        } = launch;
        Self {
            monitors: shell.monitors(),
            config,
            eq_editor: audeze_overlay::eq::Editor::default(),
            pending_profile_file: None,
            sound: if demo {
                sample()
            } else {
                SoundState {
                    status: "Connecting directly to the USB dongle…".into(),
                    ..Default::default()
                }
            },
            latest,
            controller: None,
            painter,
            presentation: super::presentation::Presentation::default(),
            motion_active: false,
            raster: None,
            hits: vec![],
            cards: [Box2::default(); 6],
            settings,
            edit: false,
            interactive: false,
            hover: Action::None,
            note,
            recording: false,
            recorded_key: None,
            number_edit: None,
            drag: Drag::None,
            reorder_hover: layout::HoverIntent::default(),
            drag_point: (0., 0.),
            drag_preview: None,
            demo,
            capture,
            hotkey_registered: false,
            mix_from: 50.,
            mix_to: 50.,
            mix_started: None,
            pending_mix: None,
            fade_in: None,
            fade_out: None,
            alpha: 255,
            draw_queued: false,
            animation_timer: false,
            last_frame: None,
            telemetry_at: None,
            applied_blur: None,
            notification_options: false,
            popup_cards: None,
            tray_registration: Default::default(),
            low_latched: false,
            wheel: 0,
            smoke,
            #[cfg(windows)]
            smoke_phase: 0,
            #[cfg(windows)]
            checks: vec![],
            shell,
        }
    }
}
pub(in crate::ui) fn sample() -> SoundState {
    SoundState {
        mix_revision: 0,
        eq_index: Some(0),
        eq_bands: None,
        eq_revision: 0,
        eq_index_revision: 0,
        mix: Some((50, 50)),
        sidetone: Some(false),
        sidetone_level: Some(26),
        anc: Some("Transparency".into()),
        noise: Some("High".into()),
        battery: Some(70),
        muted: Some(true),
        device: Some("Maxwell 2 ANC".into()),
        connection: Some("USB Dongle".into()),
        status: "Preview · sample data".into(),
        ..Default::default()
    }
}
pub(in crate::ui) fn monitor_rect(app: &App) -> Rect {
    if let Some(m) = app.monitors.iter().find(|m| m.key == app.config.monitor) {
        return m.work;
    }
    app.shell.work_area_near(app.config.position)
}
pub(in crate::ui) fn position(app: &mut App) {
    let scale = app.shell.scale();
    let r = monitor_rect(app);
    let (w, h) = if app.settings.is_some() {
        (900., 750.)
    } else if let Some(mask) = app.popup_cards {
        let popup = app.config.notification_layout(mask);
        (popup.width as f32, popup.height as f32)
    } else {
        (app.config.width as f32, app.config.height as f32)
    };
    let fit = if app.settings.is_some() {
        scale.min(r.width() as f32 / w).min(r.height() as f32 / h)
    } else {
        scale
    };
    let w = (w * fit).round() as i32;
    let h = (h * fit).round() as i32;
    let (x, y) = app
        .config
        .position
        .unwrap_or((r.right - w - 20, r.top + 20));
    let x = x.clamp(r.left, (r.right - w).max(r.left));
    let y = y.clamp(r.top, (r.bottom - h).max(r.top));
    app.shell.place(Rect {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    });
}
pub(in crate::ui) fn save(app: &mut App) {
    if !app.demo {
        if let Err(e) = app.config.save() {
            app.note = format!("Could not save settings: {e}").into();
        }
    }
}
pub(in crate::ui) fn request_draw(app: &mut App) {
    if !app.draw_queued {
        app.draw_queued = true;
        app.shell.request_redraw();
    }
}
pub(in crate::ui) fn start_animation_timer(app: &mut App) {
    if !app.animation_timer {
        app.animation_timer = app.shell.start_timer(Timer::Animation, 16);
    }
}
pub(in crate::ui) fn stop_animation_timer(app: &mut App) {
    app.shell.stop_timer(Timer::Animation);
    app.animation_timer = false;
}
pub(in crate::ui) fn show(app: &mut App, interactive: bool) {
    if interactive {
        if let Some(controller) = &app.controller {
            controller.devices_changed();
        }
        app.popup_cards = None;
        if app.settings == Some(5) {
            open_eq(app);
        } else if app.config.cards[5].visible {
            send(app, Command::EqRefresh);
        }
    }
    let was_visible = app.shell.is_visible();
    app.shell.stop_timer(Timer::Hold);
    app.interactive = interactive;
    app.fade_out = None;
    app.recording = false;
    app.number_edit = None;
    if !interactive {
        app.settings = None;
        app.edit = false;
    }
    let blur = app.config.blur_strength;
    if app.applied_blur != Some(blur) {
        app.shell.set_blur(blur);
        app.applied_blur = Some(blur);
    }
    app.shell.set_input_mode(interactive);
    if !app.shell.is_visible() {
        app.alpha = if interactive && app.config.animations && !app.capture {
            0
        } else {
            255
        };
        app.fade_in = (app.alpha == 0).then(Instant::now);
    } else {
        app.alpha = 255;
        app.fade_in = None;
    }
    position(app);
    // Replacing the retained bitmap before showing prevents a one-frame flash
    // of the mix value from the previous popup.
    if !was_visible {
        render_frame(app, true);
    }
    app.shell.show_window(interactive);
    // Acrylic/style transitions need the same clip even when size is unchanged.
    clip_window(app);
    request_draw(app);
    if interactive {
        app.shell.focus();
    } else {
        app.shell.start_timer(Timer::Hold, app.config.duration_ms);
    }
    if app.fade_in.is_some() {
        start_animation_timer(app);
    }
}
pub(in crate::ui) fn hide(app: &mut App) {
    if !confirm_recording(app) {
        return;
    }
    commit_eq_name(app);
    if app.edit {
        finish_layout(app);
    }
    if !matches!(app.drag, Drag::None) {
        save(app);
    }
    app.interactive = false;
    app.edit = false;
    app.recording = false;
    app.number_edit = None;
    app.fade_in = None;
    app.fade_out = None;
    app.motion_active = false;
    app.drag = Drag::None;
    app.shell.release_pointer();
    stop_animation_timer(app);
    app.shell.stop_timer(Timer::Hold);
    app.shell.hide_window();
}
pub(in crate::ui) fn toggle(app: &mut App) {
    if app.shell.is_visible() && app.interactive {
        hide(app)
    } else {
        app.settings = None;
        show(app, true)
    }
}
pub(in crate::ui) fn ensure_tray(app: &mut App) {
    app.shell.stop_timer(Timer::TrayRetry);
    if !app.tray_registration.can_retry() {
        return;
    }
    let added = app.shell.tray_add();
    if app.tray_registration.complete(added) {
        // Explorer can be busy/unavailable at logon. This timer does no USB work
        // and stops on success or after one minute; TaskbarCreated starts anew.
        app.shell.start_timer(Timer::TrayRetry, 2000);
    }
}
/// What the notification-area icon's menu can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TrayCommand {
    Open,
    Settings,
    ResetPosition,
    Hide,
    Quit,
}

pub(in crate::ui) fn tray_command(app: &mut App, command: TrayCommand) {
    match command {
        TrayCommand::Open => {
            app.settings = None;
            show(app, true)
        }
        TrayCommand::Settings => {
            app.settings = Some(app.config.settings_page);
            show(app, true)
        }
        TrayCommand::ResetPosition => reset(app),
        TrayCommand::Hide => hide(app),
        TrayCommand::Quit => app.shell.quit(),
    }
}

#[cfg(windows)]
pub(in crate::ui) fn menu(app: &mut App) {
    if !confirm_recording(app) {
        return;
    }
    let entries: Vec<_> = [
        (1, "Open"),
        (2, "Settings"),
        (3, "Reset position"),
        (4, "Hide"),
        (5, "Quit"),
    ]
    .into_iter()
    .map(|(id, label)| MenuEntry::item(id, label))
    .collect();
    let command = match app.shell.popup_menu(&entries, MenuOrigin::TrayIcon) {
        Some(1) => TrayCommand::Open,
        Some(2) => TrayCommand::Settings,
        Some(3) => TrayCommand::ResetPosition,
        Some(4) => TrayCommand::Hide,
        Some(5) => TrayCommand::Quit,
        _ => return,
    };
    tray_command(app, command);
}
pub(in crate::ui) fn reset(app: &mut App) {
    app.config.position = None;
    save(app);
    show(app, true);
}
pub(in crate::ui) fn send(app: &mut App, c: Command) {
    if app.demo {
        match c {
            Command::Mix(p) => app.sound.mix = Some((100 - p, p)),
            Command::Sidetone(b) => app.sound.sidetone = Some(b),
            Command::SidetoneLevel(n) => app.sound.sidetone_level = Some(n),
            Command::Anc(s) => app.sound.anc = Some(s),
            Command::Noise(s) => app.sound.noise = Some(s),
            Command::EqSelect(slot) => app.sound.eq_index = Some(slot),
            Command::EqRefresh => (),
            Command::EqLoad(slot) => app.sound.eq_bands = Some(audeze_overlay::eq::sample(slot)),
            Command::EqGain { slot, band } => {
                if let Some(eq) = app.sound.eq_bands.as_mut().filter(|eq| eq.slot == slot) {
                    if let Some(b) = eq.bands.iter_mut().find(|b| b.id == band.id) {
                        *b = band;
                    }
                }
            }
        }
        receive_eq(app);
        app.note = "Preview — headset unchanged".into();
    } else if app.sound.mix.is_none() {
        app.note = Notice::UntilConnected("Connect and turn on a supported headset.");
    } else if let Some(ctrl) = &app.controller {
        ctrl.clear_error();
        let mix = if let Command::Mix(value) = &c {
            Some(*value)
        } else {
            None
        };
        let revision = ctrl.send(c);
        if let Some(chat) = mix {
            app.pending_mix = Some(MixTarget { revision, chat });
            app.sound.mix = Some((100 - chat, chat));
        }
        app.note.clear();
    }
    request_draw(app);
}
pub(in crate::ui) fn set_mix(app: &mut App, p: u8) {
    let p = ((p as f32 / 5.).round() as u8 * 5).min(100);
    if app.sound.mix == Some((100 - p, p)) {
        return;
    }
    send(app, Command::Mix(p));
    let target = app.sound.mix.map(|mix| mix.1 as f32).unwrap_or(app.mix_to);
    if app.mix_to != target {
        app.mix_from = displayed_mix(app);
        app.mix_to = target;
        app.mix_started = (app.config.animations && !app.smoke).then(Instant::now);
        if app.mix_started.is_some() {
            start_animation_timer(app);
        }
    }
}
pub(in crate::ui) fn act(app: &mut App, action: Action, x: f32, y: f32) {
    app.note.clear();
    if app.recording
        && matches!(
            action,
            Action::Tab(_)
                | Action::Back
                | Action::Settings
                | Action::Edit
                | Action::Close
                | Action::EqEditor
        )
        && !confirm_recording(app)
    {
        return;
    }
    if !matches!(action, Action::Number(_) | Action::None) && !commit_number(app) {
        return;
    }
    if action != Action::EqRename {
        commit_eq_name(app);
    }
    match action {
        Action::EqMenu => eq_menu(app),
        Action::EqProfiles => profile_menu(app),
        Action::EqSaveNew => save_profile_copy(app),
        Action::EqEditor => { app.settings = Some(5); app.config.settings_page = 5; save(app); show(app, true); },
        Action::EqInspect(slot) => inspect_eq(app, slot),
        Action::EqUse(slot) => { app.eq_editor.preview_slot = None; send(app, Command::EqSelect(slot)); },
        Action::EqPin(slot) => { app.config.eq.pinned[slot as usize] = !app.config.eq.pinned[slot as usize]; save(app); },
        Action::EqBand(index) => { app.drag = Drag::EqBand(index); app.shell.capture_pointer(); set_eq_band(app, index, y); },
        Action::EqSave => save_eq(app),
        Action::EqDiscard => discard_eq(app),
        Action::EqFlat => { for i in 0..10 { preview_eq_gain(app, i, 0.); } },
        Action::EqRename => { if app.eq_editor.selected >= 6 { app.eq_editor.name_edit = Some(app.config.eq.name(app.eq_editor.selected).into()); app.eq_editor.name_selected = true; } },
        Action::EqReload => inspect_eq(app, app.eq_editor.selected),
        Action::Close => hide(app),
        Action::Settings => {
            if app.edit {
                finish_layout(app);
            }
            app.settings = Some(app.config.settings_page);
            show(app, true)
        }
        Action::Back => {
            app.settings = None;
            show(app, true)
        }
        Action::Tab(i) => {
            app.settings = Some(i);
            app.config.settings_page = i;
            if i == 5 { open_eq(app); }
            save(app);
        }
        Action::Edit => {
            if app.config.locked {
                app.note = "Position is locked. Unlock it in Startup & device.".into();
            } else {
                app.settings = None;
                if app.edit {
                    finish_layout(app);
                } else {
                    app.edit = true;
                }
                show(app, true);
            }
        }
        Action::CardMenu(i) => card_menu(app, Some(i)),
        Action::AddCard => card_menu(app, None),
        Action::StatusMenu => card_menu(app, Some(6)),
        Action::RemoveCard(i) => card_visibility(app, i, false),
        Action::RestoreCard(i) => card_visibility(app, i, true),
        Action::Mix => {
            app.drag = Drag::Mix;
            app.shell.capture_pointer();
            drag(app, x, y)
        }
        Action::Level if app.sound.sidetone == Some(true) => {
            app.drag = Drag::Level;
            app.shell.capture_pointer();
            drag(app, x, y)
        }
        Action::Side => {
            if let Some(b) = app.sound.sidetone {
                send(app, Command::Sidetone(!b));
            }
        }
        Action::Anc(i) => send(
            app,
            Command::Anc(["On", "Off", "Transparency"][i as usize].into()),
        ),
        Action::Noise(i) => send(app, Command::Noise(["Off", "Low", "High"][i as usize].into())),
        Action::Card(i) | Action::ResizeCard(i) if app.edit && !app.config.locked => {
            app.reorder_hover = layout::HoverIntent::default();
            app.drag_point = (x,y);
            app.drag_preview = None;
            app.drag = Drag::Card {
                index: i,
                resize: matches!(action, Action::ResizeCard(_)),
                x,
                y,
                original_cards: app.config.cards,
            };
            app.shell.capture_pointer();
        }
        Action::Setting(key, n) => setting(app, key, n),
        Action::Slider(key) => {
            app.drag = Drag::Setting(key);
            app.shell.capture_pointer();
            set_slider(app, key, x);
        }
        Action::Record => { app.recording = true; app.recorded_key = None; }
        Action::EndRecord => finish_recording(app),
        Action::DiscardRecord => { app.recording = false; app.recorded_key = None; },
        Action::NotificationOptions => app.notification_options = !app.notification_options,
        Action::Number(key) => {
            if commit_number(app) { app.number_edit = Some(audeze_overlay::input::NumberDraft::new(key, dashboard::slider(&app.config, key).0 as u32)); }
        }
        Action::ResetApp if app.shell.confirm("Reset Audeze Overlay", "Reset all overlay preferences, layout and shortcuts?\nYour headset sound settings will not change.", Buttons::OkCancel) == Choice::Ok => { factory_reset(app); }
        _ => (),
    };
    request_draw(app);
}
/// Device pixels per logical unit: the display scale, or the fitted scale of the
/// fixed-size settings view.
pub(in crate::ui) fn ui_scale(app: &App) -> f32 {
    if app.settings.is_some() {
        let (w, h) = app.shell.client_size().unwrap_or((0, 0));
        (w as f32 / 900.).min(h as f32 / 750.)
    } else {
        app.shell.scale()
    }
}
pub(in crate::ui) fn hit(app: &App, x: f32, y: f32) -> Action {
    app.hits
        .iter()
        .rev()
        .find(|h| h.rect.contains(x, y))
        .map(|h| h.action)
        .unwrap_or(Action::None)
}
pub(in crate::ui) fn changed(app: &mut App) {
    let mut next = app.latest.lock().unwrap().clone();
    if next.family != app.sound.family
        || next.transport != app.sound.transport
        || next.xbox != app.sound.xbox
    {
        app.pending_mix = None;
        app.eq_editor = Default::default();
        if let Some(family) = next.family {
            app.config.select_eq_profile(family, next.xbox);
        }
    }
    reconcile_mix(&mut app.pending_mix, &mut next);
    let connected_now = app.sound.mix.is_none() && next.mix.is_some();
    if connected_now {
        app.note.connected();
    }
    let eq_changed = next.eq_index != app.sound.eq_index;
    let prev = &app.sound;
    let changed_cards = next.changed_cards(prev, app.config.notify);
    let popup = changed_cards != 0;
    let low = next
        .battery
        .map(|b| app.config.low_battery > 0 && b <= app.config.low_battery)
        .unwrap_or(false);
    let alert = low && !app.low_latched;
    app.low_latched = low;
    if alert {
        app.note = format!("Low headset battery: {}%", next.battery.unwrap()).into();
    }
    if next.mix != prev.mix {
        app.mix_from = displayed_mix(app);
        app.mix_to = next.mix.map(|m| m.1).unwrap_or(50) as f32;
        app.mix_started = if app.config.animations && prev.mix.is_some() && app.shell.is_visible() {
            Some(Instant::now())
        } else {
            None
        };
    }
    app.sound = next;
    receive_eq(app);
    if connected_now && app.settings == Some(5) {
        open_eq(app);
    }
    if app.settings.is_none()
        && app.config.inline_eq
        && app.config.cards[5].visible
        && (eq_changed || connected_now)
    {
        if let Some(slot) = app.sound.eq_index {
            inspect_eq(app, slot);
        }
    }
    if (popup || alert) && app.config.auto_show && !app.interactive {
        let popup_cards = app
            .config
            .changed_card_only
            .then_some(changed_cards | if alert { 1 << 4 } else { 0 });
        if app.shell.is_visible() && app.popup_cards == popup_cards {
            // A physical knob can generate 20 updates/second. Refresh the hold
            // deadline without repeating ShowWindow, clipping or positioning.
            app.fade_out = None;
            app.alpha = 255;
            app.shell.start_timer(Timer::Hold, app.config.duration_ms);
        } else {
            app.popup_cards = popup_cards;
            show(app, false);
        }
    }
    if app.shell.is_visible() {
        app.telemetry_at = device::delivery_time();
        request_draw(app);
        if app.mix_started.is_some() {
            start_animation_timer(app);
        }
    }
}
