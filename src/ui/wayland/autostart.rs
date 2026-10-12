//! Starting with the desktop session. Outside a sandbox that is an XDG autostart
//! entry in the user's config directory; inside a Flatpak the app cannot write
//! there, so the Background portal creates the entry on its behalf.
use super::portal::{self, APP_ID};
use ashpd::desktop::background::Background;
use std::path::{Path, PathBuf};

pub(super) fn set(enabled: bool) -> Result<(), String> {
    if Path::new("/.flatpak-info").exists() {
        return set_with_portal(enabled);
    }
    let dir = autostart_dir().ok_or("The autostart folder could not be found.")?;
    let exe = std::env::current_exe().map_err(|e| format!("Startup could not be set: {e}"))?;
    set_in(&dir, &exe, enabled).map_err(|e| format!("Startup could not be set: {e}"))
}

fn set_with_portal(enabled: bool) -> Result<(), String> {
    async_io::block_on(async {
        portal::register_app().await;
        let response = Background::request()
            .reason("Show the headset overlay when you sign in")
            .auto_start(enabled)
            .send()
            .await
            .and_then(|request| request.response())
            .map_err(|e| format!("Startup could not be set: {e}"))?;
        if response.auto_start() == enabled {
            Ok(())
        } else {
            Err("The desktop did not allow starting with the session.".into())
        }
    })
}

fn autostart_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart"))
}

fn set_in(dir: &Path, exe: &Path, enabled: bool) -> std::io::Result<()> {
    let entry = dir.join(format!("{APP_ID}.desktop"));
    if !enabled {
        return match std::fs::remove_file(&entry) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(entry, entry_text(exe))
}

/// The desktop-entry spec reserves backslash, quote, backtick and `$` inside a
/// quoted `Exec` argument, and `%` starts a field code.
fn entry_text(exe: &Path) -> String {
    let mut exec = String::from("\"");
    for c in exe.to_string_lossy().chars() {
        match c {
            '\\' => exec.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                exec.push_str("\\\\");
                exec.push(c);
            }
            '%' => exec.push_str("%%"),
            c => exec.push(c),
        }
    }
    exec.push('"');
    format!(
        "[Desktop Entry]\nType=Application\nName=Audeze Overlay\nExec={exec}\nIcon={APP_ID}\nTerminal=false\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_quotes_the_executable_path() {
        let text = entry_text(Path::new("/opt/My $Apps/audeze-overlay"));
        assert!(text.contains("Exec=\"/opt/My \\\\$Apps/audeze-overlay\"\n"));
    }

    #[test]
    fn enabling_writes_and_disabling_removes_the_entry() {
        let dir = std::env::temp_dir().join(format!("audeze-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let entry = dir.join("autostart").join(format!("{APP_ID}.desktop"));
        set_in(
            &dir.join("autostart"),
            Path::new("/usr/bin/audeze-overlay"),
            true,
        )
        .unwrap();
        assert!(std::fs::read_to_string(&entry)
            .unwrap()
            .contains("Exec=\"/usr/bin/audeze-overlay\""));
        set_in(
            &dir.join("autostart"),
            Path::new("/usr/bin/audeze-overlay"),
            false,
        )
        .unwrap();
        assert!(!entry.exists());
        // Disabling when nothing is registered is not an error.
        set_in(&dir.join("autostart"), Path::new("/x"), false).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
