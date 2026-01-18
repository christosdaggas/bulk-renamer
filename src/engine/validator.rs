//! Filename validation utilities.

use crate::core::{
    FileEntry, RenamePreview, RenameStatus, RenamerError, RenamerResult, ValidationError,
    ValidationErrorType,
};
use std::collections::HashMap;
use std::path::Path;

/// Reserved filenames on Windows.
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Invalid characters for filenames on various platforms.
const INVALID_CHARS_WINDOWS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const INVALID_CHARS_UNIX: &[char] = &['/', '\0'];

/// Maximum path length (conservative default).
const MAX_PATH_LENGTH: usize = 4096;
const MAX_FILENAME_LENGTH: usize = 255;

/// Validator for rename operations.
pub struct RenameValidator {
    /// Maximum allowed path length.
    pub max_path_length: usize,
    /// Maximum allowed filename length.
    pub max_filename_length: usize,
    /// Whether to check for platform-specific restrictions.
    pub check_platform_restrictions: bool,
    /// Whether to allow hidden files (starting with .).
    pub allow_hidden: bool,
}

impl Default for RenameValidator {
    fn default() -> Self {
        Self {
            max_path_length: MAX_PATH_LENGTH,
            max_filename_length: MAX_FILENAME_LENGTH,
            check_platform_restrictions: true,
            allow_hidden: true,
        }
    }
}

impl RenameValidator {
    /// Create a new validator with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate a single filename.
    pub fn validate_filename(&self, name: &str) -> RenamerResult<()> {
        // Check for empty name
        if name.is_empty() {
            return Err(RenamerError::InvalidFilename {
                reason: "Filename cannot be empty".to_string(),
            });
        }

        // Check for whitespace-only name
        if name.trim().is_empty() {
            return Err(RenamerError::InvalidFilename {
                reason: "Filename cannot be only whitespace".to_string(),
            });
        }

        // Check filename length
        if name.len() > self.max_filename_length {
            return Err(RenamerError::InvalidFilename {
                reason: format!(
                    "Filename too long ({} chars, max {})",
                    name.len(),
                    self.max_filename_length
                ),
            });
        }

        // Check for invalid characters
        let invalid_chars = if cfg!(windows) {
            INVALID_CHARS_WINDOWS
        } else {
            INVALID_CHARS_UNIX
        };

        for ch in invalid_chars {
            if name.contains(*ch) {
                return Err(RenamerError::InvalidFilename {
                    reason: format!("Filename contains invalid character: '{}'", ch),
                });
            }
        }

        // Check for control characters
        if name.chars().any(|c| c.is_control()) {
            return Err(RenamerError::InvalidFilename {
                reason: "Filename contains control characters".to_string(),
            });
        }

        // Check for names ending with space or period (Windows restriction)
        if cfg!(windows) || self.check_platform_restrictions {
            if name.ends_with(' ') || name.ends_with('.') {
                return Err(RenamerError::InvalidFilename {
                    reason: "Filename cannot end with space or period".to_string(),
                });
            }
        }

        // Check for reserved names (Windows)
        if cfg!(windows) || self.check_platform_restrictions {
            let name_upper = name.to_uppercase();
            let base_name = name_upper.split('.').next().unwrap_or(&name_upper);
            if WINDOWS_RESERVED.contains(&base_name) {
                return Err(RenamerError::InvalidFilename {
                    reason: format!("'{}' is a reserved filename", name),
                });
            }
        }

        Ok(())
    }

    /// Validate a complete path.
    pub fn validate_path(&self, path: &Path) -> RenamerResult<()> {
        let path_str = path.to_string_lossy();

        // Check path length
        if path_str.len() > self.max_path_length {
            return Err(RenamerError::PathTooLong {
                path: path_str.to_string(),
                length: path_str.len(),
                max: self.max_path_length,
            });
        }

        // Validate the filename component
        if let Some(name) = path.file_name() {
            self.validate_filename(&name.to_string_lossy())?;
        }

        Ok(())
    }

    /// Validate a batch of rename previews for conflicts.
    pub fn validate_batch(&self, previews: &[RenamePreview]) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        let mut target_names: HashMap<String, Vec<usize>> = HashMap::new();

        // First pass: collect all target names and check for individual errors
        for (idx, preview) in previews.iter().enumerate() {
            // Skip already errored or unchanged entries
            if matches!(preview.status, RenameStatus::Error | RenameStatus::Unchanged) {
                continue;
            }

            // Check if target path already exists on disk
            if preview.new_path.exists() && preview.new_name != preview.original_name {
                errors.push(ValidationError {
                    file_index: idx,
                    original_name: preview.original_name.clone(),
                    error_type: ValidationErrorType::Conflict,
                    message: format!("'{}' already exists", preview.new_name),
                });
                continue;
            }

            // Track target names for internal conflict detection
            let new_path_str = preview.new_path.to_string_lossy().to_lowercase();
            target_names
                .entry(new_path_str.clone())
                .or_insert_with(Vec::new)
                .push(idx);
        }

        // Second pass: detect internal conflicts (multiple files -> same name)
        for (_target, indices) in &target_names {
            if indices.len() > 1 {
                for &idx in indices {
                    errors.push(ValidationError {
                        file_index: idx,
                        original_name: previews[idx].original_name.clone(),
                        error_type: ValidationErrorType::Conflict,
                        message: format!(
                            "Multiple files would be renamed to '{}'",
                            previews[idx].new_name
                        ),
                    });
                }
            }
        }

        errors
    }

    /// Check if a file can be renamed (permissions, existence, etc.).
    pub fn check_file_access(&self, entry: &FileEntry) -> RenamerResult<()> {
        // Check if file exists
        if !entry.path.exists() {
            return Err(RenamerError::FileNotFound {
                path: entry.path.clone(),
            });
        }

        // Check if parent directory is writable
        if let Some(parent) = entry.path.parent() {
            let metadata = std::fs::metadata(parent).map_err(|_| RenamerError::PermissionDenied {
                path: parent.to_path_buf(),
            })?;

            // On Unix, check write permission
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = metadata.permissions();
                let mode = permissions.mode();

                // Check if we can write to the directory
                // This is a simplified check; real check would need to consider user/group
                if mode & 0o200 == 0 && mode & 0o020 == 0 && mode & 0o002 == 0 {
                    return Err(RenamerError::PermissionDenied {
                        path: parent.to_path_buf(),
                    });
                }
            }
        }

        Ok(())
    }
}

/// Sanitize a filename by removing or replacing invalid characters.
pub fn sanitize_filename(name: &str, replacement: char) -> String {
    let invalid_chars: &[char] = if cfg!(windows) {
        INVALID_CHARS_WINDOWS
    } else {
        INVALID_CHARS_UNIX
    };

    let mut result: String = name
        .chars()
        .map(|c| {
            if invalid_chars.contains(&c) || c.is_control() {
                replacement
            } else {
                c
            }
        })
        .collect();

    // Remove trailing spaces and periods (Windows restriction)
    while result.ends_with(' ') || result.ends_with('.') {
        result.pop();
    }

    // Check for reserved names
    let upper = result.to_uppercase();
    let base_name = upper.split('.').next().unwrap_or(&upper);
    if WINDOWS_RESERVED.contains(&base_name) {
        result = format!("_{}", result);
    }

    // Ensure non-empty
    if result.is_empty() {
        result = String::from("unnamed");
    }

    // Truncate if too long
    if result.len() > MAX_FILENAME_LENGTH {
        // Try to preserve extension
        if let Some(dot_pos) = result.rfind('.') {
            let ext_len = result.len() - dot_pos;
            if ext_len < 20 {
                let stem_max = MAX_FILENAME_LENGTH - ext_len;
                let stem: String = result.chars().take(stem_max).collect();
                let ext: String = result.chars().skip(dot_pos).collect();
                result = format!("{}{}", stem, ext);
            } else {
                result = result.chars().take(MAX_FILENAME_LENGTH).collect();
            }
        } else {
            result = result.chars().take(MAX_FILENAME_LENGTH).collect();
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_empty_filename() {
        let validator = RenameValidator::new();
        assert!(validator.validate_filename("").is_err());
        assert!(validator.validate_filename("   ").is_err());
    }

    #[test]
    fn test_validate_valid_filename() {
        let validator = RenameValidator::new();
        assert!(validator.validate_filename("hello.txt").is_ok());
        assert!(validator.validate_filename("my document.pdf").is_ok());
        assert!(validator.validate_filename("file_name-123").is_ok());
    }

    #[test]
    fn test_validate_reserved_names() {
        let validator = RenameValidator::new();
        assert!(validator.validate_filename("CON").is_err());
        assert!(validator.validate_filename("con.txt").is_err());
        assert!(validator.validate_filename("NUL").is_err());
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("hello<world>.txt", '_'), "hello_world_.txt");
        assert_eq!(sanitize_filename("file:name", '_'), "file_name");
        assert_eq!(sanitize_filename("", '_'), "unnamed");
    }
}
