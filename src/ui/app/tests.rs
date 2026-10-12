//! The application logic running against a fake shell: no window, no headset
//! and no platform API, so these run unchanged on every platform. They state the
//! behavior that was verified on the native Windows window before the logic was
//! made portable.
use super::*;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::VecDeque,
    path::PathBuf,
};

/// Records what the logic asked of the platform and answers modal questions from
/// a script.
struct Fake {
    visible: Cell<bool>,
    rect: Cell<Rect>,
    work: Rect,
    timers: RefCell<Vec<(Timer, u32)>>,
    redraws: Cell<u32>,
    alpha: Cell<Option<u8>>,
    menus: RefCell<VecDeque<Option<u32>>>,
    confirms: RefCell<VecDeque<Choice>>,
    files: RefCell<VecDeque<Result<Option<PathBuf>, String>>>,
    deferred_file: Cell<bool>,
    cursors: RefCell<Vec<Cursor>>,
    moves: RefCell<Vec<(i32, i32)>>,
    blur: Cell<Option<u8>>,
    capture: Cell<bool>,
    hotkey_available: Cell<bool>,
    autostart: RefCell<VecDeque<Result<(), String>>>,
}
impl Fake {
    fn new() -> Self {
        Self {
            visible: Cell::new(false),
            rect: Cell::new(Rect::default()),
            work: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            timers: Default::default(),
            redraws: Cell::new(0),
            alpha: Cell::new(None),
            menus: Default::default(),
            confirms: Default::default(),
            files: Default::default(),
            deferred_file: Cell::new(false),
            cursors: Default::default(),
            moves: Default::default(),
            blur: Cell::new(None),
            capture: Cell::new(false),
            hotkey_available: Cell::new(true),
            autostart: Default::default(),
        }
    }
    fn timer(&self, timer: Timer) -> Option<u32> {
        self.timers
            .borrow()
            .iter()
            .find(|(t, _)| *t == timer)
            .map(|(_, ms)| *ms)
    }
}
impl Shell for Fake {
    fn as_any(&self) -> &dyn Any {
        self
    }
    #[cfg(windows)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn start_timer(&mut self, timer: Timer, ms: u32) -> bool {
        self.timers.borrow_mut().retain(|(t, _)| *t != timer);
        self.timers.borrow_mut().push((timer, ms));
        true
    }
    fn stop_timer(&mut self, timer: Timer) {
        self.timers.borrow_mut().retain(|(t, _)| *t != timer);
    }
    fn request_redraw(&self) {
        self.redraws.set(self.redraws.get() + 1);
    }
    fn capture_pointer(&mut self) {
        self.capture.set(true);
    }
    fn release_pointer(&mut self) {
        self.capture.set(false);
    }
    fn is_visible(&self) -> bool {
        self.visible.get()
    }
    fn scale(&self) -> f32 {
        1.
    }
    fn client_size(&self) -> Option<(i32, i32)> {
        let r = self.rect.get();
        Some((r.width(), r.height()))
    }
    fn window_rect(&self) -> Rect {
        self.rect.get()
    }
    fn place(&mut self, target: Rect) {
        self.rect.set(target);
    }
    fn move_to(&mut self, x: i32, y: i32) {
        self.moves.borrow_mut().push((x, y));
        let r = self.rect.get();
        self.rect.set(Rect {
            left: x,
            top: y,
            right: x + r.width(),
            bottom: y + r.height(),
        });
    }
    fn window_work_area(&self) -> Rect {
        self.work
    }
    fn set_cursor(&mut self, cursor: Cursor) {
        self.cursors.borrow_mut().push(cursor);
    }
    fn set_input_mode(&mut self, _: bool) {}
    fn show_window(&mut self, _: bool) {
        self.visible.set(true);
    }
    fn hide_window(&mut self) {
        self.visible.set(false);
    }
    fn focus(&mut self) {}
    fn set_blur(&mut self, strength: u8) {
        self.blur.set(Some(strength));
    }
    fn clip_rounded(&mut self, _: i32) {}
    fn quit_later(&self) {}
    fn quit(&mut self) {}
    fn store_frame(&mut self, _: &tiny_skia::Pixmap) -> bool {
        true
    }
    fn present(&self, alpha: u8) {
        self.alpha.set(Some(alpha));
    }
    fn monitors(&self) -> Vec<Monitor> {
        vec![Monitor {
            key: "DISPLAY1".into(),
            label: "Screen 1".into(),
            work: self.work,
        }]
    }
    fn work_area_near(&self, _: Option<(i32, i32)>) -> Rect {
        self.work
    }
    fn popup_menu(&mut self, _: &[MenuEntry], _: MenuOrigin) -> Option<u32> {
        self.menus.borrow_mut().pop_front().flatten()
    }
    fn confirm(&mut self, _: &str, _: &str, _: Buttons) -> Choice {
        self.confirms
            .borrow_mut()
            .pop_front()
            .unwrap_or(Choice::Cancel)
    }
    fn choose_file(&mut self, _: bool) -> Result<FileSelection, String> {
        if self.deferred_file.get() {
            Ok(FileSelection::Pending)
        } else {
            self.files
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(None))
                .map(FileSelection::Ready)
        }
    }
    fn register_hotkey(&mut self, _: u32, _: u32) -> bool {
        self.hotkey_available.get()
    }
    fn unregister_hotkey(&mut self) {}
    fn rebind_hotkey(&mut self, _: u32, _: u32) -> bool {
        self.hotkey_available.get()
    }
    fn tray_add(&mut self) -> bool {
        true
    }
    fn tray_remove(&mut self) {}
    fn set_autostart(&mut self, _: bool) -> Result<(), String> {
        self.autostart.borrow_mut().pop_front().unwrap_or(Ok(()))
    }
    fn shutdown(&mut self) {}
}

fn fake(app: &App) -> &Fake {
    app.shell.as_any().downcast_ref().expect("fake shell")
}

/// A demo-mode application with the panel open, as `--smoke-test` starts it.
fn open_app() -> App {
    let mut app = App::new(
        Box::new(Fake::new()),
        Config::default(),
        Launch {
            demo: true,
            capture: false,
            smoke: true,
            settings: None,
            note: "".into(),
        },
        Arc::new(Mutex::new(sample())),
        Painter::new(),
    );
    show(&mut app, true);
    render(&mut app);
    app
}
fn area(app: &App, action: Action) -> Box2 {
    app.hits
        .iter()
        .find(|h| h.action == action)
        .map(|h| h.rect)
        .unwrap_or_else(|| panic!("no {action:?} on screen"))
}
fn center(b: Box2) -> (f32, f32) {
    (b.x + b.w / 2., b.y + b.h / 2.)
}
fn status(app: &mut App, change: impl FnOnce(&mut SoundState)) {
    let mut next = sample();
    change(&mut next);
    *app.latest.lock().unwrap() = next;
    status_changed(app);
}
fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        text_typed(app, ch);
    }
}

#[test]
fn the_wheel_moves_the_mix_five_points_per_notch_and_stops_at_the_ends() {
    let mut app = open_app();
    let (x, y) = center(area(&app, Action::Mix));
    wheel(&mut app, x, y, 3 * 120);
    assert_eq!(app.mix_to, 65.);
    wheel(&mut app, x, y, -120);
    assert_eq!(app.mix_to, 60.);
    wheel(&mut app, x, y, 40 * 120);
    assert_eq!(app.mix_to, 100.);
    wheel(&mut app, x, y, -80 * 120);
    assert_eq!(app.mix_to, 0.);
}

#[test]
fn a_smooth_wheel_accumulates_until_a_full_notch() {
    let mut app = open_app();
    let (x, y) = center(area(&app, Action::Mix));
    for _ in 0..3 {
        wheel(&mut app, x, y, 40);
    }
    assert_eq!(app.mix_to, 55., "three 40-unit events make one notch");
    wheel(&mut app, x, y, 40);
    assert_eq!(app.mix_to, 55., "a partial notch waits for more");
}

#[test]
fn double_click_returns_the_mix_to_even() {
    let mut app = open_app();
    let (x, y) = center(area(&app, Action::Mix));
    wheel(&mut app, x, y, 4 * 120);
    assert_ne!(app.mix_to, 50.);
    pointer_double_clicked(&mut app, x, y);
    assert_eq!(app.mix_to, 50.);
    assert_eq!(app.sound.mix, Some((50, 50)));
}

#[test]
fn controls_update_the_displayed_state_when_no_headset_is_attached() {
    let mut app = open_app();
    act(&mut app, Action::Side, 0., 0.);
    assert_eq!(app.sound.sidetone, Some(true));
    act(&mut app, Action::Anc(0), 0., 0.);
    assert_eq!(app.sound.anc.as_deref(), Some("On"));
    act(&mut app, Action::Noise(2), 0., 0.);
    assert_eq!(app.sound.noise.as_deref(), Some("High"));
    assert!(app.note.text().contains("headset unchanged"));
}

#[test]
fn the_level_wheel_only_works_while_sidetone_is_on() {
    let mut app = open_app();
    render(&mut app);
    let level = area(&app, Action::Level);
    let before = app.sound.sidetone_level;
    wheel(&mut app, level.x + 5., level.y + 4., 240);
    assert_eq!(app.sound.sidetone_level, before, "sidetone is off");
    act(&mut app, Action::Side, 0., 0.);
    render(&mut app);
    let level = area(&app, Action::Level);
    wheel(&mut app, level.x + 5., level.y + 4., 240);
    assert_eq!(app.sound.sidetone_level, Some(before.unwrap() + 6));
}

#[test]
fn typed_numbers_commit_on_enter_cancel_on_escape_and_reject_bad_values() {
    let mut app = open_app();
    act(&mut app, Action::Settings, 0., 0.);
    act(&mut app, Action::Number("hold"), 0., 0.);
    type_text(&mut app, "1200");
    assert!(key_pressed(&mut app, Key::Enter));
    assert_eq!(app.config.duration_ms, 1200);
    assert!(app.number_edit.is_none());

    act(&mut app, Action::Number("fade"), 0., 0.);
    let fade = app.config.fade_ms;
    type_text(&mut app, "900");
    assert!(key_pressed(&mut app, Key::Escape));
    assert!(app.number_edit.is_none());
    assert_eq!(app.config.fade_ms, fade, "Escape discards the draft");

    act(&mut app, Action::Number("hold"), 0., 0.);
    type_text(&mut app, "5");
    key_pressed(&mut app, Key::Enter);
    assert!(
        app.number_edit.is_some(),
        "a bad value keeps the draft open"
    );
    assert!(!app.note.is_empty(), "and says why");
    assert_eq!(app.config.duration_ms, 1200);
    focus_lost(&mut app);
    assert!(
        app.number_edit.is_none(),
        "losing focus abandons a bad draft"
    );
}

#[test]
fn eq_names_are_typed_and_committed_with_enter() {
    let mut app = open_app();
    act(&mut app, Action::EqEditor, 0., 0.);
    act(&mut app, Action::EqInspect(7), 0., 0.);
    act(&mut app, Action::EqRename, 0., 0.);
    assert!(app.eq_editor.name_edit.is_some());
    type_text(&mut app, "Night");
    assert!(key_pressed(&mut app, Key::Enter));
    assert_eq!(app.config.eq.name(7), "Night");
    assert!(!text_typed(&mut app, 'x'), "no draft is open any more");
}

#[test]
fn escape_ends_layout_editing_and_then_closes_the_panel() {
    let mut app = open_app();
    act(&mut app, Action::Edit, 0., 0.);
    assert!(app.edit);
    assert!(key_pressed(&mut app, Key::Escape));
    assert!(!app.edit);
    assert!(
        fake(&app).is_visible(),
        "the first Escape only ends editing"
    );
    assert!(key_pressed(&mut app, Key::Escape));
    assert!(!fake(&app).is_visible());
    assert!(
        !key_pressed(&mut app, Key::Other),
        "other keys are not ours"
    );
}

#[test]
fn pressing_decides_between_moving_resizing_and_acting() {
    let mut app = open_app();
    assert_eq!(region_at(&app, 210., 35.), Region::Caption);
    let (x, y) = center(area(&app, Action::Mix));
    assert_eq!(region_at(&app, x, y), Region::Client);

    act(&mut app, Action::Edit, 0., 0.);
    render(&mut app);
    let (w, h) = logical_size(&app);
    assert_eq!(region_at(&app, 2., 2.), Region::Edge(Edge::TopLeft));
    assert_eq!(
        region_at(&app, w - 2., h - 2.),
        Region::Edge(Edge::BottomRight)
    );
    assert_eq!(region_at(&app, 2., h / 2.), Region::Edge(Edge::Left));
    assert_eq!(region_at(&app, w / 2., h - 2.), Region::Edge(Edge::Bottom));

    app.config.locked = true;
    assert_eq!(
        region_at(&app, 2., 2.),
        Region::Client,
        "a locked window stays put"
    );
    assert_eq!(region_at(&app, 210., 35.), Region::Client);

    app.config.locked = false;
    app.interactive = false;
    assert_eq!(region_at(&app, 210., 35.), Region::Transparent);
}

#[test]
fn a_finished_move_snaps_to_nearby_edges_and_remembers_the_position() {
    let mut app = open_app();
    let r = fake(&app).rect.get();
    fake(&app).rect.set(Rect {
        left: 10,
        top: 15,
        right: 10 + r.width(),
        bottom: 15 + r.height(),
    });
    app.config.snap = true;
    move_or_resize_finished(&mut app);
    assert_eq!(*fake(&app).moves.borrow(), vec![(0, 0)]);
    assert_eq!(app.config.position, Some((0, 0)));

    fake(&app).rect.set(Rect {
        left: 400,
        top: 300,
        right: 400 + r.width(),
        bottom: 300 + r.height(),
    });
    app.config.snap = false;
    move_or_resize_finished(&mut app);
    assert_eq!(fake(&app).moves.borrow().len(), 1, "no snapping when off");
    assert_eq!(app.config.position, Some((400, 300)));

    app.config.locked = true;
    app.config.position = None;
    move_or_resize_finished(&mut app);
    assert_eq!(
        app.config.position, None,
        "a locked window is not remembered"
    );
}

#[test]
fn a_low_battery_alert_appears_once_until_the_battery_recovers() {
    let mut app = open_app();
    app.interactive = false;
    app.config.low_battery = 20;
    status(&mut app, |s| s.battery = Some(12));
    assert!(app.note.text().starts_with("Low headset battery: 12%"));
    app.note.clear();
    status(&mut app, |s| s.battery = Some(10));
    assert!(app.note.is_empty(), "no second alert while it stays low");
    status(&mut app, |s| s.battery = Some(80));
    status(&mut app, |s| s.battery = Some(15));
    assert!(app.note.text().starts_with("Low headset battery: 15%"));
}

#[test]
fn a_changed_reading_shows_a_passive_popup_that_the_hold_timer_ends() {
    let mut app = open_app();
    hide(&mut app);
    app.config.auto_show = true;
    status(&mut app, |s| s.mix = Some((30, 70)));
    assert!(fake(&app).is_visible());
    assert!(!app.interactive, "a popup never takes over the screen");
    assert_eq!(fake(&app).timer(Timer::Hold), Some(app.config.duration_ms));

    app.config.fade_ms = 0;
    timer_fired(&mut app, Timer::Hold);
    assert!(!fake(&app).is_visible());
    assert_eq!(fake(&app).timer(Timer::Hold), None);
}

#[test]
fn a_disconnect_clears_the_readings_and_stops_eq_work() {
    let mut app = open_app();
    status(&mut app, |s| {
        s.mix = None;
        s.battery = None;
    });
    assert_eq!(app.sound.mix, None);
    assert_eq!(app.sound.battery, None);
    let before = app.note.text().to_owned();
    act(&mut app, Action::Anc(0), 0., 0.);
    assert_ne!(app.note.text(), before, "controls say to connect a headset");
}

#[test]
fn menus_act_on_the_item_the_shell_reports() {
    let mut app = open_app();
    act(&mut app, Action::Edit, 0., 0.);
    fake(&app).menus.borrow_mut().push_back(Some(4));
    card_menu(&mut app, Some(3));
    assert!(
        !app.config.cards[3].visible,
        "item 4 is Remove card for card 3"
    );

    fake(&app).menus.borrow_mut().push_back(Some(4));
    card_menu(&mut app, None);
    assert!(app.config.cards[3].visible, "and the add menu restores it");

    fake(&app).menus.borrow_mut().push_back(None);
    let before = app.config.clone();
    card_menu(&mut app, Some(2));
    assert_eq!(
        app.config.cards, before.cards,
        "dismissing a menu changes nothing"
    );
}

#[test]
fn resetting_preferences_needs_a_confirmation() {
    let mut app = open_app();
    setting(&mut app, "text", 1);
    let changed = app.config.text_scale;
    act(&mut app, Action::ResetApp, 0., 0.);
    assert_eq!(
        app.config.text_scale, changed,
        "unanswered counts as cancelled"
    );

    fake(&app).confirms.borrow_mut().push_back(Choice::Ok);
    act(&mut app, Action::ResetApp, 0., 0.);
    assert_eq!(app.config.text_scale, Config::default().text_scale);
}

#[test]
fn leaving_an_unsaved_shortcut_asks_what_to_do() {
    let mut app = open_app();
    act(&mut app, Action::Record, 0., 0.);
    app.recorded_key = Some((2, 0x24));
    fake(&app).confirms.borrow_mut().push_back(Choice::Cancel);
    assert!(!confirm_recording(&mut app), "Cancel keeps recording");
    assert!(app.recording);
    fake(&app).confirms.borrow_mut().push_back(Choice::No);
    assert!(confirm_recording(&mut app));
    assert!(
        !app.recording && app.recorded_key.is_none(),
        "No discards it"
    );
}

#[test]
fn a_taken_shortcut_keeps_the_previous_one() {
    let mut app = open_app();
    act(&mut app, Action::Record, 0., 0.);
    app.recorded_key = Some((2, 0x41));
    fake(&app).hotkey_available.set(false);
    finish_recording(&mut app);
    assert!(app.recording, "recording continues so another can be tried");
    assert!(app.note.text().contains("already in use"));
    assert_eq!((app.config.hotkey_mod, app.config.hotkey_vk), (1, 0x24));

    fake(&app).hotkey_available.set(true);
    finish_recording(&mut app);
    assert!(!app.recording);
    assert_eq!((app.config.hotkey_mod, app.config.hotkey_vk), (2, 0x41));
}

#[test]
fn blur_and_autostart_changes_reach_the_shell() {
    let mut app = open_app();
    setting(&mut app, "blur", 0);
    let strength = fake(&app).blur.get();
    assert!(strength == Some(0) || strength == Some(50));
    assert_eq!(app.config.background_blur, strength == Some(50));

    fake(&app)
        .autostart
        .borrow_mut()
        .push_back(Err("Windows could not update startup.".into()));
    let before = app.config.autostart;
    setting(&mut app, "startup", 0);
    // A preview never registers anything with the system, so the shell is not
    // asked, whatever it would answer.
    assert_eq!(app.config.autostart, !before);
    assert_eq!(
        fake(&app).autostart.borrow().len(),
        1,
        "the shell was not asked"
    );
}

#[test]
fn the_animation_timer_runs_only_while_something_moves() {
    let mut app = open_app();
    assert_eq!(
        fake(&app).timer(Timer::Animation),
        Some(16),
        "opening the panel fades it in"
    );
    app.mix_started = None;
    app.motion_active = false;
    app.fade_in = None;
    app.fade_out = None;
    timer_fired(&mut app, Timer::Animation);
    assert_eq!(
        fake(&app).timer(Timer::Animation),
        None,
        "the timer stops itself once nothing is left to animate"
    );
}

#[test]
fn drawing_is_requested_not_done_inline() {
    let mut app = open_app();
    let before = fake(&app).redraws.get();
    request_draw(&mut app);
    request_draw(&mut app);
    assert_eq!(fake(&app).redraws.get(), before + 1, "requests coalesce");
    draw_requested(&mut app);
    assert!(!app.draw_queued);
    assert_eq!(
        fake(&app).alpha.get(),
        Some(app.alpha),
        "the stored frame was presented at the current opacity"
    );
}

#[test]
fn every_setting_can_be_stepped_both_ways_without_breaking_the_layout() {
    let mut app = open_app();
    act(&mut app, Action::Settings, 0., 0.);
    for key in [
        "scale",
        "text",
        "background",
        "opacity",
        "status-icons",
        "animations",
        "preserve-space",
        "changed-card-only",
        "automatic",
        "blur",
        "notify0",
        "notify4",
        "rows",
        "snap",
        "lock",
        "hold",
        "fade",
        "battery",
        "startup",
        "monitor",
        "reset-layout",
    ] {
        for n in [1, -1, 1] {
            setting(&mut app, key, n);
            let json = serde_json::to_vec(&app.config).unwrap();
            let restored: Config = serde_json::from_slice(&json)
                .unwrap_or_else(|e| panic!("{key} left an unreadable config: {e}"));
            assert_eq!(restored.cards, app.config.cards, "{key}");
        }
    }
    for (key, value) in [
        ("scale", 700.),
        ("text", 120.),
        ("background", 33.),
        ("opacity", 88.),
        ("blur", 80.),
        ("rows", 30.),
        ("hold", 4000.),
        ("fade", 400.),
        ("battery", 25.),
    ] {
        slider_value(&mut app, key, value);
    }
    assert_eq!(app.config.text_scale, 120);
    assert_eq!(app.config.blur_strength, 80);
    assert_eq!(app.config.duration_ms, 4000);
}

#[test]
fn deferred_file_selection_can_cancel_and_retry_without_replacing_the_request() {
    let mut app = open_app();
    fake(&app).deferred_file.set(true);
    fake(&app).menus.borrow_mut().push_back(Some(1));
    profile_menu(&mut app);
    assert!(app.pending_profile_file.is_some());
    fake(&app).menus.borrow_mut().push_back(Some(1));
    profile_menu(&mut app);
    assert!(app.note.text().contains("Finish or cancel"));
    profile_file_selected(&mut app, Ok(None));
    assert!(app.pending_profile_file.is_none());
    fake(&app).menus.borrow_mut().push_back(Some(1));
    profile_menu(&mut app);
    profile_file_selected(&mut app, Err("Portal unavailable".into()));
    assert!(app.pending_profile_file.is_none());
    assert!(app.note.text().contains("Portal unavailable"));
}

#[test]
fn deferred_export_keeps_the_tuning_selected_when_the_dialog_opened() {
    let mut app = open_app();
    fake(&app).deferred_file.set(true);
    let baseline = audeze_overlay::eq::sample(6);
    app.eq_editor.selected = 6;
    app.eq_editor.receive(baseline, 1);
    fake(&app).menus.borrow_mut().push_back(Some(2));
    profile_menu(&mut app);
    app.eq_editor.selected = 7;
    app.eq_editor.receive(audeze_overlay::eq::sample(7), 2);
    let path = std::env::temp_dir().join(format!(
        "audeze-deferred-export-{}.json",
        std::process::id()
    ));
    profile_file_selected(&mut app, Ok(Some(path.clone())));
    let saved = audeze_overlay::eq::profiles::read(&path, None).unwrap();
    assert_eq!(saved.snapshot(6).unwrap(), baseline);
    assert!(app.pending_profile_file.is_none());
    std::fs::remove_file(path).unwrap();
}
