//! Dialog components.

use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;

/// Application ID following reverse-DNS convention
const APP_ID: &str = "com.chrisdaggas.bulk-renamer";

/// Create the about dialog.
pub fn create_about_dialog() -> adw::AboutWindow {
    adw::AboutWindow::builder()
        .application_name("Bulk Renamer")
        .application_icon(APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .developer_name("Christos A. Daggas")
        .license_type(gtk::License::MitX11)
        .website("https://chrisdaggas.com")
        .issue_url("https://github.com/christosdaggas/bulk-renamer/issues")
        .copyright("© 2024-2026 Christos A. Daggas")
        .developers(vec!["Christos A. Daggas"])
        .designers(vec!["Christos A. Daggas"])
        .build()
}

/// Create a confirmation dialog.
pub fn create_confirm_dialog(
    parent: &impl IsA<gtk::Window>,
    heading: &str,
    body: &str,
    confirm_label: &str,
    destructive: bool,
) -> adw::MessageDialog {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), Some(body));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", confirm_label);

    if destructive {
        dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    } else {
        dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
    }

    dialog.set_default_response(Some("confirm"));
    dialog.set_close_response("cancel");

    dialog
}

/// Create an error dialog.
pub fn create_error_dialog(parent: &impl IsA<gtk::Window>, heading: &str, body: &str) -> adw::MessageDialog {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), Some(body));

    dialog.add_response("ok", "OK");
    dialog.set_default_response(Some("ok"));

    dialog
}

/// Create a progress dialog for long-running operations.
pub fn create_progress_dialog(parent: &impl IsA<gtk::Window>, title: &str) -> adw::MessageDialog {
    let dialog = adw::MessageDialog::new(Some(parent), Some(title), None);

    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(24)
        .margin_end(24)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    let progress = gtk::ProgressBar::builder()
        .show_text(true)
        .build();
    box_.append(&progress);

    let status = gtk::Label::builder()
        .label("Processing...")
        .css_classes(vec!["dim-label"])
        .build();
    box_.append(&status);

    dialog.set_extra_child(Some(&box_));
    dialog.add_response("cancel", "Cancel");

    dialog
}

/// Create a text input dialog.
pub fn create_input_dialog(
    parent: &impl IsA<gtk::Window>,
    heading: &str,
    body: &str,
    placeholder: &str,
    initial_value: &str,
) -> (adw::MessageDialog, gtk::Entry) {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), Some(body));

    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .text(initial_value)
        .margin_start(24)
        .margin_end(24)
        .build();

    dialog.set_extra_child(Some(&entry));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", "OK");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));

    (dialog, entry.clone())
}

/// Create a preset selection dialog.
pub fn create_preset_dialog(parent: &impl IsA<gtk::Window>, presets: &[(&str, &str)]) -> adw::MessageDialog {
    let dialog = adw::MessageDialog::new(Some(parent), Some("Select Preset"), Some("Choose a preset to apply:"));

    let scroll = gtk::ScrolledWindow::builder()
        .max_content_height(300)
        .propagate_natural_height(true)
        .margin_start(12)
        .margin_end(12)
        .build();

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(vec!["boxed-list"])
        .build();

    for (name, description) in presets {
        let row = adw::ActionRow::builder()
            .title(*name)
            .subtitle(*description)
            .activatable(true)
            .build();
        list.append(&row);
    }

    scroll.set_child(Some(&list));
    dialog.set_extra_child(Some(&scroll));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("apply", "Apply");
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);

    dialog
}

/// Create a rename result dialog.
pub fn create_result_dialog(
    parent: &impl IsA<gtk::Window>,
    success_count: usize,
    error_count: usize,
    errors: &[(String, String)],
) -> adw::MessageDialog {
    let (heading, body) = if error_count == 0 {
        (
            "Rename Complete".to_string(),
            format!("Successfully renamed {} files.", success_count),
        )
    } else if success_count == 0 {
        (
            "Rename Failed".to_string(),
            format!("Failed to rename {} files.", error_count),
        )
    } else {
        (
            "Rename Completed with Errors".to_string(),
            format!(
                "Renamed {} files successfully.\n{} files failed.",
                success_count, error_count
            ),
        )
    };

    let dialog = adw::MessageDialog::new(Some(parent), Some(&heading), Some(&body));

    // Add error details if there are errors
    if !errors.is_empty() {
        let scroll = gtk::ScrolledWindow::builder()
            .max_content_height(200)
            .propagate_natural_height(true)
            .margin_top(12)
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(vec!["boxed-list"])
            .build();

        for (file, error) in errors.iter().take(10) {
            let row = adw::ActionRow::builder()
                .title(file)
                .subtitle(error)
                .build();
            row.add_prefix(&gtk::Image::builder()
                .icon_name("dialog-error-symbolic")
                .css_classes(vec!["error"])
                .build());
            list.append(&row);
        }

        if errors.len() > 10 {
            let more = adw::ActionRow::builder()
                .title(&format!("... and {} more errors", errors.len() - 10))
                .css_classes(vec!["dim-label"])
                .build();
            list.append(&more);
        }

        scroll.set_child(Some(&list));
        dialog.set_extra_child(Some(&scroll));
    }

    dialog.add_response("ok", "OK");
    dialog.set_default_response(Some("ok"));

    dialog
}
