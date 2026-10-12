//! The global shortcut, through the GlobalShortcuts portal. A Wayland client
//! cannot grab keys itself, so it asks the desktop to bind one and is told when
//! it fires. The desktop owns the binding: the user can change it in the
//! system's shortcut settings, and KDE Plasma, GNOME and Hyprland offer it.
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_lite::StreamExt;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::time::Duration;

const ID: &str = "toggle";

/// Binds the shortcut on its own thread and reports each press to the main loop.
/// Returns whether the desktop accepted it. A slow answer (for example a
/// permission prompt the user has not dealt with yet) counts as accepted, since
/// the binding still takes effect once granted.
pub(super) fn start(trigger: Option<String>, pressed: Sender<()>) -> bool {
    let (answer, answered) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("shortcut".into())
        .spawn(move || {
            async_io::block_on(async move {
                match bind(trigger).await {
                    Ok((shortcuts, session)) => {
                        let _ = answer.send(true);
                        let Ok(mut activations) = shortcuts.receive_activated().await else {
                            return;
                        };
                        while let Some(activation) = activations.next().await {
                            if activation.shortcut_id() == ID {
                                let _ = pressed.send(());
                            }
                        }
                        // Keep the session open for as long as the stream runs.
                        drop(session);
                    }
                    Err(error) => {
                        eprintln!("Global shortcut unavailable: {error:?}");
                        let _ = answer.send(false);
                    }
                }
            })
        });
    if spawned.is_err() {
        return false;
    }
    answered
        .recv_timeout(Duration::from_secs(3))
        .unwrap_or(true)
}

async fn bind(
    trigger: Option<String>,
) -> ashpd::Result<(GlobalShortcuts, ashpd::desktop::Session<GlobalShortcuts>)> {
    super::portal::register_app().await;
    let shortcuts = GlobalShortcuts::new().await?;
    let session = shortcuts.create_session(Default::default()).await?;
    // Plasma 6.7.4 and later only listen after a bind in the current session, so
    // this runs on every start, even when the desktop remembers the binding.
    let wanted = [
        NewShortcut::new(ID, "Show or hide Audeze Overlay").preferred_trigger(trigger.as_deref())
    ];
    shortcuts
        .bind_shortcuts(&session, &wanted, None, Default::default())
        .await?
        .response()?;
    Ok((shortcuts, session))
}

/// The portal's trigger text for a shortcut stored as Alt/Ctrl/Shift bits and a
/// Windows virtual-key code, or `None` for a key this does not know.
pub(super) fn trigger_for(modifiers: u32, virtual_key: u32) -> Option<String> {
    let key = match virtual_key {
        0x08 => "BackSpace".to_owned(),
        0x09 => "Tab".to_owned(),
        0x20 => "space".to_owned(),
        0x21 => "Prior".to_owned(),
        0x22 => "Next".to_owned(),
        0x23 => "End".to_owned(),
        0x24 => "Home".to_owned(),
        0x25 => "Left".to_owned(),
        0x26 => "Up".to_owned(),
        0x27 => "Right".to_owned(),
        0x28 => "Down".to_owned(),
        0x2D => "Insert".to_owned(),
        0x2E => "Delete".to_owned(),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(virtual_key)?
            .to_ascii_lowercase()
            .to_string(),
        0x70..=0x87 => format!("F{}", virtual_key - 0x6F),
        _ => return None,
    };
    let mut parts = Vec::new();
    if modifiers & 2 != 0 {
        parts.push("CTRL");
    }
    if modifiers & 1 != 0 {
        parts.push("ALT");
    }
    if modifiers & 4 != 0 {
        parts.push("SHIFT");
    }
    parts.push(&key);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_alt_home_becomes_a_portal_trigger() {
        assert_eq!(trigger_for(1, 0x24).as_deref(), Some("ALT+Home"));
    }

    #[test]
    fn modifiers_and_key_names_follow_the_shortcuts_specification() {
        assert_eq!(trigger_for(2 | 4, 0x41).as_deref(), Some("CTRL+SHIFT+a"));
        assert_eq!(trigger_for(0, 0x7B).as_deref(), Some("F12"));
        assert_eq!(trigger_for(1 | 2, 0x35).as_deref(), Some("CTRL+ALT+5"));
        assert_eq!(trigger_for(0, 0x21).as_deref(), Some("Prior"));
    }

    #[test]
    fn keys_without_a_known_name_leave_the_choice_to_the_desktop() {
        assert_eq!(trigger_for(1, 0xE5), None);
    }
}
