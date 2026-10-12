//! Turns application state into a frame and hands it to the shell.
use super::*;
/// The notification-area icon: the logo's symbol, without its wordmark, drawn at
/// `size` pixels square (the 32 pixel design scaled up for sharper high-density icons).
pub(in crate::ui) fn tray_icon_pixmap(painter: &Painter, size: u32) -> Option<tiny_skia::Pixmap> {
    let mut pix = tiny_skia::Pixmap::new(size, size)?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(245, 245, 245, 255);
    let k = size as f32 / 32.;
    // The wordmark SVG contains the symbol at x=0..29, y=0..24.
    // Draw those original vector paths only; no tiny wordmark in the tray.
    for path in painter.logo.iter().filter(|p| p.bounds().right() < 30.) {
        pix.fill_path(
            path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::from_translate(1.5, 4.).post_scale(k, k),
            None,
        );
    }
    Some(pix)
}
pub(in crate::ui) fn displayed_mix(app: &App) -> f32 {
    if let Some(t) = app.mix_started {
        let x = (t.elapsed().as_secs_f32() * 1000. / MIX_ANIMATION_MS as f32).min(1.);
        app.mix_from + (app.mix_to - app.mix_from) * (1. - (1. - x).powi(3))
    } else {
        app.mix_to
    }
}
pub(in crate::ui) fn render(app: &mut App) {
    render_frame(app, false);
}
pub(in crate::ui) fn render_frame(app: &mut App, prepare_hidden: bool) {
    app.draw_queued = false;
    if !app.shell.is_visible() && !app.capture && !prepare_hidden {
        return;
    }
    let Some((client_width, client_height)) = app.shell.client_size() else {
        return;
    };
    let (w, h) = (client_width.max(1) as u32, client_height.max(1) as u32);
    if w > 8192 || h > 8192 {
        return;
    }
    app.last_frame = Some(Instant::now());
    let mut scale = app.shell.scale();
    if app.settings.is_some() {
        scale = (w as f32 / 900.).min(h as f32 / 750.);
    }
    let label = app
        .monitors
        .iter()
        .find(|m| m.key == app.config.monitor)
        .map(|m| m.label.as_str())
        .unwrap_or("Auto");
    let mix = displayed_mix(app);
    let popup_config = app
        .popup_cards
        .map(|mask| app.config.notification_layout(mask));
    let config = popup_config.as_ref().unwrap_or(&app.config);
    let animation = app.presentation.frame(
        dashboard::card_boxes(config, w as f32 / scale, h as f32 / scale),
        &app.sound,
        matches!(app.drag, Drag::Mix),
        app.config.animations
            && app.settings.is_none()
            && !app.capture
            && !app.smoke
            && !prepare_hidden
            && app.shell.is_visible(),
        Instant::now(),
    );
    app.motion_active = animation.active;
    let view = View {
        eq_editor: Some(&app.eq_editor),
        config,
        sound: &app.sound,
        settings: app.settings,
        edit: app.edit,
        hover: app.hover,
        note: app.note.text(),
        recording: app.recording,
        recorded_key: app.recorded_key,
        notification_options: app.notification_options,
        popup_cards: app.popup_cards,
        number_edit: app.number_edit.as_ref(),
        mix,
        demo: app.demo,
        monitor_label: label,
        drag_preview: app.drag_preview,
        animation: Some(&animation),
    };
    let scene = if let Some(raster) = app.raster.take() {
        dashboard::render_reusing(&mut app.painter, w, h, scale, &view, Some(raster))
    } else {
        dashboard::render(&mut app.painter, w, h, scale, &view)
    };
    app.hits = scene.hits;
    app.cards = scene.cards;
    if app.motion_active {
        start_animation_timer(app);
    }
    if app.capture {
        let _ = std::fs::create_dir_all("diagnostics");
        let name = if let Some(section) = app.settings {
            format!("diagnostics/settings-v3-{section}.png")
        } else {
            "diagnostics/overlay-v3.png".into()
        };
        let _ = scene.pix.save_png(name);
        if std::env::args().any(|s| s == "--render-only") {
            app.shell.quit_later();
        }
    }
    if !app.shell.store_frame(&scene.pix) {
        return;
    }
    app.raster = Some(scene.pix);
    app.shell.present(app.alpha);
    if let Some(delivery) = app.telemetry_at {
        device::trace_present(delivery, app.alpha, app.sound.mix.map(|m| m.1));
        if app.alpha >= 230 {
            app.telemetry_at = None;
        }
    }
}
pub(in crate::ui) fn clip_window(app: &mut App) {
    let Some((w, h)) = app.shell.client_size() else {
        return;
    };
    if w <= 0 || h <= 0 {
        return;
    }
    let scale = if app.settings.is_some() {
        (w as f32 / 900.).min(h as f32 / 750.)
    } else {
        app.shell.scale()
    };
    app.shell.clip_rounded((16. * scale).round() as i32);
}
