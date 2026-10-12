//! Settings page composition and hit targets. Editing and persistence live in window/settings.
use super::*;
// A row explicitly describes its geometry, labels and control state.
#[allow(clippy::too_many_arguments)]
fn row(
    c: &mut Canvas,
    h: &mut Vec<Hit>,
    v: &View,
    y: f32,
    w: f32,
    title: &str,
    help: &str,
    key: &'static str,
    value: &str,
    on: Option<bool>,
) {
    let x = 218.;
    c.text(title, Box2::new(x, y, w - 470., 28.), 18., FG, true, false);
    let numeric = on.is_none() && key != "monitor";
    if numeric {
        c.wrapped_text(help, Box2::new(x, y + 29., w - 478., 38.), 14., SUB);
    } else {
        c.text(
            help,
            Box2::new(x, y + 30., w - x - 32., 25.),
            14.,
            SUB,
            false,
            false,
        );
    }
    if let Some(b) = on {
        switch(
            c,
            h,
            Box2::new(w - 92., y, 54., 28.),
            b,
            Action::Setting(key, 0),
        );
    } else if key == "monitor" {
        button(
            c,
            h,
            v,
            Box2::new(w - 220., y, 182., 30.),
            value,
            Action::Setting(key, 0),
            false,
        );
        c.line(
            &[(w - 62., y + 12.), (w - 57., y + 17.), (w - 52., y + 12.)],
            FG,
            1.,
            1.5,
        );
    } else {
        let (current, min, max) = slider(v.config, key);
        let fraction = ((current - min) / (max - min)).clamp(0., 1.);
        let editing = v.number_edit.filter(|draft| draft.key == key);
        let typed = editing.map(|draft| format!("{} |", draft.text));
        if matches!(key, "hold" | "fade") {
            c.rect(
                Box2::new(w - 220., y + 1., 182., 28.),
                if editing.is_some() {
                    0x454545
                } else {
                    0x242424
                },
                1.,
                5.,
            );
            h.push(Hit {
                rect: Box2::new(w - 220., y + 1., 182., 28.),
                action: Action::Number(key),
            });
        }
        c.text(
            typed.as_deref().unwrap_or(value),
            Box2::new(w - 220., y + 4., 182., 24.),
            16.,
            FG,
            true,
            true,
        );
        let (left, right, cy) = (w - 210., w - 48., y + 41.);
        c.line(&[(left, cy), (right, cy)], 0x525252, 1., 4.);
        c.line(
            &[(left, cy), (left + (right - left) * fraction, cy)],
            0xd5d5d5,
            1.,
            4.,
        );
        c.circle(left + (right - left) * fraction, cy, 7., FG, 1.);
        h.push(Hit {
            rect: Box2::new(w - 220., y + 25., 182., 32.),
            action: Action::Slider(key),
        });
    }
    let divider = y + if numeric { 69. } else { 50. };
    c.line(&[(x, divider), (w - 32., divider)], 0x303030, 1., 1.);
}
// A row explicitly describes its geometry, labels and control state.
#[allow(clippy::too_many_arguments)]
fn toggle_row(
    c: &mut Canvas,
    h: &mut Vec<Hit>,
    v: &View,
    y: f32,
    w: f32,
    title: &str,
    help: &str,
    key: &'static str,
    on: bool,
) {
    row(c, h, v, y, w, title, help, key, "", Some(on))
}
pub(super) fn settings_page(
    c: &mut Canvas,
    hits: &mut Vec<Hit>,
    v: &View,
    w: f32,
    h: f32,
    section: usize,
) {
    const TABS: [&str; 6] = [
        "Appearance",
        "Cards & layout",
        "Notifications",
        "Shortcuts",
        "Startup & device",
        "Equalizer",
    ];
    c.rect(
        Box2::new(208., 82., w - 224., h - 126.),
        0x161616,
        v.config.card_opacity as f32 / 100.,
        8.,
    );
    c.line(&[(198., 82.), (198., h - 30.)], 0x353535, 1., 1.);
    for (i, name) in TABS.iter().enumerate() {
        button(
            c,
            hits,
            v,
            Box2::new(16., 95. + i as f32 * 52., 166., 42.),
            name,
            Action::Tab(i),
            i == section,
        );
    }
    c.text(
        TABS[section],
        Box2::new(218., 88., w - 248., 42.),
        28.,
        FG,
        true,
        false,
    );
    let intro = match section {
        0 => "Change how the overlay looks. Text and icons always stay solid.",
        1 => "Use + to add cards, … to remove. Drag to move; corners resize.",
        2 => "Choose when the overlay appears while you play.",
        3 if !cfg!(windows) => "Show or hide the overlay from anywhere.",
        3 => "Record a key or a modifier + key, then save it.",
        4 => "The USB dongle connects directly. The Audeze app can stay closed.",
        5 => "Choose an EQ, pin quick switches, or tune a custom headset slot.",
        _ => "",
    };
    c.text(
        intro,
        Box2::new(218., 133., w - 250., 28.),
        15.,
        SUB,
        false,
        false,
    );
    match section {
        0 => {
            for (i, title, help, key, value) in [
                (
                    0,
                    "Interface scale",
                    "Scales cards together. Window edges only add or remove empty room.",
                    "scale",
                    format!("{}%", (v.config.cell_width * 1200. / 596.).round() as u32),
                ),
                (
                    1,
                    "Overlay text size",
                    "Larger labels and values inside cards.",
                    "text",
                    format!("{}%", v.config.text_scale),
                ),
                (
                    2,
                    "Outer background",
                    "0% makes the space around cards fully transparent.",
                    "background",
                    format!("{}%", v.config.background_opacity),
                ),
                (
                    3,
                    "Card opacity",
                    "How solid the dark cards are; labels remain fully visible.",
                    "opacity",
                    format!("{}%", v.config.card_opacity),
                ),
            ] {
                row(
                    c,
                    hits,
                    v,
                    185. + i as f32 * 80.,
                    w,
                    title,
                    help,
                    key,
                    &value,
                    None,
                );
            }
            row(
                c,
                hits,
                v,
                505.,
                w,
                "Backdrop blur strength",
                if cfg!(windows) {
                    "Adjusts the Windows acrylic effect. 0 turns it off."
                } else {
                    "Adjusts the backdrop blur. 0 turns it off."
                },
                "blur",
                &format!("{}%", v.config.blur_strength),
                None,
            );
            toggle_row(
                c,
                hits,
                v,
                585.,
                w,
                "Animate controls",
                "Smooths mix numbers, buttons and card moves. Stops when idle.",
                "animations",
                v.config.animations,
            );
        }
        1 => {
            toggle_row(
                c,
                hits,
                v,
                185.,
                w,
                "Keep empty space",
                "Preserve gaps between cards and room around the edges.",
                "preserve-space",
                v.config.preserve_space,
            );
            button(
                c,
                hits,
                v,
                Box2::new(218., 268., 172., 36.),
                "Edit card layout",
                Action::Edit,
                false,
            );
            button(
                c,
                hits,
                v,
                Box2::new(402., 268., 172., 36.),
                "Reset card layout",
                Action::Setting("reset-layout", 0),
                false,
            );
            c.wrapped_text("Add and remove cards in layout mode. Use a card's three-dot menu for its display options. Resizing a card moves neighbours and makes room automatically.", Box2::new(218., 330., w - 256., 100.), 16., SUB);
        }
        2 => {
            toggle_row(
                c,
                hits,
                v,
                174.,
                w,
                "Automatic popups",
                "Show headset changes while you play.",
                "automatic",
                v.config.auto_show,
            );
            toggle_row(
                c,
                hits,
                v,
                240.,
                w,
                "Only show changed cards",
                "Temporary popups show the affected controls. Manual opening shows all cards.",
                "changed-card-only",
                v.config.changed_card_only,
            );
            button(
                c,
                hits,
                v,
                Box2::new(218., 306., w - 256., 32.),
                if v.notification_options {
                    "Hide notification types  −"
                } else {
                    "Choose notification types  +"
                },
                Action::NotificationOptions,
                false,
            );
            if v.notification_options {
                for (i, title) in [
                    "Game / Chat Mix",
                    "Sidetone",
                    "Noise cancellation",
                    "AI noise removal",
                    "Microphone mute",
                ]
                .iter()
                .enumerate()
                {
                    let y = 350. + i as f32 * 36.;
                    c.text(
                        title,
                        Box2::new(230., y, w - 350., 28.),
                        16.,
                        FG,
                        false,
                        false,
                    );
                    switch(
                        c,
                        hits,
                        Box2::new(w - 92., y, 54., 24.),
                        v.config.notify[i],
                        Action::Setting(
                            ["notify0", "notify1", "notify2", "notify3", "notify4"][i],
                            0,
                        ),
                    );
                }
            }
            let y = if v.notification_options { 537. } else { 363. };
            row(
                c,
                hits,
                v,
                y,
                w,
                "Keep visible for",
                "Delay before the popup starts to fade (milliseconds).",
                "hold",
                &format!("{} ms", v.config.duration_ms),
                None,
            );
            row(
                c,
                hits,
                v,
                y + 76.,
                w,
                "Fade-out duration",
                "0 ms hides immediately. Manual controls stay open.",
                "fade",
                &format!("{} ms", v.config.fade_ms),
                None,
            );
            button(
                c,
                hits,
                v,
                Box2::new(
                    218.,
                    if v.notification_options { 691. } else { 527. },
                    170.,
                    26.,
                ),
                "Preview popup",
                Action::Setting("preview", 0),
                false,
            );
        }
        3 => {
            c.text(
                "Show / hide overlay",
                Box2::new(218., 190., w - 250., 28.),
                20.,
                FG,
                true,
                false,
            );
            c.text(
                "Opens the interactive controls. Press the same shortcut again to hide.",
                Box2::new(218., 224., w - 250., 30.),
                15.,
                SUB,
                false,
                false,
            );
            if cfg!(windows) {
                let binding = if let Some((mods, key)) = v.recorded_key.filter(|_| v.recording) {
                    audeze_overlay::config::hotkey_label(mods, key)
                } else {
                    v.config.hotkey_label()
                };
                button(
                    c,
                    hits,
                    v,
                    Box2::new(218., 272., w - 250., 50.),
                    if v.recording && v.recorded_key.is_none() {
                        "Press your shortcut…"
                    } else {
                        &binding
                    },
                    Action::Record,
                    false,
                );
                button(
                    c,
                    hits,
                    v,
                    Box2::new(218., 332., 190., 38.),
                    if v.recording {
                        "Save shortcut"
                    } else {
                        "Record shortcut"
                    },
                    if v.recording {
                        Action::EndRecord
                    } else {
                        Action::Record
                    },
                    false,
                );
                if v.recording {
                    button(
                        c,
                        hits,
                        v,
                        Box2::new(420., 332., 150., 38.),
                        "Discard",
                        Action::DiscardRecord,
                        false,
                    );
                }
                c.text(
                    "Examples: Home, Alt + Home, Ctrl + F9",
                    Box2::new(218., 388., w - 250., 28.),
                    16.,
                    FG,
                    false,
                    false,
                );
                c.text(
                    "A shortcut already used by another app will be rejected.",
                    Box2::new(218., 428., w - 250., 28.),
                    15.,
                    SUB,
                    false,
                    false,
                );
                c.text(
                    "Your previous shortcut stays active until a new one is accepted.",
                    Box2::new(218., 458., w - 250., 28.),
                    15.,
                    SUB,
                    false,
                    false,
                );
            } else {
                // The desktop owns global shortcuts here; it shows and changes them.
                for (top, line, size, color) in [
                    (272., "Your desktop manages this shortcut.", 18., FG),
                    (
                        306.,
                        "Change it in System Settings > Shortcuts > Audeze Overlay.",
                        15.,
                        SUB,
                    ),
                    (336., "Or bind the command below to any key.", 15., SUB),
                    (376., "audeze-overlay --toggle", 16., FG),
                ] {
                    c.text(
                        line,
                        Box2::new(218., top, w - 250., 28.),
                        size,
                        color,
                        false,
                        false,
                    );
                }
            }
            if cfg!(windows) {
                c.text(
                    "Escape cancels. Recording stays open until you save or discard.",
                    Box2::new(218., 508., w - 250., 28.),
                    15.,
                    SUB,
                    false,
                    false,
                );
            }
        }
        4 => {
            toggle_row(
                c,
                hits,
                v,
                178.,
                w,
                if cfg!(windows) {
                    "Launch with Windows"
                } else {
                    "Launch at login"
                },
                "Starts in the tray when you sign in. You can turn this off here.",
                "startup",
                v.config.autostart,
            );
            row(
                c,
                hits,
                v,
                252.,
                w,
                "Preferred monitor",
                "Auto uses your active screen. Reset position uses this choice.",
                "monitor",
                v.monitor_label,
                None,
            );
            toggle_row(
                c,
                hits,
                v,
                326.,
                w,
                "Snap to screen edges",
                "Gently aligns the window when you drop it near a screen edge.",
                "snap",
                v.config.snap,
            );
            toggle_row(
                c,
                hits,
                v,
                400.,
                w,
                "Lock position and layout",
                "Prevents moving or resizing the window and cards.",
                "lock",
                v.config.locked,
            );
            row(
                c,
                hits,
                v,
                474.,
                w,
                "Low-battery alert",
                "0% disables alerts. One popup each time the battery crosses the limit.",
                "battery",
                &format!("{}%", v.config.low_battery),
                None,
            );
            button(
                c,
                hits,
                v,
                Box2::new(218., 562., 172., 38.),
                "Reset position",
                Action::Setting("reset-position", 0),
                false,
            );
            button(
                c,
                hits,
                v,
                Box2::new(410., 562., 172., 38.),
                "RESET APP",
                Action::ResetApp,
                false,
            );
            c.text("Reset app restores all preferences, layout and shortcuts. Headset sound is unchanged.", Box2::new(218., 630., w - 250., 40.), 13., SUB, false, false);
            c.text(
                &format!(
                    "{} · {}{}",
                    v.sound.device.as_deref().unwrap_or("Headset disconnected"),
                    v.sound.connection.as_deref().unwrap_or("No connection"),
                    if v.sound.experimental_device {
                        " · Experimental"
                    } else {
                        ""
                    }
                ),
                Box2::new(218., 651., w - 250., 24.),
                15.,
                SUB,
                false,
                false,
            );
        }
        5 => super::equalizer::page(c, hits, v, w),
        _ => (),
    }
    let note = if v.note.is_empty() {
        if v.settings == Some(5) {
            ""
        } else if v.settings == Some(2) {
            "Click a time to type milliseconds. Enter saves; Escape cancels."
        } else {
            "Sliders: drag or scroll in steps of 5. Changes save automatically."
        }
    } else {
        v.note
    };
    c.text(
        note,
        Box2::new(218., h - 27., w - 250., 24.),
        14.,
        FG,
        false,
        false,
    );
}
