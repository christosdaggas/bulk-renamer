//! Preferences window.

use libadwaita as adw;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4 as gtk;
use gtk::{gio, glib};
use std::cell::RefCell;

use crate::core::types::AppSettings;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct PreferencesWindow {
        pub settings: RefCell<AppSettings>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesWindow {
        const NAME: &'static str = "RenamerPreferencesWindow";
        type Type = super::PreferencesWindow;
        type ParentType = adw::PreferencesWindow;
    }

    impl ObjectImpl for PreferencesWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_ui();
        }
    }

    impl WidgetImpl for PreferencesWindow {}
    impl WindowImpl for PreferencesWindow {}
    impl AdwWindowImpl for PreferencesWindow {}
    impl PreferencesWindowImpl for PreferencesWindow {}
}

glib::wrapper! {
    pub struct PreferencesWindow(ObjectSubclass<imp::PreferencesWindow>)
        @extends adw::PreferencesWindow, adw::Window, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl PreferencesWindow {
    pub fn new(parent: &impl IsA<gtk::Window>) -> Self {
        let win: Self = glib::Object::builder()
            .property("transient-for", parent)
            .property("modal", true)
            .property("title", "Preferences")
            .build();
        win
    }

    fn setup_ui(&self) {
        self.set_default_size(500, 500);

        // Appearance page
        let appearance_page = adw::PreferencesPage::builder()
            .title("Appearance")
            .icon_name("applications-graphics-symbolic")
            .build();

        // Theme group
        let theme_group = adw::PreferencesGroup::builder()
            .title("Theme")
            .build();

        let theme_row = adw::ComboRow::builder()
            .title("Color Scheme")
            .subtitle("Choose the application color scheme")
            .build();

        let theme_model = gtk::StringList::new(&[
            "Follow System",
            "Light",
            "Dark",
        ]);
        theme_row.set_model(Some(&theme_model));
        
        // Set initial selection based on current theme
        let style_manager = adw::StyleManager::default();
        match style_manager.color_scheme() {
            adw::ColorScheme::ForceLight => theme_row.set_selected(1),
            adw::ColorScheme::ForceDark => theme_row.set_selected(2),
            _ => theme_row.set_selected(0),
        }

        theme_row.connect_selected_notify(|row| {
            let style_manager = adw::StyleManager::default();
            match row.selected() {
                0 => style_manager.set_color_scheme(adw::ColorScheme::Default),
                1 => style_manager.set_color_scheme(adw::ColorScheme::ForceLight),
                2 => style_manager.set_color_scheme(adw::ColorScheme::ForceDark),
                _ => {}
            }
        });

        theme_group.add(&theme_row);
        appearance_page.add(&theme_group);


        // Preview group
        let preview_group = adw::PreferencesGroup::builder()
            .title("Preview")
            .build();

        let live_preview = adw::SwitchRow::builder()
            .title("Live Preview")
            .subtitle("Update preview automatically as you type")
            .active(true)
            .build();
        preview_group.add(&live_preview);

        let show_unchanged = adw::SwitchRow::builder()
            .title("Show Unchanged Files")
            .subtitle("Display files that won't be renamed")
            .active(true)
            .build();
        preview_group.add(&show_unchanged);

        let highlight_changes = adw::SwitchRow::builder()
            .title("Highlight Changes")
            .subtitle("Visually highlight the parts of names that will change")
            .active(true)
            .build();
        preview_group.add(&highlight_changes);

        appearance_page.add(&preview_group);

        self.add(&appearance_page);

        // Behavior page
        let behavior_page = adw::PreferencesPage::builder()
            .title("Behavior")
            .icon_name("preferences-system-symbolic")
            .build();

        // File handling group
        let files_group = adw::PreferencesGroup::builder()
            .title("File Handling")
            .build();

        let confirm_rename = adw::SwitchRow::builder()
            .title("Confirm Before Rename")
            .subtitle("Show confirmation dialog before renaming files")
            .active(true)
            .build();
        files_group.add(&confirm_rename);

        let recursive = adw::SwitchRow::builder()
            .title("Include Subdirectories")
            .subtitle("Process files in subdirectories when adding folders")
            .active(false)
            .build();
        files_group.add(&recursive);

        let hidden_files = adw::SwitchRow::builder()
            .title("Show Hidden Files")
            .subtitle("Include files starting with a dot")
            .active(false)
            .build();
        files_group.add(&hidden_files);

        behavior_page.add(&files_group);

        // Undo group
        let undo_group = adw::PreferencesGroup::builder()
            .title("Undo History")
            .build();

        let max_history = adw::SpinRow::builder()
            .title("Maximum History")
            .subtitle("Number of rename operations to keep in history")
            .adjustment(&gtk::Adjustment::new(50.0, 1.0, 1000.0, 1.0, 10.0, 0.0))
            .build();
        undo_group.add(&max_history);

        let auto_save = adw::SwitchRow::builder()
            .title("Auto-save Undo Scripts")
            .subtitle("Automatically save shell scripts for reverting renames")
            .active(true)
            .build();
        undo_group.add(&auto_save);

        behavior_page.add(&undo_group);

        self.add(&behavior_page);

        // Advanced page
        let advanced_page = adw::PreferencesPage::builder()
            .title("Advanced")
            .icon_name("applications-utilities-symbolic")
            .build();

        // Validation group
        let validation_group = adw::PreferencesGroup::builder()
            .title("Validation")
            .build();

        let check_conflicts = adw::SwitchRow::builder()
            .title("Check for Conflicts")
            .subtitle("Warn when renamed files would overwrite existing files")
            .active(true)
            .build();
        validation_group.add(&check_conflicts);

        let max_length_row = adw::SpinRow::builder()
            .title("Maximum Filename Length")
            .subtitle("Maximum allowed characters in filename")
            .adjustment(&gtk::Adjustment::new(255.0, 10.0, 255.0, 1.0, 10.0, 0.0))
            .build();
        validation_group.add(&max_length_row);

        advanced_page.add(&validation_group);

        // Logging group
        let logging_group = adw::PreferencesGroup::builder()
            .title("Logging")
            .build();

        let enable_logging = adw::SwitchRow::builder()
            .title("Enable Logging")
            .subtitle("Keep a log of all rename operations")
            .active(true)
            .build();
        logging_group.add(&enable_logging);

        let log_location = adw::ActionRow::builder()
            .title("Log Directory")
            .subtitle("~/.local/share/bulk-renamer/logs")
            .activatable(true)
            .build();
        log_location.add_suffix(&gtk::Button::builder()
            .icon_name("folder-open-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Open log directory")
            .css_classes(vec!["flat"])
            .build());
        logging_group.add(&log_location);

        advanced_page.add(&logging_group);

        // Metadata group
        let metadata_group = adw::PreferencesGroup::builder()
            .title("Metadata")
            .build();

        let cache_metadata = adw::SwitchRow::builder()
            .title("Cache Metadata")
            .subtitle("Cache file metadata for faster preview updates")
            .active(true)
            .build();
        metadata_group.add(&cache_metadata);

        let exif_enabled = adw::SwitchRow::builder()
            .title("Read EXIF Data")
            .subtitle("Extract metadata from image files")
            .active(true)
            .build();
        metadata_group.add(&exif_enabled);

        let id3_enabled = adw::SwitchRow::builder()
            .title("Read Audio Tags")
            .subtitle("Extract metadata from audio files")
            .active(true)
            .build();
        metadata_group.add(&id3_enabled);

        advanced_page.add(&metadata_group);

        self.add(&advanced_page);
    }

    pub fn get_settings(&self) -> AppSettings {
        self.imp().settings.borrow().clone()
    }

    pub fn set_settings(&self, settings: AppSettings) {
        *self.imp().settings.borrow_mut() = settings;
    }
}
