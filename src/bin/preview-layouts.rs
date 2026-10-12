//! Offline rendering diagnostics. Does not create a window or connect to hardware.
#![allow(dead_code)]
#[path = "../ui/view/mod.rs"]
mod dashboard;
#[path = "../ui/draw.rs"]
mod draw;
#[path = "../ui/number_roll.rs"]
mod number_roll;
#[path = "../ui/presentation.rs"]
mod presentation;

use audeze_overlay::{config::Config, layout, model::SoundState};
use dashboard::{Action, View};
use draw::Painter;

fn main() {
    std::fs::create_dir_all("diagnostics").expect("diagnostics directory");
    let mut painter = Painter::try_new().expect("A usable font is required for rendering previews");
    showcase();
    let sound = SoundState {
        mix_revision: 0,
        eq_index: Some(0),
        eq_bands: None,
        eq_revision: 0,
        eq_index_revision: 0,
        mix: Some((50, 50)),
        sidetone: Some(false),
        sidetone_level: Some(26),
        anc: Some("Transparency".into()),
        noise: Some("Off".into()),
        battery: Some(69),
        muted: Some(true),
        device: Some("Maxwell 2 ANC".into()),
        connection: Some("USB Dongle".into()),
        status: String::new(),
        family: Some(audeze_overlay::model::HeadsetFamily::Maxwell2Anc),
        transport: Some(audeze_overlay::model::Connection::WirelessDongle),
        capabilities: Some(audeze_overlay::model::Capabilities {
            anc: true,
            eq_edit: true,
        }),
        ..Default::default()
    };
    let base = Config {
        width: 520,
        height: 500,
        cell_width: 496. / 12.,
        cell_height: 394. / 15.,
        ..Config::default()
    };
    let mut tall = base.clone();
    tall.height = 723;
    tall.cell_height = 617. / 15.;
    let mut swapped = base.clone();
    let proposed = layout::Card {
        x: 6,
        ..swapped.cards[0]
    };
    swapped.cards = layout::preview_move(&base.cards, 0, proposed, (9., 2.), 15)
        .expect("mix swaps into the small card column");
    let mut narrow = base.clone();
    narrow.cards[2].w = 5;
    let mut narrow_mix = swapped.clone();
    narrow_mix.cards[0].w = 5;
    let mut minimum = Config {
        width: 440,
        height: 440,
        cell_width: 40.,
        cell_height: 26.,
        status_icons: true,
        ..Config::default()
    };
    minimum.cards[0] = layout::Card {
        x: 0,
        y: 0,
        w: 5,
        h: 8,
        visible: true,
    };
    for i in 1..6 {
        minimum.cards[i] = layout::Card {
            x: 5,
            y: (i as i32 - 1) * 3,
            w: 5,
            h: 3,
            visible: true,
        };
    }
    minimum.cards[4].x = 0;
    minimum.cards[4].y = 8;
    minimum.cards[5].y = 9;
    minimum.resize_canvas(440, 440);
    let mut column = minimum.clone();
    let mut y = 0;
    for card in &mut column.cards {
        card.x = 0;
        card.y = y;
        y += card.h;
    }
    column.trim_canvas();
    let mut large_text = minimum.clone();
    large_text.text_scale = 140;
    for (name, config) in [
        ("single-column-status", column),
        ("minimum-cards", minimum),
        ("minimum-large-text", large_text),
        ("compact", base),
        ("compact-tall", tall),
        ("swapped", swapped),
        ("compact-five-columns", narrow),
        ("swapped-five-columns", narrow_mix),
    ] {
        let view = View {
            eq_editor: None,
            config: &config,
            sound: &sound,
            settings: None,
            edit: false,
            hover: Action::None,
            note: "",
            recording: false,
            recorded_key: None,
            notification_options: false,
            popup_cards: None,
            number_edit: None,
            mix: 50.,
            demo: false,
            monitor_label: "Automatic",
            drag_preview: None,
            animation: None,
        };
        let scene = dashboard::render(&mut painter, config.width, config.height, 1., &view);
        let buttons: Vec<_> = scene
            .hits
            .iter()
            .filter(|h| matches!(h.action, Action::Anc(_)))
            .collect();
        assert_eq!(buttons.len(), 3, "{name}: every ANC mode has a hit target");
        let card = scene.cards[2];
        let padding = if card.h < 110. { 7.9 } else { 15.9 };
        for (i, hit) in buttons.iter().enumerate() {
            let rect = hit.rect;
            assert!(
                rect.x >= card.x + padding && rect.y >= card.y + 30.,
                "{name}: button inside card"
            );
            assert!(
                rect.x + rect.w <= card.x + card.w - padding,
                "{name}: button fits width"
            );
            assert!(
                rect.y + rect.h <= card.y + card.h - padding,
                "{name}: button fits height"
            );
            for other in &buttons[i + 1..] {
                assert!(
                    rect.x + rect.w <= other.rect.x
                        || other.rect.x + other.rect.w <= rect.x
                        || rect.y + rect.h <= other.rect.y
                        || other.rect.y + other.rect.h <= rect.y,
                    "{name}: ANC buttons do not overlap"
                );
            }
        }
        let path = format!("diagnostics/layout-{name}.png");
        scene.pix.save_png(&path).expect("save preview PNG");
        println!("{path}: ANC bounds and nonoverlap verified");
    }
    let config = Config::default();
    for (name, transport, cable, waiting) in [
        (
            "wireless-charging",
            audeze_overlay::model::Connection::WirelessDongle,
            true,
            false,
        ),
        (
            "wired-power",
            audeze_overlay::model::Connection::WiredUsb,
            true,
            false,
        ),
        (
            "bluetooth-status",
            audeze_overlay::model::Connection::Bluetooth,
            false,
            false,
        ),
        (
            "controls-unavailable",
            audeze_overlay::model::Connection::WirelessDongle,
            false,
            true,
        ),
    ] {
        let status_config = Config {
            status_icons: true,
            ..Config::default()
        };
        let mut state = sound.clone();
        state.transport = Some(transport);
        state.usb_cable = cable;
        if transport == audeze_overlay::model::Connection::Bluetooth {
            state.mix = None;
            state.battery = None;
            state.muted = None;
        }
        if waiting {
            state = state.waiting_for_controls();
        }
        let view = View {
            config: &status_config,
            sound: &state,
            eq_editor: None,
            settings: None,
            edit: false,
            hover: Action::None,
            note: "",
            recording: false,
            notification_options: false,
            popup_cards: None,
            recorded_key: None,
            number_edit: None,
            mix: 50.,
            demo: false,
            monitor_label: "Automatic",
            drag_preview: None,
            animation: None,
        };
        dashboard::render(
            &mut painter,
            status_config.width,
            status_config.height,
            1.,
            &view,
        )
        .pix
        .save_png(format!("diagnostics/layout-{name}.png"))
        .unwrap();
    }
    let mut eq_editor = audeze_overlay::eq::Editor {
        selected: 6,
        ..Default::default()
    };
    eq_editor.receive(audeze_overlay::eq::sample(6), 0);
    let mut minimal = Config {
        status_icons: true,
        ..Config::default()
    };
    for card in &mut minimal.cards {
        card.visible = false;
    }
    minimal.cards[0] = layout::Card {
        x: 0,
        y: 0,
        w: 5,
        h: 3,
        visible: true,
    };
    minimal.minimal_cards[0] = true;
    minimal.trim_canvas();
    let mut inline = Config {
        inline_eq: true,
        ..Config::default()
    };
    for card in &mut inline.cards {
        card.visible = false;
    }
    inline.cards[5] = layout::Card {
        x: 0,
        y: 0,
        w: 7,
        h: 10,
        visible: true,
    };
    inline.trim_canvas();
    let popup = config.notification_layout(1);
    let mut inline_sound = sound.clone();
    inline_sound.eq_index = Some(6);
    for (name, config) in [
        ("minimal", minimal),
        ("inline-eq", inline),
        ("changed-popup", popup),
    ] {
        for scale in [1., 1.5, 2.] {
            let view = View {
                config: &config,
                sound: &inline_sound,
                eq_editor: Some(&eq_editor),
                settings: None,
                edit: false,
                hover: Action::None,
                note: "",
                recording: false,
                recorded_key: None,
                notification_options: false,
                popup_cards: config.popup.then_some(1),
                number_edit: None,
                mix: 50.,
                demo: false,
                monitor_label: "Auto",
                drag_preview: None,
                animation: None,
            };
            let scene = dashboard::render(
                &mut painter,
                (config.width as f32 * scale) as u32,
                (config.height as f32 * scale) as u32,
                scale,
                &view,
            );
            if name == "inline-eq" {
                assert_eq!(
                    scene
                        .hits
                        .iter()
                        .filter(|h| matches!(h.action, Action::EqBand(_)))
                        .count(),
                    10
                );
            }
            if config.popup {
                assert!(scene.hits.is_empty());
            }
            scene
                .pix
                .save_png(format!("diagnostics/layout-{name}-{scale}.png"))
                .unwrap();
        }
    }
    let draft = audeze_overlay::input::NumberDraft::new("hold", 797);
    for (name, section) in [
        ("equalizer", Some(5)),
        ("cards-settings", Some(1)),
        ("editor", None),
        ("notifications", Some(2)),
        ("shortcuts", Some(3)),
        ("reset", Some(4)),
    ] {
        let view = View {
            eq_editor: Some(&eq_editor),
            config: &config,
            sound: &sound,
            settings: section,
            edit: section.is_none(),
            hover: Action::None,
            note: "",
            recording: section == Some(3),
            recorded_key: Some((2, 0x24)),
            notification_options: true,
            popup_cards: None,
            number_edit: if section == Some(2) {
                Some(&draft)
            } else {
                None
            },
            mix: 50.,
            demo: false,
            monitor_label: "Automatic",
            drag_preview: None,
            animation: None,
        };
        let (width, height) = if section.is_some() {
            (900, 750)
        } else {
            (config.width, config.height)
        };
        dashboard::render(&mut painter, width, height, 1., &view)
            .pix
            .save_png(format!("diagnostics/layout-{name}.png"))
            .unwrap();
    }
    let now = std::time::Instant::now();
    for (name, from, to) in [
        ("motion", (50, 50), (20, 80)),
        ("roll-step", (20, 80), (25, 75)),
        ("roll-carry", (95, 5), (100, 0)),
        ("roll-down", (30, 70), (25, 75)),
    ] {
        let mut motion = presentation::Presentation::default();
        let mut before = sound.clone();
        before.mix = Some(from);
        before.anc = Some("On".into());
        let boxes = dashboard::card_boxes(&config, config.width as f32, config.height as f32);
        motion.frame(boxes, &before, false, true, now);
        let mut after = before.clone();
        after.mix = Some(to);
        after.anc = Some("Transparency".into());
        for millis in [0, 16, 32, 45, 64, 90, 160, 300] {
            let frame = motion.frame(
                boxes,
                &after,
                true,
                true,
                now + std::time::Duration::from_millis(millis),
            );
            let view = View {
                eq_editor: None,
                config: &config,
                sound: &after,
                settings: None,
                edit: false,
                hover: Action::Mix,
                note: "",
                recording: false,
                recorded_key: None,
                notification_options: false,
                popup_cards: None,
                number_edit: None,
                mix: to.1 as f32,
                demo: false,
                monitor_label: "Automatic",
                drag_preview: None,
                animation: Some(&frame),
            };
            dashboard::render(&mut painter, config.width, config.height, 1., &view)
                .pix
                .save_png(format!("diagnostics/layout-{name}-{millis}.png"))
                .unwrap();
            if millis == 300 {
                assert!(!frame.active, "Animations must stop when idle");
            }
        }
    }
}

/// Reproducible documentation views, using public fonts and sample readings.
/// The neutral backdrop exposes opacity differences; Windows owns live blur.
fn showcase() {
    use tiny_skia::{
        Color, GradientStop, LinearGradient, Paint, Pixmap, Point, SpreadMode, Transform,
    };

    // The published screenshots use Segoe UI so they can be reproduced exactly.
    // Without it the views would silently change font, so they are skipped.
    let segoe = ["segoeui.ttf", "seguisb.ttf"]
        .map(|name| std::fs::read(format!(r"C:\Windows\Fonts\{name}")).ok());
    let [Some(regular), Some(semibold)] = segoe else {
        eprintln!("Skipping the documentation showcase: it needs Segoe UI (Windows).");
        return;
    };
    let mut painter = Painter::try_new().expect("A usable font is required for rendering previews");
    painter.fonts = [regular, semibold].map(|bytes| {
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .expect("valid public font")
    });
    let mut vertical = Config {
        background_opacity: 50,
        card_opacity: 10,
        blur_strength: 50,
        status_icons: true,
        cell_width: 596. / 12.,
        cell_height: 28.,
        ..Config::default()
    };
    vertical.eq.names[0] = "WickedCushions".into();
    let mut y = 0;
    for (index, height) in [(0, 12), (2, 4), (5, 4), (1, 4), (3, 4), (4, 3)] {
        vertical.cards[index] = layout::Card {
            x: 0,
            y,
            w: 5,
            h: height,
            visible: true,
        };
        y += height;
    }
    vertical.trim_canvas();
    let mut stacked = Config {
        status_icons: true,
        ..Config::default()
    };
    stacked.trim_canvas();
    let sound = SoundState {
        mix: Some((20, 80)),
        sidetone: Some(false),
        sidetone_level: Some(35),
        anc: Some("On".into()),
        noise: Some("Off".into()),
        battery: Some(86),
        muted: Some(true),
        device: Some("Maxwell 2 ANC".into()),
        connection: Some("Wireless dongle".into()),
        family: Some(audeze_overlay::model::HeadsetFamily::Maxwell2Anc),
        transport: Some(audeze_overlay::model::Connection::WirelessDongle),
        capabilities: Some(audeze_overlay::model::Capabilities {
            anc: true,
            eq_edit: true,
        }),
        eq_index: Some(6),
        ..Default::default()
    };
    let mut editor = audeze_overlay::eq::Editor {
        selected: 6,
        ..Default::default()
    };
    editor.receive(audeze_overlay::eq::sample(6), 0);
    for (name, config, settings, edit) in [
        ("vertical", vertical, None, false),
        ("stacked", stacked.clone(), None, false),
        ("layout", stacked.clone(), None, true),
        ("appearance", stacked.clone(), Some(0), false),
        ("equalizer", stacked, Some(5), false),
    ] {
        let view = View {
            config: &config,
            sound: &sound,
            eq_editor: Some(&editor),
            settings,
            edit,
            hover: Action::None,
            note: "",
            recording: false,
            notification_options: false,
            popup_cards: None,
            recorded_key: None,
            number_edit: None,
            mix: 80.,
            demo: false,
            monitor_label: "Automatic",
            drag_preview: None,
            animation: None,
        };
        let scale = 2.;
        let (width, height) = if settings.is_some() {
            (900, 750)
        } else {
            (config.width, config.height)
        };
        let scene = dashboard::render(&mut painter, width * 2, height * 2, scale, &view);
        let mut backdrop =
            Pixmap::new(scene.pix.width(), scene.pix.height()).expect("showcase dimensions");
        let paint = Paint {
            shader: LinearGradient::new(
                Point::from_xy(0., 0.),
                Point::from_xy(backdrop.width() as f32, backdrop.height() as f32),
                vec![
                    GradientStop::new(0., Color::from_rgba8(86, 110, 123, 255)),
                    GradientStop::new(0.5, Color::from_rgba8(49, 64, 74, 255)),
                    GradientStop::new(1., Color::from_rgba8(91, 79, 100, 255)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
            .expect("nonzero gradient"),
            ..Paint::default()
        };
        backdrop.fill_rect(
            tiny_skia::Rect::from_xywh(0., 0., backdrop.width() as f32, backdrop.height() as f32)
                .unwrap(),
            &paint,
            Transform::identity(),
            None,
        );
        backdrop.draw_pixmap(
            0,
            0,
            scene.pix.as_ref(),
            &Default::default(),
            Transform::identity(),
            None,
        );
        backdrop
            .save_png(format!("diagnostics/layout-showcase-{name}.png"))
            .expect("save showcase");
    }
}
