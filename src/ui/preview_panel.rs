//! Preview panel component.

use crate::core::{RenamePreview, RenameStatus};
use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;

/// Create a preview row for the list.
pub fn create_preview_row(preview: &RenamePreview) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&preview.original_name)
        .subtitle(&preview.new_name)
        .build();

    // Status indicator
    let status_icon = match preview.status {
        RenameStatus::WillRename => {
            let icon = gtk::Image::from_icon_name("object-select-symbolic");
            icon.add_css_class("success");
            icon
        }
        RenameStatus::Unchanged => {
            let icon = gtk::Image::from_icon_name("action-unavailable-symbolic");
            icon.add_css_class("dim-label");
            icon
        }
        RenameStatus::Conflict | RenameStatus::InternalConflict => {
            let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
            icon.add_css_class("warning");
            icon
        }
        RenameStatus::Error => {
            let icon = gtk::Image::from_icon_name("dialog-error-symbolic");
            icon.add_css_class("error");
            icon
        }
        RenameStatus::Completed => {
            let icon = gtk::Image::from_icon_name("object-select-symbolic");
            icon.add_css_class("success");
            icon
        }
        RenameStatus::Failed => {
            let icon = gtk::Image::from_icon_name("process-stop-symbolic");
            icon.add_css_class("error");
            icon
        }
        RenameStatus::Skipped => {
            let icon = gtk::Image::from_icon_name("action-unavailable-symbolic");
            icon.add_css_class("dim-label");
            icon
        }
    };

    row.add_suffix(&status_icon);

    // Tooltip with more details
    if let Some(ref msg) = preview.message {
        row.set_tooltip_text(Some(msg));
    }

    row
}

/// Get the status label text.
pub fn status_label(status: RenameStatus) -> &'static str {
    match status {
        RenameStatus::WillRename => "Will rename",
        RenameStatus::Unchanged => "Unchanged",
        RenameStatus::Conflict => "Conflict",
        RenameStatus::InternalConflict => "Duplicate",
        RenameStatus::Error => "Error",
        RenameStatus::Completed => "Renamed",
        RenameStatus::Failed => "Failed",
        RenameStatus::Skipped => "Skipped",
    }
}

/// Create the preview statistics widget.
pub fn create_stats_widget(previews: &[RenamePreview]) -> gtk::Box {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let total = previews.len();
    let will_rename = previews.iter().filter(|p| matches!(p.status, RenameStatus::WillRename)).count();
    let unchanged = previews.iter().filter(|p| matches!(p.status, RenameStatus::Unchanged)).count();
    let conflicts = previews.iter().filter(|p| matches!(p.status, RenameStatus::Conflict | RenameStatus::InternalConflict)).count();
    let errors = previews.iter().filter(|p| matches!(p.status, RenameStatus::Error)).count();

    let total_label = gtk::Label::new(Some(&format!("{} files", total)));
    total_label.add_css_class("dim-label");
    box_.append(&total_label);

    if will_rename > 0 {
        let label = gtk::Label::new(Some(&format!("{} to rename", will_rename)));
        label.add_css_class("success");
        box_.append(&label);
    }

    if unchanged > 0 {
        let label = gtk::Label::new(Some(&format!("{} unchanged", unchanged)));
        label.add_css_class("dim-label");
        box_.append(&label);
    }

    if conflicts > 0 {
        let label = gtk::Label::new(Some(&format!("{} conflicts", conflicts)));
        label.add_css_class("warning");
        box_.append(&label);
    }

    if errors > 0 {
        let label = gtk::Label::new(Some(&format!("{} errors", errors)));
        label.add_css_class("error");
        box_.append(&label);
    }

    box_
}
