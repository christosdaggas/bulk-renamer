//! Core rename engine implementation.

use crate::core::{
    BracketType, CaseRule, CleanupRule, DateSource, ExpressionRule, FileEntry, FilterConfig,
    FilterField, FilterMode, FilterOperator, FilterRule, InsertPosition, InsertRule, InsertText,
    MetadataField, MetadataRule, NumberingRule, PadRule, RearrangeRule, RemoveRule, RemoveTarget,
    RenameBatch, RenameConfig, RenamePreview, RenameRecord, RenameStatus, RenameTarget,
    RenamerError, RenamerResult, ReplaceRule, RuleType, SortColumn, SortDirection, TrimRule,
    TrimType, TransliterateRule, TransliterationMapping, DateTimeRule,
};
use crate::engine::transformer::*;
use crate::engine::validator::RenameValidator;
use crate::expression::ExpressionEngine;
use chrono::Local;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// The main rename engine that processes files and applies rules.
pub struct RenameEngine {
    /// The rename configuration.
    config: RenameConfig,
    /// Expression engine for template evaluation.
    expression_engine: ExpressionEngine,
    /// Counter state for numbering rules.
    counter_state: HashMap<String, i64>,
    /// Clipboard content cache.
    clipboard_content: Option<String>,
    /// Rename target type.
    target: RenameTarget,
}

impl RenameEngine {
    /// Create a new rename engine with the given configuration.
    pub fn new(config: RenameConfig) -> Self {
        Self {
            config,
            expression_engine: ExpressionEngine::new(),
            counter_state: HashMap::new(),
            clipboard_content: None,
            target: RenameTarget::FilesOnly,
        }
    }

    /// Set the rename target type.
    pub fn set_target(&mut self, target: RenameTarget) {
        self.target = target;
    }

    /// Set clipboard content.
    pub fn set_clipboard(&mut self, content: Option<String>) {
        self.clipboard_content = content;
    }

    /// Update the configuration.
    pub fn set_config(&mut self, config: RenameConfig) {
        self.config = config;
        self.reset_counters();
    }

    /// Reset counter state.
    pub fn reset_counters(&mut self) {
        self.counter_state.clear();
    }

    /// Generate rename previews for a list of files.
    pub fn generate_previews(&mut self, files: &[FileEntry]) -> Vec<RenamePreview> {
        self.reset_counters();
        
        let filtered_files = self.apply_filter(files);
        
        filtered_files
            .iter()
            .map(|entry| self.generate_preview(entry))
            .collect()
    }

    /// Apply filter to file list.
    fn apply_filter<'a>(&self, files: &'a [FileEntry]) -> Vec<&'a FileEntry> {
        files
            .iter()
            .filter(|entry| {
                // Filter by target type
                match self.target {
                    RenameTarget::FilesOnly => !entry.is_directory,
                    RenameTarget::FoldersOnly => entry.is_directory,
                    RenameTarget::Both => true,
                }
            })
            .filter(|entry| {
                // Apply custom filter config
                if let Some(filter) = &self.config.filter {
                    self.matches_filter(entry, filter)
                } else {
                    true
                }
            })
            .collect()
    }

    /// Check if a file matches the filter configuration.
    fn matches_filter(&self, entry: &FileEntry, filter: &FilterConfig) -> bool {
        let matches = filter.rules.iter().all(|rule| self.matches_rule(entry, rule));
        
        match filter.mode {
            FilterMode::Include => matches,
            FilterMode::Exclude => !matches,
        }
    }

    /// Check if a file matches a single filter rule.
    fn matches_rule(&self, entry: &FileEntry, rule: &FilterRule) -> bool {
        let value = match rule.field {
            FilterField::Name => entry.original_name.clone(),
            FilterField::Extension => entry.extension.clone().unwrap_or_default(),
            FilterField::Path => entry.path.to_string_lossy().to_string(),
            FilterField::Size => entry.size.to_string(),
            FilterField::Created => entry
                .created
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            FilterField::Modified => entry
                .modified
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            FilterField::NameLength => entry.original_name.len().to_string(),
            FilterField::PathLength => entry.path.to_string_lossy().len().to_string(),
        };

        match rule.operator {
            FilterOperator::Equals => value.to_lowercase() == rule.value.to_lowercase(),
            FilterOperator::NotEquals => value.to_lowercase() != rule.value.to_lowercase(),
            FilterOperator::Contains => value.to_lowercase().contains(&rule.value.to_lowercase()),
            FilterOperator::NotContains => !value.to_lowercase().contains(&rule.value.to_lowercase()),
            FilterOperator::StartsWith => value.to_lowercase().starts_with(&rule.value.to_lowercase()),
            FilterOperator::EndsWith => value.to_lowercase().ends_with(&rule.value.to_lowercase()),
            FilterOperator::Matches => {
                if let Ok(re) = Regex::new(&rule.value) {
                    re.is_match(&value)
                } else {
                    false
                }
            }
            FilterOperator::MatchesGlob => {
                if let Ok(pattern) = glob::Pattern::new(&rule.value) {
                    pattern.matches(&value)
                } else {
                    false
                }
            }
            FilterOperator::GreaterThan => {
                value.parse::<f64>().ok().zip(rule.value.parse::<f64>().ok())
                    .map(|(v, r)| v > r)
                    .unwrap_or(false)
            }
            FilterOperator::LessThan => {
                value.parse::<f64>().ok().zip(rule.value.parse::<f64>().ok())
                    .map(|(v, r)| v < r)
                    .unwrap_or(false)
            }
            FilterOperator::GreaterOrEqual => {
                value.parse::<f64>().ok().zip(rule.value.parse::<f64>().ok())
                    .map(|(v, r)| v >= r)
                    .unwrap_or(false)
            }
            FilterOperator::LessOrEqual => {
                value.parse::<f64>().ok().zip(rule.value.parse::<f64>().ok())
                    .map(|(v, r)| v <= r)
                    .unwrap_or(false)
            }
            FilterOperator::Between => {
                // Format: "min,max"
                let parts: Vec<&str> = rule.value.split(',').collect();
                if parts.len() == 2 {
                    if let (Some(v), Some(min), Some(max)) = (
                        value.parse::<f64>().ok(),
                        parts[0].trim().parse::<f64>().ok(),
                        parts[1].trim().parse::<f64>().ok(),
                    ) {
                        return v >= min && v <= max;
                    }
                }
                false
            }
        }
    }

    /// Generate a preview for a single file.
    pub fn generate_preview(&mut self, entry: &FileEntry) -> RenamePreview {
        let mut name = if self.config.separate_extension {
            entry.stem()
        } else {
            entry.original_name.clone()
        };

        // Clone the rules to avoid borrow issues
        let rules: Vec<_> = self.config.rules.clone();

        // Apply each enabled rule in order
        for rule in &rules {
            if !rule.enabled {
                continue;
            }

            match self.apply_rule(&name, entry, &rule.rule_type) {
                Ok(new_name) => name = new_name,
                Err(e) => {
                    return RenamePreview {
                        file_id: entry.id,
                        original_name: entry.original_name.clone(),
                        new_name: entry.original_name.clone(),
                        new_path: entry.path.clone(),
                        status: RenameStatus::Error,
                        message: Some(e.to_string()),
                    };
                }
            }
        }

        // Re-add extension if processing separately
        if self.config.separate_extension {
            if let Some(ext) = &entry.extension {
                name = format!("{}.{}", name, ext);
            }
        }

        // Build new path
        let new_path = entry
            .path
            .parent()
            .map(|p| p.join(&name))
            .unwrap_or_else(|| PathBuf::from(&name));

        // Determine status
        let status = if name == entry.original_name {
            RenameStatus::Unchanged
        } else if new_path.exists() && new_path != entry.path {
            RenameStatus::Conflict
        } else {
            RenameStatus::WillRename
        };

        RenamePreview {
            file_id: entry.id,
            original_name: entry.original_name.clone(),
            new_name: name,
            new_path,
            status,
            message: None,
        }
    }

    /// Apply a single rule to a filename.
    fn apply_rule(
        &mut self,
        name: &str,
        entry: &FileEntry,
        rule_type: &RuleType,
    ) -> RenamerResult<String> {
        match rule_type {
            RuleType::Replace(rule) => self.apply_replace(name, rule),
            RuleType::Insert(rule) => self.apply_insert(name, entry, rule),
            RuleType::Remove(rule) => self.apply_remove(name, rule),
            RuleType::ChangeCase(rule) => self.apply_case_change(name, rule),
            RuleType::Numbering(rule) => self.apply_numbering(name, entry, rule),
            RuleType::Trim(rule) => self.apply_trim(name, rule),
            RuleType::Pad(rule) => self.apply_pad(name, rule),
            RuleType::Expression(rule) => self.apply_expression(name, entry, rule),
            RuleType::Rearrange(rule) => self.apply_rearrange(name, rule),
            RuleType::DateTime(rule) => self.apply_datetime(name, entry, rule),
            RuleType::Metadata(rule) => self.apply_metadata(name, entry, rule),
            RuleType::Cleanup(rule) => self.apply_cleanup(name, rule),
            RuleType::Transliterate(rule) => self.apply_transliterate(name, rule),
        }
    }

    /// Apply replace rule.
    fn apply_replace(&self, name: &str, rule: &ReplaceRule) -> RenamerResult<String> {
        if rule.use_regex {
            let regex_pattern = if rule.case_sensitive {
                Regex::new(&rule.find)?
            } else {
                Regex::new(&format!("(?i){}", rule.find))?
            };

            let result = if rule.replace_all {
                regex_pattern.replace_all(name, &rule.replace).to_string()
            } else {
                regex_pattern.replace(name, &rule.replace).to_string()
            };

            Ok(result)
        } else {
            let result = if rule.case_sensitive {
                if rule.replace_all {
                    name.replace(&rule.find, &rule.replace)
                } else {
                    name.replacen(&rule.find, &rule.replace, 1)
                }
            } else {
                // Case-insensitive replace
                let pattern = Regex::new(&format!("(?i){}", regex::escape(&rule.find)))?;
                if rule.replace_all {
                    pattern.replace_all(name, &rule.replace).to_string()
                } else {
                    pattern.replace(name, &rule.replace).to_string()
                }
            };

            Ok(result)
        }
    }

    /// Apply insert rule.
    fn apply_insert(
        &mut self,
        name: &str,
        entry: &FileEntry,
        rule: &InsertRule,
    ) -> RenamerResult<String> {
        // Determine what to insert
        let insert_text = match &rule.text {
            InsertText::Fixed(text) => text.clone(),
            InsertText::ParentFolder => entry.parent_name.clone().unwrap_or_default(),
            InsertText::GrandparentFolder => entry
                .path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            InsertText::CurrentDate(format) => Local::now().format(format).to_string(),
            InsertText::FileDate { source, format } => {
                let date = match source {
                    DateSource::Created => entry.created,
                    DateSource::Modified => entry.modified,
                    DateSource::Accessed => entry.accessed,
                    DateSource::Now => Some(Local::now()),
                    DateSource::ExifDateTaken => entry
                        .metadata_cache
                        .as_ref()
                        .and_then(|m| m.exif.as_ref())
                        .and_then(|e| e.date_taken),
                };
                date.map(|d| d.format(format).to_string())
                    .unwrap_or_default()
            }
            InsertText::Counter(config) => {
                let key = if config.start == 1 {
                    "default".to_string()
                } else {
                    format!("counter_{}", config.start)
                };
                let counter = self.counter_state.entry(key).or_insert(config.start);
                let result = format_number(*counter, config.format, config.padding);
                *counter += config.increment;
                result
            }
            InsertText::Clipboard => self.clipboard_content.clone().unwrap_or_default(),
            InsertText::Expression(expr) => {
                self.expression_engine.evaluate(expr, entry, name)?
            }
        };

        // Determine where to insert
        let result = match &rule.position {
            InsertPosition::Prefix => format!("{}{}", insert_text, name),
            InsertPosition::Suffix => format!("{}{}", name, insert_text),
            InsertPosition::Position(pos) => insert_at_position(name, &insert_text, *pos),
            InsertPosition::BeforeText(text) => {
                if let Some(idx) = name.find(text) {
                    let (before, after) = name.split_at(idx);
                    format!("{}{}{}", before, insert_text, after)
                } else {
                    name.to_string()
                }
            }
            InsertPosition::AfterText(text) => {
                if let Some(idx) = name.find(text) {
                    let split_pos = idx + text.len();
                    let (before, after) = name.split_at(split_pos);
                    format!("{}{}{}", before, insert_text, after)
                } else {
                    name.to_string()
                }
            }
            InsertPosition::BeforeNth { pattern, n } => {
                let mut count = 0;
                let mut result = String::new();
                let mut remaining = name;

                while let Some(idx) = remaining.find(pattern) {
                    count += 1;
                    if count == *n {
                        result.push_str(&remaining[..idx]);
                        result.push_str(&insert_text);
                        result.push_str(&remaining[idx..]);
                        return Ok(result);
                    }
                    result.push_str(&remaining[..idx + pattern.len()]);
                    remaining = &remaining[idx + pattern.len()..];
                }
                result.push_str(remaining);
                result
            }
            InsertPosition::AfterNth { pattern, n } => {
                let mut count = 0;
                let mut result = String::new();
                let mut remaining = name;

                while let Some(idx) = remaining.find(pattern) {
                    count += 1;
                    let end_idx = idx + pattern.len();
                    if count == *n {
                        result.push_str(&remaining[..end_idx]);
                        result.push_str(&insert_text);
                        result.push_str(&remaining[end_idx..]);
                        return Ok(result);
                    }
                    result.push_str(&remaining[..end_idx]);
                    remaining = &remaining[end_idx..];
                }
                result.push_str(remaining);
                result
            }
        };

        Ok(result)
    }

    /// Apply remove rule.
    fn apply_remove(&self, name: &str, rule: &RemoveRule) -> RenamerResult<String> {
        let result = match &rule.target {
            RemoveTarget::Text { text, case_sensitive } => {
                if *case_sensitive {
                    name.replace(text, "")
                } else {
                    let pattern = Regex::new(&format!("(?i){}", regex::escape(text)))?;
                    pattern.replace_all(name, "").to_string()
                }
            }
            RemoveTarget::Pattern(pattern) => {
                let regex = Regex::new(pattern)?;
                regex.replace_all(name, "").to_string()
            }
            RemoveTarget::Range { start, end } => remove_range(name, *start, *end),
            RemoveTarget::FirstN(n) => {
                let chars: Vec<char> = name.chars().collect();
                chars.iter().skip(*n).collect()
            }
            RemoveTarget::LastN(n) => {
                let chars: Vec<char> = name.chars().collect();
                let len = chars.len();
                if *n >= len {
                    String::new()
                } else {
                    chars.iter().take(len - n).collect()
                }
            }
            RemoveTarget::Digits => name.chars().filter(|c| !c.is_ascii_digit()).collect(),
            RemoveTarget::Letters => name.chars().filter(|c| !c.is_alphabetic()).collect(),
            RemoveTarget::Symbols => name
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace())
                .collect(),
            RemoveTarget::Whitespace => name.chars().filter(|c| !c.is_whitespace()).collect(),
            RemoveTarget::Characters(chars) => {
                name.chars().filter(|c| !chars.contains(*c)).collect()
            }
            RemoveTarget::Words(words) => {
                let mut result = name.to_string();
                for word in words {
                    result = result.replace(word, "");
                }
                collapse_spaces(&result)
            }
            RemoveTarget::Bracketed(bracket_type) => match bracket_type {
                BracketType::Round => remove_bracketed(name, '(', ')'),
                BracketType::Square => remove_bracketed(name, '[', ']'),
                BracketType::Curly => remove_bracketed(name, '{', '}'),
                BracketType::Angle => remove_bracketed(name, '<', '>'),
                BracketType::All => {
                    let mut result = remove_bracketed(name, '(', ')');
                    result = remove_bracketed(&result, '[', ']');
                    result = remove_bracketed(&result, '{', '}');
                    remove_bracketed(&result, '<', '>')
                }
            },
            RemoveTarget::Duplicates => {
                let mut result = String::new();
                let mut prev: Option<char> = None;
                for c in name.chars() {
                    if Some(c) != prev {
                        result.push(c);
                    }
                    prev = Some(c);
                }
                result
            }
            RemoveTarget::LeadingZeros => {
                use std::sync::OnceLock;
                static LEADING_ZEROS_RE: OnceLock<Regex> = OnceLock::new();
                let re = LEADING_ZEROS_RE.get_or_init(|| {
                    Regex::new(r"^0+").expect("valid leading zeros regex")
                });
                re.replace(name, "").to_string()
            }
            RemoveTarget::BeforeAfter {
                marker,
                remove_before,
                include_marker,
            } => {
                if let Some(idx) = name.find(marker) {
                    if *remove_before {
                        if *include_marker {
                            name[idx + marker.len()..].to_string()
                        } else {
                            name[idx..].to_string()
                        }
                    } else {
                        if *include_marker {
                            name[..idx].to_string()
                        } else {
                            name[..idx + marker.len()].to_string()
                        }
                    }
                } else {
                    name.to_string()
                }
            }
        };

        Ok(result)
    }

    /// Apply case change rule.
    fn apply_case_change(&self, name: &str, rule: &CaseRule) -> RenamerResult<String> {
        Ok(transform_case(name, rule.case_type))
    }

    /// Apply numbering rule.
    fn apply_numbering(
        &mut self,
        name: &str,
        entry: &FileEntry,
        rule: &NumberingRule,
    ) -> RenamerResult<String> {
        let key = if rule.reset_per_folder {
            entry
                .parent_name
                .clone()
                .unwrap_or_else(|| "root".to_string())
        } else {
            "global".to_string()
        };

        let counter = self.counter_state.entry(key).or_insert(rule.start);
        let number_str = format!(
            "{}{}{}",
            rule.prefix,
            format_number(*counter, rule.format, rule.padding),
            rule.suffix
        );
        *counter += rule.increment;

        // Apply position
        let result = match &rule.position {
            InsertPosition::Prefix => format!("{}{}", number_str, name),
            InsertPosition::Suffix => format!("{}{}", name, number_str),
            InsertPosition::Position(pos) => insert_at_position(name, &number_str, *pos),
            _ => format!("{}{}", name, number_str), // Default to suffix
        };

        Ok(result)
    }

    /// Apply trim rule.
    fn apply_trim(&self, name: &str, rule: &TrimRule) -> RenamerResult<String> {
        let result = match rule.trim_type {
            TrimType::Both => name.trim().to_string(),
            TrimType::Start => name.trim_start().to_string(),
            TrimType::End => name.trim_end().to_string(),
            TrimType::Characters => {
                if let Some(chars) = &rule.characters {
                    let char_set: Vec<char> = chars.chars().collect();
                    name.trim_matches(|c| char_set.contains(&c)).to_string()
                } else {
                    name.to_string()
                }
            }
            TrimType::MaxLength(max) => {
                let chars: Vec<char> = name.chars().collect();
                chars.into_iter().take(max).collect()
            }
        };

        Ok(result)
    }

    /// Apply pad rule.
    fn apply_pad(&self, name: &str, rule: &PadRule) -> RenamerResult<String> {
        let current_len = name.chars().count();
        if current_len >= rule.length {
            return Ok(name.to_string());
        }

        let padding: String = std::iter::repeat(rule.pad_char)
            .take(rule.length - current_len)
            .collect();

        if rule.pad_start {
            Ok(format!("{}{}", padding, name))
        } else {
            Ok(format!("{}{}", name, padding))
        }
    }

    /// Apply expression rule.
    fn apply_expression(
        &self,
        name: &str,
        entry: &FileEntry,
        rule: &ExpressionRule,
    ) -> RenamerResult<String> {
        self.expression_engine.evaluate(&rule.expression, entry, name)
    }

    /// Apply rearrange rule.
    fn apply_rearrange(&self, name: &str, rule: &RearrangeRule) -> RenamerResult<String> {
        let parts: Vec<&str> = name.split(&rule.separator).collect();

        let rearranged: Vec<&str> = rule
            .order
            .iter()
            .filter_map(|&idx| parts.get(idx).copied())
            .collect();

        Ok(rearranged.join(&rule.new_separator))
    }

    /// Apply datetime rule.
    fn apply_datetime(
        &self,
        name: &str,
        entry: &FileEntry,
        rule: &DateTimeRule,
    ) -> RenamerResult<String> {
        let date = match rule.source {
            DateSource::Created => entry.created,
            DateSource::Modified => entry.modified,
            DateSource::Accessed => entry.accessed,
            DateSource::Now => Some(Local::now()),
            DateSource::ExifDateTaken => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.exif.as_ref())
                .and_then(|e| e.date_taken),
        };

        let date_str = date
            .map(|d| d.format(&rule.format).to_string())
            .unwrap_or_default();

        // Apply position
        let result = match &rule.position {
            InsertPosition::Prefix => format!("{}{}", date_str, name),
            InsertPosition::Suffix => format!("{}{}", name, date_str),
            InsertPosition::Position(pos) => insert_at_position(name, &date_str, *pos),
            _ => format!("{}{}", name, date_str),
        };

        Ok(result)
    }

    /// Apply metadata rule.
    fn apply_metadata(
        &self,
        name: &str,
        entry: &FileEntry,
        rule: &MetadataRule,
    ) -> RenamerResult<String> {
        let metadata_value = self.get_metadata_value(entry, &rule.field);
        let value = metadata_value.unwrap_or_else(|| rule.fallback.clone());

        // Apply position
        let result = match &rule.position {
            InsertPosition::Prefix => format!("{}{}", value, name),
            InsertPosition::Suffix => format!("{}{}", name, value),
            InsertPosition::Position(pos) => insert_at_position(name, &value, *pos),
            _ => format!("{}{}", name, value),
        };

        Ok(result)
    }

    /// Get metadata value for a field.
    fn get_metadata_value(&self, entry: &FileEntry, field: &MetadataField) -> Option<String> {
        match field {
            MetadataField::ExifDateTaken => entry
                .metadata_cache
                .as_ref()?
                .exif
                .as_ref()?
                .date_taken
                .map(|d| d.format("%Y-%m-%d").to_string()),
            MetadataField::ExifCameraMake => entry
                .metadata_cache
                .as_ref()?
                .exif
                .as_ref()?
                .camera_make
                .clone(),
            MetadataField::ExifCameraModel => entry
                .metadata_cache
                .as_ref()?
                .exif
                .as_ref()?
                .camera_model
                .clone(),
            MetadataField::ExifWidth => entry
                .metadata_cache
                .as_ref()?
                .exif
                .as_ref()?
                .width
                .map(|w| w.to_string()),
            MetadataField::ExifHeight => entry
                .metadata_cache
                .as_ref()?
                .exif
                .as_ref()?
                .height
                .map(|h| h.to_string()),
            MetadataField::Id3Title => entry
                .metadata_cache
                .as_ref()?
                .id3
                .as_ref()?
                .title
                .clone(),
            MetadataField::Id3Artist => entry
                .metadata_cache
                .as_ref()?
                .id3
                .as_ref()?
                .artist
                .clone(),
            MetadataField::Id3Album => entry
                .metadata_cache
                .as_ref()?
                .id3
                .as_ref()?
                .album
                .clone(),
            MetadataField::Id3Year => entry
                .metadata_cache
                .as_ref()?
                .id3
                .as_ref()?
                .year
                .map(|y| y.to_string()),
            MetadataField::Id3Track => entry
                .metadata_cache
                .as_ref()?
                .id3
                .as_ref()?
                .track
                .map(|t| t.to_string()),
            MetadataField::Id3Genre => entry
                .metadata_cache
                .as_ref()?
                .id3
                .as_ref()?
                .genre
                .clone(),
            MetadataField::FileSize => Some(format_file_size(entry.size)),
            MetadataField::FileCreated => entry.created.map(|d| d.format("%Y-%m-%d").to_string()),
            MetadataField::FileModified => entry.modified.map(|d| d.format("%Y-%m-%d").to_string()),
            MetadataField::FileAccessed => entry.accessed.map(|d| d.format("%Y-%m-%d").to_string()),
            MetadataField::FilePath => entry.path.parent().map(|p| p.to_string_lossy().to_string()),
            MetadataField::FileParent => entry.parent_name.clone(),
            MetadataField::FileExtension => entry.extension.clone(),
            _ => None,
        }
    }

    /// Apply cleanup rule.
    fn apply_cleanup(&self, name: &str, rule: &CleanupRule) -> RenamerResult<String> {
        let mut result = name.to_string();

        if rule.remove_diacritics {
            result = remove_diacritics(&result);
        }

        if rule.remove_special {
            let preserve_chars: Vec<char> = rule.preserve.chars().collect();
            result = result
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace() || preserve_chars.contains(c))
                .collect();
        }

        if rule.collapse_spaces {
            result = collapse_spaces(&result);
        }

        if let Some(replacement) = rule.space_replacement {
            result = result.replace(' ', &replacement.to_string());
        }

        Ok(result)
    }

    /// Apply transliterate rule.
    fn apply_transliterate(&self, name: &str, rule: &TransliterateRule) -> RenamerResult<String> {
        let result = match rule.mapping {
            TransliterationMapping::CyrillicToLatin => cyrillic_to_latin(name),
            TransliterationMapping::GreekToLatin => greek_to_latin(name),
            TransliterationMapping::RemoveDiacritics => remove_diacritics(name),
            TransliterationMapping::NormalizeUnicode => {
                // Basic unicode normalization
                name.to_string()
            }
        };

        Ok(result)
    }

    /// Sort files by column.
    pub fn sort_files(
        files: &mut [FileEntry],
        column: SortColumn,
        direction: SortDirection,
    ) {
        files.sort_by(|a, b| {
            let cmp = match column {
                SortColumn::OriginalName => a.original_name.cmp(&b.original_name),
                SortColumn::NewName => a.original_name.cmp(&b.original_name), // Preview sorts separately
                SortColumn::Status => std::cmp::Ordering::Equal,
                SortColumn::Size => a.size.cmp(&b.size),
                SortColumn::Modified => a.modified.cmp(&b.modified),
                SortColumn::Extension => a.extension.cmp(&b.extension),
                SortColumn::Path => a.path.cmp(&b.path),
            };

            match direction {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
            }
        });
    }

    /// Sort previews by column.
    pub fn sort_previews(
        previews: &mut [RenamePreview],
        column: SortColumn,
        direction: SortDirection,
    ) {
        previews.sort_by(|a, b| {
            let cmp = match column {
                SortColumn::OriginalName => a.original_name.cmp(&b.original_name),
                SortColumn::NewName => a.new_name.cmp(&b.new_name),
                SortColumn::Status => (a.status as u8).cmp(&(b.status as u8)),
                _ => std::cmp::Ordering::Equal,
            };

            match direction {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
            }
        });
    }
}

/// Format file size in human-readable format.
fn format_file_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

/// A planned rename operation. Every item is first moved to a temporary path,
/// then moved from the temporary path to its final destination.
#[derive(Debug, Clone)]
pub struct RenamePlan {
    pub items: Vec<RenamePlanItem>,
    pub skipped: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub struct RenamePlanItem {
    pub file_id: Uuid,
    pub original_path: PathBuf,
    pub temp_path: PathBuf,
    pub new_path: PathBuf,
    pub was_directory: bool,
}

/// Failure for a single rename candidate.
#[derive(Debug, Clone)]
pub struct RenameFailure {
    pub file_id: Uuid,
    pub original_path: Option<PathBuf>,
    pub target_path: PathBuf,
    pub error: String,
}

/// Structured result for a batch rename.
#[derive(Debug, Clone)]
pub struct RenameBatchResult {
    pub batch: Option<RenameBatch>,
    pub successes: Vec<RenameRecord>,
    pub failures: Vec<RenameFailure>,
    pub skipped: Vec<Uuid>,
}

impl RenameBatchResult {
    pub fn success_count(&self) -> usize {
        self.successes.len()
    }

    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    pub fn all_successful(&self) -> bool {
        self.failure_count() == 0
    }
}

/// Validate and plan rename operations for execution.
pub fn plan_renames(
    previews: &[RenamePreview],
    files: &HashMap<Uuid, FileEntry>,
) -> RenamerResult<RenamePlan> {
    let validator = RenameValidator::new();
    let validation_errors = validator.validate_batch_with_files(previews, files);
    if !validation_errors.is_empty() {
        let messages = validation_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(RenamerError::Internal(messages));
    }

    let mut items = Vec::new();
    let mut skipped = Vec::new();

    for preview in previews {
        if !matches!(preview.status, RenameStatus::WillRename) {
            skipped.push(preview.file_id);
            continue;
        }

        let entry = files
            .get(&preview.file_id)
            .ok_or_else(|| RenamerError::FileNotFound {
                path: preview.new_path.clone(),
            })?;

        let temp_path = unique_temp_path(entry);
        items.push(RenamePlanItem {
            file_id: preview.file_id,
            original_path: entry.path.clone(),
            temp_path,
            new_path: preview.new_path.clone(),
            was_directory: entry.is_directory,
        });
    }

    Ok(RenamePlan { items, skipped })
}

/// Execute a prepared rename plan using two phases to avoid source/target swaps
/// overwriting each other.
pub fn execute_rename_plan(plan: RenamePlan) -> RenameBatchResult {
    let mut staged = Vec::new();
    let mut failures = Vec::new();

    for item in plan.items {
        match std::fs::rename(&item.original_path, &item.temp_path) {
            Ok(()) => staged.push(item),
            Err(err) => failures.push(RenameFailure {
                file_id: item.file_id,
                original_path: Some(item.original_path),
                target_path: item.new_path,
                error: err.to_string(),
            }),
        }
    }

    let mut successes = Vec::new();
    for item in staged {
        match std::fs::rename(&item.temp_path, &item.new_path) {
            Ok(()) => successes.push(RenameRecord {
                id: Uuid::new_v4(),
                timestamp: Local::now(),
                original_path: item.original_path,
                new_path: item.new_path,
                was_directory: item.was_directory,
            }),
            Err(err) => {
                let rollback_error = std::fs::rename(&item.temp_path, &item.original_path)
                    .err()
                    .map(|rollback| format!("; rollback failed: {}", rollback))
                    .unwrap_or_default();
                failures.push(RenameFailure {
                    file_id: item.file_id,
                    original_path: Some(item.original_path),
                    target_path: item.new_path,
                    error: format!("{}{}", err, rollback_error),
                });
            }
        }
    }

    let batch = if successes.is_empty() {
        None
    } else {
        Some(RenameBatch::new(successes.clone()))
    };

    RenameBatchResult {
        batch,
        successes,
        failures,
        skipped: plan.skipped,
    }
}

/// Execute the actual rename operations.
pub fn execute_renames(
    previews: &[RenamePreview],
    files: &HashMap<Uuid, FileEntry>,
) -> RenamerResult<RenameBatchResult> {
    let plan = plan_renames(previews, files)?;
    Ok(execute_rename_plan(plan))
}

fn unique_temp_path(entry: &FileEntry) -> PathBuf {
    let parent = entry.path.parent().map(PathBuf::from).unwrap_or_default();
    let stem = entry
        .path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();

    loop {
        let candidate = parent.join(format!(".bulk-renamer-{}-{}", Uuid::new_v4(), stem));
        if !candidate.exists() {
            return candidate;
        }
    }
}

#[cfg(test)]
mod rename_safety_tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bulk-renamer-{}-{}", name, Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn preview_for(entry: &FileEntry, new_name: &str) -> RenamePreview {
        let new_path = entry.path.parent().unwrap().join(new_name);
        RenamePreview {
            file_id: entry.id,
            original_name: entry.original_name.clone(),
            new_name: new_name.to_string(),
            new_path,
            status: RenameStatus::WillRename,
            message: None,
        }
    }

    #[test]
    fn execute_renames_handles_name_swap() {
        let dir = temp_dir("swap");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        fs::write(&a, "a").expect("write a");
        fs::write(&b, "b").expect("write b");

        let entry_a = FileEntry::from_path(a.clone(), 0).expect("entry a");
        let entry_b = FileEntry::from_path(b.clone(), 0).expect("entry b");
        let previews = vec![preview_for(&entry_a, "b.txt"), preview_for(&entry_b, "a.txt")];
        let files = HashMap::from([(entry_a.id, entry_a), (entry_b.id, entry_b)]);

        let result = execute_renames(&previews, &files).expect("execute swap");
        assert!(result.all_successful());
        assert_eq!(fs::read_to_string(&a).expect("read a"), "b");
        assert_eq!(fs::read_to_string(&b).expect("read b"), "a");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn plan_renames_rejects_existing_external_target() {
        let dir = temp_dir("external-conflict");
        let source = dir.join("source.txt");
        let existing = dir.join("existing.txt");
        fs::write(&source, "source").expect("write source");
        fs::write(&existing, "existing").expect("write existing");

        let entry = FileEntry::from_path(source, 0).expect("entry");
        let previews = vec![preview_for(&entry, "existing.txt")];
        let files = HashMap::from([(entry.id, entry)]);

        assert!(plan_renames(&previews, &files).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn plan_renames_rejects_duplicate_targets() {
        let dir = temp_dir("duplicate-target");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        fs::write(&a, "a").expect("write a");
        fs::write(&b, "b").expect("write b");

        let entry_a = FileEntry::from_path(a, 0).expect("entry a");
        let entry_b = FileEntry::from_path(b, 0).expect("entry b");
        let previews = vec![
            preview_for(&entry_a, "same.txt"),
            preview_for(&entry_b, "same.txt"),
        ];
        let files = HashMap::from([(entry_a.id, entry_a), (entry_b.id, entry_b)]);

        assert!(plan_renames(&previews, &files).is_err());
        fs::remove_dir_all(dir).ok();
    }
}
