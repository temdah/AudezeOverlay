//! Local EQ library and file actions. Loading explicitly previews a custom slot.
use super::*;
use audeze_overlay::eq::profiles::{self, Profile};
use audeze_overlay::eq::Snapshot;
use std::path::PathBuf;

pub(super) enum ProfileFile {
    Import(Option<Snapshot>),
    Export(Profile),
}

fn current_profile(app: &App) -> std::result::Result<Profile, String> {
    let draft = app
        .eq_editor
        .draft()
        .ok_or("Load a custom headset slot first.")?;
    if draft.saving {
        return Err("Wait for the current tuning to be confirmed.".into());
    }
    Profile::new(app.config.eq.name(draft.value.slot), draft.value)
}

fn store_profile(
    app: &mut App,
    mut profile: Profile,
    replace: Option<usize>,
) -> std::result::Result<usize, String> {
    profile.validate()?;
    let mut config = app.config.clone();
    let index = if let Some(index) = replace.filter(|i| *i < config.eq_library.len()) {
        // Save updates tuning, while Save as new keeps a distinct library name.
        profile.name = config.eq_library[index].name.clone();
        config.eq_library[index] = profile;
        index
    } else {
        profiles::add(&mut config.eq_library, profile)?
    };
    if !app.demo {
        config
            .save()
            .map_err(|e| format!("Could not save the profile library: {e}"))?;
    }
    app.config = config;
    Ok(index)
}

pub(in crate::ui) fn remember_current_profile(app: &mut App, copy: bool) -> bool {
    let result = (|| {
        let profile = current_profile(app)?;
        let bank = app.eq_editor.selected as usize - 6;
        let replace = if copy {
            None
        } else {
            app.eq_editor.library_entries[bank]
        };
        let index = store_profile(app, profile, replace)?;
        app.eq_editor.library_entries[bank] = Some(index);
        Ok::<_, String>(app.config.eq_library[index].name.clone())
    })();
    match result {
        Ok(name) => {
            app.note = format!("Saved “{name}” in your profile library.").into();
            true
        }
        Err(error) => {
            app.note = error.into();
            false
        }
    }
}
pub(in crate::ui) fn save_profile_copy(app: &mut App) {
    remember_current_profile(app, true);
    request_draw(app);
}

fn import_profile(app: &mut App) -> std::result::Result<(), String> {
    let baseline = app.eq_editor.draft().map(|d| d.value);
    choose_profile_file(app, ProfileFile::Import(baseline))
}
fn export_profile(app: &mut App, profile: Profile) -> std::result::Result<(), String> {
    choose_profile_file(app, ProfileFile::Export(profile))
}

fn choose_profile_file(app: &mut App, action: ProfileFile) -> Result<(), String> {
    if app.pending_profile_file.is_some() {
        return Err("Finish or cancel the open file dialog first.".into());
    }
    let save = matches!(action, ProfileFile::Export(_));
    app.pending_profile_file = Some(action);
    match app.shell.choose_file(save) {
        #[cfg(any(windows, test))]
        Ok(FileSelection::Ready(path)) => profile_file_selected(app, Ok(path)),
        #[cfg(any(target_os = "linux", test))]
        Ok(FileSelection::Pending) => {}
        Err(error) => {
            app.pending_profile_file = None;
            return Err(error);
        }
    }
    Ok(())
}

/// Completes the one outstanding request using the tuning captured when it
/// opened. Changes to the selected slot while a portal is open cannot alter it.
pub(in crate::ui) fn profile_file_selected(app: &mut App, result: Result<Option<PathBuf>, String>) {
    let Some(action) = app.pending_profile_file.take() else {
        return;
    };
    let result = (|| {
        let Some(path) = result? else {
            return Ok::<_, String>(());
        };
        match action {
            ProfileFile::Import(baseline) => {
                let profile = profiles::read(&path, baseline)?;
                let index = store_profile(app, profile, None)?;
                app.note = format!(
                    "Imported “{}”. Choose it in Profiles to load into a custom slot.",
                    app.config.eq_library[index].name
                )
                .into();
            }
            ProfileFile::Export(profile) => {
                profiles::write(&path, &profile)?;
                app.note = "EQ profile exported. It can be imported on another PC.".into();
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        app.note = error.into();
    };
    request_draw(app);
}
fn load_profile(app: &mut App, index: usize) -> std::result::Result<(), String> {
    let profile = app
        .config
        .eq_library
        .get(index)
        .ok_or("Profile no longer exists.")?
        .clone();
    if app.sound.mix.is_none() || app.sound.capabilities.is_some_and(|c| !c.eq_edit) {
        return Err("Connect a headset with verified custom EQ editing first.".into());
    }
    let draft = app
        .eq_editor
        .draft()
        .ok_or("Choose and load a custom headset slot first.")?;
    if draft.saving {
        return Err("Wait for the current tuning to be confirmed.".into());
    }
    let target = profile.snapshot(draft.value.slot)?;
    let question = format!("Preview “{}” in {}?\n\nThis changes the sound immediately. Save keeps it on the headset; Discard restores the slot's previous curve. Your saved library profile stays separate.", profile.name, app.config.eq.name(target.slot));
    if app
        .shell
        .confirm("Load EQ profile", &question, Buttons::YesNo)
        != Choice::Yes
    {
        return Ok(());
    }
    // Keep the baseline (including an existing preview) until ordered readback.
    let Some(draft) = app.eq_editor.draft_mut() else {
        return Err("The custom slot is no longer available.".into());
    };
    draft.value = target;
    draft.touched = (1 << 10) - 1;
    draft.previewing = true;
    app.eq_editor.library_entries[target.slot as usize - 6] = Some(index);
    app.eq_editor.preview_names[target.slot as usize - 6]
        .get_or_insert_with(|| app.config.eq.name(target.slot).to_owned());
    app.config.eq.names[target.slot as usize - 6] = profile.name.chars().take(32).collect();
    save(app);
    if app.sound.eq_index != Some(target.slot) {
        send(app, Command::EqSelect(target.slot));
    }
    app.eq_editor.preview_slot = Some(target.slot);
    for band in target.bands {
        send(
            app,
            Command::EqGain {
                slot: target.slot,
                band,
            },
        );
    }
    app.note = "Profile preview loaded. Save keeps it; Discard restores the previous curve.".into();
    Ok(())
}

pub(in crate::ui) fn profile_menu(app: &mut App) {
    let ready = app.eq_editor.draft().is_some_and(|d| !d.saving);
    let mut entries = vec![
        MenuEntry::item(1, "Import from file…"),
        MenuEntry::item_if(ready, 2, "Export current tuning…"),
        MenuEntry::item_if(ready, 3, "Save current tuning as new profile"),
        MenuEntry::Separator,
    ];
    for (i, profile) in app.config.eq_library.iter().enumerate() {
        entries.push(MenuEntry::Submenu {
            label: profile.name.clone(),
            entries: vec![
                MenuEntry::item_if(ready, 1000 + i as u32, "Load into selected custom slot…"),
                MenuEntry::item(2000 + i as u32, "Export profile…"),
                MenuEntry::item(3000 + i as u32, "Remove from library…"),
            ],
        });
    }
    if app.config.eq_library.is_empty() {
        entries.push(MenuEntry::disabled("No saved profiles yet"));
    }
    let id = app
        .shell
        .popup_menu(&entries, MenuOrigin::Panel)
        .unwrap_or(0) as usize;
    let result = match id {
        1 => import_profile(app),
        2 => current_profile(app).and_then(|p| export_profile(app, p)),
        3 => {
            save_profile_copy(app);
            Ok(())
        }
        1000..2000 => load_profile(app, id - 1000),
        2000..3000 => match app.config.eq_library.get(id - 2000).cloned() {
            Some(profile) => export_profile(app, profile),
            None => Ok(()),
        },
        3000..4000 => remove_profile(app, id - 3000),
        _ => Ok(()),
    };
    if let Err(error) = result {
        app.note = error.into();
    }
    request_draw(app);
}
fn remove_profile(app: &mut App, index: usize) -> std::result::Result<(), String> {
    let Some(profile) = app.config.eq_library.get(index) else {
        return Ok(());
    };
    let text = format!("Remove “{}” from this PC's library? Headset slots and exported files will stay as they are.", profile.name);
    if app.shell.confirm("Remove profile", &text, Buttons::YesNo) != Choice::Yes {
        return Ok(());
    }
    let mut config = app.config.clone();
    config.eq_library.remove(index);
    if !app.demo {
        config
            .save()
            .map_err(|e| format!("Could not save the library: {e}"))?;
    }
    app.config = config;
    for entry in &mut app.eq_editor.library_entries {
        *entry = entry.and_then(|i| {
            if i == index {
                None
            } else {
                Some(if i > index { i - 1 } else { i })
            }
        });
    }
    app.note = "Profile removed from the library.".into();
    Ok(())
}
