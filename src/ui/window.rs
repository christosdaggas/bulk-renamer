//! Main application window with three-panel layout.

use crate::core::{AppSettings, FileEntry, RenameBatch, RenameConfig, RenamePreview, RenameStatus, RenameTarget};
use crate::core::types::ThemePreference;
use crate::engine::{RenameEngine, RenameValidator};
use crate::presets::Preset;
use crate::undo::{RenameLogEntry, RenameLogger, UndoManager, UndoResult};
use super::util::{format_size, get_icon_for_extension, get_icon_for_filename};
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
        let popover = super::menu::build(self);
        menu_btn.set_popover(Some(&popover));

        header.pack_end(&menu_btn);
        header.pack_end(&rename_btn);
        self.imp().rename_button.replace(Some(rename_btn));

        header
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
        // Accept gdk::FileList first: text/uri-list also deserializes to a single
        // gio::File, which would silently drop every file but the first.
        let drop_target = gtk::DropTarget::new(glib::Type::INVALID, gdk4::DragAction::COPY);
        drop_target.set_types(&[gdk4::FileList::static_type(), gio::File::static_type()]);
        drop_target.connect_drop(clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let paths = Self::paths_from_drop_value(value);
                if paths.is_empty() {
                    return false;
                }
                window.add_paths(paths);
                true
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

    pub(crate) fn set_theme(&self, theme: ThemePreference) {
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
                    let paths: Vec<PathBuf> = (0..files.n_items())
                        .filter_map(|i| files.item(i).and_downcast::<gio::File>())
                        .filter_map(|file| file.path())
                        .collect();
                    window.add_paths(paths);
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

    /// Extract every dropped path. A multi-file drop arrives as a gdk::FileList;
    /// the single gio::File form is kept as a fallback for sources that offer it.
    fn paths_from_drop_value(value: &glib::Value) -> Vec<PathBuf> {
        if let Ok(list) = value.get::<gdk4::FileList>() {
            list.files().iter().filter_map(|file| file.path()).collect()
        } else if let Ok(file) = value.get::<gio::File>() {
            file.path().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    pub fn add_path(&self, path: PathBuf) {
        self.add_paths(vec![path]);
    }

    /// Add several paths in a single pass. Plain files are collected and appended
    /// once, so the list and preview are rebuilt one time instead of per path.
    pub fn add_paths(&self, paths: Vec<PathBuf>) {
        let mut entries = Vec::new();

        for path in paths {
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
                entries.push(file_entry);
            }
        }

        if !entries.is_empty() {
            self.append_file_entries(entries);
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

    pub(crate) fn handle_rename_result(&self, result: crate::engine::RenameBatchResult) {
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
            // Drop only the entries that were actually renamed; anything that failed or
            // was skipped stays in the list so the user can retry it.
            let renamed: std::collections::HashSet<&PathBuf> = result
                .successes
                .iter()
                .map(|record| &record.original_path)
                .collect();
            self.imp()
                .files
                .borrow_mut()
                .retain(|entry| !renamed.contains(&entry.path));
            self.refresh_file_list();
        }
        self.update_preview();
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

    pub(crate) fn show_info_dialog(&self, title: &str, message: &str) {
        let dialog = adw::MessageDialog::new(Some(self), Some(title), Some(message));
        dialog.add_response("ok", "OK");
        dialog.set_default_response(Some("ok"));
        dialog.present();
    }

    /// Undo and redo refuse a move when the destination is occupied or the file no
    /// longer matches what was recorded. That refusal is the one outcome a user has
    /// to act on, so the per-record reason has to reach the dialog rather than be
    /// implied by a shortfall in the count.
    fn undo_result_message(summary: String, result: &UndoResult) -> String {
        const MAX_REASONS: usize = 5;

        let reasons: Vec<&str> = result
            .results
            .iter()
            .filter(|record| !record.success)
            .filter_map(|record| record.error.as_deref())
            .collect();

        if reasons.is_empty() {
            return summary;
        }

        let mut message = summary;
        for reason in reasons.iter().take(MAX_REASONS) {
            message.push_str(&format!("\n\n{}", reason));
        }
        if reasons.len() > MAX_REASONS {
            message.push_str(&format!(
                "\n\n…and {} more.",
                reasons.len() - MAX_REASONS
            ));
        }
        message
    }

    fn undo_last_batch(&self) {
        match self.imp().undo_manager.borrow_mut().undo() {
            Ok(result) => {
                let summary = format!(
                    "Restored {} of {} renamed files.",
                    result.success_count, result.total_records
                );
                let title = if result.all_successful() {
                    "Undo Complete"
                } else {
                    "Undo Incomplete"
                };
                self.show_info_dialog(title, &Self::undo_result_message(summary, &result));
                self.update_preview();
            }
            Err(err) => self.show_info_dialog("Undo Unavailable", &err.to_string()),
        }
    }

    fn redo_last_batch(&self) {
        match self.imp().undo_manager.borrow_mut().redo() {
            Ok(result) => {
                let summary = format!(
                    "Renamed {} of {} files again.",
                    result.success_count, result.total_records
                );
                let title = if result.all_successful() {
                    "Redo Complete"
                } else {
                    "Redo Incomplete"
                };
                self.show_info_dialog(title, &Self::undo_result_message(summary, &result));
                self.update_preview();
            }
            Err(err) => self.show_info_dialog("Redo Unavailable", &err.to_string()),
        }
    }

    fn show_preferences_dialog(&self) {
        super::preferences_dialog::show(self);
    }

    /// Snapshot of the current settings, for dialogs.
    pub(crate) fn settings_snapshot(&self) -> AppSettings {
        self.imp().settings.borrow().clone()
    }

    /// Mutate the settings, then persist them and refresh everything they affect.
    pub(crate) fn update_settings(&self, apply: impl FnOnce(&mut AppSettings)) {
        {
            let mut settings = self.imp().settings.borrow_mut();
            apply(&mut settings);
        }
        let log_enabled = self.imp().settings.borrow().log_operations;
        self.imp().logger.borrow_mut().set_enabled(log_enabled);
        self.save_settings();
        self.update_preview();
    }

    // ============ Rule Dialogs ============

    fn show_add_rule_dialog(&self, _rules_list: &gtk::ListBox) {
        use super::rule_dialogs::RuleKind;

        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("Add Rule"),
            Some("Select the type of rule to add:"),
        );

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec!["boxed-list"])
            .build();

        for (_, icon, name, desc) in RuleKind::catalog() {
            let row = adw::ActionRow::builder()
                .title(*name)
                .subtitle(*desc)
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(icon));
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            list.append(&row);
        }

        let scroll = gtk::ScrolledWindow::builder()
            .max_content_height(420)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        dialog.set_extra_child(Some(&scroll));
        dialog.add_response("cancel", "Cancel");
        dialog.present();

        let dialog_clone = dialog.clone();
        list.connect_row_activated(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, row| {
                let index = row.index() as usize;
                dialog_clone.close();
                if let Some((kind, ..)) = RuleKind::catalog().get(index) {
                    super::rule_dialogs::open(&window, *kind, None);
                }
            }
        ));
    }

    /// Read a rule back for the edit dialogs.
    pub(crate) fn rule_at(&self, index: usize) -> Option<crate::core::RenameRule> {
        self.imp().config.borrow().rules.get(index).cloned()
    }

    /// Insert or replace a rule, rebuild the rules list, refresh the preview.
    /// Editing preserves the rule's id and enabled flag.
    pub(crate) fn commit_rule(&self, rule_type: crate::core::RuleType, edit_index: Option<usize>) {
        {
            let mut config = self.imp().config.borrow_mut();
            match edit_index {
                Some(idx) if idx < config.rules.len() => {
                    config.rules[idx].rule_type = rule_type;
                }
                _ => config.rules.push(crate::core::RenameRule::new(rule_type)),
            }
        }
        if let Some(rules_list) = self.imp().rules_list.borrow().as_ref() {
            self.rebuild_rules_list(rules_list);
        }
        self.update_preview();
    }

    // Thin rule builders kept for the quick-rule actions and the widget tests;
    // the dialogs go through rule_dialogs::open and commit_rule directly.

    #[cfg(test)]
    fn add_replace_rule(
        &self,
        _rules_list: &gtk::ListBox,
        find: String,
        replace: String,
        case_sensitive: bool,
        use_regex: bool,
        replace_all: bool,
    ) {
        use crate::core::{ReplaceRule, RuleType};
        self.commit_rule(
            RuleType::Replace(ReplaceRule {
                find,
                replace,
                use_regex,
                case_sensitive,
                replace_all,
                include_extension: false,
            }),
            None,
        );
    }

    fn add_case_rule_at(&self, _rules_list: &gtk::ListBox, case_type_idx: usize, edit_index: Option<usize>) {
        use crate::core::{CaseRule, CaseType, RuleType};
        // Index order matches the CaseType enum, which the quick actions cast.
        let case_type = match case_type_idx {
            0 => CaseType::Lower,
            1 => CaseType::Upper,
            2 => CaseType::Title,
            3 => CaseType::Sentence,
            _ => CaseType::Lower,
        };
        self.commit_rule(
            RuleType::ChangeCase(CaseRule {
                case_type,
                include_extension: false,
            }),
            edit_index,
        );
    }

    fn add_numbering_rule_at(
        &self,
        _rules_list: &gtk::ListBox,
        start: i64,
        increment: i64,
        padding: usize,
        position: usize,
        separator: String,
        edit_index: Option<usize>,
    ) {
        use crate::core::{InsertPosition, NumberFormat, NumberingRule, RuleType};
        let (insert_pos, prefix, suffix) = if position == 0 {
            (InsertPosition::Prefix, String::new(), separator)
        } else {
            (InsertPosition::Suffix, separator, String::new())
        };
        self.commit_rule(
            RuleType::Numbering(NumberingRule {
                start,
                increment,
                padding,
                position: insert_pos,
                prefix,
                suffix,
                reset_per_folder: false,
                format: NumberFormat::Decimal,
            }),
            edit_index,
        );
    }

    #[cfg(test)]
    fn add_datetime_rule(&self, rules_list: &gtk::ListBox, source: usize, format: usize, position: usize) {
        self.add_datetime_rule_at(rules_list, source, format, position, None);
    }

    #[cfg(test)]
    fn add_datetime_rule_at(
        &self,
        _rules_list: &gtk::ListBox,
        source: usize,
        format: usize,
        position: usize,
        edit_index: Option<usize>,
    ) {
        use crate::core::{DateSource, InsertPosition, InsertRule, InsertText, RuleType};

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
        self.commit_rule(
            RuleType::Insert(InsertRule {
                text: InsertText::FileDate {
                    source: date_source,
                    format: format_str,
                },
                position: insert_pos,
            }),
            edit_index,
        );
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
        drag_handle.add_css_class("drag-handle");
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
                // Guard the model: a detached row reports index -1, which would wrap
                // to a huge usize and panic inside Vec::remove.
                let mut config = window.imp().config.borrow_mut();
                if idx < config.rules.len() {
                    config.rules.remove(idx);
                }
                drop(config);
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
        super::rule_dialogs::rule_summary(&rule.rule_type)
    }

    fn edit_rule_at_index(&self, index: usize, _rules_list: &gtk::ListBox) {
        use super::rule_dialogs::RuleKind;
        let Some(rule) = self.rule_at(index) else { return };
        super::rule_dialogs::open(self, RuleKind::of(&rule.rule_type), Some(index));
    }

    /// Subtitle shared with rule_dialogs so a rebuilt row is identical to the
    /// one created when the rule was added.
    #[cfg(test)]
    fn datetime_subtitle(rule: &crate::core::InsertRule) -> String {
        super::rule_dialogs::datetime_subtitle_for(rule)
    }
    // ============ Preset/Import/Export Dialogs ============

    fn show_save_preset_dialog(&self) {
        super::presets_dialog::show_save(self);
    }

    fn show_load_preset_dialog(&self) {
        super::presets_dialog::show_load(self);
    }

    /// Clone of the active rule configuration, for preset dialogs.
    pub(crate) fn config_snapshot(&self) -> RenameConfig {
        self.imp().config.borrow().clone()
    }

    pub(crate) fn apply_preset(&self, preset: Preset) {
        self.imp().config.replace(preset.config);
        if let Some(rules_list) = self.imp().rules_list.borrow().as_ref() {
            self.rebuild_rules_list(rules_list);
        }
        self.update_preview();
        self.show_info_dialog("Preset Loaded", &format!("Loaded '{}'.", preset.name));
    }

    fn show_import_csv_dialog(&self) {
        super::csv_io::show_import_dialog(self);
    }

    fn show_export_log_dialog(&self) {
        super::csv_io::show_export_dialog(self);
    }

    pub(crate) fn export_log_csv(&self, path: &std::path::Path) -> crate::core::RenamerResult<()> {
        self.imp().logger.borrow().export_csv(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{DateSource, InsertPosition, InsertText};
    use std::sync::Once;

    static GTK_INIT: Once = Once::new();

    /// These tests drive real widgets, so they need a display; run them with
    /// `cargo test -- --ignored --test-threads=1`.
    fn test_window() -> RenamerWindow {
        GTK_INIT.call_once(|| {
            adw::init().expect("libadwaita must initialise for widget tests");
        });
        let app = adw::Application::builder()
            .application_id("com.chrisdaggas.bulk-renamer.Tests")
            .build();
        RenamerWindow::new(&app)
    }

    fn rules_list_of(window: &RenamerWindow) -> gtk::ListBox {
        window
            .imp()
            .rules_list
            .borrow()
            .clone()
            .expect("window builds a rules list")
    }

    /// Last child of the row box is the delete button built by create_rule_row.
    fn delete_button(row: &gtk::ListBoxRow) -> gtk::Button {
        row.child()
            .and_downcast::<gtk::Box>()
            .and_then(|row_box| row_box.last_child())
            .and_downcast::<gtk::Button>()
            .expect("rule row ends with a delete button")
    }

    /// Third child of the row box is the label box; its first label is the title.
    fn row_title(row: &gtk::ListBoxRow) -> String {
        let label_box = row
            .child()
            .and_downcast::<gtk::Box>()
            .and_then(|row_box| row_box.first_child())
            .and_then(|drag_handle| drag_handle.next_sibling())
            .and_then(|icon| icon.next_sibling())
            .and_downcast::<gtk::Box>()
            .expect("rule row has a label box");
        label_box
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("label box starts with the title label")
            .label()
            .to_string()
    }

    fn row_titles(rules_list: &gtk::ListBox) -> Vec<String> {
        let mut titles = Vec::new();
        let mut idx = 0;
        while let Some(row) = rules_list.row_at_index(idx) {
            titles.push(row_title(&row));
            idx += 1;
        }
        titles
    }

    fn rule_titles(window: &RenamerWindow) -> Vec<String> {
        window
            .imp()
            .config
            .borrow()
            .rules
            .iter()
            .map(|rule| window.get_rule_display_info(rule).0)
            .collect()
    }

    fn add_replace(window: &RenamerWindow, rules_list: &gtk::ListBox, find: &str) {
        window.add_replace_rule(
            rules_list,
            find.to_string(),
            "x".to_string(),
            true,
            false,
            true,
        );
    }

    fn scenario_datetime_row_delete_removes_the_datetime_rule() {
        let window = test_window();
        let rules_list = rules_list_of(&window);

        add_replace(&window, &rules_list, "a");
        window.add_datetime_rule(&rules_list, 0, 0, 0);
        add_replace(&window, &rules_list, "b");

        // Delete the first Replace rule, which shifts the Date/Time rule down.
        delete_button(&rules_list.row_at_index(0).expect("row 0")).emit_clicked();
        assert_eq!(rule_titles(&window), vec!["Date/Time", "Replace"]);

        // Now delete the Date/Time row: it must remove the Date/Time rule.
        delete_button(&rules_list.row_at_index(0).expect("row 0")).emit_clicked();

        assert_eq!(rule_titles(&window), vec!["Replace"]);
        assert_eq!(row_titles(&rules_list), vec!["Replace"]);
    }

    fn scenario_datetime_row_delete_does_not_panic_when_it_is_the_last_rule() {
        let window = test_window();
        let rules_list = rules_list_of(&window);

        add_replace(&window, &rules_list, "a");
        window.add_datetime_rule(&rules_list, 0, 0, 0);

        delete_button(&rules_list.row_at_index(0).expect("row 0")).emit_clicked();
        // Used to panic with "removal index (is 1) should be < len (is 1)".
        delete_button(&rules_list.row_at_index(0).expect("row 0")).emit_clicked();

        assert!(window.imp().config.borrow().rules.is_empty());
        assert!(rules_list.row_at_index(0).is_none());
    }

    fn scenario_datetime_row_survives_a_rules_list_rebuild() {
        let window = test_window();
        let rules_list = rules_list_of(&window);

        window.add_datetime_rule(&rules_list, 3, 0, 1);
        let before = row_titles(&rules_list);

        window.rebuild_rules_list(&rules_list);

        assert_eq!(before, vec!["Date/Time"]);
        assert_eq!(row_titles(&rules_list), before);
    }

    #[test]
    fn datetime_rules_are_labelled_as_date_time() {
        let rule = crate::core::InsertRule {
            text: InsertText::FileDate {
                source: DateSource::ExifDateTaken,
                format: "%Y-%m-%d".to_string(),
            },
            position: InsertPosition::Suffix,
        };
        assert_eq!(
            RenamerWindow::datetime_subtitle(&rule),
            "EXIF date as suffix"
        );
    }

    fn scenario_drop_value_yields_every_file_of_a_multi_file_drop() {
        let _window = test_window();
        let files: Vec<gio::File> = ["/tmp/one.txt", "/tmp/two.txt", "/tmp/three.txt"]
            .iter()
            .map(gio::File::for_path)
            .collect();
        let value = gdk4::FileList::from_array(&files).to_value();

        let paths = RenamerWindow::paths_from_drop_value(&value);

        assert_eq!(paths.len(), 3);
        assert_eq!(paths[2], PathBuf::from("/tmp/three.txt"));
    }

    fn scenario_drop_value_still_accepts_a_single_file() {
        let _window = test_window();
        let value = gio::File::for_path("/tmp/one.txt").to_value();

        assert_eq!(
            RenamerWindow::paths_from_drop_value(&value),
            vec![PathBuf::from("/tmp/one.txt")]
        );
    }

    fn scenario_drop_target_advertises_the_file_list_type() {
        let window = test_window();
        let file_list = window
            .imp()
            .file_list
            .borrow()
            .clone()
            .expect("window builds a file list");

        let controllers = file_list.observe_controllers();
        let mut types = Vec::new();
        for i in 0..controllers.n_items() {
            if let Some(target) = controllers.item(i).and_downcast::<gtk::DropTarget>() {
                types = target.types().to_vec();
            }
        }

        assert!(types.contains(&gdk4::FileList::static_type()));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bulk-renamer-{}-{}", name, Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn scenario_adding_many_paths_rebuilds_the_list_once() {
        let window = test_window();
        let dir = temp_dir("batch-add");
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.join(name), b"x").expect("write file");
        }

        // Seed the list so the count label already has a value, then count how many
        // times refresh_file_list rewrites it while adding the rest.
        window.add_path(dir.join("a.txt"));
        let label = window
            .imp()
            .files_count_label
            .borrow()
            .clone()
            .expect("window builds a count label");
        let rebuilds = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let rebuilds_clone = rebuilds.clone();
        label.connect_label_notify(move |_| {
            rebuilds_clone.set(rebuilds_clone.get() + 1);
        });

        window.add_paths(vec![dir.join("b.txt"), dir.join("c.txt")]);

        assert_eq!(window.imp().files.borrow().len(), 3);
        assert_eq!(rebuilds.get(), 1, "two paths must cost one list rebuild");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scenario_a_failed_rename_unwinds_the_whole_batch() {
        let window = test_window();
        let rules_list = rules_list_of(&window);
        let dir = temp_dir("partial-rename");
        for name in ["keep_a.txt", "keep_b.txt"] {
            std::fs::write(dir.join(name), b"x").expect("write file");
        }

        window.add_paths(vec![dir.join("keep_a.txt"), dir.join("keep_b.txt")]);
        add_replace(&window, &rules_list, "keep");

        // Real pipeline: update_preview generated and validated the previews above.
        let files: HashMap<Uuid, FileEntry> = window
            .imp()
            .files
            .borrow()
            .iter()
            .map(|entry| (entry.id, entry.clone()))
            .collect();
        let previews = window.imp().previews.borrow().clone();
        let plan = crate::engine::plan_renames(&previews, &files).expect("batch plans cleanly");

        // Occupy the second target with a directory between planning and execution, so
        // its rename fails. Phase 1 has already vacated every source, so the failed item
        // has nowhere safe to return to while the finished moves stand: the batch unwinds
        // rather than landing half-applied.
        std::fs::create_dir(dir.join("x_b.txt")).expect("block b's target");
        let result = crate::engine::execute_rename_plan(plan);
        assert_eq!(result.success_count(), 0, "an unwound batch renames nothing");
        assert!(result.failure_count() >= 1);

        window.handle_rename_result(result);

        let remaining: Vec<PathBuf> = window
            .imp()
            .files
            .borrow()
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        assert_eq!(
            remaining,
            vec![dir.join("keep_a.txt"), dir.join("keep_b.txt")],
            "nothing was renamed, so both files stay in the list"
        );
        assert!(
            dir.join("keep_a.txt").exists() && dir.join("keep_b.txt").exists(),
            "both originals must be back in place after the unwind"
        );
        assert!(!dir.join("x_a.txt").exists(), "no target may survive an unwind");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused undo is reported as a plain shortfall in the count, which leaves the
    /// user with no idea that something is sitting on the destination. Drives a real
    /// refusal rather than a hand-built result, so it stays honest about the reason
    /// string the undo manager actually produces.
    #[test]
    fn a_refused_undo_tells_the_user_why() {
        use crate::core::{RenameRule, ReplaceRule, RuleType};
        use crate::engine::execute_renames;

        let dir = std::env::temp_dir().join(format!("bulk-renamer-undo-msg-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let data = dir.join("undo-data");
        std::fs::create_dir_all(&data).expect("create data dir");

        let a = dir.join("a.txt");
        std::fs::write(&a, "original").expect("write a");

        let config = RenameConfig {
            rules: vec![RenameRule::new(RuleType::Replace(ReplaceRule {
                find: "a".to_string(),
                replace: "z".to_string(),
                ..Default::default()
            }))],
            ..Default::default()
        };

        let entry = FileEntry::from_path(a.clone(), 0).expect("file entry");
        let mut engine = RenameEngine::new(config);
        let previews = engine.generate_previews(std::slice::from_ref(&entry));
        let map: HashMap<Uuid, FileEntry> = std::iter::once((entry.id, entry)).collect();
        let batch = execute_renames(&previews, &map)
            .expect("execute renames")
            .batch
            .expect("batch");

        let mut manager = UndoManager::new(data, true);
        manager.record_batch(batch).expect("record");

        // Put something back at the original path so the undo has to refuse.
        std::fs::write(&a, "precious").expect("write precious");
        let result = manager.undo().expect("undo");
        assert_eq!(result.success_count, 0, "the undo must have been refused");

        let summary = format!(
            "Restored {} of {} renamed files.",
            result.success_count, result.total_records
        );
        let message = RenamerWindow::undo_result_message(summary, &result);

        let reason = result.results[0]
            .error
            .as_deref()
            .expect("a refused record carries a reason");
        assert!(
            message.contains(reason),
            "the dialog must repeat the refusal reason {:?}, got {:?}",
            reason,
            message
        );
        assert!(
            !result.all_successful(),
            "a refused undo is not a complete one"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Rust harness runs every test on its own thread, but libadwaita may only
    /// be used from the thread that initialised it, so all widget scenarios share
    /// one test. Run with `cargo test --lib -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a display"]
    fn gtk_widget_regressions() {
        let scenarios: Vec<(&str, fn())> = vec![
            ("datetime_row_delete_removes_the_datetime_rule", scenario_datetime_row_delete_removes_the_datetime_rule),
            ("datetime_row_delete_does_not_panic_when_it_is_the_last_rule", scenario_datetime_row_delete_does_not_panic_when_it_is_the_last_rule),
            ("datetime_row_survives_a_rules_list_rebuild", scenario_datetime_row_survives_a_rules_list_rebuild),
            ("drop_value_yields_every_file_of_a_multi_file_drop", scenario_drop_value_yields_every_file_of_a_multi_file_drop),
            ("drop_value_still_accepts_a_single_file", scenario_drop_value_still_accepts_a_single_file),
            ("drop_target_advertises_the_file_list_type", scenario_drop_target_advertises_the_file_list_type),
            ("adding_many_paths_rebuilds_the_list_once", scenario_adding_many_paths_rebuilds_the_list_once),
            ("a_failed_rename_unwinds_the_whole_batch", scenario_a_failed_rename_unwinds_the_whole_batch),
        ];

        for (name, scenario) in scenarios {
            scenario();
            eprintln!("scenario ok: {}", name);
        }
    }
}
