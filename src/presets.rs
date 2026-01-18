//! Preset management system.

use crate::core::{RenameConfig, RenamerError, RenamerResult};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A saved preset containing a rename configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Unique identifier.
    pub id: Uuid,
    /// Display name.
    pub name: String,
    /// Description.
    pub description: Option<String>,
    /// The rename configuration.
    pub config: RenameConfig,
    /// When the preset was created.
    pub created: DateTime<Local>,
    /// When the preset was last modified.
    pub modified: DateTime<Local>,
    /// Tags for organization.
    pub tags: Vec<String>,
    /// Whether this is a built-in preset.
    pub builtin: bool,
}

impl Preset {
    /// Create a new preset from a config.
    pub fn new(name: &str, config: RenameConfig) -> Self {
        let now = Local::now();
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: None,
            config,
            created: now,
            modified: now,
            tags: Vec::new(),
            builtin: false,
        }
    }

    /// Create with description.
    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }

    /// Add tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// Manager for presets.
pub struct PresetManager {
    /// Directory where presets are stored.
    presets_dir: PathBuf,
    /// Loaded presets.
    presets: HashMap<Uuid, Preset>,
}

impl PresetManager {
    /// Create a new preset manager.
    pub fn new(presets_dir: PathBuf) -> Self {
        // Ensure directory exists
        let _ = fs::create_dir_all(&presets_dir);

        let mut manager = Self {
            presets_dir,
            presets: HashMap::new(),
        };

        // Load presets and add built-in ones
        let _ = manager.load_all();
        manager.ensure_builtin_presets();

        manager
    }

    /// Load all presets from disk.
    pub fn load_all(&mut self) -> RenamerResult<()> {
        if !self.presets_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.presets_dir).map_err(|e| RenamerError::Io(e))? {
            let entry = entry.map_err(|e| RenamerError::Io(e))?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(preset) = self.load_preset_from_file(&path) {
                    self.presets.insert(preset.id, preset);
                }
            }
        }

        Ok(())
    }

    /// Load a single preset from a file.
    fn load_preset_from_file(&self, path: &Path) -> RenamerResult<Preset> {
        let file = File::open(path).map_err(|e| RenamerError::Io(e))?;
        let reader = BufReader::new(file);
        let preset: Preset =
            serde_json::from_reader(reader).map_err(|e| RenamerError::JsonError(e))?;
        Ok(preset)
    }

    /// Save a preset to disk.
    pub fn save_preset(&mut self, preset: &Preset) -> RenamerResult<()> {
        let path = self.presets_dir.join(format!("{}.json", preset.id));
        let file = File::create(&path).map_err(|e| RenamerError::Io(e))?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, preset).map_err(|e| RenamerError::JsonError(e))?;

        // Update in-memory cache
        self.presets.insert(preset.id, preset.clone());

        Ok(())
    }

    /// Add a new preset.
    pub fn add_preset(&mut self, preset: Preset) -> RenamerResult<()> {
        self.save_preset(&preset)
    }

    /// Update an existing preset.
    pub fn update_preset(&mut self, mut preset: Preset) -> RenamerResult<()> {
        preset.modified = Local::now();
        self.save_preset(&preset)
    }

    /// Delete a preset.
    pub fn delete_preset(&mut self, id: &Uuid) -> RenamerResult<()> {
        // Don't delete built-in presets
        if let Some(preset) = self.presets.get(id) {
            if preset.builtin {
                return Err(RenamerError::Internal(
                    "Cannot delete built-in preset".to_string(),
                ));
            }
        }

        let path = self.presets_dir.join(format!("{}.json", id));
        if path.exists() {
            fs::remove_file(&path).map_err(|e| RenamerError::Io(e))?;
        }

        self.presets.remove(id);
        Ok(())
    }

    /// Get a preset by ID.
    pub fn get_preset(&self, id: &Uuid) -> Option<&Preset> {
        self.presets.get(id)
    }

    /// Get a preset by name.
    pub fn get_preset_by_name(&self, name: &str) -> Option<&Preset> {
        self.presets.values().find(|p| p.name == name)
    }

    /// Get all presets.
    pub fn get_all(&self) -> Vec<&Preset> {
        let mut presets: Vec<_> = self.presets.values().collect();
        presets.sort_by(|a, b| a.name.cmp(&b.name));
        presets
    }

    /// Get presets by tag.
    pub fn get_by_tag(&self, tag: &str) -> Vec<&Preset> {
        self.presets
            .values()
            .filter(|p| p.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Search presets by name or description.
    pub fn search(&self, query: &str) -> Vec<&Preset> {
        let query_lower = query.to_lowercase();
        self.presets
            .values()
            .filter(|p| {
                p.name.to_lowercase().contains(&query_lower)
                    || p.description
                        .as_ref()
                        .map(|d| d.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Export a preset to a file.
    pub fn export_preset(&self, id: &Uuid, path: &Path) -> RenamerResult<()> {
        let preset = self
            .presets
            .get(id)
            .ok_or(RenamerError::PresetNotFound { name: id.to_string() })?;

        let file = File::create(path).map_err(|e| RenamerError::Io(e))?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, preset).map_err(|e| RenamerError::JsonError(e))?;

        Ok(())
    }

    /// Import a preset from a file.
    pub fn import_preset(&mut self, path: &Path) -> RenamerResult<Preset> {
        let mut preset = self.load_preset_from_file(path)?;

        // Generate new ID to avoid conflicts
        preset.id = Uuid::new_v4();
        preset.builtin = false;

        // Check for name conflicts
        let original_name = preset.name.clone();
        let mut counter = 1;
        while self.get_preset_by_name(&preset.name).is_some() {
            preset.name = format!("{} ({})", original_name, counter);
            counter += 1;
        }

        self.save_preset(&preset)?;
        Ok(preset)
    }

    /// Ensure built-in presets exist.
    fn ensure_builtin_presets(&mut self) {
        let builtin_presets = create_builtin_presets();

        for preset in builtin_presets {
            if self.get_preset_by_name(&preset.name).is_none() {
                let _ = self.save_preset(&preset);
            }
        }
    }

    /// Get all tags used across presets.
    pub fn get_all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .presets
            .values()
            .flat_map(|p| p.tags.clone())
            .collect();

        tags.sort();
        tags.dedup();
        tags
    }

    /// Duplicate a preset.
    pub fn duplicate_preset(&mut self, id: &Uuid) -> RenamerResult<Preset> {
        let original = self
            .presets
            .get(id)
            .ok_or(RenamerError::PresetNotFound { name: id.to_string() })?
            .clone();

        let mut new_preset = original;
        new_preset.id = Uuid::new_v4();
        new_preset.name = format!("{} (Copy)", new_preset.name);
        new_preset.builtin = false;
        new_preset.created = Local::now();
        new_preset.modified = Local::now();

        self.save_preset(&new_preset)?;
        Ok(new_preset)
    }
}

impl Default for PresetManager {
    fn default() -> Self {
        let presets_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("bulk-renamer")
            .join("presets");

        Self::new(presets_dir)
    }
}

/// Create built-in presets.
fn create_builtin_presets() -> Vec<Preset> {
    use crate::core::*;

    let mut presets = Vec::new();

    // 1. Lowercase all
    let mut preset = Preset::new(
        "Lowercase All",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Lowercase All".to_string(),
            rules: vec![RenameRule::new(RuleType::ChangeCase(CaseRule {
                case_type: CaseType::Lower,
                include_extension: true,
            }))],
            separate_extension: false,
            filter: None,
        },
    );
    preset.description = Some("Convert all filenames to lowercase".to_string());
    preset.tags = vec!["case".to_string(), "simple".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 2. Title Case
    let mut preset = Preset::new(
        "Title Case",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Title Case".to_string(),
            rules: vec![RenameRule::new(RuleType::ChangeCase(CaseRule {
                case_type: CaseType::Title,
                include_extension: false,
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Convert filenames to Title Case".to_string());
    preset.tags = vec!["case".to_string(), "simple".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 3. Replace Spaces with Underscores
    let mut preset = Preset::new(
        "Spaces to Underscores",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Spaces to Underscores".to_string(),
            rules: vec![RenameRule::new(RuleType::Replace(ReplaceRule {
                find: " ".to_string(),
                replace: "_".to_string(),
                use_regex: false,
                case_sensitive: true,
                replace_all: true,
                include_extension: false,
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Replace all spaces with underscores".to_string());
    preset.tags = vec!["cleanup".to_string(), "simple".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 4. Add Sequential Numbers
    let mut preset = Preset::new(
        "Add Sequential Numbers",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Add Sequential Numbers".to_string(),
            rules: vec![RenameRule::new(RuleType::Numbering(NumberingRule {
                start: 1,
                increment: 1,
                padding: 3,
                position: InsertPosition::Prefix,
                prefix: String::new(),
                suffix: "_".to_string(),
                reset_per_folder: false,
                format: NumberFormat::Decimal,
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Add sequential numbers at the beginning (001_, 002_, ...)".to_string());
    preset.tags = vec!["numbering".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 5. Date Prefix (Modified Date)
    let mut preset = Preset::new(
        "Date Prefix (Modified)",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Date Prefix (Modified)".to_string(),
            rules: vec![RenameRule::new(RuleType::DateTime(DateTimeRule {
                source: DateSource::Modified,
                format: "%Y-%m-%d_".to_string(),
                position: InsertPosition::Prefix,
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Add file modification date as prefix (YYYY-MM-DD_)".to_string());
    preset.tags = vec!["date".to_string(), "organization".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 6. Remove Brackets
    let mut preset = Preset::new(
        "Remove Brackets",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Remove Brackets".to_string(),
            rules: vec![RenameRule::new(RuleType::Remove(RemoveRule {
                target: RemoveTarget::Bracketed(BracketType::All),
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Remove all bracketed content ((), [], {}, <>)".to_string());
    preset.tags = vec!["cleanup".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 7. Clean Filename
    let mut preset = Preset::new(
        "Clean Filename",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Clean Filename".to_string(),
            rules: vec![RenameRule::new(RuleType::Cleanup(CleanupRule {
                collapse_spaces: true,
                remove_special: true,
                preserve: "-_".to_string(),
                space_replacement: None,
                remove_diacritics: true,
                normalize_unicode: true,
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Clean up filename: remove special chars, diacritics, extra spaces".to_string());
    preset.tags = vec!["cleanup".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 8. Photo Rename (EXIF Date)
    let mut preset = Preset::new(
        "Photo Rename (EXIF Date)",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Photo Rename (EXIF Date)".to_string(),
            rules: vec![RenameRule::new(RuleType::Expression(ExpressionRule {
                expression: "${filedate('exif', '%Y%m%d_%H%M%S')}_${camera}".to_string(),
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Rename photos using EXIF date and camera model".to_string());
    preset.tags = vec!["photo".to_string(), "exif".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 9. Music Rename (ID3 Tags)
    let mut preset = Preset::new(
        "Music Rename (ID3)",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Music Rename (ID3)".to_string(),
            rules: vec![RenameRule::new(RuleType::Expression(ExpressionRule {
                expression: "${num(track, 2)} - ${artist} - ${title}".to_string(),
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Rename music files using ID3 tags: Track - Artist - Title".to_string());
    preset.tags = vec!["music".to_string(), "id3".to_string()];
    preset.builtin = true;
    presets.push(preset);

    // 10. Snake Case
    let mut preset = Preset::new(
        "Snake Case",
        RenameConfig {
            id: Uuid::new_v4(),
            name: "Snake Case".to_string(),
            rules: vec![RenameRule::new(RuleType::ChangeCase(CaseRule {
                case_type: CaseType::Snake,
                include_extension: false,
            }))],
            separate_extension: true,
            filter: None,
        },
    );
    preset.description = Some("Convert filenames to snake_case".to_string());
    preset.tags = vec!["case".to_string(), "developer".to_string()];
    preset.builtin = true;
    presets.push(preset);

    presets
}
