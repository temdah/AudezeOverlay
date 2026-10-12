use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey_mod: u32,
    pub hotkey_vk: u32,
    pub duration_ms: u32,
    pub fade_ms: u32,
    pub position: Option<(i32, i32)>,
    pub auto_show: bool,
    pub width: u32,
    pub height: u32,
    pub text_scale: u32,
    pub background_opacity: u8,
    pub card_opacity: u8,
    pub background_blur: bool,
    /// Acrylic backdrop strength. The legacy boolean still migrates disabled blur.
    pub blur_strength: u8,
    pub preserve_space: bool,
    pub minimal_cards: [bool; 6],
    pub inline_eq: bool,
    pub changed_card_only: bool,
    #[serde(skip)]
    pub popup: bool,
    #[serde(deserialize_with = "deserialize_cards")]
    pub cards: [crate::layout::Card; 6],
    pub grid_rows: i32,
    pub grid_columns: i32,
    pub cell_width: f32,
    pub cell_height: f32,
    #[serde(default)]
    pub layout_version: u8,
    pub notify: [bool; 5],
    pub monitor: String,
    pub snap: bool,
    pub locked: bool,
    pub autostart: bool,
    pub low_battery: u8,
    pub animations: bool,
    pub settings_page: usize,
    pub status_icons: bool,
    pub eq: crate::eq::Preferences,
    pub eq_library: Vec<crate::eq::profiles::Profile>,
    pub eq_profile: String,
    pub eq_profiles: std::collections::BTreeMap<String, crate::eq::Preferences>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey_mod: 1,
            hotkey_vk: 0x24,
            duration_ms: 2500,
            fade_ms: 180,
            position: None,
            auto_show: true,
            width: 710,
            height: 548,
            text_scale: 100,
            background_opacity: 70,
            card_opacity: 70,
            background_blur: true,
            blur_strength: 50,
            preserve_space: false,
            minimal_cards: [false; 6],
            inline_eq: false,
            changed_card_only: false,
            popup: false,
            cards: crate::layout::defaults(),
            grid_rows: 15,
            grid_columns: 12,
            cell_width: 57.116_665,
            cell_height: 30.493_206,
            layout_version: 2,
            notify: [true; 5],
            monitor: String::new(),
            snap: true,
            locked: false,
            autostart: false,
            low_battery: 20,
            animations: true,
            settings_page: 0,
            status_icons: false,
            eq: crate::eq::Preferences::default(),
            eq_library: Vec::new(),
            eq_profile: "maxwell_2_anc/ps".into(),
            eq_profiles: Default::default(),
        }
    }
}
impl Config {
    /// Preserve local renames and pins independently for each model/platform.
    pub fn select_eq_profile(&mut self, family: crate::model::HeadsetFamily, xbox: bool) {
        let key = format!(
            "{}/{}",
            family.settings_section(),
            if xbox { "xbox" } else { "ps" }
        );
        if self.eq_profile != key {
            self.eq_profiles
                .insert(self.eq_profile.clone(), self.eq.clone());
            self.eq = self.eq_profiles.remove(&key).unwrap_or_default();
            self.eq_profile = key;
        }
        self.eq.import_model_names(family, xbox);
    }
    pub fn card_top(&self) -> f32 {
        if self.popup {
            8.
        } else if self.status_icons {
            142.
        } else {
            74.
        }
    }
    /// A temporary layout only; never overwrites the player's saved card positions.
    pub fn notification_layout(&self, mask: u8) -> Self {
        let mut result = self.clone();
        result.popup = true;
        result.status_icons = false;
        result.preserve_space = false;
        let mut y = 0;
        for (i, card) in result.cards.iter_mut().enumerate() {
            card.visible = mask & (1 << i) != 0;
            if card.visible {
                card.x = 0;
                card.y = y;
                card.w = card.w.clamp(5, 7);
                card.h = if i == 0 && !result.minimal_cards[0] {
                    8
                } else {
                    3
                };
                y += card.h;
            }
        }
        result.width = result.minimum_width();
        result.height = result.minimum_height();
        result
    }
    pub fn vertical_padding(&self) -> f32 {
        self.card_top() + 16.
    }
    pub fn minimum_height(&self) -> u32 {
        (self.vertical_padding() + crate::layout::extent(&self.cards).1 as f32 * self.cell_height)
            .ceil()
            .max(160.) as u32
    }
    pub fn minimum_width(&self) -> u32 {
        (24. + crate::layout::extent(&self.cards).0.max(5) as f32 * self.cell_width)
            .ceil()
            .max(232.) as u32
    }
    pub fn canvas_cells(&self, width: f32, height: f32) -> (i32, i32) {
        (
            ((width - 24.) / self.cell_width + 0.001)
                .floor()
                .clamp(1., 80.) as i32,
            ((height - self.vertical_padding()) / self.cell_height + 0.001)
                .floor()
                .clamp(1., 80.) as i32,
        )
    }
    pub fn resize_canvas(&mut self, width: u32, height: u32) {
        self.width = width.max(self.minimum_width()).min(3200);
        self.height = height.max(self.minimum_height()).min(2400);
        (self.grid_columns, self.grid_rows) =
            self.canvas_cells(self.width as f32, self.height as f32);
    }
    pub fn trim_canvas(&mut self) {
        if self.preserve_space {
            self.fit_layout();
            return;
        }
        crate::layout::compact(&mut self.cards);
        self.resize_canvas(self.minimum_width(), self.minimum_height());
    }
    pub fn restore_card(&mut self, index: usize) -> bool {
        if let Some(rows) =
            crate::layout::restore(&mut self.cards, index, self.grid_columns, self.grid_rows)
        {
            self.height = self
                .height
                .max((self.vertical_padding() + rows as f32 * self.cell_height).ceil() as u32);
            self.fit_layout();
            true
        } else {
            false
        }
    }
    pub fn fit_layout(&mut self) {
        self.resize_canvas(self.width, self.height);
    }
    /// Where preferences live: the user's local application data on Windows, the
    /// XDG config directory elsewhere.
    pub fn path() -> PathBuf {
        #[cfg(windows)]
        let (base, app) = (
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            "AudezeOverlay",
        );
        #[cfg(not(windows))]
        let (base, app) = (
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                }),
            "audeze-overlay",
        );
        base.unwrap_or_else(|| PathBuf::from("."))
            .join(app)
            .join("settings.json")
    }
    pub fn load() -> Self {
        let mut c: Self = fs::read(Self::path())
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default();
        if !valid_hotkey(c.hotkey_mod, c.hotkey_vk) {
            c.hotkey_mod = 1;
            c.hotkey_vk = 0x24;
        }
        c.duration_ms = c.duration_ms.clamp(500, 10000);
        c.fade_ms = c.fade_ms.min(1000);
        c.width = c.width.clamp(232, 3200);
        c.height = c.height.clamp(160, 2400);
        c.text_scale = c.text_scale.clamp(100, 140);
        c.background_opacity = c.background_opacity.min(100);
        c.card_opacity = c.card_opacity.clamp(10, 100);
        c.blur_strength = if c.background_blur {
            c.blur_strength.min(100)
        } else {
            0
        };
        c.grid_rows = c.grid_rows.clamp(1, 80);
        c.migrate_layout();
        c.low_battery = c.low_battery.min(50);
        for i in 0..6 {
            let card = c.cards[i];
            if !card.visible {
                continue;
            }
            let (w, h) = crate::layout::minimum(i);
            if card.x < 0
                || card.y < 0
                || card.w < w
                || card.h < h
                || card.x + card.w > c.grid_columns
                || card.y + card.h > c.grid_rows
            {
                c.cards = crate::layout::defaults();
                break;
            }
        }
        if (0..6).any(|i| {
            (i + 1..6).any(|j| {
                c.cards[i].visible && c.cards[j].visible && c.cards[i].overlaps(c.cards[j])
            })
        }) {
            c.cards = crate::layout::defaults();
        }
        c.settings_page = c.settings_page.min(5);
        c.eq_library.retain(|p| p.validate().is_ok());
        c.eq_library.truncate(crate::eq::profiles::MAX_PROFILES);
        if c.eq_profile == "maxwell_2_anc/ps" {
            c.eq.import_native_names();
        }
        c.fit_layout();
        c
    }
    pub fn migrate_layout(&mut self) {
        if self.layout_version == 0 {
            self.cell_width = (self.width as f32 - 24.) / 12.;
            self.cell_height = (self.height as f32 - 106.) / self.grid_rows.max(1) as f32;
            self.grid_columns = 12;
            self.layout_version = 1;
        }
        if self.layout_version == 1 {
            self.height = self.height.saturating_sub(16);
            self.layout_version = 2;
        }
        if !self.cell_width.is_finite() {
            self.cell_width = 596. / 12.;
        }
        if !self.cell_height.is_finite() {
            self.cell_height = 394. / 15.;
        }
        self.cell_width = self.cell_width.clamp(41.3333, 200.);
        self.cell_height = self.cell_height.clamp(26., 100.);
        self.grid_columns = self.grid_columns.clamp(5, 80);
    }
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }
    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self)?;
        let result = (|| {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
    pub fn hotkey_label(&self) -> String {
        hotkey_label(self.hotkey_mod, self.hotkey_vk)
    }
}
pub fn valid_hotkey(mods: u32, vk: u32) -> bool {
    matches!(mods, 0 | 1 | 2 | 4)
        && (0x08..=0xfe).contains(&vk)
        && !matches!(vk,0x10..=0x12|0x1b|0x5b..=0x5c|0x7b|0xa0..=0xa5)
}
pub fn hotkey_label(mods: u32, vk: u32) -> String {
    let prefix = match mods {
        1 => "Alt + ",
        2 => "Ctrl + ",
        4 => "Shift + ",
        _ => "",
    };
    let key = match vk {
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "Page Up".into(),
        0x22 => "Page Down".into(),
        0x2d => "Insert".into(),
        0x2e => "Delete".into(),
        0x20 => "Space".into(),
        0x09 => "Tab".into(),
        0x0d => "Enter".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6f),
        0x30..=0x39 | 0x41..=0x5a => char::from_u32(vk).unwrap().to_string(),
        _ => format!("Key {vk:02X}"),
    };
    format!("{prefix}{key}")
}
fn deserialize_cards<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<[crate::layout::Card; 6], D::Error> {
    let saved = Vec::<crate::layout::Card>::deserialize(d)?;
    if !(5..=6).contains(&saved.len()) {
        return Err(serde::de::Error::custom("Expected five or six cards"));
    }
    let mut cards = crate::layout::defaults();
    cards[..saved.len()].copy_from_slice(&saved);
    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserving_space_keeps_gaps_edges_and_card_dimensions() {
        let mut c = Config {
            preserve_space: true,
            ..Default::default()
        };
        for card in &mut c.cards {
            card.x += 2;
            card.y += 2;
        }
        c.resize_canvas(1200, 1000);
        let before = c.clone();
        c.trim_canvas();
        assert_eq!(c.cards, before.cards);
        assert_eq!((c.width, c.height), (before.width, before.height));
        c.preserve_space = false;
        c.trim_canvas();
        assert!(c.width < before.width && c.height < before.height);
    }
    #[test]
    fn changed_card_popup_does_not_modify_saved_layout() {
        let c = Config::default();
        let popup = c.notification_layout((1 << 1) | (1 << 3));
        assert_eq!(popup.cards.iter().filter(|card| card.visible).count(), 2);
        assert_eq!(popup.cards[1].y, 0);
        assert_eq!(popup.cards[3].y, 3);
        assert_eq!(c.cards, crate::layout::defaults());
        assert!(popup.width < c.width && popup.height < c.height);
    }
    #[test]
    fn settings_replace_an_existing_file_and_keep_exact_input() {
        let path = std::env::temp_dir().join(format!(
            "audeze-settings-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"old preferences").unwrap();
        let config = Config {
            duration_ms: 797,
            settings_page: 2,
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        let restored: Config = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored.duration_ms, 797);
        assert_eq!(restored.settings_page, 2);
        assert!(!path
            .with_extension(format!("{}.tmp", std::process::id()))
            .exists());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn old_layout_loses_only_footer_padding_once() {
        let mut config = Config {
            layout_version: 1,
            height: 600,
            ..Config::default()
        };
        let cells = (config.cell_width, config.cell_height);
        config.migrate_layout();
        assert_eq!(config.height, 584);
        assert_eq!((config.cell_width, config.cell_height), cells);
        config.migrate_layout();
        assert_eq!(config.height, 584);
    }
    #[test]
    fn resizing_creates_space_without_stretching_cards_and_finish_trims_it() {
        let mut c = Config::default();
        let cards = c.cards;
        let cell = (c.cell_width, c.cell_height);
        c.resize_canvas(1000, 900);
        assert!(c.grid_columns > 12 && c.grid_rows > 15);
        assert_eq!(c.cards, cards);
        assert_eq!((c.cell_width, c.cell_height), cell);
        c.trim_canvas();
        assert!(c.width < 715 && c.height < 555);
        assert_eq!(c.cards, cards);
    }
    #[test]
    fn legacy_layout_keeps_its_visible_pixel_dimensions() {
        let mut c: Config =
            serde_json::from_str(r#"{"width":740,"height":706,"grid_rows":15}"#).unwrap();
        assert_eq!(c.layout_version, 0);
        c.migrate_layout();
        assert!((c.cell_width - 716. / 12.).abs() < 0.01);
        assert_eq!(c.cell_height, 40.);
        let before = (c.cell_width, c.cell_height);
        c.migrate_layout();
        assert_eq!((c.cell_width, c.cell_height), before);
    }
    #[test]
    fn hidden_cards_can_be_restored_after_trimming_an_empty_layout() {
        let mut c = Config::default();
        for card in &mut c.cards {
            card.visible = false;
        }
        c.trim_canvas();
        assert!(c.restore_card(4));
        assert!(c.cards[4].visible);
        assert!(crate::layout::valid_in(
            &c.cards,
            c.grid_columns,
            c.grid_rows
        ));
    }
    #[test]
    fn existing_five_card_settings_keep_layout_and_add_hidden_eq() {
        let current = Config::default();
        let mut value = serde_json::to_value(&current).unwrap();
        value["cards"].as_array_mut().unwrap().truncate(5);
        value.as_object_mut().unwrap().remove("eq");
        value.as_object_mut().unwrap().remove("status_icons");
        let restored: Config = serde_json::from_value(value).unwrap();
        assert_eq!(restored.cards[..5], current.cards[..5]);
        assert!(!restored.cards[5].visible);
        assert!(!restored.status_icons);
        assert!(restored.eq.show_factory);
    }
    #[test]
    fn one_or_two_keys_only() {
        assert!(valid_hotkey(0, 0x24));
        assert!(valid_hotkey(1, 0x24));
        assert!(!valid_hotkey(3, 0x24));
        assert!(!valid_hotkey(0, 0x12));
        assert!(!valid_hotkey(0, 0x7b));
    }
}
