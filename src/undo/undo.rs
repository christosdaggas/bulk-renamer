//! Undo system for rename operations.

use crate::core::{RenameBatch, RenameRecord, RenamerError, RenamerResult};
use chrono::Local;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Maximum number of undo batches to keep in memory.
const MAX_UNDO_HISTORY: usize = 100;

/// Manager for undo operations.
pub struct UndoManager {
    /// Stack of completed batches that can be undone.
    undo_stack: VecDeque<RenameBatch>,
    /// Stack of undone batches that can be redone.
    redo_stack: VecDeque<RenameBatch>,
    /// Directory for storing undo data files.
    data_dir: PathBuf,
    /// Whether to persist undo data to disk.
    persist_to_disk: bool,
}

impl UndoManager {
    /// Create a new undo manager.
    pub fn new(data_dir: PathBuf, persist_to_disk: bool) -> Self {
        // Ensure data directory exists
        if persist_to_disk {
            let _ = fs::create_dir_all(&data_dir);
        }

        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            data_dir,
            persist_to_disk,
        }
    }

    /// Record a completed rename batch.
    pub fn record_batch(&mut self, batch: RenameBatch) -> RenamerResult<()> {
        // Clear redo stack when new action is performed
        self.redo_stack.clear();

        // Add to undo stack
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.pop_front();
        }

        // Persist to disk if enabled
        if self.persist_to_disk {
            self.save_batch_to_disk(&batch)?;
        }

        self.undo_stack.push_back(batch);
        Ok(())
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Get the description of the next undo operation.
    pub fn peek_undo(&self) -> Option<&RenameBatch> {
        self.undo_stack.back()
    }

    /// Get the description of the next redo operation.
    pub fn peek_redo(&self) -> Option<&RenameBatch> {
        self.redo_stack.back()
    }

    /// Undo the last rename batch.
    pub fn undo(&mut self) -> RenamerResult<UndoResult> {
        let batch = self.undo_stack.pop_back().ok_or(RenamerError::UndoNotAvailable {
            reason: "No operations to undo".to_string(),
        })?;

        let mut results = Vec::new();
        let mut success_count = 0;
        let mut failed_records = Vec::new();

        // Undo in reverse order
        for record in batch.records.iter().rev() {
            match fs::rename(&record.new_path, &record.original_path) {
                Ok(_) => {
                    success_count += 1;
                    results.push(UndoRecordResult {
                        record_id: record.id,
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    failed_records.push(record.clone());
                    results.push(UndoRecordResult {
                        record_id: record.id,
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        // Create inverse batch for redo (only successful operations)
        let redo_batch = RenameBatch {
            id: Uuid::new_v4(),
            timestamp: Local::now(),
            records: batch
                .records
                .iter()
                .filter(|r| !failed_records.iter().any(|f| f.id == r.id))
                .map(|r| RenameRecord {
                    id: Uuid::new_v4(),
                    timestamp: Local::now(),
                    original_path: r.new_path.clone(),
                    new_path: r.original_path.clone(),
                    was_directory: r.was_directory,
                })
                .collect(),
            description: batch.description.clone(),
        };

        if !redo_batch.records.is_empty() {
            self.redo_stack.push_back(redo_batch);
        }

        // Delete persisted batch file
        if self.persist_to_disk {
            let _ = self.delete_batch_from_disk(&batch.id);
        }

        Ok(UndoResult {
            batch_id: batch.id,
            total_records: batch.records.len(),
            success_count,
            results,
        })
    }

    /// Redo the last undone batch.
    pub fn redo(&mut self) -> RenamerResult<UndoResult> {
        let batch = self.redo_stack.pop_back().ok_or(RenamerError::UndoNotAvailable {
            reason: "No operations to redo".to_string(),
        })?;

        let mut results = Vec::new();
        let mut success_count = 0;
        let mut success_records = Vec::new();

        // Redo operations
        for record in &batch.records {
            match fs::rename(&record.original_path, &record.new_path) {
                Ok(_) => {
                    success_count += 1;
                    success_records.push(record.clone());
                    results.push(UndoRecordResult {
                        record_id: record.id,
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    results.push(UndoRecordResult {
                        record_id: record.id,
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        // Push successful operations back to undo stack
        if !success_records.is_empty() {
            let undo_batch = RenameBatch {
                id: Uuid::new_v4(),
                timestamp: Local::now(),
                records: success_records
                    .iter()
                    .map(|r| RenameRecord {
                        id: Uuid::new_v4(),
                        timestamp: Local::now(),
                        original_path: r.original_path.clone(),
                        new_path: r.new_path.clone(),
                        was_directory: r.was_directory,
                    })
                    .collect(),
                description: batch.description.clone(),
            };

            if self.persist_to_disk {
                let _ = self.save_batch_to_disk(&undo_batch);
            }

            self.undo_stack.push_back(undo_batch);
        }

        Ok(UndoResult {
            batch_id: batch.id,
            total_records: batch.records.len(),
            success_count,
            results,
        })
    }

    /// Generate an undo shell script for a batch.
    pub fn generate_undo_script(&self, batch: &RenameBatch) -> String {
        let mut script = String::new();
        script.push_str("#!/bin/bash\n");
        script.push_str("# Bulk Renamer Undo Script\n");
        script.push_str(&format!("# Generated: {}\n", batch.timestamp.format("%Y-%m-%d %H:%M:%S")));
        script.push_str(&format!("# Batch ID: {}\n\n", batch.id));
        script.push_str("set -e\n\n");

        // Add rename commands in reverse order
        for record in batch.records.iter().rev() {
            let from = shell_escape(&record.new_path.to_string_lossy());
            let to = shell_escape(&record.original_path.to_string_lossy());
            script.push_str(&format!("mv {} {}\n", from, to));
        }

        script.push_str("\necho \"Undo completed successfully.\"\n");
        script
    }

    /// Save undo script to a file.
    pub fn save_undo_script(&self, batch: &RenameBatch, path: &Path) -> RenamerResult<()> {
        let script = self.generate_undo_script(batch);
        let mut file = File::create(path).map_err(|e| RenamerError::Io(e))?;
        file.write_all(script.as_bytes())
            .map_err(|e| RenamerError::Io(e))?;

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = file.metadata().map_err(|e| RenamerError::Io(e))?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).map_err(|e| RenamerError::Io(e))?;
        }

        Ok(())
    }

    /// Save a batch to disk for persistence.
    fn save_batch_to_disk(&self, batch: &RenameBatch) -> RenamerResult<()> {
        let path = self.data_dir.join(format!("{}.json", batch.id));
        let file = File::create(&path).map_err(|e| RenamerError::Io(e))?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, batch).map_err(|e| RenamerError::JsonError(e))?;
        Ok(())
    }

    /// Delete a batch file from disk.
    fn delete_batch_from_disk(&self, batch_id: &Uuid) -> RenamerResult<()> {
        let path = self.data_dir.join(format!("{}.json", batch_id));
        if path.exists() {
            fs::remove_file(&path).map_err(|e| RenamerError::Io(e))?;
        }
        Ok(())
    }

    /// Load undo history from disk.
    pub fn load_from_disk(&mut self) -> RenamerResult<()> {
        if !self.data_dir.exists() {
            return Ok(());
        }

        let mut batches: Vec<RenameBatch> = Vec::new();

        for entry in fs::read_dir(&self.data_dir).map_err(|e| RenamerError::Io(e))? {
            let entry = entry.map_err(|e| RenamerError::Io(e))?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(file) = File::open(&path) {
                    let reader = BufReader::new(file);
                    if let Ok(batch) = serde_json::from_reader::<_, RenameBatch>(reader) {
                        batches.push(batch);
                    }
                }
            }
        }

        // Sort by timestamp
        batches.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Keep only the most recent batches
        self.undo_stack = batches
            .into_iter()
            .rev()
            .take(MAX_UNDO_HISTORY)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        Ok(())
    }

    /// Clear all undo history.
    pub fn clear(&mut self) -> RenamerResult<()> {
        self.undo_stack.clear();
        self.redo_stack.clear();

        // Clear persisted files
        if self.persist_to_disk && self.data_dir.exists() {
            for entry in fs::read_dir(&self.data_dir).map_err(|e| RenamerError::Io(e))? {
                let entry = entry.map_err(|e| RenamerError::Io(e))?;
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        Ok(())
    }

    /// Get the number of undo operations available.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Get the number of redo operations available.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

/// Result of an undo/redo operation.
#[derive(Debug, Clone)]
pub struct UndoResult {
    pub batch_id: Uuid,
    pub total_records: usize,
    pub success_count: usize,
    pub results: Vec<UndoRecordResult>,
}

/// Result of undoing a single record.
#[derive(Debug, Clone)]
pub struct UndoRecordResult {
    pub record_id: Uuid,
    pub success: bool,
    pub error: Option<String>,
}

impl UndoResult {
    pub fn all_successful(&self) -> bool {
        self.success_count == self.total_records
    }
}

/// Escape a string for use in a shell script.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl Default for UndoManager {
    fn default() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("bulk-renamer")
            .join("undo");

        Self::new(data_dir, true)
    }
}
