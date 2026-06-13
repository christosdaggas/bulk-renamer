//! File list component.

use crate::core::FileEntry;
use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;

/// Create a file entry row for the list.
pub fn create_file_row(entry: &FileEntry) -> adw::ActionRow {
    let icon_name = if entry.is_directory {
        "folder-symbolic"
    } else {
        get_icon_for_extension(entry.extension.as_deref())
    };

    let row = adw::ActionRow::builder()
        .title(&entry.original_name)
        .subtitle(&entry.path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default())
        .build();

    let icon = gtk::Image::from_icon_name(icon_name);
    row.add_prefix(&icon);

    // File size
    let size_label = gtk::Label::builder()
        .label(&format_size(entry.size))
        .css_classes(vec!["dim-label"])
        .build();
    row.add_suffix(&size_label);

    row
}

/// Get an appropriate icon name for a file extension.
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
        Some("rs") | Some("py") | Some("js") | Some("ts") | Some("c") | Some("cpp") | Some("h")
        | Some("hpp") | Some("css") | Some("scss") => "text-x-generic-symbolic",
        Some("zip") | Some("tar") | Some("gz") | Some("xz") | Some("7z") | Some("rar") => {
            "package-x-generic-symbolic"
        }
        Some("exe") | Some("msi") | Some("deb") | Some("rpm") | Some("appimage") => {
            "application-x-executable-symbolic"
        }
        _ => "text-x-generic-symbolic",
    }
}

/// Format file size in human-readable form.
fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if size >= TB {
        format!("{:.1} TB", size as f64 / TB as f64)
    } else if size >= GB {
        format!("{:.1} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

/// Create an empty state widget for when no files are loaded.
pub fn create_empty_state() -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("folder-documents-symbolic")
        .title("No Files Added")
        .description("Drag and drop files here, or use the toolbar to add files")
        .build()
}
