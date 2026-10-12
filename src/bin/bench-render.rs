//! Offline knob-animation raster benchmark. No window, USB, or user settings.
#![allow(dead_code)]
#[path = "../ui/view/mod.rs"]
mod dashboard;
#[path = "../ui/draw.rs"]
mod draw;
#[path = "../ui/number_roll.rs"]
mod number_roll;
#[path = "../ui/presentation.rs"]
mod presentation;

use audeze_overlay::{config::Config, model::SoundState};
use std::time::{Duration, Instant};

fn main() {
    let config = Config::default();
    let mut painter =
        draw::Painter::try_new().expect("A usable font is required for the benchmark");
    let mut motion = presentation::Presentation::default();
    let mut sound = SoundState::default();
    let start = Instant::now();
    let mut times = Vec::new();
    let mut raster = None;
    for i in 0..660 {
        let chat = ((i / 3 * 5) % 200) as u8;
        let chat = if chat > 100 { 200 - chat } else { chat };
        sound.mix = Some((100 - chat, chat));
        let animation = motion.frame(
            dashboard::card_boxes(&config, 620., 500.),
            &sound,
            false,
            true,
            start + Duration::from_millis(i * 16),
        );
        let view = dashboard::View {
            config: &config,
            sound: &sound,
            eq_editor: None,
            settings: None,
            edit: false,
            hover: dashboard::Action::None,
            note: "",
            recording: false,
            recorded_key: None,
            notification_options: false,
            popup_cards: None,
            number_edit: None,
            mix: chat as f32,
            demo: false,
            monitor_label: "Auto",
            drag_preview: None,
            animation: Some(&animation),
        };
        let frame_start = Instant::now();
        let scene = std::hint::black_box(dashboard::render_reusing(
            &mut painter,
            620,
            500,
            1.,
            &view,
            raster.take(),
        ));
        if i >= 60 {
            times.push(frame_start.elapsed().as_secs_f64() * 1000.);
        }
        // Every warm-up frame compares reused pixels with a fresh render;
        // clearing the retained surface must also remove previous glyphs.
        if i < 60 {
            let fresh = dashboard::render(&mut painter, 620, 500, 1., &view);
            assert_eq!(scene.pix.data(), fresh.pix.data());
        }
        raster = Some(scene.pix);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "frames={} mean_ms={:.3} p95_ms={:.3}",
        times.len(),
        times.iter().sum::<f64>() / times.len() as f64,
        times[times.len() * 95 / 100]
    );
}
