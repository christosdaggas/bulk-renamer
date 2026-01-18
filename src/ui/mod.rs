//! UI components module.
//!
//! This module contains all GTK4/libadwaita UI components for the application.

pub mod window;
pub mod preview_panel;
pub mod rule_editor;
pub mod file_list;
pub mod dialogs;
pub mod preferences;
pub mod header;
pub mod theme_popover;

pub use window::RenamerWindow;
pub use preview_panel::*;
pub use rule_editor::*;
pub use file_list::*;
pub use dialogs::*;
pub use preferences::PreferencesWindow;
pub use header::*;
pub use theme_popover::ThemePopover;
