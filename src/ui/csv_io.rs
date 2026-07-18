//! CSV import (rename plans) and log export.

use super::window::RenamerWindow;
use crate::core::{FileEntry, RenamePreview, RenameStatus};
use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;
use gtk::{gio, glib};
use glib::clone;
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

pub fn show_import_dialog(window: &RenamerWindow) {
    let dialog = gtk::FileDialog::builder()
        .title("Import from CSV")
        .modal(true)
        .build();

    dialog.open(
        Some(window),
        gio::Cancellable::NONE,
        clone!(
            #[weak(rename_to = window)]
            window,
            move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        import_csv_file(&window, path);
                    }
                }
            }
        ),
    );
}

pub fn show_export_dialog(window: &RenamerWindow) {
    let dialog = gtk::FileDialog::builder()
        .title("Export Log")
        .modal(true)
        .build();

    dialog.save(
        Some(window),
        gio::Cancellable::NONE,
        clone!(
            #[weak(rename_to = window)]
            window,
            move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        match window.export_log_csv(&path) {
                            Ok(()) => window.show_toast("Rename log exported to CSV"),
                            Err(err) => window.show_info_dialog("Export Failed", &err.to_string()),
                        }
                    }
                }
            }
        ),
    );
}

fn import_csv_file(window: &RenamerWindow, path: PathBuf) {
    match read_csv_rename_plan(path) {
        Ok((previews, files)) => {
            let count = previews.len();
            let dialog = adw::MessageDialog::new(
                Some(window),
                Some("Import CSV Renames"),
                Some(&format!("Apply {} renames from the CSV file?", count)),
            );
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("rename", "Rename");
            dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
            dialog.connect_response(
                None,
                clone!(
                    #[weak(rename_to = window)]
                    window,
                    move |dialog, response| {
                        if response == "rename" {
                            super::execution::run_rename(&window, previews.clone(), files.clone());
                        }
                        dialog.close();
                    }
                ),
            );
            dialog.present();
        }
        Err(err) => window.show_info_dialog("CSV Import Failed", &err.to_string()),
    }
}

fn read_csv_rename_plan(
    path: PathBuf,
) -> crate::core::RenamerResult<(Vec<RenamePreview>, HashMap<Uuid, FileEntry>)> {
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader.headers()?.clone();
    let path_idx = headers
        .iter()
        .position(|header| header == "original_path")
        .ok_or_else(|| {
            crate::core::RenamerError::Internal(
                "CSV must include an original_path column".to_string(),
            )
        })?;
    let name_idx = headers
        .iter()
        .position(|header| header == "new_name")
        .ok_or_else(|| {
            crate::core::RenamerError::Internal("CSV must include a new_name column".to_string())
        })?;

    let mut previews = Vec::new();
    let mut files = HashMap::new();

    for record in reader.records() {
        let record = record?;
        let original_path = PathBuf::from(record.get(path_idx).unwrap_or_default());
        let new_name = record.get(name_idx).unwrap_or_default().trim().to_string();
        let entry = FileEntry::from_path(original_path.clone(), 0)?;
        let new_path = original_path
            .parent()
            .map(|parent| parent.join(&new_name))
            .unwrap_or_else(|| PathBuf::from(&new_name));
        let status = if new_name == entry.original_name {
            RenameStatus::Unchanged
        } else {
            RenameStatus::WillRename
        };
        previews.push(RenamePreview {
            file_id: entry.id,
            original_name: entry.original_name.clone(),
            new_name,
            new_path,
            status,
            message: None,
        });
        files.insert(entry.id, entry);
    }

    Ok((previews, files))
}
