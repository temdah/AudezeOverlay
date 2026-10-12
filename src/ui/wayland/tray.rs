//! The notification-area icon, as a StatusNotifierItem. The desktop's tray (KDE
//! Plasma's, Waybar, COSMIC's and others) draws the icon and its menu; choosing
//! an entry sends a command back to the main loop.
use crate::ui::app::TrayCommand;
use ksni::{
    blocking::Handle, blocking::TrayMethods, menu::StandardItem, Category, Icon, MenuItem, Tray,
};
use smithay_client_toolkit::reexports::calloop::channel::Sender;

pub(super) struct OverlayTray {
    icons: Vec<Icon>,
    commands: Sender<TrayCommand>,
}

impl OverlayTray {
    fn entry(&self, label: &str, command: TrayCommand) -> MenuItem<Self> {
        StandardItem {
            label: label.into(),
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.commands.send(command);
            }),
            ..Default::default()
        }
        .into()
    }
}

impl Tray for OverlayTray {
    fn id(&self) -> String {
        "audeze-overlay".into()
    }
    fn title(&self) -> String {
        "Audeze Overlay".into()
    }
    fn category(&self) -> Category {
        Category::Hardware
    }
    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }
    /// A left click opens the panel.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.commands.send(TrayCommand::Open);
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            self.entry("Open", TrayCommand::Open),
            self.entry("Settings", TrayCommand::Settings),
            self.entry("Reset position", TrayCommand::ResetPosition),
            self.entry("Hide", TrayCommand::Hide),
            MenuItem::Separator,
            self.entry("Quit", TrayCommand::Quit),
        ]
    }
}

/// A drawn icon in the byte order the specification wants: straight (not
/// premultiplied) alpha, then red, green, blue.
pub(super) fn icon_from_pixmap(pixmap: &tiny_skia::Pixmap) -> Icon {
    let mut data = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.data().as_chunks::<4>().0 {
        let alpha = u32::from(px[3]);
        let straight = |channel: u8| {
            (u32::from(channel) * 255 + alpha / 2)
                .checked_div(alpha)
                .map_or(0, |value| value.min(255) as u8)
        };
        data.extend_from_slice(&[px[3], straight(px[0]), straight(px[1]), straight(px[2])]);
    }
    Icon {
        width: pixmap.width() as i32,
        height: pixmap.height() as i32,
        data,
    }
}

/// Registers the icon with the desktop. `None` if there is no tray to show it in
/// (yet): the caller retries while the desktop starts.
pub(super) fn spawn(
    icons: Vec<Icon>,
    commands: Sender<TrayCommand>,
) -> Option<Handle<OverlayTray>> {
    OverlayTray { icons, commands }.spawn().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_keeps_its_colors_when_alpha_is_unpremultiplied() {
        let mut pixmap = tiny_skia::Pixmap::new(2, 1).unwrap();
        // Premultiplied: half-transparent white, and a fully transparent pixel.
        pixmap
            .data_mut()
            .copy_from_slice(&[128, 128, 128, 128, 0, 0, 0, 0]);
        let icon = icon_from_pixmap(&pixmap);
        assert_eq!((icon.width, icon.height), (2, 1));
        assert_eq!(
            icon.data,
            [128, 255, 255, 255, 0, 0, 0, 0],
            "ARGB, straight alpha"
        );
    }
}
