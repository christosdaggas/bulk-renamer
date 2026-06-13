//! Header bar component.

use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;

use super::ThemePopover;

/// Create the main header bar for the application.
pub fn create_header_bar() -> adw::HeaderBar {
    let header = adw::HeaderBar::builder()
        .build();

    // Add files button
    let add_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Files")
        .build();
    add_button.set_action_name(Some("win.add-files"));
    header.pack_start(&add_button);

    // Add folder button
    let folder_button = gtk::Button::builder()
        .icon_name("folder-new-symbolic")
        .tooltip_text("Add Folder")
        .build();
    folder_button.set_action_name(Some("win.add-folder"));
    header.pack_start(&folder_button);

    // Primary menu with theme popover
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .build();
    let theme_popover = ThemePopover::new();
    menu_button.set_popover(Some(&theme_popover));
    header.pack_end(&menu_button);

    // Rename button
    let rename_button = gtk::Button::builder()
        .label("Rename")
        .css_classes(vec!["suggested-action"])
        .tooltip_text("Apply rename to all files")
        .build();
    rename_button.set_action_name(Some("win.execute-rename"));
    header.pack_end(&rename_button);

    // Undo/Redo buttons
    let redo_button = gtk::Button::builder()
        .icon_name("edit-redo-symbolic")
        .tooltip_text("Redo")
        .build();
    redo_button.set_action_name(Some("win.redo"));

    let undo_button = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .tooltip_text("Undo")
        .build();
    undo_button.set_action_name(Some("win.undo"));

    header.pack_end(&redo_button);
    header.pack_end(&undo_button);

    header
}

/// Create a secondary header bar for the preview panel.
pub fn create_preview_header() -> adw::HeaderBar {
    let header = adw::HeaderBar::builder()
        .show_title(false)
        .build();

    // Search button
    let search_button = gtk::ToggleButton::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text("Search files")
        .build();
    header.pack_start(&search_button);

    // Sort menu
    let sort_menu = gio::Menu::new();
    sort_menu.append(Some("Name"), Some("win.sort::name"));
    sort_menu.append(Some("Extension"), Some("win.sort::ext"));
    sort_menu.append(Some("Size"), Some("win.sort::size"));
    sort_menu.append(Some("Date Modified"), Some("win.sort::date"));
    sort_menu.append(Some("Path"), Some("win.sort::path"));

    let sort_button = gtk::MenuButton::builder()
        .icon_name("view-sort-ascending-symbolic")
        .menu_model(&sort_menu)
        .tooltip_text("Sort files")
        .build();
    header.pack_end(&sort_button);

    // View toggle
    let view_button = gtk::ToggleButton::builder()
        .icon_name("view-list-symbolic")
        .tooltip_text("Toggle compact view")
        .build();
    header.pack_end(&view_button);

    header
}

/// Create a toolbar for quick rule actions.
pub fn create_rule_toolbar() -> gtk::Box {
    let toolbar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();

    // Quick action buttons
    let lowercase_btn = gtk::Button::builder()
        .icon_name("format-text-plaintext-symbolic")
        .tooltip_text("Lowercase")
        .css_classes(vec!["flat"])
        .build();
    lowercase_btn.set_action_name(Some("win.quick-lowercase"));
    toolbar.append(&lowercase_btn);

    let uppercase_btn = gtk::Button::builder()
        .icon_name("format-text-rich-symbolic")
        .tooltip_text("Uppercase")
        .css_classes(vec!["flat"])
        .build();
    uppercase_btn.set_action_name(Some("win.quick-uppercase"));
    toolbar.append(&uppercase_btn);

    let title_btn = gtk::Button::builder()
        .icon_name("format-text-bold-symbolic")
        .tooltip_text("Title Case")
        .css_classes(vec!["flat"])
        .build();
    title_btn.set_action_name(Some("win.quick-titlecase"));
    toolbar.append(&title_btn);

    toolbar.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let number_btn = gtk::Button::builder()
        .icon_name("view-list-ordered-symbolic")
        .tooltip_text("Add Numbers")
        .css_classes(vec!["flat"])
        .build();
    number_btn.set_action_name(Some("win.quick-number"));
    toolbar.append(&number_btn);

    let date_btn = gtk::Button::builder()
        .icon_name("x-office-calendar-symbolic")
        .tooltip_text("Add Date")
        .css_classes(vec!["flat"])
        .build();
    date_btn.set_action_name(Some("win.quick-date"));
    toolbar.append(&date_btn);

    toolbar.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let trim_btn = gtk::Button::builder()
        .icon_name("edit-cut-symbolic")
        .tooltip_text("Trim Whitespace")
        .css_classes(vec!["flat"])
        .build();
    trim_btn.set_action_name(Some("win.quick-trim"));
    toolbar.append(&trim_btn);

    let cleanup_btn = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text("Clean Filename")
        .css_classes(vec!["flat"])
        .build();
    cleanup_btn.set_action_name(Some("win.quick-cleanup"));
    toolbar.append(&cleanup_btn);

    // Spacer
    let spacer = gtk::Box::builder()
        .hexpand(true)
        .build();
    toolbar.append(&spacer);

    // Clear rules button
    let clear_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Clear all rules")
        .css_classes(vec!["flat"])
        .build();
    clear_btn.set_action_name(Some("win.clear-rules"));
    toolbar.append(&clear_btn);

    toolbar
}

/// Create a bottom action bar.
pub fn create_action_bar() -> gtk::ActionBar {
    let bar = gtk::ActionBar::builder()
        .build();

    // File count label
    let count_label = gtk::Label::builder()
        .label("0 files")
        .css_classes(vec!["dim-label"])
        .build();
    bar.pack_start(&count_label);

    // Selected count
    let selected_label = gtk::Label::builder()
        .label("")
        .css_classes(vec!["dim-label"])
        .visible(false)
        .build();
    bar.pack_start(&selected_label);

    // Status
    let status = gtk::Label::builder()
        .label("Ready")
        .css_classes(vec!["dim-label"])
        .build();
    bar.pack_end(&status);

    bar
}
