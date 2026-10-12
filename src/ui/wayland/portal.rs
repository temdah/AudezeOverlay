//! Desktop portals: the file chooser and, below, the global shortcut. A portal
//! is the sandbox-friendly way to ask the desktop for something, so the same
//! code works on KDE, GNOME and others, inside a Flatpak or not.
use ashpd::desktop::{
    file_chooser::{FileFilter, SelectedFiles},
    ResponseError,
};
use std::path::PathBuf;

/// The application's identity for the desktop: its `.desktop` file, its Flatpak
/// id and the name portals know it by.
pub(super) const APP_ID: &str = "io.github.temdah.AudezeOverlay";

/// Tells the portal which application this process is. A host (non-Flatpak)
/// program has no identity the portal can see, and some portals, such as
/// GlobalShortcuts, refuse to work without one. Inside a Flatpak this does nothing.
pub(super) async fn register_app() {
    if let Ok(id) = ashpd::AppID::try_from(APP_ID) {
        // It can only be registered once per connection; later attempts are harmless.
        let _ = ashpd::register_host_app(id).await;
    }
}

/// Asks the desktop for a profile file to open or to write. `Ok(None)` means the
/// user cancelled. Returns once the dialog has been answered.
pub(super) fn choose_file(save: bool) -> Result<Option<PathBuf>, String> {
    async_io::block_on(async {
        register_app().await;
        let response = if save {
            SelectedFiles::save_file()
                .title("Export EQ profile")
                .accept_label("Export")
                .current_name("My EQ.audeze-eq.json")
                .modal(true)
                .filter(FileFilter::new("Audeze Overlay EQ (*.json)").glob("*.json"))
                .send()
                .await
        } else {
            SelectedFiles::open_file()
                .title("Import EQ profile")
                .accept_label("Import")
                .modal(true)
                .filter(
                    FileFilter::new("EQ profiles (*.json, *.pnreq)")
                        .glob("*.json")
                        .glob("*.pnreq"),
                )
                .send()
                .await
        };
        match response.and_then(|request| request.response()) {
            Ok(files) => Ok(files
                .uris()
                .first()
                .and_then(|uri| path_from_file_uri(uri.as_str()))),
            Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(None),
            Err(error) => Err(format!("The file dialog could not be shown: {error}")),
        }
    })
}

/// `file:///home/me/My%20EQ.json` -> `/home/me/My EQ.json`. Anything that is not
/// a local file URI, or that has a malformed escape, is rejected.
fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let rest = uri.strip_prefix("file://")?;
    // An empty host or `localhost` means this machine; anything else is remote.
    let path = match rest.strip_prefix("localhost") {
        Some(path) => path,
        None => rest,
    };
    if !path.starts_with('/') {
        return None;
    }
    let mut bytes = Vec::with_capacity(path.len());
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = (input.next()? as char).to_digit(16)?;
            let low = (input.next()? as char).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    if bytes.contains(&0) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths_with_escapes_decoded() {
        assert_eq!(
            path_from_file_uri("file:///home/me/My%20EQ%20%26%20more.json"),
            Some(PathBuf::from("/home/me/My EQ & more.json"))
        );
        assert_eq!(
            path_from_file_uri("file://localhost/tmp/a.json"),
            Some(PathBuf::from("/tmp/a.json"))
        );
        assert_eq!(
            path_from_file_uri("file:///tmp/%C3%A9.json"),
            Some(PathBuf::from("/tmp/é.json")),
            "multi-byte characters decode as UTF-8 bytes"
        );
    }

    #[test]
    fn anything_that_is_not_a_plain_local_file_is_rejected() {
        for bad in [
            "https://example.com/a.json",
            "file://remote-host/tmp/a.json",
            "file:///tmp/bad%2",
            "file:///tmp/bad%zz",
            "file:///tmp/nul%00.json",
            "file:relative.json",
            "",
        ] {
            assert_eq!(path_from_file_uri(bad), None, "{bad}");
        }
    }
}
