//! Save/load preset dialogs.

use super::window::RenamerWindow;
use crate::presets::{Preset, PresetManager};
use gettextrs::gettext;
use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use glib::clone;

pub fn show_save(window: &RenamerWindow) {
    let dialog = adw::MessageDialog::new(
        Some(window),
        Some(gettext("Save Preset").as_str()),
        Some(gettext("Enter a name for this preset:").as_str()),
    );

    let entry = gtk::Entry::builder()
        .placeholder_text(gettext("Preset name"))
        .build();
    dialog.set_extra_child(Some(&entry));

    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("save", &gettext("Save"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));

    dialog.connect_response(
        None,
        clone!(
            #[weak(rename_to = window)]
            window,
            move |dialog, response| {
                if response == "save" {
                    let name = entry.text().trim().to_string();
                    if name.is_empty() {
                        window.show_info_dialog(
                            &gettext("Preset Not Saved"),
                            &gettext("Preset name cannot be empty."),
                        );
                        return;
                    }
                    let config = window.config_snapshot();
                    let preset = Preset::new(&name, config);
                    let mut manager = PresetManager::default();
                    match manager.add_preset(preset) {
                        Ok(()) => window.show_toast(&gettext("Preset saved")),
                        Err(err) => {
                            window.show_info_dialog(&gettext("Preset Not Saved"), &err.to_string())
                        }
                    }
                }
                dialog.close();
            }
        ),
    );
    dialog.present();
}

pub fn show_load(window: &RenamerWindow) {
    let manager = PresetManager::default();
    let presets = manager.get_all().into_iter().cloned().collect::<Vec<_>>();

    if presets.is_empty() {
        window.show_info_dialog(
            &gettext("No Presets"),
            &gettext("There are no presets to load yet."),
        );
        return;
    }

    let dialog = adw::MessageDialog::new(
        Some(window),
        Some(gettext("Load Preset").as_str()),
        Some(gettext("Choose a preset to apply.").as_str()),
    );

    let scroll = gtk::ScrolledWindow::builder()
        .max_content_height(360)
        .propagate_natural_height(true)
        .margin_start(12)
        .margin_end(12)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(vec!["boxed-list"])
        .build();

    for preset in &presets {
        let subtitle = preset.description.clone().unwrap_or_else(|| {
            gettext("{} rules").replacen("{}", &preset.config.rules.len().to_string(), 1)
        });
        let row = adw::ActionRow::builder()
            .title(&preset.name)
            .subtitle(&subtitle)
            .activatable(true)
            .build();
        list.append(&row);
    }
    list.select_row(list.row_at_index(0).as_ref());

    scroll.set_child(Some(&list));
    dialog.set_extra_child(Some(&scroll));
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("load", &gettext("Load"));
    dialog.set_response_appearance("load", adw::ResponseAppearance::Suggested);

    dialog.connect_response(
        None,
        clone!(
            #[weak(rename_to = window)]
            window,
            move |dialog, response| {
                if response == "load" {
                    if let Some(row) = list.selected_row() {
                        let idx = row.index() as usize;
                        if let Some(preset) = presets.get(idx) {
                            window.apply_preset(preset.clone());
                        }
                    }
                }
                dialog.close();
            }
        ),
    );

    dialog.present();
}
