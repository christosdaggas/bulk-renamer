//! Background execution of renames and undo/redo with progress dialogs.
//!
//! Every filesystem-heavy operation runs in `gio::spawn_blocking`, so the GTK
//! main loop keeps painting; progress and results come back over channels.

use super::window::RenamerWindow;
use crate::core::{FileEntry, RenamePreview};
use crate::engine::RenameBatchResult;
use crate::undo::UndoResult;
use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;
use gtk::{gio, glib};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

/// Modal progress window with a cancel button. Returns the window, the bar,
/// and the cancel button so callers can wire them up.
fn progress_window(
    parent: &RenamerWindow,
    title: &str,
) -> (adw::Window, gtk::ProgressBar, gtk::Button) {
    let dialog = adw::Window::builder()
        .title(title)
        .modal(true)
        .transient_for(parent)
        .deletable(false)
        .default_width(360)
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .build();

    let heading = gtk::Label::builder()
        .label(title)
        .css_classes(vec!["title-4"])
        .build();
    content.append(&heading);

    let bar = gtk::ProgressBar::builder().show_text(true).build();
    content.append(&bar);

    let cancel_btn = gtk::Button::with_label("Cancel");
    cancel_btn.set_halign(gtk::Align::Center);
    content.append(&cancel_btn);

    dialog.set_content(Some(&content));
    (dialog, bar, cancel_btn)
}

/// Whether a batch result is a clean user cancellation: everything failed with
/// the cancel reason and the rollback put every file back.
fn is_clean_cancellation(result: &RenameBatchResult) -> bool {
    result.success_count() == 0
        && !result.failures.is_empty()
        && result
            .failures
            .iter()
            .all(|failure| failure.error == crate::engine::CANCELLED)
}

/// Execute a rename plan off the main thread with live progress and a working
/// Cancel button. All-or-nothing semantics are preserved by the engine.
pub fn run_rename(
    window: &RenamerWindow,
    previews: Vec<RenamePreview>,
    files: HashMap<Uuid, FileEntry>,
) {
    if window.is_busy() {
        return;
    }
    window.set_busy(true);

    let cancel = Arc::new(AtomicBool::new(false));
    let (dialog, bar, cancel_btn) = progress_window(window, "Renaming Files…");
    bar.set_text(Some("Preparing…"));

    {
        let cancel = cancel.clone();
        let bar = bar.clone();
        cancel_btn.connect_clicked(move |btn| {
            cancel.store(true, Ordering::Relaxed);
            btn.set_sensitive(false);
            bar.set_text(Some("Cancelling…"));
        });
    }

    let (progress_tx, progress_rx) = async_channel::unbounded::<(usize, usize)>();
    let (result_tx, result_rx) =
        async_channel::bounded::<crate::core::RenamerResult<RenameBatchResult>>(1);

    let worker_cancel = cancel.clone();
    gio::spawn_blocking(move || {
        let result = crate::engine::execute_renames_with(
            &previews,
            &files,
            move |done, total| {
                let _ = progress_tx.send_blocking((done, total));
            },
            &worker_cancel,
        );
        let _ = result_tx.send_blocking(result);
    });

    // Progress updates until the sender is dropped.
    let bar_clone = bar.clone();
    let cancel_for_bar = cancel.clone();
    glib::spawn_future_local(async move {
        while let Ok((done, total)) = progress_rx.recv().await {
            if cancel_for_bar.load(Ordering::Relaxed) {
                continue;
            }
            if total > 0 {
                bar_clone.set_fraction(done as f64 / total as f64);
                bar_clone.set_text(Some(&format!("{} of {} steps", done, total)));
            }
        }
    });

    let window_weak = window.downgrade();
    let dialog_clone = dialog.clone();
    glib::spawn_future_local(async move {
        let outcome = result_rx.recv().await;
        dialog_clone.close();
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        window.set_busy(false);
        match outcome {
            Ok(Ok(result)) if is_clean_cancellation(&result) => {
                window.show_info_dialog("Rename Cancelled", "No files were changed.");
                window.update_preview();
            }
            Ok(Ok(result)) => window.handle_rename_result(result),
            Ok(Err(err)) => window.show_info_dialog("Rename Blocked", &err.to_string()),
            Err(_) => window.set_busy(false),
        }
    });

    dialog.present();
}

enum HistoryDirection {
    Undo,
    Redo,
}

/// Run undo/redo on a worker thread. The UndoManager is moved out of the
/// window for the duration and always put back, so the busy flag is the only
/// thing guarding re-entrancy.
fn run_history(window: &RenamerWindow, direction: HistoryDirection) {
    if window.is_busy() {
        return;
    }
    window.set_busy(true);

    let title = match direction {
        HistoryDirection::Undo => "Undoing Rename…",
        HistoryDirection::Redo => "Redoing Rename…",
    };
    let (dialog, bar, cancel_btn) = progress_window(window, title);
    cancel_btn.set_visible(false);
    bar.set_text(Some("Working…"));

    // Pulse while the dialog is up.
    let bar_weak = bar.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        match bar_weak.upgrade() {
            Some(bar) if bar.is_visible() => {
                bar.pulse();
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        }
    });

    let mut manager = window.take_undo_manager();
    let (result_tx, result_rx) = async_channel::bounded::<(
        crate::undo::UndoManager,
        crate::core::RenamerResult<UndoResult>,
    )>(1);

    let is_undo = matches!(direction, HistoryDirection::Undo);
    gio::spawn_blocking(move || {
        let result = if is_undo { manager.undo() } else { manager.redo() };
        let _ = result_tx.send_blocking((manager, result));
    });

    let window_weak = window.downgrade();
    let dialog_clone = dialog.clone();
    glib::spawn_future_local(async move {
        let received = result_rx.recv().await;
        dialog_clone.close();
        let Some(window) = window_weak.upgrade() else {
            return;
        };
        let Ok((manager, result)) = received else {
            window.set_busy(false);
            return;
        };
        window.restore_undo_manager(manager);
        window.set_busy(false);

        let (verb, done_title, partial_title, unavailable_title) = if is_undo {
            ("Restored", "Undo Complete", "Undo Incomplete", "Undo Unavailable")
        } else {
            ("Renamed", "Redo Complete", "Redo Incomplete", "Redo Unavailable")
        };
        match result {
            Ok(result) => {
                let summary = if is_undo {
                    format!(
                        "{} {} of {} renamed files.",
                        verb, result.success_count, result.total_records
                    )
                } else {
                    format!(
                        "{} {} of {} files again.",
                        verb, result.success_count, result.total_records
                    )
                };
                let title = if result.all_successful() {
                    done_title
                } else {
                    partial_title
                };
                window.show_info_dialog(
                    title,
                    &RenamerWindow::undo_result_message(summary, &result),
                );
                window.update_preview();
            }
            Err(err) => window.show_info_dialog(unavailable_title, &err.to_string()),
        }
    });

    dialog.present();
}

pub fn run_undo(window: &RenamerWindow) {
    run_history(window, HistoryDirection::Undo);
}

pub fn run_redo(window: &RenamerWindow) {
    run_history(window, HistoryDirection::Redo);
}
