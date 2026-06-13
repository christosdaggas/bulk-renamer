//! Main application window with three-panel layout.

use crate::core::{AppSettings, FileEntry, RenameBatch, RenameConfig, RenamePreview, RenameStatus, RenameTarget};
use crate::core::types::ThemePreference;
use crate::engine::{RenameEngine, RenameValidator};
use crate::presets::{Preset, PresetManager};
use crate::undo::{RenameLogEntry, RenameLogger, UndoManager};
use async_channel;
use libadwaita as adw;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4 as gtk;
use gtk::{gio, glib};
use glib::clone;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct RenamerWindow {
        pub files: RefCell<Vec<FileEntry>>,
        pub previews: RefCell<Vec<RenamePreview>>,
        pub config: RefCell<RenameConfig>,
        pub target: RefCell<RenameTarget>,
        pub settings: RefCell<AppSettings>,
        pub file_list: RefCell<Option<gtk::ListBox>>,
        pub preview_list: RefCell<Option<gtk::ListBox>>,
        pub rules_list: RefCell<Option<gtk::ListBox>>,
        pub files_count_label: RefCell<Option<gtk::Label>>,
        pub selected_count_label: RefCell<Option<gtk::Label>>,
        pub preview_count_label: RefCell<Option<gtk::Label>>,
        pub rename_button: RefCell<Option<gtk::Button>>,
        pub undo_manager: RefCell<UndoManager>,
        pub logger: RefCell<RenameLogger>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RenamerWindow {
        const NAME: &'static str = "GnomeRenamerWindow";
        type Type = super::RenamerWindow;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for RenamerWindow {}
    impl WidgetImpl for RenamerWindow {}
    impl WindowImpl for RenamerWindow {}
    impl ApplicationWindowImpl for RenamerWindow {}
    impl AdwApplicationWindowImpl for RenamerWindow {}
}

glib::wrapper! {
    pub struct RenamerWindow(ObjectSubclass<imp::RenamerWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl RenamerWindow {
    pub fn new(app: &impl IsA<gtk::Application>) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", app)
            .property("default-width", 1400)
            .property("default-height", 800)
            .property("title", "Bulk Renamer")
            .build();

        window.setup_ui();
        window.setup_actions();
        window.load_settings();

        window
    }

    fn setup_ui(&self) {
        // Create header bar
        let header = self.create_header_bar();

        // Create main content with three panels
        let content = self.create_main_content();

        // Create toolbar view
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&content));

        self.set_content(Some(&toolbar_view));
    }

    fn create_header_bar(&self) -> adw::HeaderBar {
        let header = adw::HeaderBar::new();

        // Title
        let title = adw::WindowTitle::new("Bulk Renamer", "");
        header.set_title_widget(Some(&title));

        // Right side: Rename button and menu
        let rename_btn = gtk::Button::builder()
            .label("Rename")
            .sensitive(false)
            .build();
        rename_btn.add_css_class("suggested-action");

        rename_btn.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.execute_rename();
            }
        ));

        // Menu button with custom popover
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Menu")
            .build();
        menu_btn.add_css_class("flat");

        // Create custom popover with theme selector
        let popover = self.create_main_menu_popover();
        menu_btn.set_popover(Some(&popover));

        header.pack_end(&menu_btn);
        header.pack_end(&rename_btn);
        self.imp().rename_button.replace(Some(rename_btn));

        header
    }

    fn create_main_menu_popover(&self) -> gtk::Popover {
        let popover = gtk::Popover::new();
        popover.add_css_class("menu");

        let main_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .width_request(280)
            .build();

        // Theme selector section
        let theme_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(18)
            .halign(gtk::Align::Center)
            .margin_top(18)
            .margin_bottom(18)
            .build();

        // Create theme toggle buttons with larger circles and checkmarks
        let default_btn = gtk::ToggleButton::new();
        let light_btn = gtk::ToggleButton::new();
        let dark_btn = gtk::ToggleButton::new();

        // Helper to create theme button content with optional checkmark
        fn create_theme_content(css_class: &str, is_selected: bool) -> gtk::Overlay {
            let overlay = gtk::Overlay::new();
            
            let icon = gtk::Box::builder()
                .width_request(44)
                .height_request(44)
                .build();
            icon.add_css_class("theme-selector");
            icon.add_css_class(css_class);
            overlay.set_child(Some(&icon));
            
            if is_selected {
                let check = gtk::Image::from_icon_name("object-select-symbolic");
                check.add_css_class("theme-check");
                check.set_halign(gtk::Align::Center);
                check.set_valign(gtk::Align::Center);
                overlay.add_overlay(&check);
            }
            
            overlay
        }

        // Set initial content
        default_btn.set_child(Some(&create_theme_content("theme-default", false)));
        default_btn.set_tooltip_text(Some("System"));
        default_btn.add_css_class("flat");
        default_btn.add_css_class("circular");
        default_btn.add_css_class("theme-button");

        light_btn.set_child(Some(&create_theme_content("theme-light", false)));
        light_btn.set_tooltip_text(Some("Light"));
        light_btn.add_css_class("flat");
        light_btn.add_css_class("circular");
        light_btn.add_css_class("theme-button");

        dark_btn.set_child(Some(&create_theme_content("theme-dark", false)));
        dark_btn.set_tooltip_text(Some("Dark"));
        dark_btn.add_css_class("flat");
        dark_btn.add_css_class("circular");
        dark_btn.add_css_class("theme-button");

        // Group the toggle buttons
        light_btn.set_group(Some(&default_btn));
        dark_btn.set_group(Some(&default_btn));

        // Set initial state based on current theme
        let style_manager = adw::StyleManager::default();
        // Update checkmark on active button based on current theme
        let update_theme_buttons = |default: &gtk::ToggleButton, light: &gtk::ToggleButton, dark: &gtk::ToggleButton| {
            let style_manager = adw::StyleManager::default();
            let (def_sel, light_sel, dark_sel) = match style_manager.color_scheme() {
                adw::ColorScheme::ForceLight => (false, true, false),
                adw::ColorScheme::ForceDark => (false, false, true),
                _ => (true, false, false),
            };
            default.set_child(Some(&create_theme_content("theme-default", def_sel)));
            light.set_child(Some(&create_theme_content("theme-light", light_sel)));
            dark.set_child(Some(&create_theme_content("theme-dark", dark_sel)));
        };
        
        update_theme_buttons(&default_btn, &light_btn, &dark_btn);
        
        match style_manager.color_scheme() {
            adw::ColorScheme::Default | adw::ColorScheme::PreferLight | adw::ColorScheme::PreferDark => {
                default_btn.set_active(true);
            }
            adw::ColorScheme::ForceLight => {
                light_btn.set_active(true);
            }
            adw::ColorScheme::ForceDark => {
                dark_btn.set_active(true);
            }
            _ => {
                default_btn.set_active(true);
            }
        }

        // Connect theme button signals
        let light_btn_clone = light_btn.clone();
        let dark_btn_clone = dark_btn.clone();
        default_btn.connect_toggled(clone!(
            #[weak(rename_to = window)]
            self,
            move |btn| {
                if btn.is_active() {
                    window.set_theme(ThemePreference::System);
                    btn.set_child(Some(&create_theme_content("theme-default", true)));
                    light_btn_clone.set_child(Some(&create_theme_content("theme-light", false)));
                    dark_btn_clone.set_child(Some(&create_theme_content("theme-dark", false)));
                }
            }
        ));

        let default_btn_clone = default_btn.clone();
        let dark_btn_clone2 = dark_btn.clone();
        light_btn.connect_toggled(clone!(
            #[weak(rename_to = window)]
            self,
            move |btn| {
                if btn.is_active() {
                    window.set_theme(ThemePreference::Light);
                    btn.set_child(Some(&create_theme_content("theme-light", true)));
                    default_btn_clone.set_child(Some(&create_theme_content("theme-default", false)));
                    dark_btn_clone2.set_child(Some(&create_theme_content("theme-dark", false)));
                }
            }
        ));

        let default_btn_clone2 = default_btn.clone();
        let light_btn_clone2 = light_btn.clone();
        dark_btn.connect_toggled(clone!(
            #[weak(rename_to = window)]
            self,
            move |btn| {
                if btn.is_active() {
                    window.set_theme(ThemePreference::Dark);
                    btn.set_child(Some(&create_theme_content("theme-dark", true)));
                    default_btn_clone2.set_child(Some(&create_theme_content("theme-default", false)));
                    light_btn_clone2.set_child(Some(&create_theme_content("theme-light", false)));
                }
            }
        ));

        theme_box.append(&default_btn);
        theme_box.append(&light_btn);
        theme_box.append(&dark_btn);
        main_box.append(&theme_box);

        // Separator
        let sep1 = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep1.set_margin_start(12);
        sep1.set_margin_end(12);
        main_box.append(&sep1);

        // Menu items
        let menu_list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();

        // Presets row
        let presets_row = gtk::Button::new();
        let presets_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        presets_content.set_margin_start(6);
        presets_content.set_margin_end(6);
        presets_content.set_margin_top(8);
        presets_content.set_margin_bottom(8);
        let presets_icon = gtk::Image::from_icon_name("document-save-symbolic");
        let presets_label = gtk::Label::new(Some("Presets"));
        presets_label.set_halign(gtk::Align::Start);
        presets_label.set_hexpand(true);
        let presets_arrow = gtk::Image::from_icon_name("go-next-symbolic");
        presets_content.append(&presets_icon);
        presets_content.append(&presets_label);
        presets_content.append(&presets_arrow);
        presets_row.set_child(Some(&presets_content));
        presets_row.add_css_class("flat");
        presets_row.add_css_class("menu-item");
        menu_list.append(&presets_row);

        main_box.append(&menu_list);

        // Separator
        let sep2 = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep2.set_margin_start(12);
        sep2.set_margin_end(12);
        main_box.append(&sep2);

        // Tools section
        let tools_list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();

        // Import row
        let import_row = gtk::Button::new();
        let import_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        import_content.set_margin_start(6);
        import_content.set_margin_end(6);
        import_content.set_margin_top(8);
        import_content.set_margin_bottom(8);
        let import_icon = gtk::Image::from_icon_name("document-open-symbolic");
        let import_label = gtk::Label::new(Some("Import from CSV…"));
        import_label.set_halign(gtk::Align::Start);
        import_label.set_hexpand(true);
        import_content.append(&import_icon);
        import_content.append(&import_label);
        import_row.set_child(Some(&import_content));
        import_row.add_css_class("flat");
        import_row.add_css_class("menu-item");
        tools_list.append(&import_row);

        // Export row
        let export_row = gtk::Button::new();
        let export_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        export_content.set_margin_start(6);
        export_content.set_margin_end(6);
        export_content.set_margin_top(8);
        export_content.set_margin_bottom(8);
        let export_icon = gtk::Image::from_icon_name("document-save-as-symbolic");
        let export_label = gtk::Label::new(Some("Export Log…"));
        export_label.set_halign(gtk::Align::Start);
        export_label.set_hexpand(true);
        export_content.append(&export_icon);
        export_content.append(&export_label);
        export_row.set_child(Some(&export_content));
        export_row.add_css_class("flat");
        export_row.add_css_class("menu-item");
        tools_list.append(&export_row);

        // Undo row
        let undo_row = gtk::Button::new();
        let undo_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        undo_content.set_margin_start(6);
        undo_content.set_margin_end(6);
        undo_content.set_margin_top(8);
        undo_content.set_margin_bottom(8);
        let undo_icon = gtk::Image::from_icon_name("edit-undo-symbolic");
        let undo_label = gtk::Label::new(Some("Undo Last Rename"));
        undo_label.set_halign(gtk::Align::Start);
        undo_label.set_hexpand(true);
        undo_content.append(&undo_icon);
        undo_content.append(&undo_label);
        undo_row.set_child(Some(&undo_content));
        undo_row.add_css_class("flat");
        undo_row.add_css_class("menu-item");
        tools_list.append(&undo_row);

        // Redo row
        let redo_row = gtk::Button::new();
        let redo_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        redo_content.set_margin_start(6);
        redo_content.set_margin_end(6);
        redo_content.set_margin_top(8);
        redo_content.set_margin_bottom(8);
        let redo_icon = gtk::Image::from_icon_name("edit-redo-symbolic");
        let redo_label = gtk::Label::new(Some("Redo Rename"));
        redo_label.set_halign(gtk::Align::Start);
        redo_label.set_hexpand(true);
        redo_content.append(&redo_icon);
        redo_content.append(&redo_label);
        redo_row.set_child(Some(&redo_content));
        redo_row.add_css_class("flat");
        redo_row.add_css_class("menu-item");
        tools_list.append(&redo_row);

        // Preferences row
        let preferences_row = gtk::Button::new();
        let preferences_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        preferences_content.set_margin_start(6);
        preferences_content.set_margin_end(6);
        preferences_content.set_margin_top(8);
        preferences_content.set_margin_bottom(8);
        let preferences_icon = gtk::Image::from_icon_name("preferences-system-symbolic");
        let preferences_label = gtk::Label::new(Some("Preferences"));
        preferences_label.set_halign(gtk::Align::Start);
        preferences_label.set_hexpand(true);
        preferences_content.append(&preferences_icon);
        preferences_content.append(&preferences_label);
        preferences_row.set_child(Some(&preferences_content));
        preferences_row.add_css_class("flat");
        preferences_row.add_css_class("menu-item");
        tools_list.append(&preferences_row);

        main_box.append(&tools_list);

        // Separator
        let sep3 = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep3.set_margin_start(12);
        sep3.set_margin_end(12);
        main_box.append(&sep3);

        // About section
        let about_list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();

        // About row
        let about_row = gtk::Button::new();
        let about_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        about_content.set_margin_start(6);
        about_content.set_margin_end(6);
        about_content.set_margin_top(8);
        about_content.set_margin_bottom(8);
        let about_icon = gtk::Image::from_icon_name("help-about-symbolic");
        let about_label = gtk::Label::new(Some("About"));
        about_label.set_halign(gtk::Align::Start);
        about_label.set_hexpand(true);
        about_content.append(&about_icon);
        about_content.append(&about_label);
        about_row.set_child(Some(&about_content));
        about_row.add_css_class("flat");
        about_row.add_css_class("menu-item");
        about_list.append(&about_row);

        main_box.append(&about_list);

        // Connect button click events
        let popover_weak = popover.downgrade();
        presets_row.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if let Some(pop) = popover_weak.upgrade() {
                    pop.popdown();
                }
                window.show_presets_submenu();
            }
        ));

        let popover_weak = popover.downgrade();
        import_row.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if let Some(pop) = popover_weak.upgrade() {
                    pop.popdown();
                }
                window.show_import_csv_dialog();
            }
        ));

        let popover_weak = popover.downgrade();
        export_row.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if let Some(pop) = popover_weak.upgrade() {
                    pop.popdown();
                }
                window.show_export_log_dialog();
            }
        ));

        let popover_weak = popover.downgrade();
        undo_row.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if let Some(pop) = popover_weak.upgrade() {
                    pop.popdown();
                }
                window.undo_last_batch();
            }
        ));

        let popover_weak = popover.downgrade();
        redo_row.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if let Some(pop) = popover_weak.upgrade() {
                    pop.popdown();
                }
                window.redo_last_batch();
            }
        ));

        let popover_weak = popover.downgrade();
        preferences_row.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if let Some(pop) = popover_weak.upgrade() {
                    pop.popdown();
                }
                window.show_preferences_dialog();
            }
        ));

        let popover_weak = popover.downgrade();
        about_row.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if let Some(pop) = popover_weak.upgrade() {
                    pop.popdown();
                }
                window.show_about_dialog();
            }
        ));

        popover.set_child(Some(&main_box));
        popover
    }

    fn show_presets_submenu(&self) {
        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("Presets"),
            Some("Choose an action:"),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("save", "Save Preset…");
        dialog.add_response("load", "Load Preset…");
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

        dialog.connect_response(None, clone!(
            #[weak(rename_to = window)]
            self,
            move |_, response| {
                match response {
                    "save" => window.show_save_preset_dialog(),
                    "load" => window.show_load_preset_dialog(),
                    _ => {}
                }
            }
        ));

        dialog.present();
    }

    fn show_about_dialog(&self) {
        let about = adw::AboutWindow::builder()
            .transient_for(self)
            .application_name("Bulk Renamer")
            .application_icon("com.chrisdaggas.bulk-renamer")
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Christos A. Daggas")
            .license_type(gtk::License::MitX11)
            .website("https://chrisdaggas.com")
            .issue_url("https://github.com/christosdaggas/bulk-renamer/issues")
            .copyright("© 2024-2026 Christos A. Daggas")
            .comments("A powerful bulk file renaming application for Linux")
            .build();

        about.add_credit_section(Some("Created by"), &["Christos A. Daggas"]);
        about.present();
    }

    fn create_main_content(&self) -> gtk::Widget {
        // Main horizontal box with three panels
        let main_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();

        // Left panel: File browser
        let files_panel = self.create_files_panel();
        files_panel.set_width_request(320);
        
        // Center panel: Rules
        let rules_panel = self.create_rules_panel();
        rules_panel.set_width_request(300);
        
        // Right panel: Preview
        let preview_panel = self.create_preview_panel();
        preview_panel.set_hexpand(true);

        // Add separators
        let sep1 = gtk::Separator::new(gtk::Orientation::Vertical);
        let sep2 = gtk::Separator::new(gtk::Orientation::Vertical);

        main_box.append(&files_panel);
        main_box.append(&sep1);
        main_box.append(&rules_panel);
        main_box.append(&sep2);
        main_box.append(&preview_panel);

        main_box.into()
    }

    fn create_files_panel(&self) -> gtk::Widget {
        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        panel.add_css_class("view");

        // Header with title and buttons
        let header_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(6)
            .build();

        let title_label = gtk::Label::builder()
            .label("Files")
            .css_classes(vec!["title-4"])
            .hexpand(true)
            .xalign(0.0)
            .build();

        let add_files_btn = gtk::Button::builder()
            .icon_name("document-open-symbolic")
            .tooltip_text("Add Files")
            .build();
        add_files_btn.add_css_class("flat");
        add_files_btn.add_css_class("circular");

        let add_folder_btn = gtk::Button::builder()
            .icon_name("folder-open-symbolic")
            .tooltip_text("Add Folder")
            .build();
        add_folder_btn.add_css_class("flat");
        add_folder_btn.add_css_class("circular");

        let clear_btn = gtk::Button::builder()
            .icon_name("edit-clear-all-symbolic")
            .tooltip_text("Clear All")
            .build();
        clear_btn.add_css_class("flat");
        clear_btn.add_css_class("circular");

        header_box.append(&title_label);
        header_box.append(&add_files_btn);
        header_box.append(&add_folder_btn);
        header_box.append(&clear_btn);
        panel.append(&header_box);

        // Status bar
        let status_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .spacing(12)
            .build();

        let files_count = gtk::Label::builder()
            .label("0 files")
            .css_classes(vec!["dim-label", "caption"])
            .build();

        let selected_count = gtk::Label::builder()
            .label("0 selected")
            .css_classes(vec!["dim-label", "caption"])
            .build();

        status_box.append(&files_count);
        status_box.append(&selected_count);
        panel.append(&status_box);

        // Store labels for later updates
        self.imp().files_count_label.replace(Some(files_count));
        self.imp().selected_count_label.replace(Some(selected_count));

        // File list with multi-selection
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .build();

        let file_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Multiple)
            .css_classes(vec!["navigation-sidebar"])
            .build();

        // Enable keyboard multi-select with Ctrl/Shift
        file_list.set_activate_on_single_click(false);

        // Placeholder
        let placeholder = adw::StatusPage::builder()
            .icon_name("folder-documents-symbolic")
            .title("No Files")
            .description("Drop files here or click + to add")
            .build();
        placeholder.add_css_class("compact");
        file_list.set_placeholder(Some(&placeholder));

        // Selection changed handler
        file_list.connect_selected_rows_changed(clone!(
            #[weak(rename_to = window)]
            self,
            move |list| {
                let count = list.selected_rows().len();
                let label_ref = window.imp().selected_count_label.borrow();
                if let Some(label) = label_ref.as_ref() {
                    label.set_label(&format!("{} selected", count));
                }
            }
        ));

        scroll.set_child(Some(&file_list));
        panel.append(&scroll);

        // Store file list reference
        self.imp().file_list.replace(Some(file_list.clone()));

        // Bottom action bar
        let action_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(12)
            .spacing(6)
            .build();

        let select_all_btn = gtk::Button::builder()
            .label("Select All")
            .hexpand(true)
            .build();
        select_all_btn.add_css_class("flat");

        let remove_selected_btn = gtk::Button::builder()
            .label("Remove Selected")
            .hexpand(true)
            .build();
        remove_selected_btn.add_css_class("flat");

        action_bar.append(&select_all_btn);
        action_bar.append(&remove_selected_btn);
        panel.append(&action_bar);

        // Connect button signals
        add_files_btn.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.show_add_files_dialog();
            }
        ));

        add_folder_btn.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.show_add_folder_dialog();
            }
        ));

        clear_btn.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.clear_files();
            }
        ));

        select_all_btn.connect_clicked(clone!(
            #[weak]
            file_list,
            move |_| {
                file_list.select_all();
            }
        ));

        remove_selected_btn.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.remove_selected_files();
            }
        ));

        // Setup drag and drop
        let drop_target = gtk::DropTarget::new(gio::File::static_type(), gdk4::DragAction::COPY);
        drop_target.connect_drop(clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                if let Ok(file) = value.get::<gio::File>() {
                    if let Some(path) = file.path() {
                        window.add_path(path);
                        return true;
                    }
                }
                false
            }
        ));
        file_list.add_controller(drop_target);

        panel.into()
    }

    fn create_rules_panel(&self) -> gtk::Widget {
        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        panel.add_css_class("view");

        // Header
        let header_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(6)
            .build();

        let title_label = gtk::Label::builder()
            .label("Rules")
            .css_classes(vec!["title-4"])
            .hexpand(true)
            .xalign(0.0)
            .build();

        let add_rule_btn = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add Rule")
            .build();
        add_rule_btn.add_css_class("flat");
        add_rule_btn.add_css_class("circular");

        header_box.append(&title_label);
        header_box.append(&add_rule_btn);
        panel.append(&header_box);

        // Target type selector
        let target_group = adw::PreferencesGroup::new();
        target_group.set_margin_start(12);
        target_group.set_margin_end(12);
        target_group.set_margin_bottom(6);

        let target_row = adw::ComboRow::builder()
            .title("Apply to")
            .model(&gtk::StringList::new(&["Files only", "Folders only", "Both"]))
            .build();
        target_group.add(&target_row);
        panel.append(&target_group);

        // Rules list
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .build();

        let rules_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(vec!["boxed-list"])
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        // Enable drag and drop reordering
        rules_list.set_show_separators(false);

        rules_list.set_placeholder(Some(&gtk::Label::builder()
            .label("No rules added\nClick + to add a rule")
            .css_classes(vec!["dim-label"])
            .margin_top(24)
            .margin_bottom(24)
            .justify(gtk::Justification::Center)
            .build()));

        scroll.set_child(Some(&rules_list));
        panel.append(&scroll);

        // Store rules_list for later reference
        self.imp().rules_list.replace(Some(rules_list.clone()));

        // Options
        let options_group = adw::PreferencesGroup::new();
        options_group.set_margin_start(12);
        options_group.set_margin_end(12);
        options_group.set_margin_bottom(12);

        let ext_switch = adw::SwitchRow::builder()
            .title("Process extension separately")
            .active(true)
            .build();
        options_group.add(&ext_switch);

        panel.append(&options_group);

        // Connect add rule button
        add_rule_btn.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            rules_list,
            move |_| {
                window.show_add_rule_dialog(&rules_list);
            }
        ));

        target_row.connect_selected_notify(clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                let target = match row.selected() {
                    1 => RenameTarget::FoldersOnly,
                    2 => RenameTarget::Both,
                    _ => RenameTarget::FilesOnly,
                };
                *window.imp().target.borrow_mut() = target;
                window.update_preview();
            }
        ));

        ext_switch.connect_active_notify(clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                window.imp().config.borrow_mut().separate_extension = row.is_active();
                window.update_preview();
            }
        ));

        panel.into()
    }

    fn create_preview_panel(&self) -> gtk::Widget {
        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        panel.add_css_class("view");

        // Header
        let header_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(6)
            .build();

        let title_label = gtk::Label::builder()
            .label("Preview")
            .css_classes(vec!["title-4"])
            .hexpand(true)
            .xalign(0.0)
            .build();

        let refresh_btn = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh Preview")
            .build();
        refresh_btn.add_css_class("flat");
        refresh_btn.add_css_class("circular");

        header_box.append(&title_label);
        header_box.append(&refresh_btn);
        panel.append(&header_box);

        // Status/stats bar
        let stats_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .spacing(12)
            .build();

        let preview_count = gtk::Label::builder()
            .label("0 will be renamed")
            .css_classes(vec!["dim-label", "caption"])
            .build();

        let conflicts_label = gtk::Label::builder()
            .label("")
            .css_classes(vec!["caption"])
            .build();

        stats_box.append(&preview_count);
        stats_box.append(&conflicts_label);
        panel.append(&stats_box);

        self.imp().preview_count_label.replace(Some(preview_count));

        // Column headers
        let headers = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .spacing(12)
            .build();

        let original_header = gtk::Label::builder()
            .label("Original")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(vec!["dim-label", "caption"])
            .build();

        let new_header = gtk::Label::builder()
            .label("New Name")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(vec!["dim-label", "caption"])
            .build();

        headers.append(&original_header);
        headers.append(&new_header);
        panel.append(&headers);

        // Preview list
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .build();

        let preview_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(vec!["boxed-list"])
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(12)
            .build();

        preview_list.set_placeholder(Some(&gtk::Label::builder()
            .label("Add files and rules to see preview")
            .css_classes(vec!["dim-label"])
            .margin_top(48)
            .margin_bottom(48)
            .build()));

        scroll.set_child(Some(&preview_list));
        panel.append(&scroll);

        self.imp().preview_list.replace(Some(preview_list));

        // Refresh button handler
        refresh_btn.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.update_preview();
            }
        ));

        panel.into()
    }

    fn setup_actions(&self) {
        // Execute rename action
        let execute_action = gio::SimpleAction::new("execute-rename", None);
        execute_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.execute_rename();
            }
        ));
        self.add_action(&execute_action);

        // Add files/folders actions
        let add_files_action = gio::SimpleAction::new("add-files", None);
        add_files_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_add_files_dialog();
            }
        ));
        self.add_action(&add_files_action);

        let add_folder_action = gio::SimpleAction::new("add-folder", None);
        add_folder_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_add_folder_dialog();
            }
        ));
        self.add_action(&add_folder_action);

        let clear_files_action = gio::SimpleAction::new("clear-files", None);
        clear_files_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.clear_files();
            }
        ));
        self.add_action(&clear_files_action);

        // Undo/redo actions
        let undo_action = gio::SimpleAction::new("undo", None);
        undo_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.undo_last_batch();
            }
        ));
        self.add_action(&undo_action);

        let redo_action = gio::SimpleAction::new("redo", None);
        redo_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.redo_last_batch();
            }
        ));
        self.add_action(&redo_action);

        // Preferences action
        let preferences_action = gio::SimpleAction::new("preferences", None);
        preferences_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_preferences_dialog();
            }
        ));
        self.add_action(&preferences_action);

        // Save preset action
        let save_preset_action = gio::SimpleAction::new("save-preset", None);
        save_preset_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_save_preset_dialog();
            }
        ));
        self.add_action(&save_preset_action);

        // Load preset action
        let load_preset_action = gio::SimpleAction::new("load-preset", None);
        load_preset_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_load_preset_dialog();
            }
        ));
        self.add_action(&load_preset_action);

        // Import CSV action
        let import_csv_action = gio::SimpleAction::new("import-csv", None);
        import_csv_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_import_csv_dialog();
            }
        ));
        self.add_action(&import_csv_action);

        // Export log action
        let export_log_action = gio::SimpleAction::new("export-log", None);
        export_log_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_export_log_dialog();
            }
        ));
        self.add_action(&export_log_action);

        let about_action = gio::SimpleAction::new("about", None);
        about_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                window.show_about_dialog();
            }
        ));
        self.add_action(&about_action);

        self.add_quick_rule_action("quick-lowercase", crate::core::CaseType::Lower);
        self.add_quick_rule_action("quick-uppercase", crate::core::CaseType::Upper);
        self.add_quick_rule_action("quick-titlecase", crate::core::CaseType::Title);

        let quick_number_action = gio::SimpleAction::new("quick-number", None);
        quick_number_action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                if let Some(rules_list) = window.imp().rules_list.borrow().as_ref() {
                    window.add_numbering_rule_at(rules_list, 1, 1, 2, 1, "_".to_string(), None);
                }
            }
        ));
        self.add_action(&quick_number_action);
    }

    fn add_quick_rule_action(&self, name: &str, case_type: crate::core::CaseType) {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| {
                if let Some(rules_list) = window.imp().rules_list.borrow().as_ref() {
                    window.add_case_rule_at(rules_list, case_type as usize, None);
                }
            }
        ));
        self.add_action(&action);
    }

    fn load_settings(&self) {
        // Load settings from disk
        let settings = AppSettings::load();
        
        // Apply theme preference
        let style_manager = adw::StyleManager::default();
        match settings.theme {
            ThemePreference::System => style_manager.set_color_scheme(adw::ColorScheme::Default),
            ThemePreference::Light => style_manager.set_color_scheme(adw::ColorScheme::ForceLight),
            ThemePreference::Dark => style_manager.set_color_scheme(adw::ColorScheme::ForceDark),
        }
        self.imp().logger.borrow_mut().set_enabled(settings.log_operations);
        if settings.undo_persistence_enabled {
            if let Err(err) = self.imp().undo_manager.borrow_mut().load_from_disk() {
                tracing::error!("Failed to load undo history: {}", err);
            }
        }
        
        self.imp().settings.replace(settings);
    }

    fn save_settings(&self) {
        let settings = self.imp().settings.borrow();
        if let Err(e) = settings.save() {
            tracing::error!("Failed to save settings: {}", e);
        }
    }

    /// Called during application shutdown to save any pending state
    pub fn save_on_shutdown(&self) {
        tracing::debug!("Saving window state on shutdown");
        self.save_settings();
    }

    fn set_theme(&self, theme: ThemePreference) {
        let style_manager = adw::StyleManager::default();
        match theme {
            ThemePreference::System => style_manager.set_color_scheme(adw::ColorScheme::Default),
            ThemePreference::Light => style_manager.set_color_scheme(adw::ColorScheme::ForceLight),
            ThemePreference::Dark => style_manager.set_color_scheme(adw::ColorScheme::ForceDark),
        }
        
        // Update settings and save
        self.imp().settings.borrow_mut().theme = theme;
        self.save_settings();
    }

    // ============ File Operations ============

    pub fn show_add_files_dialog(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Select Files")
            .modal(true)
            .build();

        dialog.open_multiple(Some(self), gio::Cancellable::NONE, clone!(
            #[weak(rename_to = window)]
            self,
            move |result| {
                if let Ok(files) = result {
                    for i in 0..files.n_items() {
                        if let Some(file) = files.item(i).and_downcast::<gio::File>() {
                            if let Some(path) = file.path() {
                                window.add_path(path);
                            }
                        }
                    }
                }
            }
        ));
    }

    pub fn show_add_folder_dialog(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Select Folder")
            .modal(true)
            .build();

        dialog.select_folder(Some(self), gio::Cancellable::NONE, clone!(
            #[weak(rename_to = window)]
            self,
            move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        window.add_path(path);
                    }
                }
            }
        ));
    }

    pub fn add_path(&self, path: PathBuf) {
        if path.is_dir() {
            // Use gio::spawn_blocking for directory traversal to avoid blocking the UI
            let (sender, receiver) = async_channel::bounded::<Vec<FileEntry>>(1);
            let settings = self.imp().settings.borrow().clone();
            
            gio::spawn_blocking(move || {
                let mut entries = Vec::new();
                for entry in walkdir::WalkDir::new(&path)
                    .min_depth(1)
                    .max_depth(settings.recursive_folder_depth)
                    .follow_links(settings.follow_symlinks)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let is_hidden = entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with('.');
                    if is_hidden && !settings.show_hidden_files {
                        continue;
                    }
                    if let Ok(mut file_entry) = FileEntry::from_path(entry.path().to_path_buf(), entry.depth()) {
                        if settings.metadata_loading_enabled && file_entry.metadata_cache.is_none() {
                            let _ = crate::metadata::load_metadata(&mut file_entry);
                        }
                        entries.push(file_entry);
                    }
                }
                let _ = sender.send_blocking(entries);
            });
            
            // Receive results on main thread
            glib::spawn_future_local(clone!(
                #[weak(rename_to = window)]
                self,
                async move {
                    if let Ok(entries) = receiver.recv().await {
                        window.append_file_entries(entries);
                    }
                }
            ));
        } else if let Ok(mut file_entry) = FileEntry::from_path(path, 0) {
            if self.imp().settings.borrow().metadata_loading_enabled {
                let _ = crate::metadata::load_metadata(&mut file_entry);
            }
            let mut files = self.imp().files.borrow_mut();
            files.push(file_entry);
            drop(files);
            self.refresh_file_list();
            self.update_preview();
        }
    }

    /// Append file entries to the list (called from async context)
    fn append_file_entries(&self, entries: Vec<FileEntry>) {
        let mut files = self.imp().files.borrow_mut();
        files.extend(entries);
        drop(files);
        self.refresh_file_list();
        self.update_preview();
    }

    pub fn clear_files(&self) {
        self.imp().files.borrow_mut().clear();
        self.imp().previews.borrow_mut().clear();
        self.refresh_file_list();
        self.update_preview();
    }

    fn remove_selected_files(&self) {
        if let Some(file_list) = self.imp().file_list.borrow().as_ref() {
            let selected = file_list.selected_rows();
            let indices: Vec<i32> = selected.iter().map(|row| row.index()).collect();

            let mut files = self.imp().files.borrow_mut();
            // Remove in reverse order to maintain indices
            for idx in indices.into_iter().rev() {
                if (idx as usize) < files.len() {
                    files.remove(idx as usize);
                }
            }
        }
        self.refresh_file_list();
        self.update_preview();
    }

    fn refresh_file_list(&self) {
        let files = self.imp().files.borrow();
        
        // Update count label
        if let Some(label) = self.imp().files_count_label.borrow().as_ref() {
            label.set_label(&format!("{} files", files.len()));
        }

        // Clear and rebuild file list
        if let Some(list) = self.imp().file_list.borrow().as_ref() {
            // Remove all children
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            // Add file rows
            for entry in files.iter() {
                let row = self.create_file_row(entry);
                list.append(&row);
            }
        }
    }

    fn create_file_row(&self, entry: &FileEntry) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        
        let box_ = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(8)
            .margin_end(8)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        // Icon
        let icon_name = if entry.is_directory {
            "folder-symbolic"
        } else {
            get_icon_for_extension(entry.extension.as_deref())
        };
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.add_css_class("dim-label");

        // File name
        let name_label = gtk::Label::builder()
            .label(&entry.original_name)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();

        // Size
        let size_label = gtk::Label::builder()
            .label(&format_size(entry.size))
            .css_classes(vec!["dim-label", "caption"])
            .build();

        box_.append(&icon);
        box_.append(&name_label);
        box_.append(&size_label);
        
        row.set_child(Some(&box_));
        row
    }

    // ============ Preview ============

    pub fn update_preview(&self) {
        let files = self.imp().files.borrow();
        let config = self.imp().config.borrow().clone();

        let mut engine = RenameEngine::new(config);
        engine.set_target(*self.imp().target.borrow());
        let mut previews = engine.generate_previews(&files);
        let files_by_id: HashMap<Uuid, FileEntry> = files
            .iter()
            .map(|entry| (entry.id, entry.clone()))
            .collect();
        let validator = RenameValidator::new();
        for error in validator.validate_batch_with_files(&previews, &files_by_id) {
            if let Some(preview) = previews.get_mut(error.file_index) {
                preview.status = match error.error_type {
                    crate::core::ValidationErrorType::Conflict => RenameStatus::InternalConflict,
                    _ => RenameStatus::Error,
                };
                preview.message = Some(error.message);
            }
        }

        // Count stats
        let will_rename = previews.iter()
            .filter(|p| matches!(p.status, RenameStatus::WillRename))
            .count();
        let unchanged = previews.iter()
            .filter(|p| matches!(p.status, RenameStatus::Unchanged))
            .count();
        let conflicts = previews.iter()
            .filter(|p| matches!(p.status, RenameStatus::Conflict | RenameStatus::InternalConflict))
            .count();
        let errors = previews.iter()
            .filter(|p| matches!(p.status, RenameStatus::Error | RenameStatus::Failed))
            .count();

        if let Some(label) = self.imp().preview_count_label.borrow().as_ref() {
            label.set_label(&format!(
                "{} rename, {} unchanged, {} conflicts, {} errors",
                will_rename, unchanged, conflicts, errors
            ));
        }

        if let Some(button) = self.imp().rename_button.borrow().as_ref() {
            button.set_sensitive(will_rename > 0 && conflicts == 0 && errors == 0);
        }

        // Update preview list
        if let Some(list) = self.imp().preview_list.borrow().as_ref() {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            let settings = self.imp().settings.borrow();
            for preview in &previews {
                if !settings.show_unchanged_files && matches!(preview.status, RenameStatus::Unchanged) {
                    continue;
                }
                let row = self.create_preview_row(preview);
                list.append(&row);
            }
        }

        drop(files);
        self.imp().previews.replace(previews);
    }

    fn create_preview_row(&self, preview: &RenamePreview) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(&preview.original_name)
            .build();

        let file_icon = gtk::Image::from_icon_name(get_icon_for_filename(&preview.original_name));
        file_icon.add_css_class("dim-label");
        row.add_prefix(&file_icon);

        // Status icon based on preview status
        let (icon_name, css_class) = match preview.status {
            RenameStatus::WillRename => ("object-select-symbolic", "success"),
            RenameStatus::Unchanged => ("action-unavailable-symbolic", "dim-label"),
            RenameStatus::Error | RenameStatus::Failed => ("dialog-error-symbolic", "error"),
            RenameStatus::Conflict | RenameStatus::InternalConflict => ("dialog-warning-symbolic", "warning"),
            RenameStatus::Completed => ("object-select-symbolic", "success"),
            RenameStatus::Skipped => ("action-unavailable-symbolic", "dim-label"),
        };

        let status_icon = gtk::Image::from_icon_name(icon_name);
        status_icon.add_css_class(css_class);
        row.add_suffix(&status_icon);

        // Show new name if different from original
        if preview.new_name != preview.original_name {
            row.set_subtitle(&preview.new_name);
        }
        if let Some(message) = &preview.message {
            row.set_tooltip_text(Some(message));
            if preview.new_name == preview.original_name {
                row.set_subtitle(message);
            }
        }

        row
    }

    // ============ Rename Operations ============

    pub fn execute_rename(&self) {
        if !self.imp().settings.borrow().confirm_before_rename {
            self.perform_rename();
            return;
        }

        let to_rename_count = {
            let previews = self.imp().previews.borrow();
            previews
                .iter()
                .filter(|p| matches!(p.status, RenameStatus::WillRename))
                .count()
        };

        if to_rename_count == 0 {
            self.show_info_dialog("Nothing to Rename", "No files will be renamed with the current rules.");
            return;
        }

        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("Confirm Rename"),
            Some(&format!("Rename {} files?", to_rename_count)),
        );

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("rename", "Rename");
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("rename"));

        dialog.connect_response(None, clone!(
            #[weak(rename_to = window)]
            self,
            move |_, response| {
                if response == "rename" {
                    window.perform_rename();
                }
            }
        ));
        dialog.present();
    }

    fn perform_rename(&self) {
        let files: HashMap<Uuid, FileEntry> = self
            .imp()
            .files
            .borrow()
            .iter()
            .map(|f| (f.id, f.clone()))
            .collect();

        let previews = self.imp().previews.borrow().clone();
        match crate::engine::execute_renames(&previews, &files) {
            Ok(result) => self.handle_rename_result(result),
            Err(err) => self.show_info_dialog("Rename Blocked", &err.to_string()),
        }
    }

    fn handle_rename_result(&self, result: crate::engine::RenameBatchResult) {
        let success_count = result.success_count();
        let error_count = result.failure_count();

        if let Some(batch) = result.batch.clone() {
            if self.imp().settings.borrow().undo_persistence_enabled {
                if let Err(err) = self.imp().undo_manager.borrow_mut().record_batch(batch.clone()) {
                    tracing::error!("Failed to record undo batch: {}", err);
                }
            }
            self.log_rename_batch(&batch);
        }

        if error_count == 0 {
            self.show_info_dialog(
                "Rename Complete",
                &format!("Successfully renamed {} files.", success_count),
            );
        } else {
            let details = result
                .failures
                .iter()
                .take(8)
                .map(|failure| {
                    format!(
                        "{}: {}",
                        failure.target_path.display(),
                        failure.error
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.show_info_dialog(
                "Rename Completed with Errors",
                &format!(
                    "Renamed {} files successfully, {} failed.\n{}",
                    success_count, error_count, details
                ),
            );
        }

        if success_count > 0 {
            self.clear_files();
        } else {
            self.update_preview();
        }
    }

    fn log_rename_batch(&self, batch: &RenameBatch) {
        let settings = self.imp().settings.borrow();
        if !settings.log_operations {
            return;
        }
        drop(settings);

        let statuses = batch
            .records
            .iter()
            .map(|record| (record.id, RenameStatus::Completed, None))
            .collect::<Vec<_>>();

        let logger = self.imp().logger.borrow();
        if let Err(err) = logger.log_batch(batch, &statuses) {
            tracing::error!("Failed to write rename log: {}", err);
        }

        for record in &batch.records {
            let entry = RenameLogEntry {
                id: Uuid::new_v4(),
                timestamp: record.timestamp,
                batch_id: batch.id,
                original_path: record.original_path.clone(),
                new_path: record.new_path.clone(),
                is_directory: record.was_directory,
                status: RenameStatus::Completed,
                error: None,
            };
            if let Err(err) = logger.log_jsonl(&entry) {
                tracing::error!("Failed to write rename JSONL log: {}", err);
            }
        }
    }

    fn show_info_dialog(&self, title: &str, message: &str) {
        let dialog = adw::MessageDialog::new(Some(self), Some(title), Some(message));
        dialog.add_response("ok", "OK");
        dialog.set_default_response(Some("ok"));
        dialog.present();
    }

    fn undo_last_batch(&self) {
        match self.imp().undo_manager.borrow_mut().undo() {
            Ok(result) => {
                self.show_info_dialog(
                    "Undo Complete",
                    &format!(
                        "Restored {} of {} renamed files.",
                        result.success_count, result.total_records
                    ),
                );
                self.update_preview();
            }
            Err(err) => self.show_info_dialog("Undo Unavailable", &err.to_string()),
        }
    }

    fn redo_last_batch(&self) {
        match self.imp().undo_manager.borrow_mut().redo() {
            Ok(result) => {
                self.show_info_dialog(
                    "Redo Complete",
                    &format!(
                        "Renamed {} of {} files again.",
                        result.success_count, result.total_records
                    ),
                );
                self.update_preview();
            }
            Err(err) => self.show_info_dialog("Redo Unavailable", &err.to_string()),
        }
    }

    fn show_preferences_dialog(&self) {
        let settings = self.imp().settings.borrow().clone();
        let dialog = adw::Window::builder()
            .title("Preferences")
            .default_width(520)
            .default_height(520)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let save_btn = gtk::Button::with_label("Save");
        save_btn.add_css_class("suggested-action");
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);
        toolbar_view.add_top_bar(&header);

        let page = adw::PreferencesPage::new();

        let behavior = adw::PreferencesGroup::builder()
            .title("Behavior")
            .build();
        let confirm_row = adw::SwitchRow::builder()
            .title("Confirm Before Rename")
            .active(settings.confirm_before_rename)
            .build();
        let live_preview_row = adw::SwitchRow::builder()
            .title("Live Preview")
            .active(settings.live_preview)
            .build();
        let show_unchanged_row = adw::SwitchRow::builder()
            .title("Show Unchanged Files")
            .active(settings.show_unchanged_files)
            .build();
        behavior.add(&confirm_row);
        behavior.add(&live_preview_row);
        behavior.add(&show_unchanged_row);
        page.add(&behavior);

        let files = adw::PreferencesGroup::builder()
            .title("Files")
            .build();
        let hidden_row = adw::SwitchRow::builder()
            .title("Include Hidden Files")
            .active(settings.show_hidden_files)
            .build();
        let symlink_row = adw::SwitchRow::builder()
            .title("Follow Symlinks")
            .active(settings.follow_symlinks)
            .build();
        let metadata_row = adw::SwitchRow::builder()
            .title("Load Metadata")
            .active(settings.metadata_loading_enabled)
            .build();
        let depth_row = adw::SpinRow::builder()
            .title("Folder Scan Depth")
            .adjustment(&gtk::Adjustment::new(
                settings.recursive_folder_depth as f64,
                1.0,
                100.0,
                1.0,
                5.0,
                0.0,
            ))
            .build();
        files.add(&hidden_row);
        files.add(&symlink_row);
        files.add(&metadata_row);
        files.add(&depth_row);
        page.add(&files);

        let history = adw::PreferencesGroup::builder()
            .title("History and Logs")
            .build();
        let undo_row = adw::SwitchRow::builder()
            .title("Persist Undo History")
            .active(settings.undo_persistence_enabled)
            .build();
        let log_row = adw::SwitchRow::builder()
            .title("Log Rename Operations")
            .active(settings.log_operations)
            .build();
        history.add(&undo_row);
        history.add(&log_row);
        page.add(&history);

        toolbar_view.set_content(Some(&page));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let window = self.clone();
        save_btn.connect_clicked(move |_| {
            {
                let mut settings = window.imp().settings.borrow_mut();
                settings.confirm_before_rename = confirm_row.is_active();
                settings.live_preview = live_preview_row.is_active();
                settings.show_unchanged_files = show_unchanged_row.is_active();
                settings.show_hidden_files = hidden_row.is_active();
                settings.follow_symlinks = symlink_row.is_active();
                settings.metadata_loading_enabled = metadata_row.is_active();
                settings.recursive_folder_depth = depth_row.value() as usize;
                settings.undo_persistence_enabled = undo_row.is_active();
                settings.log_operations = log_row.is_active();
            }
            window.imp().logger.borrow_mut().set_enabled(log_row.is_active());
            window.save_settings();
            window.update_preview();
            dialog_clone.close();
        });

        dialog.present();
    }

    // ============ Rule Dialogs ============

    fn show_add_rule_dialog(&self, rules_list: &gtk::ListBox) {
        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("Add Rule"),
            Some("Select the type of rule to add:"),
        );

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec!["boxed-list"])
            .build();

        let rule_types = [
            ("edit-find-replace-symbolic", "Replace Text", "Find and replace text"),
            ("format-text-rich-symbolic", "Change Case", "UPPER, lower, Title Case"),
            ("insert-text-symbolic", "Insert Text", "Add text at position"),
            ("edit-delete-symbolic", "Remove Text", "Remove characters or patterns"),
            ("view-list-ordered-symbolic", "Numbering", "Add sequential numbers"),
            ("x-office-calendar-symbolic", "Date/Time", "Insert date information"),
        ];

        for (icon, name, desc) in &rule_types {
            let row = adw::ActionRow::builder()
                .title(*name)
                .subtitle(*desc)
                .activatable(true)
                .build();
            
            let icon_widget = gtk::Image::from_icon_name(*icon);
            row.add_prefix(&icon_widget);
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            
            list.append(&row);
        }

        dialog.set_extra_child(Some(&list));
        dialog.add_response("cancel", "Cancel");
        dialog.present();

        // Handle selection
        let rules_list_clone = rules_list.clone();
        let dialog_clone = dialog.clone();
        list.connect_row_activated(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, row| {
                let index = row.index();
                dialog_clone.close();
                window.show_rule_config_dialog(index as usize, &rules_list_clone);
            }
        ));
    }

    fn show_rule_config_dialog(&self, rule_type: usize, rules_list: &gtk::ListBox) {
        match rule_type {
            0 => self.show_replace_config_dialog(rules_list),
            1 => self.show_case_config_dialog(rules_list),
            2 => self.show_insert_config_dialog(rules_list),
            3 => self.show_remove_config_dialog(rules_list),
            4 => self.show_numbering_config_dialog(rules_list),
            5 => self.show_datetime_config_dialog(rules_list),
            _ => {}
        }
    }

    fn show_replace_config_dialog(&self, rules_list: &gtk::ListBox) {
        let dialog = adw::Window::builder()
            .title("Replace Text")
            .default_width(400)
            .default_height(380)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let add_btn = gtk::Button::with_label("Add Rule");
        add_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&add_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Find field
        let find_group = adw::PreferencesGroup::new();
        let find_entry = adw::EntryRow::builder()
            .title("Find")
            .build();
        find_group.add(&find_entry);
        content.append(&find_group);

        // Replace field
        let replace_group = adw::PreferencesGroup::new();
        let replace_entry = adw::EntryRow::builder()
            .title("Replace with")
            .build();
        replace_group.add(&replace_entry);
        content.append(&replace_group);

        // Options
        let options_group = adw::PreferencesGroup::builder()
            .title("Options")
            .build();
        
        let case_sensitive = adw::SwitchRow::builder()
            .title("Case sensitive")
            .active(true)
            .build();
        options_group.add(&case_sensitive);
        
        let use_regex = adw::SwitchRow::builder()
            .title("Use regular expressions")
            .active(false)
            .build();
        options_group.add(&use_regex);
        
        let replace_all = adw::SwitchRow::builder()
            .title("Replace all occurrences")
            .active(true)
            .build();
        options_group.add(&replace_all);

        content.append(&options_group);
        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        // Connect buttons
        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        add_btn.connect_clicked(move |_| {
            let find_text = find_entry.text().to_string();
            let replace_text = replace_entry.text().to_string();
            let is_case_sensitive = case_sensitive.is_active();
            let is_regex = use_regex.is_active();
            let is_replace_all = replace_all.is_active();

            if !find_text.is_empty() {
                window.add_replace_rule(
                    &rules_list_clone,
                    find_text,
                    replace_text,
                    is_case_sensitive,
                    is_regex,
                    is_replace_all,
                );
            }
            dialog_clone.close();
        });

        dialog.present();
    }

    fn add_replace_rule(
        &self,
        rules_list: &gtk::ListBox,
        find: String,
        replace: String,
        case_sensitive: bool,
        use_regex: bool,
        replace_all: bool,
    ) {
        self.add_replace_rule_at(rules_list, find, replace, case_sensitive, use_regex, replace_all, None);
    }

    fn add_replace_rule_at(
        &self,
        rules_list: &gtk::ListBox,
        find: String,
        replace: String,
        case_sensitive: bool,
        use_regex: bool,
        replace_all: bool,
        edit_index: Option<usize>,
    ) {
        use crate::core::{RenameRule, RuleType, ReplaceRule};

        let rule = RenameRule::new(RuleType::Replace(ReplaceRule {
            find: find.clone(),
            replace: replace.clone(),
            use_regex,
            case_sensitive,
            replace_all,
            include_extension: false,
        }));

        let rule_index = if let Some(idx) = edit_index {
            // Update existing rule
            self.imp().config.borrow_mut().rules[idx] = rule;
            idx
        } else {
            // Add new rule
            self.imp().config.borrow_mut().rules.push(rule);
            self.imp().config.borrow().rules.len() - 1
        };

        // Create UI row
        let subtitle = if replace.is_empty() {
            format!("Remove \"{}\"", find)
        } else {
            format!("\"{}\" → \"{}\"", find, replace)
        };

        let row = self.create_rule_row(
            "Replace",
            &subtitle,
            "edit-find-replace-symbolic",
            rule_index,
            rules_list,
        );

        if edit_index.is_some() {
            // Remove old row and insert new one at same position
            if let Some(old_row) = rules_list.row_at_index(rule_index as i32) {
                let position = old_row.index();
                rules_list.remove(&old_row);
                rules_list.insert(&row, position);
            }
        } else {
            rules_list.append(&row);
        }
        self.update_preview();
    }

    fn create_rule_row(
        &self,
        title: &str,
        subtitle: &str,
        icon_name: &str,
        rule_index: usize,
        rules_list: &gtk::ListBox,
    ) -> gtk::ListBoxRow {
        let row_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(6)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        // Drag handle
        let drag_handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        drag_handle.add_css_class("dim-label");
        drag_handle.set_tooltip_text(Some("Drag to reorder"));
        row_box.append(&drag_handle);

        // Icon
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.add_css_class("dim-label");
        row_box.append(&icon);

        // Labels
        let label_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .spacing(2)
            .build();

        let title_label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(vec!["heading"])
            .build();
        label_box.append(&title_label);

        let subtitle_label = gtk::Label::builder()
            .label(subtitle)
            .xalign(0.0)
            .css_classes(vec!["dim-label", "caption"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        label_box.append(&subtitle_label);

        row_box.append(&label_box);

        // Edit button
        let edit_btn = gtk::Button::from_icon_name("document-edit-symbolic");
        edit_btn.add_css_class("flat");
        edit_btn.add_css_class("circular");
        edit_btn.set_valign(gtk::Align::Center);
        edit_btn.set_tooltip_text(Some("Edit rule"));

        // Delete button
        let delete_btn = gtk::Button::from_icon_name("edit-delete-symbolic");
        delete_btn.add_css_class("flat");
        delete_btn.add_css_class("circular");
        delete_btn.set_valign(gtk::Align::Center);
        delete_btn.set_tooltip_text(Some("Remove rule"));

        row_box.append(&edit_btn);
        row_box.append(&delete_btn);

        let row = gtk::ListBoxRow::builder()
            .child(&row_box)
            .build();
        row.add_css_class("rule-row");

        // Store rule index as widget name for easy access
        row.set_widget_name(&format!("rule_{}", rule_index));

        // Setup drag source
        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gtk::gdk::DragAction::MOVE);
        
        let row_weak = row.downgrade();
        drag_source.connect_prepare(move |_source, _x, _y| {
            if let Some(row) = row_weak.upgrade() {
                let idx = row.index();
                Some(gtk::gdk::ContentProvider::for_value(&idx.to_value()))
            } else {
                None
            }
        });

        let row_weak = row.downgrade();
        drag_source.connect_drag_begin(move |_source, _drag| {
            if let Some(row) = row_weak.upgrade() {
                row.add_css_class("dragging");
            }
        });

        let row_weak = row.downgrade();
        drag_source.connect_drag_end(move |_source, _drag, _delete| {
            if let Some(row) = row_weak.upgrade() {
                row.remove_css_class("dragging");
            }
        });

        row.add_controller(drag_source);

        // Setup drop target
        let drop_target = gtk::DropTarget::new(i32::static_type(), gtk::gdk::DragAction::MOVE);
        
        let rules_list_weak = rules_list.downgrade();
        let window_weak = self.downgrade();
        drop_target.connect_drop(move |_target, value, _x, _y| {
            if let (Some(rules_list), Some(window)) = (rules_list_weak.upgrade(), window_weak.upgrade()) {
                if let Ok(source_idx) = value.get::<i32>() {
                    let source_idx = source_idx as usize;
                    // Get target index from current row position
                    if let Some(target_row) = _target.widget().and_downcast::<gtk::ListBoxRow>() {
                        let target_idx = target_row.index() as usize;
                        if source_idx != target_idx {
                            window.reorder_rule(source_idx, target_idx, &rules_list);
                            return true;
                        }
                    }
                }
            }
            false
        });

        row.add_controller(drop_target);

        // Connect edit button
        let window = self.clone();
        let rules_list_clone = rules_list.clone();
        let row_weak = row.downgrade();
        edit_btn.connect_clicked(move |_| {
            if let Some(r) = row_weak.upgrade() {
                let idx = r.index() as usize;
                window.edit_rule_at_index(idx, &rules_list_clone);
            }
        });

        // Connect delete button
        let window = self.clone();
        let rules_list_clone = rules_list.clone();
        let row_weak = row.downgrade();
        delete_btn.connect_clicked(move |_| {
            if let Some(r) = row_weak.upgrade() {
                let idx = r.index() as usize;
                rules_list_clone.remove(&r);
                window.imp().config.borrow_mut().rules.remove(idx);
                window.update_preview();
            }
        });

        row
    }

    fn reorder_rule(&self, from: usize, to: usize, rules_list: &gtk::ListBox) {
        // Reorder in data model
        let mut rules = self.imp().config.borrow_mut();
        if from < rules.rules.len() {
            let rule = rules.rules.remove(from);
            let insert_at = if to > from { to - 1 } else { to };
            let insert_at = insert_at.min(rules.rules.len());
            rules.rules.insert(insert_at, rule);
        }
        drop(rules);

        // Rebuild the rules list UI
        self.rebuild_rules_list(rules_list);
        self.update_preview();
    }

    fn rebuild_rules_list(&self, rules_list: &gtk::ListBox) {
        // Remove all rows
        while let Some(row) = rules_list.row_at_index(0) {
            rules_list.remove(&row);
        }

        // Re-add all rules
        let rules = self.imp().config.borrow().rules.clone();
        for (idx, rule) in rules.iter().enumerate() {
            let (title, subtitle, icon) = self.get_rule_display_info(rule);
            let row = self.create_rule_row(&title, &subtitle, &icon, idx, rules_list);
            rules_list.append(&row);
        }
    }

    fn get_rule_display_info(&self, rule: &crate::core::RenameRule) -> (String, String, String) {
        use crate::core::RuleType;
        
        match &rule.rule_type {
            RuleType::Replace(r) => {
                let subtitle = if r.replace.is_empty() {
                    format!("Remove \"{}\"", r.find)
                } else {
                    format!("\"{}\" → \"{}\"", r.find, r.replace)
                };
                ("Replace".to_string(), subtitle, "edit-find-replace-symbolic".to_string())
            }
            RuleType::ChangeCase(c) => {
                let case_name = match c.case_type {
                    crate::core::CaseType::Lower => "lowercase",
                    crate::core::CaseType::Upper => "UPPERCASE",
                    crate::core::CaseType::Title => "Title Case",
                    crate::core::CaseType::Sentence => "Sentence case",
                    crate::core::CaseType::Snake => "snake_case",
                    crate::core::CaseType::Kebab => "kebab-case",
                    _ => "Unknown",
                };
                ("Change Case".to_string(), case_name.to_string(), "format-text-rich-symbolic".to_string())
            }
            RuleType::Insert(i) => {
                let text = match &i.text {
                    crate::core::InsertText::Fixed(t) => t.clone(),
                    _ => "Dynamic".to_string(),
                };
                let pos = match &i.position {
                    crate::core::InsertPosition::Prefix => "at beginning",
                    crate::core::InsertPosition::Suffix => "at end",
                    crate::core::InsertPosition::Position(p) => &format!("at position {}", p),
                    _ => "custom",
                };
                ("Insert".to_string(), format!("\"{}\" {}", text, pos), "insert-text-symbolic".to_string())
            }
            RuleType::Remove(r) => {
                let subtitle = match &r.target {
                    crate::core::RemoveTarget::Text { text, .. } => format!("\"{}\"", text),
                    crate::core::RemoveTarget::FirstN(n) => format!("First {} chars", n),
                    crate::core::RemoveTarget::LastN(n) => format!("Last {} chars", n),
                    crate::core::RemoveTarget::Digits => "All digits".to_string(),
                    crate::core::RemoveTarget::Whitespace => "All whitespace".to_string(),
                    crate::core::RemoveTarget::Bracketed(_) => "Bracketed content".to_string(),
                    _ => "Custom".to_string(),
                };
                ("Remove".to_string(), subtitle, "edit-delete-symbolic".to_string())
            }
            RuleType::Numbering(n) => {
                let pos = match &n.position {
                    crate::core::InsertPosition::Prefix => "prefix",
                    crate::core::InsertPosition::Suffix => "suffix",
                    _ => "custom",
                };
                ("Numbering".to_string(), format!("Start: {}, Pad: {} digits, {}", n.start, n.padding, pos), "view-list-ordered-symbolic".to_string())
            }
            _ => ("Rule".to_string(), "Custom rule".to_string(), "emblem-system-symbolic".to_string())
        }
    }

    fn edit_rule_at_index(&self, index: usize, rules_list: &gtk::ListBox) {
        use crate::core::RuleType;
        
        let rules = self.imp().config.borrow();
        if index >= rules.rules.len() {
            return;
        }
        let rule = rules.rules[index].clone();
        drop(rules);

        match &rule.rule_type {
            RuleType::Replace(r) => {
                self.show_replace_edit_dialog(rules_list, index, r.clone());
            }
            RuleType::ChangeCase(c) => {
                self.show_case_edit_dialog(rules_list, index, c.clone());
            }
            RuleType::Insert(i) => {
                self.show_insert_edit_dialog(rules_list, index, i.clone());
            }
            RuleType::Remove(r) => {
                self.show_remove_edit_dialog(rules_list, index, r.clone());
            }
            RuleType::Numbering(n) => {
                self.show_numbering_edit_dialog(rules_list, index, n.clone());
            }
            _ => {}
        }
    }

    fn show_replace_edit_dialog(&self, rules_list: &gtk::ListBox, edit_index: usize, existing: crate::core::ReplaceRule) {
        let dialog = adw::Window::builder()
            .title("Edit Replace Rule")
            .default_width(400)
            .default_height(380)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let save_btn = gtk::Button::with_label("Save");
        save_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        let find_group = adw::PreferencesGroup::new();
        let find_entry = adw::EntryRow::builder()
            .title("Find")
            .text(&existing.find)
            .build();
        find_group.add(&find_entry);
        content.append(&find_group);

        let replace_group = adw::PreferencesGroup::new();
        let replace_entry = adw::EntryRow::builder()
            .title("Replace with")
            .text(&existing.replace)
            .build();
        replace_group.add(&replace_entry);
        content.append(&replace_group);

        let options_group = adw::PreferencesGroup::builder()
            .title("Options")
            .build();
        
        let case_sensitive = adw::SwitchRow::builder()
            .title("Case sensitive")
            .active(existing.case_sensitive)
            .build();
        options_group.add(&case_sensitive);
        
        let use_regex = adw::SwitchRow::builder()
            .title("Use regular expressions")
            .active(existing.use_regex)
            .build();
        options_group.add(&use_regex);
        
        let replace_all = adw::SwitchRow::builder()
            .title("Replace all occurrences")
            .active(existing.replace_all)
            .build();
        options_group.add(&replace_all);

        content.append(&options_group);
        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        save_btn.connect_clicked(move |_| {
            let find_text = find_entry.text().to_string();
            let replace_text = replace_entry.text().to_string();

            if !find_text.is_empty() {
                window.add_replace_rule_at(
                    &rules_list_clone,
                    find_text,
                    replace_text,
                    case_sensitive.is_active(),
                    use_regex.is_active(),
                    replace_all.is_active(),
                    Some(edit_index),
                );
            }
            dialog_clone.close();
        });

        dialog.present();
    }

    fn show_case_config_dialog(&self, rules_list: &gtk::ListBox) {
        let dialog = adw::Window::builder()
            .title("Change Case")
            .default_width(400)
            .default_height(400)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let add_btn = gtk::Button::with_label("Add Rule");
        add_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&add_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(12)
            .build();

        let group = adw::PreferencesGroup::builder()
            .title("Case Type")
            .build();

        let case_types = [
            ("lowercase", "All letters become lowercase"),
            ("UPPERCASE", "All letters become uppercase"),
            ("Title Case", "First letter of each word uppercase"),
            ("Sentence case", "First letter uppercase, rest lowercase"),
            ("snake_case", "Words joined with underscores"),
            ("kebab-case", "Words joined with hyphens"),
        ];

        let case_dropdown = adw::ComboRow::builder()
            .title("Convert to")
            .model(&gtk::StringList::new(&case_types.map(|(name, _)| name)))
            .build();
        group.add(&case_dropdown);

        // Description label
        let desc_label = gtk::Label::builder()
            .label(case_types[0].1)
            .css_classes(vec!["dim-label", "caption"])
            .xalign(0.0)
            .margin_top(6)
            .build();

        case_dropdown.connect_selected_notify(clone!(
            #[weak]
            desc_label,
            move |dropdown| {
                let idx = dropdown.selected() as usize;
                if idx < case_types.len() {
                    desc_label.set_label(case_types[idx].1);
                }
            }
        ));

        content.append(&group);
        content.append(&desc_label);

        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        add_btn.connect_clicked(move |_| {
            let case_idx = case_dropdown.selected() as usize;
            window.add_case_rule(&rules_list_clone, case_idx);
            dialog_clone.close();
        });

        dialog.present();
    }

    fn add_case_rule(&self, rules_list: &gtk::ListBox, case_type_idx: usize) {
        self.add_case_rule_at(rules_list, case_type_idx, None);
    }

    fn add_case_rule_at(&self, rules_list: &gtk::ListBox, case_type_idx: usize, edit_index: Option<usize>) {
        use crate::core::{RenameRule, RuleType, CaseRule, CaseType};

        let case_type = match case_type_idx {
            0 => CaseType::Lower,
            1 => CaseType::Upper,
            2 => CaseType::Title,
            3 => CaseType::Sentence,
            4 => CaseType::Snake,
            5 => CaseType::Kebab,
            _ => CaseType::Lower,
        };

        let case_names = ["lowercase", "UPPERCASE", "Title Case", "Sentence case", "snake_case", "kebab-case"];

        let rule = RenameRule::new(RuleType::ChangeCase(CaseRule {
            case_type,
            include_extension: false,
        }));

        let subtitle = case_names.get(case_type_idx).copied().unwrap_or("Unknown");
        
        if let Some(idx) = edit_index {
            // Update existing rule
            self.imp().config.borrow_mut().rules[idx] = rule;
            self.rebuild_rules_list(rules_list);
        } else {
            // Add new rule
            self.imp().config.borrow_mut().rules.push(rule);
            let rule_index = self.imp().config.borrow().rules.len() - 1;
            let row = self.create_rule_row("Change Case", subtitle, "format-text-rich-symbolic", rule_index, rules_list);
            rules_list.append(&row);
        }
        self.update_preview();
    }

    fn show_case_edit_dialog(&self, rules_list: &gtk::ListBox, edit_index: usize, existing: crate::core::CaseRule) {
        let dialog = adw::Window::builder()
            .title("Edit Change Case Rule")
            .default_width(400)
            .default_height(400)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let save_btn = gtk::Button::with_label("Save");
        save_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(12)
            .build();

        let group = adw::PreferencesGroup::builder()
            .title("Case Type")
            .build();

        let case_types = [
            ("lowercase", "All letters become lowercase"),
            ("UPPERCASE", "All letters become uppercase"),
            ("Title Case", "First letter of each word uppercase"),
            ("Sentence case", "First letter uppercase, rest lowercase"),
            ("snake_case", "Words joined with underscores"),
            ("kebab-case", "Words joined with hyphens"),
        ];

        // Map existing case type to dropdown index
        let existing_idx = match existing.case_type {
            crate::core::CaseType::Lower => 0,
            crate::core::CaseType::Upper => 1,
            crate::core::CaseType::Title => 2,
            crate::core::CaseType::Sentence => 3,
            crate::core::CaseType::Snake => 4,
            crate::core::CaseType::Kebab => 5,
            _ => 0,
        };

        let case_dropdown = adw::ComboRow::builder()
            .title("Convert to")
            .model(&gtk::StringList::new(&case_types.map(|(name, _)| name)))
            .selected(existing_idx)
            .build();
        group.add(&case_dropdown);

        let desc_label = gtk::Label::builder()
            .label(case_types[existing_idx as usize].1)
            .css_classes(vec!["dim-label", "caption"])
            .xalign(0.0)
            .margin_top(6)
            .build();

        case_dropdown.connect_selected_notify(clone!(
            #[weak]
            desc_label,
            move |dropdown| {
                let idx = dropdown.selected() as usize;
                if idx < case_types.len() {
                    desc_label.set_label(case_types[idx].1);
                }
            }
        ));

        content.append(&group);
        content.append(&desc_label);

        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        save_btn.connect_clicked(move |_| {
            let case_idx = case_dropdown.selected() as usize;
            window.add_case_rule_at(&rules_list_clone, case_idx, Some(edit_index));
            dialog_clone.close();
        });

        dialog.present();
    }

    fn show_insert_config_dialog(&self, rules_list: &gtk::ListBox) {
        let dialog = adw::Window::builder()
            .title("Insert Text")
            .default_width(400)
            .default_height(380)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let add_btn = gtk::Button::with_label("Add Rule");
        add_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&add_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Text to insert
        let text_group = adw::PreferencesGroup::new();
        let text_entry = adw::EntryRow::builder()
            .title("Text to insert")
            .build();
        text_group.add(&text_entry);
        content.append(&text_group);

        // Position
        let pos_group = adw::PreferencesGroup::builder()
            .title("Position")
            .build();
        
        let position_dropdown = adw::ComboRow::builder()
            .title("Insert at")
            .model(&gtk::StringList::new(&["Beginning (prefix)", "End (suffix)", "At position"]))
            .build();
        pos_group.add(&position_dropdown);

        let position_spin = adw::SpinRow::builder()
            .title("Character position")
            .adjustment(&gtk::Adjustment::new(0.0, 0.0, 999.0, 1.0, 10.0, 0.0))
            .sensitive(false)
            .build();
        pos_group.add(&position_spin);

        position_dropdown.connect_selected_notify(clone!(
            #[weak]
            position_spin,
            move |dropdown| {
                position_spin.set_sensitive(dropdown.selected() == 2);
            }
        ));

        content.append(&pos_group);
        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        add_btn.connect_clicked(move |_| {
            let text = text_entry.text().to_string();
            let position = position_dropdown.selected();
            let pos_value = position_spin.value() as i32;

            if !text.is_empty() {
                window.add_insert_rule(&rules_list_clone, text, position as usize, pos_value);
            }
            dialog_clone.close();
        });

        dialog.present();
    }

    fn add_insert_rule(&self, rules_list: &gtk::ListBox, text: String, position: usize, pos_value: i32) {
        self.add_insert_rule_at(rules_list, text, position, pos_value, None);
    }

    fn add_insert_rule_at(&self, rules_list: &gtk::ListBox, text: String, position: usize, pos_value: i32, edit_index: Option<usize>) {
        use crate::core::{RenameRule, RuleType, InsertRule, InsertText, InsertPosition};

        let insert_pos = match position {
            0 => InsertPosition::Prefix,
            1 => InsertPosition::Suffix,
            _ => InsertPosition::Position(pos_value),
        };

        let pos_names = ["at beginning", "at end", &format!("at position {}", pos_value)];

        let rule = RenameRule::new(RuleType::Insert(InsertRule {
            text: InsertText::Fixed(text.clone()),
            position: insert_pos,
        }));

        let subtitle = format!("\"{}\" {}", text, pos_names.get(position).unwrap_or(&""));
        
        if let Some(idx) = edit_index {
            // Update existing rule
            self.imp().config.borrow_mut().rules[idx] = rule;
            self.rebuild_rules_list(rules_list);
        } else {
            // Add new rule
            self.imp().config.borrow_mut().rules.push(rule);
            let rule_index = self.imp().config.borrow().rules.len() - 1;
            let row = self.create_rule_row("Insert", &subtitle, "insert-text-symbolic", rule_index, rules_list);
            rules_list.append(&row);
        }
        self.update_preview();
    }

    fn show_insert_edit_dialog(&self, rules_list: &gtk::ListBox, edit_index: usize, existing: crate::core::InsertRule) {
        let dialog = adw::Window::builder()
            .title("Edit Insert Rule")
            .default_width(400)
            .default_height(380)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let save_btn = gtk::Button::with_label("Save");
        save_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Extract existing values
        let existing_text = match &existing.text {
            crate::core::InsertText::Fixed(t) => t.clone(),
            _ => String::new(),
        };
        
        let (existing_position, existing_pos_value) = match &existing.position {
            crate::core::InsertPosition::Prefix => (0u32, 0),
            crate::core::InsertPosition::Suffix => (1, 0),
            crate::core::InsertPosition::Position(p) => (2, *p),
            _ => (0, 0),
        };

        // Text to insert
        let text_group = adw::PreferencesGroup::new();
        let text_entry = adw::EntryRow::builder()
            .title("Text to insert")
            .text(&existing_text)
            .build();
        text_group.add(&text_entry);
        content.append(&text_group);

        // Position
        let pos_group = adw::PreferencesGroup::builder()
            .title("Position")
            .build();
        
        let position_dropdown = adw::ComboRow::builder()
            .title("Insert at")
            .model(&gtk::StringList::new(&["Beginning (prefix)", "End (suffix)", "At position"]))
            .selected(existing_position)
            .build();
        pos_group.add(&position_dropdown);

        let position_spin = adw::SpinRow::builder()
            .title("Character position")
            .adjustment(&gtk::Adjustment::new(existing_pos_value as f64, 0.0, 999.0, 1.0, 10.0, 0.0))
            .sensitive(existing_position == 2)
            .build();
        pos_group.add(&position_spin);

        position_dropdown.connect_selected_notify(clone!(
            #[weak]
            position_spin,
            move |dropdown| {
                position_spin.set_sensitive(dropdown.selected() == 2);
            }
        ));

        content.append(&pos_group);
        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        save_btn.connect_clicked(move |_| {
            let text = text_entry.text().to_string();
            let position = position_dropdown.selected();
            let pos_value = position_spin.value() as i32;

            if !text.is_empty() {
                window.add_insert_rule_at(&rules_list_clone, text, position as usize, pos_value, Some(edit_index));
            }
            dialog_clone.close();
        });

        dialog.present();
    }

    fn show_remove_config_dialog(&self, rules_list: &gtk::ListBox) {
        let dialog = adw::Window::builder()
            .title("Remove Text")
            .default_width(400)
            .default_height(430)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let add_btn = gtk::Button::with_label("Add Rule");
        add_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&add_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Remove type
        let type_group = adw::PreferencesGroup::new();
        let remove_type = adw::ComboRow::builder()
            .title("Remove")
            .model(&gtk::StringList::new(&[
                "Specific text",
                "First N characters",
                "Last N characters",
                "All digits",
                "All whitespace",
                "Bracketed content (…)",
            ]))
            .build();
        type_group.add(&remove_type);
        content.append(&type_group);

        // Text to remove (for specific text)
        let text_group = adw::PreferencesGroup::new();
        let text_entry = adw::EntryRow::builder()
            .title("Text to remove")
            .build();
        text_group.add(&text_entry);
        content.append(&text_group);

        // Number of characters
        let num_group = adw::PreferencesGroup::new();
        let num_spin = adw::SpinRow::builder()
            .title("Number of characters")
            .adjustment(&gtk::Adjustment::new(1.0, 1.0, 999.0, 1.0, 10.0, 0.0))
            .sensitive(false)
            .build();
        num_group.add(&num_spin);
        content.append(&num_group);

        remove_type.connect_selected_notify(clone!(
            #[weak]
            text_entry,
            #[weak]
            num_spin,
            move |dropdown| {
                let idx = dropdown.selected();
                text_entry.set_sensitive(idx == 0);
                num_spin.set_sensitive(idx == 1 || idx == 2);
            }
        ));

        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        add_btn.connect_clicked(move |_| {
            let remove_type_idx = remove_type.selected() as usize;
            let text = text_entry.text().to_string();
            let num = num_spin.value() as usize;

            window.add_remove_rule(&rules_list_clone, remove_type_idx, text, num);
            dialog_clone.close();
        });

        dialog.present();
    }

    fn add_remove_rule(&self, rules_list: &gtk::ListBox, remove_type: usize, text: String, num: usize) {
        self.add_remove_rule_at(rules_list, remove_type, text, num, None);
    }

    fn add_remove_rule_at(&self, rules_list: &gtk::ListBox, remove_type: usize, text: String, num: usize, edit_index: Option<usize>) {
        use crate::core::{RenameRule, RuleType, RemoveRule, RemoveTarget, BracketType};

        let (target, subtitle) = match remove_type {
            0 => (RemoveTarget::Text { text: text.clone(), case_sensitive: true }, format!("\"{}\"", text)),
            1 => (RemoveTarget::FirstN(num), format!("First {} characters", num)),
            2 => (RemoveTarget::LastN(num), format!("Last {} characters", num)),
            3 => (RemoveTarget::Digits, "All digits".to_string()),
            4 => (RemoveTarget::Whitespace, "All whitespace".to_string()),
            5 => (RemoveTarget::Bracketed(BracketType::All), "Bracketed content".to_string()),
            _ => return,
        };

        let rule = RenameRule::new(RuleType::Remove(RemoveRule { target }));
        
        if let Some(idx) = edit_index {
            // Update existing rule
            self.imp().config.borrow_mut().rules[idx] = rule;
            self.rebuild_rules_list(rules_list);
        } else {
            // Add new rule
            self.imp().config.borrow_mut().rules.push(rule);
            let rule_index = self.imp().config.borrow().rules.len() - 1;
            let row = self.create_rule_row("Remove", &subtitle, "edit-delete-symbolic", rule_index, rules_list);
            rules_list.append(&row);
        }
        self.update_preview();
    }

    fn show_remove_edit_dialog(&self, rules_list: &gtk::ListBox, edit_index: usize, existing: crate::core::RemoveRule) {
        let dialog = adw::Window::builder()
            .title("Edit Remove Rule")
            .default_width(400)
            .default_height(430)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let save_btn = gtk::Button::with_label("Save");
        save_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Extract existing values
        let (existing_type, existing_text, existing_num): (u32, String, usize) = match &existing.target {
            crate::core::RemoveTarget::Text { text, .. } => (0, text.clone(), 0),
            crate::core::RemoveTarget::FirstN(n) => (1, String::new(), *n),
            crate::core::RemoveTarget::LastN(n) => (2, String::new(), *n),
            crate::core::RemoveTarget::Digits => (3, String::new(), 0),
            crate::core::RemoveTarget::Whitespace => (4, String::new(), 0),
            crate::core::RemoveTarget::Bracketed(_) => (5, String::new(), 0),
            _ => (0, String::new(), 0),
        };

        // Remove type
        let type_group = adw::PreferencesGroup::new();
        let remove_type = adw::ComboRow::builder()
            .title("Remove")
            .model(&gtk::StringList::new(&[
                "Specific text",
                "First N characters",
                "Last N characters",
                "All digits",
                "All whitespace",
                "Bracketed content (…)",
            ]))
            .selected(existing_type)
            .build();
        type_group.add(&remove_type);
        content.append(&type_group);

        // Text to remove (for specific text)
        let text_group = adw::PreferencesGroup::new();
        let text_entry = adw::EntryRow::builder()
            .title("Text to remove")
            .text(&existing_text)
            .sensitive(existing_type == 0)
            .build();
        text_group.add(&text_entry);
        content.append(&text_group);

        // Number of characters
        let num_group = adw::PreferencesGroup::new();
        let num_spin = adw::SpinRow::builder()
            .title("Number of characters")
            .adjustment(&gtk::Adjustment::new(existing_num.max(1) as f64, 1.0, 999.0, 1.0, 10.0, 0.0))
            .sensitive(existing_type == 1 || existing_type == 2)
            .build();
        num_group.add(&num_spin);
        content.append(&num_group);

        remove_type.connect_selected_notify(clone!(
            #[weak]
            text_entry,
            #[weak]
            num_spin,
            move |dropdown| {
                let idx = dropdown.selected();
                text_entry.set_sensitive(idx == 0);
                num_spin.set_sensitive(idx == 1 || idx == 2);
            }
        ));

        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        save_btn.connect_clicked(move |_| {
            let remove_type_idx = remove_type.selected() as usize;
            let text = text_entry.text().to_string();
            let num = num_spin.value() as usize;

            window.add_remove_rule_at(&rules_list_clone, remove_type_idx, text, num, Some(edit_index));
            dialog_clone.close();
        });

        dialog.present();
    }

    fn show_numbering_config_dialog(&self, rules_list: &gtk::ListBox) {
        let dialog = adw::Window::builder()
            .title("Add Numbering")
            .default_width(400)
            .default_height(480)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let add_btn = gtk::Button::with_label("Add Rule");
        add_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&add_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Number settings
        let num_group = adw::PreferencesGroup::builder()
            .title("Numbering")
            .build();

        let start_spin = adw::SpinRow::builder()
            .title("Start at")
            .adjustment(&gtk::Adjustment::new(1.0, 0.0, 9999.0, 1.0, 10.0, 0.0))
            .build();
        num_group.add(&start_spin);

        let increment_spin = adw::SpinRow::builder()
            .title("Increment by")
            .adjustment(&gtk::Adjustment::new(1.0, 1.0, 100.0, 1.0, 10.0, 0.0))
            .build();
        num_group.add(&increment_spin);

        let padding_spin = adw::SpinRow::builder()
            .title("Digits (zero-padding)")
            .adjustment(&gtk::Adjustment::new(2.0, 1.0, 10.0, 1.0, 1.0, 0.0))
            .build();
        num_group.add(&padding_spin);

        content.append(&num_group);

        // Position
        let pos_group = adw::PreferencesGroup::builder()
            .title("Position")
            .build();

        let position_dropdown = adw::ComboRow::builder()
            .title("Insert at")
            .model(&gtk::StringList::new(&["Beginning (prefix)", "End (suffix)"]))
            .selected(1)
            .build();
        pos_group.add(&position_dropdown);

        let separator_entry = adw::EntryRow::builder()
            .title("Separator")
            .text("_")
            .build();
        pos_group.add(&separator_entry);

        content.append(&pos_group);

        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        add_btn.connect_clicked(move |_| {
            let start = start_spin.value() as i64;
            let increment = increment_spin.value() as i64;
            let padding = padding_spin.value() as usize;
            let position = position_dropdown.selected() as usize;
            let separator = separator_entry.text().to_string();

            window.add_numbering_rule(&rules_list_clone, start, increment, padding, position, separator);
            dialog_clone.close();
        });

        dialog.present();
    }

    fn add_numbering_rule(
        &self,
        rules_list: &gtk::ListBox,
        start: i64,
        increment: i64,
        padding: usize,
        position: usize,
        separator: String,
    ) {
        self.add_numbering_rule_at(rules_list, start, increment, padding, position, separator, None);
    }

    fn add_numbering_rule_at(
        &self,
        rules_list: &gtk::ListBox,
        start: i64,
        increment: i64,
        padding: usize,
        position: usize,
        separator: String,
        edit_index: Option<usize>,
    ) {
        use crate::core::{RenameRule, RuleType, NumberingRule, InsertPosition, NumberFormat};

        let (insert_pos, prefix, suffix) = if position == 0 {
            (InsertPosition::Prefix, String::new(), separator)
        } else {
            (InsertPosition::Suffix, separator, String::new())
        };

        let rule = RenameRule::new(RuleType::Numbering(NumberingRule {
            start,
            increment,
            padding,
            position: insert_pos,
            prefix,
            suffix,
            reset_per_folder: false,
            format: NumberFormat::Decimal,
        }));

        let pos_name = if position == 0 { "prefix" } else { "suffix" };
        let subtitle = format!("Start: {}, Pad: {} digits, {}", start, padding, pos_name);

        if let Some(idx) = edit_index {
            // Update existing rule
            self.imp().config.borrow_mut().rules[idx] = rule;
            self.rebuild_rules_list(rules_list);
        } else {
            // Add new rule
            self.imp().config.borrow_mut().rules.push(rule);
            let rule_index = self.imp().config.borrow().rules.len() - 1;
            let row = self.create_rule_row("Numbering", &subtitle, "view-list-ordered-symbolic", rule_index, rules_list);
            rules_list.append(&row);
        }
        self.update_preview();
    }

    fn show_numbering_edit_dialog(&self, rules_list: &gtk::ListBox, edit_index: usize, existing: crate::core::NumberingRule) {
        let dialog = adw::Window::builder()
            .title("Edit Numbering Rule")
            .default_width(400)
            .default_height(480)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let save_btn = gtk::Button::with_label("Save");
        save_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&save_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Extract existing values
        let (existing_position, existing_separator) = match &existing.position {
            crate::core::InsertPosition::Prefix => (0u32, existing.suffix.clone()),
            crate::core::InsertPosition::Suffix => (1, existing.prefix.clone()),
            _ => (1, String::new()),
        };

        // Number settings
        let num_group = adw::PreferencesGroup::builder()
            .title("Numbering")
            .build();

        let start_spin = adw::SpinRow::builder()
            .title("Start at")
            .adjustment(&gtk::Adjustment::new(existing.start as f64, 0.0, 9999.0, 1.0, 10.0, 0.0))
            .build();
        num_group.add(&start_spin);

        let increment_spin = adw::SpinRow::builder()
            .title("Increment by")
            .adjustment(&gtk::Adjustment::new(existing.increment as f64, 1.0, 100.0, 1.0, 10.0, 0.0))
            .build();
        num_group.add(&increment_spin);

        let padding_spin = adw::SpinRow::builder()
            .title("Digits (zero-padding)")
            .adjustment(&gtk::Adjustment::new(existing.padding as f64, 1.0, 10.0, 1.0, 1.0, 0.0))
            .build();
        num_group.add(&padding_spin);

        content.append(&num_group);

        // Position
        let pos_group = adw::PreferencesGroup::builder()
            .title("Position")
            .build();

        let position_dropdown = adw::ComboRow::builder()
            .title("Insert at")
            .model(&gtk::StringList::new(&["Beginning (prefix)", "End (suffix)"]))
            .selected(existing_position)
            .build();
        pos_group.add(&position_dropdown);

        let separator_entry = adw::EntryRow::builder()
            .title("Separator")
            .text(&existing_separator)
            .build();
        pos_group.add(&separator_entry);

        content.append(&pos_group);

        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        save_btn.connect_clicked(move |_| {
            let start = start_spin.value() as i64;
            let increment = increment_spin.value() as i64;
            let padding = padding_spin.value() as usize;
            let position = position_dropdown.selected() as usize;
            let separator = separator_entry.text().to_string();

            window.add_numbering_rule_at(&rules_list_clone, start, increment, padding, position, separator, Some(edit_index));
            dialog_clone.close();
        });

        dialog.present();
    }

    fn show_datetime_config_dialog(&self, rules_list: &gtk::ListBox) {
        let dialog = adw::Window::builder()
            .title("Date/Time")
            .default_width(400)
            .default_height(420)
            .modal(true)
            .transient_for(self)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("flat");
        let add_btn = gtk::Button::with_label("Add Rule");
        add_btn.add_css_class("suggested-action");
        
        header.pack_start(&cancel_btn);
        header.pack_end(&add_btn);
        toolbar_view.add_top_bar(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(24)
            .margin_end(24)
            .margin_top(12)
            .margin_bottom(24)
            .spacing(18)
            .build();

        // Date source
        let source_group = adw::PreferencesGroup::new();
        let source_dropdown = adw::ComboRow::builder()
            .title("Date source")
            .model(&gtk::StringList::new(&["File modified date", "File created date", "Current date", "EXIF date taken"]))
            .build();
        source_group.add(&source_dropdown);
        content.append(&source_group);

        // Format
        let format_group = adw::PreferencesGroup::builder()
            .title("Format")
            .build();
        
        let format_dropdown = adw::ComboRow::builder()
            .title("Date format")
            .model(&gtk::StringList::new(&[
                "2026-01-06",
                "20260106",
                "06-01-2026",
                "Jan 06, 2026",
                "2026-01-06_14-30-00",
            ]))
            .build();
        format_group.add(&format_dropdown);

        let position_dropdown = adw::ComboRow::builder()
            .title("Insert at")
            .model(&gtk::StringList::new(&["Beginning (prefix)", "End (suffix)"]))
            .build();
        format_group.add(&position_dropdown);

        content.append(&format_group);

        toolbar_view.set_content(Some(&content));
        dialog.set_content(Some(&toolbar_view));

        let dialog_clone = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dialog_clone.close();
        });

        let dialog_clone = dialog.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        add_btn.connect_clicked(move |_| {
            let source = source_dropdown.selected() as usize;
            let format = format_dropdown.selected() as usize;
            let position = position_dropdown.selected() as usize;

            window.add_datetime_rule(&rules_list_clone, source, format, position);
            dialog_clone.close();
        });

        dialog.present();
    }

    fn add_datetime_rule(&self, rules_list: &gtk::ListBox, source: usize, format: usize, position: usize) {
        use crate::core::{RenameRule, RuleType, InsertRule, InsertText, InsertPosition, DateSource};

        let formats = ["%Y-%m-%d", "%Y%m%d", "%d-%m-%Y", "%b %d, %Y", "%Y-%m-%d_%H-%M-%S"];
        let format_str = formats.get(format).unwrap_or(&"%Y-%m-%d").to_string();

        let date_source = match source {
            0 => DateSource::Modified,
            1 => DateSource::Created,
            2 => DateSource::Now,
            3 => DateSource::ExifDateTaken,
            _ => DateSource::Modified,
        };

        let insert_pos = if position == 0 {
            InsertPosition::Prefix
        } else {
            InsertPosition::Suffix
        };

        let rule = RenameRule::new(RuleType::Insert(InsertRule {
            text: InsertText::FileDate {
                source: date_source,
                format: format_str.clone(),
            },
            position: insert_pos,
        }));

        self.imp().config.borrow_mut().rules.push(rule);

        let source_names = ["Modified date", "Created date", "Current date", "EXIF date"];
        let pos_name = if position == 0 { "prefix" } else { "suffix" };
        let subtitle = format!("{} as {}", source_names.get(source).unwrap_or(&"Date"), pos_name);

        let row = adw::ActionRow::builder()
            .title("Date/Time")
            .subtitle(&subtitle)
            .build();

        let icon = gtk::Image::from_icon_name("x-office-calendar-symbolic");
        icon.add_css_class("dim-label");
        row.add_prefix(&icon);

        let remove_btn = gtk::Button::from_icon_name("edit-delete-symbolic");
        remove_btn.add_css_class("flat");
        remove_btn.add_css_class("circular");
        remove_btn.set_valign(gtk::Align::Center);

        let row_clone = row.clone();
        let rules_list_clone = rules_list.clone();
        let window = self.clone();
        let rule_index = self.imp().config.borrow().rules.len() - 1;
        remove_btn.connect_clicked(move |_| {
            rules_list_clone.remove(&row_clone);
            window.imp().config.borrow_mut().rules.remove(rule_index);
            window.update_preview();
        });

        row.add_suffix(&remove_btn);
        rules_list.append(&row);
        self.update_preview();
    }

    // ============ Preset/Import/Export Dialogs ============

    fn show_save_preset_dialog(&self) {
        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("Save Preset"),
            Some("Enter a name for this preset:"),
        );

        let entry = gtk::Entry::builder()
            .placeholder_text("Preset name")
            .build();
        dialog.set_extra_child(Some(&entry));

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("save", "Save");
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));

        dialog.connect_response(None, clone!(
            #[weak(rename_to = window)]
            self,
            move |dialog, response| {
                if response == "save" {
                    let name = entry.text().trim().to_string();
                    if name.is_empty() {
                        window.show_info_dialog("Preset Not Saved", "Preset name cannot be empty.");
                        return;
                    }
                    let config = window.imp().config.borrow().clone();
                    let preset = Preset::new(&name, config);
                    let mut manager = PresetManager::default();
                    match manager.add_preset(preset) {
                        Ok(()) => window.show_info_dialog("Preset Saved", "The current rules were saved."),
                        Err(err) => window.show_info_dialog("Preset Not Saved", &err.to_string()),
                    }
                }
                dialog.close();
            }
        ));
        dialog.present();
    }

    fn show_load_preset_dialog(&self) {
        let manager = PresetManager::default();
        let presets = manager
            .get_all()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();

        if presets.is_empty() {
            self.show_info_dialog("No Presets", "There are no presets to load yet.");
            return;
        }

        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("Load Preset"),
            Some("Choose a preset to apply."),
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
            let subtitle = preset
                .description
                .clone()
                .unwrap_or_else(|| format!("{} rules", preset.config.rules.len()));
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
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("load", "Load");
        dialog.set_response_appearance("load", adw::ResponseAppearance::Suggested);

        dialog.connect_response(None, clone!(
            #[weak(rename_to = window)]
            self,
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
        ));

        dialog.present();
    }

    fn apply_preset(&self, preset: Preset) {
        self.imp().config.replace(preset.config);
        if let Some(rules_list) = self.imp().rules_list.borrow().as_ref() {
            self.rebuild_rules_list(rules_list);
        }
        self.update_preview();
        self.show_info_dialog("Preset Loaded", &format!("Loaded '{}'.", preset.name));
    }

    fn show_import_csv_dialog(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Import from CSV")
            .modal(true)
            .build();

        dialog.open(Some(self), gio::Cancellable::NONE, clone!(
            #[weak(rename_to = window)]
            self,
            move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        window.import_csv_file(path);
                    }
                }
            }
        ));
    }

    fn show_export_log_dialog(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Export Log")
            .modal(true)
            .build();

        dialog.save(Some(self), gio::Cancellable::NONE, clone!(
            #[weak(rename_to = window)]
            self,
            move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        match window.imp().logger.borrow().export_csv(&path) {
                            Ok(()) => window.show_info_dialog("Log Exported", "Rename log exported to CSV."),
                            Err(err) => window.show_info_dialog("Export Failed", &err.to_string()),
                        }
                    }
                }
            }
        ));
    }

    fn import_csv_file(&self, path: PathBuf) {
        match self.read_csv_rename_plan(path) {
            Ok((previews, files)) => {
                let count = previews.len();
                let dialog = adw::MessageDialog::new(
                    Some(self),
                    Some("Import CSV Renames"),
                    Some(&format!("Apply {} renames from the CSV file?", count)),
                );
                dialog.add_response("cancel", "Cancel");
                dialog.add_response("rename", "Rename");
                dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
                dialog.connect_response(None, clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |dialog, response| {
                        if response == "rename" {
                            match crate::engine::execute_renames(&previews, &files) {
                                Ok(result) => window.handle_rename_result(result),
                                Err(err) => window.show_info_dialog("CSV Import Blocked", &err.to_string()),
                            }
                        }
                        dialog.close();
                    }
                ));
                dialog.present();
            }
            Err(err) => self.show_info_dialog("CSV Import Failed", &err.to_string()),
        }
    }

    fn read_csv_rename_plan(
        &self,
        path: PathBuf,
    ) -> crate::core::RenamerResult<(Vec<RenamePreview>, HashMap<Uuid, FileEntry>)> {
        let mut reader = csv::Reader::from_path(path)?;
        let headers = reader.headers()?.clone();
        let path_idx = headers
            .iter()
            .position(|header| header == "original_path")
            .ok_or_else(|| crate::core::RenamerError::Internal(
                "CSV must include an original_path column".to_string(),
            ))?;
        let name_idx = headers
            .iter()
            .position(|header| header == "new_name")
            .ok_or_else(|| crate::core::RenamerError::Internal(
                "CSV must include a new_name column".to_string(),
            ))?;

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
}

// ============ Utility Functions ============

fn get_icon_for_extension(ext: Option<&str>) -> &'static str {
    match ext.map(str::to_ascii_lowercase).as_deref() {
        Some("jpg") | Some("jpeg") | Some("png") | Some("gif") | Some("webp") | Some("svg") => {
            "image-x-generic-symbolic"
        }
        Some("mp3") | Some("flac") | Some("ogg") | Some("wav") | Some("m4a") | Some("aac") => {
            "audio-x-generic-symbolic"
        }
        Some("mp4") | Some("mkv") | Some("avi") | Some("mov") | Some("webm") => {
            "video-x-generic-symbolic"
        }
        Some("pdf") => "x-office-document-symbolic",
        Some("doc") | Some("docx") | Some("odt") => "x-office-document-symbolic",
        Some("xls") | Some("xlsx") | Some("ods") => "x-office-spreadsheet-symbolic",
        Some("ppt") | Some("pptx") | Some("odp") => "x-office-presentation-symbolic",
        Some("txt") | Some("md") | Some("rst") | Some("html") | Some("htm") | Some("xml")
        | Some("json") | Some("yaml") | Some("yml") | Some("toml") => "text-x-generic-symbolic",
        Some("rs") | Some("py") | Some("js") | Some("ts") | Some("c") | Some("cpp")
        | Some("h") | Some("hpp") | Some("css") | Some("scss") => "text-x-generic-symbolic",
        Some("zip") | Some("tar") | Some("gz") | Some("7z") | Some("rar") => {
            "package-x-generic-symbolic"
        }
        Some("appimage") | Some("exe") | Some("deb") | Some("rpm") => {
            "application-x-executable-symbolic"
        }
        _ => "text-x-generic-symbolic",
    }
}

fn get_icon_for_filename(name: &str) -> &'static str {
    std::path::Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| get_icon_for_extension(Some(ext)))
        .unwrap_or("text-x-generic-symbolic")
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.1} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

// Additional imports for drag-and-drop
