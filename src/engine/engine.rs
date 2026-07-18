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
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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

/// An enabled rule with everything that only has to be built once per preview pass.
struct PreparedRule {
    /// Identity of the originating rule, so counter state stays per rule.
    id: Uuid,
    rule_type: RuleType,
    /// Pattern for the rule types that use one.
    regex: Option<Regex>,
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
        self.expression_engine.reset_counter();
    }

    /// Generate rename previews for a list of files.
    pub fn generate_previews(&mut self, files: &[FileEntry]) -> Vec<RenamePreview> {
        self.reset_counters();

        let filtered_files = self.apply_filter(files);
        self.expression_engine.set_total(filtered_files.len() as i64);

        let prepared = match self.prepare_rules() {
            Ok(prepared) => prepared,
            Err(err) => {
                return filtered_files
                    .iter()
                    .map(|entry| error_preview(entry, &err))
                    .collect();
            }
        };

        // A destination that lands on another batch member's original path is not a
        // conflict: the two-phase executor vacates every source before any final move,
        // which is what makes swaps and rotations work.
        let batch_originals: HashSet<PathBuf> = filtered_files
            .iter()
            .map(|entry| entry.path.clone())
            .collect();

        filtered_files
            .iter()
            .map(|entry| {
                let preview = self.build_preview(entry, &prepared, &batch_originals);
                // ${index} identifies the file, so it advances once per file rather than
                // once per expression that reads it.
                self.expression_engine.next_counter();
                preview
            })
            .collect()
    }

    /// Compile the enabled rules once for a whole preview pass.
    fn prepare_rules(&self) -> RenamerResult<Vec<PreparedRule>> {
        self.config
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .map(|rule| {
                let regex = match &rule.rule_type {
                    RuleType::Replace(replace) if replace.use_regex => {
                        Some(compile_pattern(&replace.find, replace.case_sensitive)?)
                    }
                    RuleType::Remove(remove) => match &remove.target {
                        RemoveTarget::Pattern(pattern) => Some(Regex::new(pattern)?),
                        _ => None,
                    },
                    _ => None,
                };

                Ok(PreparedRule {
                    id: rule.id,
                    rule_type: rule.rule_type.clone(),
                    regex,
                })
            })
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
        match self.prepare_rules() {
            // A single file is its own batch, so nothing else can vacate a destination.
            Ok(prepared) => self.build_preview(entry, &prepared, &HashSet::new()),
            Err(err) => error_preview(entry, &err),
        }
    }

    /// Generate a preview using rules already prepared for this pass.
    fn build_preview(
        &mut self,
        entry: &FileEntry,
        rules: &[PreparedRule],
        batch_originals: &HashSet<PathBuf>,
    ) -> RenamePreview {
        let mut name = if self.config.separate_extension {
            entry.stem()
        } else {
            entry.original_name.clone()
        };

        // Apply each rule in order
        for rule in rules {
            match self.apply_rule(&name, entry, rule) {
                Ok(new_name) => name = new_name,
                Err(e) => return error_preview(entry, &e),
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

        // Determine status. A target that "exists" is not a conflict when it is
        // this entry itself reached under different casing (case-insensitive
        // filesystems): the two-phase executor vacates the source first, so a
        // case-only rename is safe.
        let status = if name == entry.original_name {
            RenameStatus::Unchanged
        } else if new_path.exists()
            && new_path != entry.path
            && !batch_originals.contains(&new_path)
            && !paths_are_same_file(&new_path, &entry.path)
        {
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
        rule: &PreparedRule,
    ) -> RenamerResult<String> {
        match &rule.rule_type {
            RuleType::Replace(cfg) => self.apply_replace(name, cfg, rule.regex.as_ref()),
            RuleType::Insert(cfg) => self.apply_insert(name, entry, cfg, rule.id),
            RuleType::Remove(cfg) => self.apply_remove(name, cfg, rule.regex.as_ref()),
            RuleType::ChangeCase(cfg) => self.apply_case_change(name, cfg),
            RuleType::Numbering(cfg) => self.apply_numbering(name, entry, cfg, rule.id),
            RuleType::Trim(cfg) => self.apply_trim(name, cfg),
            RuleType::Pad(cfg) => self.apply_pad(name, cfg),
            RuleType::Expression(cfg) => self.apply_expression(name, entry, cfg),
            RuleType::Rearrange(cfg) => self.apply_rearrange(name, cfg),
            RuleType::DateTime(cfg) => self.apply_datetime(name, entry, cfg),
            RuleType::Metadata(cfg) => self.apply_metadata(name, entry, cfg),
            RuleType::Cleanup(cfg) => self.apply_cleanup(name, cfg),
            RuleType::Transliterate(cfg) => self.apply_transliterate(name, cfg),
        }
    }

    /// Apply replace rule.
    fn apply_replace(
        &self,
        name: &str,
        rule: &ReplaceRule,
        regex: Option<&Regex>,
    ) -> RenamerResult<String> {
        if rule.use_regex {
            let regex_pattern = regex.ok_or_else(|| {
                RenamerError::Internal("Regex rule was not prepared".to_string())
            })?;

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
                // A literal replace has no regex syntax to honour, and routing it through
                // the regex engine cost two orders of magnitude for nothing.
                replace_ignore_case(name, &rule.find, &rule.replace, rule.replace_all)
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
        rule_id: Uuid,
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
                let counter = self
                    .counter_state
                    .entry(counter_key(rule_id, None))
                    .or_insert(config.start);
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
    fn apply_remove(
        &self,
        name: &str,
        rule: &RemoveRule,
        regex: Option<&Regex>,
    ) -> RenamerResult<String> {
        let result = match &rule.target {
            RemoveTarget::Text { text, case_sensitive } => {
                if *case_sensitive {
                    name.replace(text, "")
                } else {
                    replace_ignore_case(name, text, "", true)
                }
            }
            RemoveTarget::Pattern(_) => {
                let pattern = regex.ok_or_else(|| {
                    RenamerError::Internal("Regex rule was not prepared".to_string())
                })?;
                pattern.replace_all(name, "").to_string()
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
        rule_id: Uuid,
    ) -> RenamerResult<String> {
        let scope = if rule.reset_per_folder {
            Some(entry.parent_name.as_deref().unwrap_or("root"))
        } else {
            None
        };

        let counter = self
            .counter_state
            .entry(counter_key(rule_id, scope))
            .or_insert(rule.start);
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
                SortColumn::OriginalName => natural_cmp(&a.original_name, &b.original_name),
                SortColumn::NewName => natural_cmp(&a.original_name, &b.original_name), // Preview sorts separately
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

/// Compile a user pattern, honouring the rule's case sensitivity.
fn compile_pattern(pattern: &str, case_sensitive: bool) -> RenamerResult<Regex> {
    let regex = if case_sensitive {
        Regex::new(pattern)?
    } else {
        Regex::new(&format!("(?i){}", pattern))?
    };

    Ok(regex)
}

/// Key for a rule's counter state. Two numbering rules in one configuration each keep
/// their own sequence, and `reset_per_folder` splits a rule's sequence by folder.
fn counter_key(rule_id: Uuid, scope: Option<&str>) -> String {
    match scope {
        Some(scope) => format!("{}:{}", rule_id, scope),
        None => format!("{}:global", rule_id),
    }
}

/// Build the preview shown when a rule cannot be applied to a file.
fn error_preview(entry: &FileEntry, error: &RenamerError) -> RenamePreview {
    RenamePreview {
        file_id: entry.id,
        original_name: entry.original_name.clone(),
        new_name: entry.original_name.clone(),
        new_path: entry.path.clone(),
        status: RenameStatus::Error,
        message: Some(error.to_string()),
    }
}

/// Replace literal occurrences of `needle`, ignoring case.
fn replace_ignore_case(haystack: &str, needle: &str, replacement: &str, all: bool) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }

    let mut result = String::with_capacity(haystack.len());
    let mut cursor = 0;

    while let Some((start, end)) = find_ignore_case(haystack, needle, cursor) {
        result.push_str(&haystack[cursor..start]);
        result.push_str(replacement);
        cursor = end;

        if !all {
            break;
        }
    }

    result.push_str(&haystack[cursor..]);
    result
}

/// Byte range of the first case-insensitive match of `needle` at or after `from`.
fn find_ignore_case(haystack: &str, needle: &str, from: usize) -> Option<(usize, usize)> {
    let mut start = from;

    while start <= haystack.len() {
        let mut candidate = haystack[start..].chars();
        let mut matched = 0;
        let mut is_match = true;

        for wanted in needle.chars() {
            // Per-character simple folding, which is what `(?i)` gave us here before.
            match candidate.next() {
                Some(found) if found.to_lowercase().eq(wanted.to_lowercase()) => {
                    matched += found.len_utf8();
                }
                _ => {
                    is_match = false;
                    break;
                }
            }
        }

        if is_match {
            return Some((start, start + matched));
        }

        // Advance one character, so the scan always lands on a boundary.
        match haystack[start..].chars().next() {
            Some(c) => start += c.len_utf8(),
            None => break,
        }
    }

    None
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenamePlan {
    pub items: Vec<RenamePlanItem>,
    pub skipped: Vec<Uuid>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

        let temp_path = unique_temp_path(&entry.path);
        items.push(RenamePlanItem {
            file_id: preview.file_id,
            original_path: entry.path.clone(),
            temp_path,
            new_path: preview.new_path.clone(),
            was_directory: entry.is_directory,
        });
    }

    reject_nested_items(&items)?;

    Ok(RenamePlan { items, skipped })
}

/// Refuse a batch that renames a directory together with anything inside it.
///
/// Every item stages next to the file it stands in for, so renaming the directory moves
/// the staging paths of its contents too. The descendant's recorded temp path then no
/// longer exists, its own move and its rollback both fail, and its file is left inside
/// the renamed directory under a staging name. The final path is wrong as well: the
/// preview builds it from the directory's old name. Neither is repairable here.
fn reject_nested_items(items: &[RenamePlanItem]) -> RenamerResult<()> {
    let directories: HashSet<&Path> = items
        .iter()
        .filter(|item| item.was_directory)
        .map(|item| item.original_path.as_path())
        .collect();

    if directories.is_empty() {
        return Ok(());
    }

    for item in items {
        // `ancestors` starts at the path itself, which is this item's own entry.
        if let Some(parent) = item
            .original_path
            .ancestors()
            .skip(1)
            .find(|ancestor| directories.contains(ancestor))
        {
            return Err(RenamerError::Internal(format!(
                "'{}' and '{}' inside it cannot be renamed in the same batch; \
                 rename the folder and its contents separately",
                parent.display(),
                item.original_path.display()
            )));
        }
    }

    Ok(())
}

/// Error string used for items that were not renamed because the user
/// cancelled the batch. The rollback restores everything, so a result whose
/// failures all carry this reason means "nothing changed".
pub const CANCELLED: &str = "cancelled by the user";

/// Execute a prepared rename plan using two phases to avoid source/target swaps
/// overwriting each other.
pub fn execute_rename_plan(plan: RenamePlan) -> RenameBatchResult {
    execute_rename_plan_with(plan, |_, _| {}, &std::sync::atomic::AtomicBool::new(false))
}

/// Progress- and cancellation-aware executor. `progress(done, total)` is called
/// after each filesystem move; `total` counts both phases. A cancellation
/// requested between moves unwinds exactly like a failure, so the batch stays
/// all-or-nothing: either every rename lands or every file is back where it was.
pub fn execute_rename_plan_with(
    plan: RenamePlan,
    progress: impl Fn(usize, usize),
    cancel: &std::sync::atomic::AtomicBool,
) -> RenameBatchResult {
    use std::sync::atomic::Ordering;

    let total_steps = plan.items.len() * 2;
    let mut done_steps = 0;
    let mut staged = Vec::new();
    let mut failures = Vec::new();

    for item in plan.items {
        if cancel.load(Ordering::Relaxed) {
            failures.push(RenameFailure {
                file_id: item.file_id,
                original_path: Some(item.original_path),
                target_path: item.new_path,
                error: CANCELLED.to_string(),
            });
            continue;
        }
        match std::fs::rename(&item.original_path, &item.temp_path) {
            Ok(()) => {
                done_steps += 1;
                progress(done_steps, total_steps);
                staged.push(item);
            }
            Err(err) => failures.push(RenameFailure {
                file_id: item.file_id,
                original_path: Some(item.original_path),
                target_path: item.new_path,
                error: err.to_string(),
            }),
        }
    }

    // An item that failed to stage is still sitting on its original path, and that path
    // may be the destination of an item that did stage. Finishing phase 2 would rename
    // straight over it, so the batch is all-or-nothing.
    if !failures.is_empty() {
        for item in staged {
            let rollback_error = restore_staged(&item);
            if !rollback_error.is_empty() {
                failures.push(RenameFailure {
                    file_id: item.file_id,
                    original_path: Some(item.original_path),
                    target_path: item.new_path,
                    error: format!("batch aborted{}", rollback_error),
                });
            }
        }

        return RenameBatchResult {
            batch: None,
            successes: Vec::new(),
            failures,
            skipped: plan.skipped,
        };
    }

    let mut successes = Vec::new();
    for (index, item) in staged.iter().enumerate() {
        // Phase 1 vacated every source in the batch, so anything occupying a destination
        // now arrived from outside it. std::fs::rename replaces the destination silently,
        // which would destroy a file nobody asked this batch to touch.
        let moved = if cancel.load(Ordering::Relaxed) {
            Err(CANCELLED.to_string())
        } else if path_is_occupied(&item.new_path) {
            Err(format!("'{}' already exists", item.new_path.display()))
        } else {
            std::fs::rename(&item.temp_path, &item.new_path).map_err(|err| err.to_string())
        };

        if let Err(reason) = moved {
            failures.push(RenameFailure {
                file_id: item.file_id,
                original_path: Some(item.original_path.clone()),
                target_path: item.new_path.clone(),
                error: reason,
            });
            // One member's destination is routinely another member's source, which is
            // what the two phases exist for. A file that stops here therefore has
            // nowhere safe to go back to while the finished moves stand, so phase 2 is
            // all-or-nothing the same way phase 1 is.
            failures.extend(unwind_phase_two(&staged, index));

            return RenameBatchResult {
                batch: None,
                successes: Vec::new(),
                failures,
                skipped: plan.skipped,
            };
        }

        done_steps += 1;
        progress(done_steps, total_steps);
        successes.push(RenameRecord {
            id: Uuid::new_v4(),
            timestamp: Local::now(),
            original_path: item.original_path.clone(),
            new_path: item.new_path.clone(),
            was_directory: item.was_directory,
        });
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

/// Whether anything at all sits at `path`. `Path::exists` follows symlinks and reports a
/// dangling one as free, but renaming onto it still destroys the link.
fn path_is_occupied(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Human ("natural") ordering: digit runs compare as numbers, letters
/// case-insensitively, so file2 sorts before file10.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();

    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let mut num_a = 0u128;
                    let mut len_a = 0u32;
                    while let Some(c) = a_chars.peek().copied().filter(char::is_ascii_digit) {
                        num_a = num_a.saturating_mul(10).saturating_add(c as u128 - '0' as u128);
                        len_a += 1;
                        a_chars.next();
                    }
                    let mut num_b = 0u128;
                    let mut len_b = 0u32;
                    while let Some(c) = b_chars.peek().copied().filter(char::is_ascii_digit) {
                        num_b = num_b.saturating_mul(10).saturating_add(c as u128 - '0' as u128);
                        len_b += 1;
                        b_chars.next();
                    }
                    match num_a.cmp(&num_b) {
                        std::cmp::Ordering::Equal => match len_a.cmp(&len_b) {
                            // "01" vs "1": shorter run of the same value first.
                            std::cmp::Ordering::Equal => continue,
                            other => return other,
                        },
                        other => return other,
                    }
                }
                let fa = ca.to_lowercase().next().unwrap_or(ca);
                let fb = cb.to_lowercase().next().unwrap_or(cb);
                match fa.cmp(&fb) {
                    std::cmp::Ordering::Equal => {
                        a_chars.next();
                        b_chars.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

/// How to resolve preview conflicts automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Append " (n)" before the extension until the name is free.
    AppendCounter,
    /// Leave the conflicting files out of the batch.
    Skip,
}

/// Post-preview pass turning conflicts into renameable or skipped entries.
///
/// `AppendCounter` picks the first " (n)" suffix that is free both on disk and
/// among the batch's own targets, which also untangles internal collisions
/// (several files mapping to one name). A name that stays taken through
/// `(999)` is left as a conflict.
pub fn resolve_preview_conflicts(previews: &mut [RenamePreview], resolution: ConflictResolution) {
    let mut taken: HashSet<PathBuf> = previews
        .iter()
        .filter(|p| matches!(p.status, RenameStatus::WillRename))
        .map(|p| p.new_path.clone())
        .collect();

    for preview in previews.iter_mut() {
        if !matches!(
            preview.status,
            RenameStatus::Conflict | RenameStatus::InternalConflict
        ) {
            continue;
        }

        match resolution {
            ConflictResolution::Skip => {
                preview.status = RenameStatus::Skipped;
                preview.message = Some("Skipped: the name is already taken".to_string());
            }
            ConflictResolution::AppendCounter => {
                let parent = preview.new_path.parent().map(Path::to_path_buf);
                let (stem, ext) = match preview.new_name.rsplit_once('.') {
                    // A leading dot is a hidden file, not an extension.
                    Some((stem, ext)) if !stem.is_empty() => {
                        (stem.to_string(), format!(".{}", ext))
                    }
                    _ => (preview.new_name.clone(), String::new()),
                };

                let mut resolved = None;
                for n in 1..=999u32 {
                    let candidate_name = format!("{} ({}){}", stem, n, ext);
                    let candidate_path = parent
                        .as_ref()
                        .map(|p| p.join(&candidate_name))
                        .unwrap_or_else(|| PathBuf::from(&candidate_name));
                    if !path_is_occupied(&candidate_path) && !taken.contains(&candidate_path) {
                        resolved = Some((candidate_name, candidate_path));
                        break;
                    }
                }

                if let Some((name, path)) = resolved {
                    taken.insert(path.clone());
                    preview.new_name = name;
                    preview.new_path = path;
                    preview.status = RenameStatus::WillRename;
                    preview.message = Some("Numbered to avoid a collision".to_string());
                }
            }
        }
    }
}

/// Whether two paths name the same filesystem object right now.
///
/// On a case-insensitive mount (vfat/exFAT USB sticks, NTFS, CIFS, ext4
/// casefold directories) the target of a case-only rename "exists" because it
/// is the source itself under different casing; comparing device and inode
/// numbers detects that without guessing filesystem semantics. Hardlinked
/// siblings also compare equal, so a rename onto a hardlink of the source is
/// previewed as fine and only refused at execution, where phase 2's occupancy
/// check unwinds the batch — a late failure, never an overwrite.
pub(crate) fn paths_are_same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
            (Ok(meta_a), Ok(meta_b)) => {
                meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino()
            }
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        // Windows filesystems are case-insensitive by default; case-folded
        // path equality is the practical equivalent.
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}

/// Take back a phase 2 that could not be finished, and report what would not move.
///
/// Everything already moved goes back to its staging path before anything goes home:
/// staging paths are unique and unused, so this cannot land one member of the batch on
/// another, whereas moving a finished rename straight back to its original path would
/// overwrite whichever member had been renamed into it.
fn unwind_phase_two(staged: &[RenamePlanItem], failed_at: usize) -> Vec<RenameFailure> {
    let mut failures = Vec::new();
    let mut restorable = Vec::new();

    for (index, item) in staged.iter().enumerate() {
        if index >= failed_at {
            // Never left staging: this item and everything queued behind it.
            restorable.push(item);
            continue;
        }

        match std::fs::rename(&item.new_path, &item.temp_path) {
            Ok(()) => restorable.push(item),
            Err(err) => failures.push(RenameFailure {
                file_id: item.file_id,
                original_path: Some(item.original_path.clone()),
                target_path: item.new_path.clone(),
                error: format!(
                    "batch aborted and a finished rename could not be taken back, \
                     file left at '{}': {}",
                    item.new_path.display(),
                    err
                ),
            }),
        }
    }

    for item in restorable {
        let rollback_error = restore_staged(item);
        if !rollback_error.is_empty() {
            failures.push(RenameFailure {
                file_id: item.file_id,
                original_path: Some(item.original_path.clone()),
                target_path: item.new_path.clone(),
                error: format!("batch aborted{}", rollback_error),
            });
        }
    }

    failures
}

/// Move a staged item back to the path it came from, and describe what went wrong when it
/// could not be. Returns an empty string once the file is home again.
fn restore_staged(item: &RenamePlanItem) -> String {
    // A swap or rotation makes one item's original path another item's destination, so
    // this path can still hold a finished rename that would not come back off it, or an
    // entry that arrived from outside the batch. Putting the file back would overwrite
    // it, which is the loss the rollback exists to prevent; leaving it staged keeps both
    // files and says where this one is.
    if path_is_occupied(&item.original_path) {
        return format!(
            "; rollback skipped because '{}' is occupied, file left at '{}'",
            item.original_path.display(),
            item.temp_path.display()
        );
    }

    std::fs::rename(&item.temp_path, &item.original_path)
        .err()
        .map(|err| {
            format!(
                "; rollback failed, file left at '{}': {}",
                item.temp_path.display(),
                err
            )
        })
        .unwrap_or_default()
}

/// Execute the actual rename operations.
pub fn execute_renames(
    previews: &[RenamePreview],
    files: &HashMap<Uuid, FileEntry>,
) -> RenamerResult<RenameBatchResult> {
    let plan = plan_renames(previews, files)?;
    Ok(execute_rename_plan(plan))
}

/// Plan and execute with progress reporting, cooperative cancellation, and an
/// optional crash-recovery journal.
///
/// Between the two phases every file in the batch sits under a hidden staging
/// name; a crash or power loss there would strand the whole batch with no
/// record of what maps where. Writing the plan to `journal_dir` before phase 1
/// and deleting it after a clean finish lets `recover_interrupted` put
/// stranded files back on the next start.
pub fn execute_renames_with(
    previews: &[RenamePreview],
    files: &HashMap<Uuid, FileEntry>,
    journal_dir: Option<&Path>,
    progress: impl Fn(usize, usize),
    cancel: &std::sync::atomic::AtomicBool,
) -> RenamerResult<RenameBatchResult> {
    let plan = plan_renames(previews, files)?;
    let journal = journal_dir.and_then(|dir| match RecoveryJournal::write(dir, &plan) {
        Ok(journal) => Some(journal),
        Err(err) => {
            tracing::warn!("Could not write the crash-recovery journal: {}", err);
            None
        }
    });
    let result = execute_rename_plan_with(plan, progress, cancel);
    if let Some(journal) = journal {
        journal.finish(&result);
    }
    Ok(result)
}

/// Marker used in failure messages for files left under a staging name; a
/// journal whose batch reports one is kept for later recovery.
const STRANDED_MARKER: &str = "file left at";

/// On-disk record of a batch's rename plan, written before phase 1 begins and
/// removed once every file is either renamed or restored.
pub struct RecoveryJournal {
    path: PathBuf,
}

impl RecoveryJournal {
    /// Journal file prefix inside the data directory.
    const PREFIX: &'static str = "recovery-";

    pub fn write(dir: &Path, plan: &RenamePlan) -> RenamerResult<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}{}.json", Self::PREFIX, Uuid::new_v4()));
        let tmp = dir.join(format!("{}{}.json.tmp", Self::PREFIX, Uuid::new_v4()));
        std::fs::write(&tmp, serde_json::to_vec(plan)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(Self { path })
    }

    /// Remove the journal unless the batch left files under staging names.
    pub fn finish(self, result: &RenameBatchResult) {
        let stranded = result
            .failures
            .iter()
            .any(|failure| failure.error.contains(STRANDED_MARKER));
        if !stranded {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// How many files interrupted runs have left under staging names.
pub fn pending_recovery_count(dir: &Path) -> usize {
    read_journals(dir)
        .iter()
        .flat_map(|(_, plan)| plan.items.iter())
        .filter(|item| path_is_occupied(&item.temp_path))
        .count()
}

/// Result of restoring stranded staging files.
#[derive(Debug, Default)]
pub struct RecoveryOutcome {
    /// Files moved back to their original paths.
    pub restored: Vec<PathBuf>,
    /// Files that could not be restored, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

/// Put files stranded by an interrupted batch back on their original paths.
///
/// An item whose staging path no longer exists was either finished (phase 2
/// moved it to its new name) or already restored; both are left alone. A
/// journal is removed once none of its staging paths remain.
pub fn recover_interrupted(dir: &Path) -> RecoveryOutcome {
    let mut outcome = RecoveryOutcome::default();

    for (journal_path, plan) in read_journals(dir) {
        let mut remaining = false;
        for item in &plan.items {
            if !path_is_occupied(&item.temp_path) {
                continue;
            }
            if path_is_occupied(&item.original_path) {
                remaining = true;
                outcome.failed.push((
                    item.original_path.clone(),
                    format!(
                        "'{}' is occupied; the file is still at '{}'",
                        item.original_path.display(),
                        item.temp_path.display()
                    ),
                ));
                continue;
            }
            match std::fs::rename(&item.temp_path, &item.original_path) {
                Ok(()) => outcome.restored.push(item.original_path.clone()),
                Err(err) => {
                    remaining = true;
                    outcome.failed.push((
                        item.original_path.clone(),
                        format!(
                            "could not restore from '{}': {}",
                            item.temp_path.display(),
                            err
                        ),
                    ));
                }
            }
        }
        if !remaining {
            let _ = std::fs::remove_file(&journal_path);
        }
    }

    outcome
}

fn read_journals(dir: &Path) -> Vec<(PathBuf, RenamePlan)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(RecoveryJournal::PREFIX) && name.ends_with(".json")
                })
        })
        .filter_map(|path| {
            let bytes = std::fs::read(&path).ok()?;
            let plan = serde_json::from_slice::<RenamePlan>(&bytes).ok()?;
            Some((path, plan))
        })
        .collect()
}

/// Filename limit (in bytes) of the common Linux filesystems.
const MAX_TEMP_NAME_BYTES: usize = 255;

/// Staging path for a two-phase move, next to the file it stands in for.
///
/// Also used by undo, which has to stage for the same reason the engine does.
pub(crate) fn unique_temp_path(source: &Path) -> PathBuf {
    let parent = source.parent().map(PathBuf::from).unwrap_or_default();
    let stem = source
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();

    loop {
        let prefix = format!(".bulk-renamer-{}-", Uuid::new_v4());
        // The uuid prefix can push an otherwise legal name past the filesystem's
        // NAME_MAX, and a staging failure aborts the whole batch.
        let budget = MAX_TEMP_NAME_BYTES.saturating_sub(prefix.len());
        let candidate = parent.join(format!("{}{}", prefix, truncate_bytes(&stem, budget)));
        if !candidate.exists() {
            return candidate;
        }
    }
}

/// Truncate to at most `max_bytes`, never splitting a character.
fn truncate_bytes(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod rename_safety_tests {
    use super::*;
    use crate::core::{RenameRule, ValidationErrorType};
    use std::fs;
    use std::path::Path;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bulk-renamer-{}-{}", name, Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).expect("write file");
        path
    }

    fn entries(dir: &Path, names: &[&str]) -> Vec<FileEntry> {
        names
            .iter()
            .map(|name| FileEntry::from_path(dir.join(name), 0).expect("file entry"))
            .collect()
    }

    fn by_id(files: &[FileEntry]) -> HashMap<Uuid, FileEntry> {
        files.iter().map(|entry| (entry.id, entry.clone())).collect()
    }

    fn config_with(rules: Vec<RuleType>) -> RenameConfig {
        RenameConfig {
            rules: rules.into_iter().map(RenameRule::new).collect(),
            ..RenameConfig::default()
        }
    }

    fn replace_rule(find: &str, replace: &str) -> RuleType {
        RuleType::Replace(ReplaceRule {
            find: find.to_string(),
            replace: replace.to_string(),
            ..ReplaceRule::default()
        })
    }

    /// Run the real preview pass over `names` in `dir`.
    fn preview(dir: &Path, names: &[&str], rules: Vec<RuleType>) -> (Vec<RenamePreview>, Vec<FileEntry>) {
        let files = entries(dir, names);
        let mut engine = RenameEngine::new(config_with(rules));
        let previews = engine.generate_previews(&files);
        (previews, files)
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
    fn two_name_cycle_survives_the_whole_pipeline() {
        let dir = temp_dir("two-cycle");
        write_file(&dir, "a.txt", "a");
        write_file(&dir, "b.txt", "b");

        // a -> b and b -> a, routed through a sentinel neither name contains.
        let (previews, files) = preview(
            &dir,
            &["a.txt", "b.txt"],
            vec![
                replace_rule("a", "X"),
                replace_rule("b", "a"),
                replace_rule("X", "b"),
            ],
        );

        assert!(
            previews
                .iter()
                .all(|preview| preview.status == RenameStatus::WillRename),
            "a swap is not a conflict: {:?}",
            previews
        );

        let result = execute_renames(&previews, &by_id(&files)).expect("execute cycle");
        assert_eq!(result.success_count(), 2);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "b");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "a");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn three_name_cycle_survives_the_whole_pipeline() {
        let dir = temp_dir("three-cycle");
        write_file(&dir, "a.txt", "a");
        write_file(&dir, "b.txt", "b");
        write_file(&dir, "c.txt", "c");

        // a -> b -> c -> a.
        let (previews, files) = preview(
            &dir,
            &["a.txt", "b.txt", "c.txt"],
            vec![
                replace_rule("c", "X"),
                replace_rule("b", "c"),
                replace_rule("a", "b"),
                replace_rule("X", "a"),
            ],
        );

        assert!(
            previews
                .iter()
                .all(|preview| preview.status == RenameStatus::WillRename),
            "a rotation is not a conflict: {:?}",
            previews
        );

        let result = execute_renames(&previews, &by_id(&files)).expect("execute cycle");
        assert_eq!(result.success_count(), 3);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "c");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "a");
        assert_eq!(fs::read_to_string(dir.join("c.txt")).expect("read c"), "b");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_stage_aborts_the_batch_instead_of_overwriting() {
        let dir = temp_dir("stage-failure");
        write_file(&dir, "a.txt", "a");
        write_file(&dir, "b.txt", "b");

        // a -> c and b -> a, so b's destination is a's original path.
        let (previews, files) = preview(
            &dir,
            &["a.txt", "b.txt"],
            vec![replace_rule("a", "c"), replace_rule("b", "a")],
        );

        let mut plan = plan_renames(&previews, &by_id(&files)).expect("plan");
        // Stage a.txt into a directory that does not exist, the way a name over
        // NAME_MAX or a file that vanished would fail.
        for item in &mut plan.items {
            if item.original_path == dir.join("a.txt") {
                item.temp_path = dir.join("no-such-dir").join("staged");
            }
        }

        let result = execute_rename_plan(plan);

        assert_eq!(result.success_count(), 0);
        assert!(!result.all_successful());
        // a.txt kept its own contents rather than being overwritten by b.txt.
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "a");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "b");
        assert!(!dir.join("c.txt").exists());

        fs::remove_dir_all(dir).ok();
    }

    /// Point one planned item at a destination that cannot be created, so its final
    /// move fails the way a target directory that vanished mid-batch would.
    fn break_final_move(plan: &mut RenamePlan, dir: &Path, original: &str) {
        for item in &mut plan.items {
            if item.original_path == dir.join(original) {
                item.new_path = dir.join("no-such-dir").join("x");
            }
        }
    }

    #[test]
    fn a_failed_final_move_does_not_overwrite_a_queued_source() {
        let dir = temp_dir("final-move-before");
        write_file(&dir, "keep", "keep");
        write_file(&dir, "mover", "mover");

        // mover -> keep is only allowed because the batch promised to vacate keep
        // first, and keep is listed first so its own move is attempted first.
        let (previews, files) = preview(
            &dir,
            &["keep", "mover"],
            vec![replace_rule("keep", "renamed"), replace_rule("mover", "keep")],
        );
        assert!(
            previews
                .iter()
                .all(|preview| preview.status == RenameStatus::WillRename),
            "both moves were planned: {:?}",
            previews
        );

        let mut plan = plan_renames(&previews, &by_id(&files)).expect("plan");
        break_final_move(&mut plan, &dir, "keep");
        let result = execute_rename_plan(plan);

        // keep never reached a destination, so its contents must still be its own.
        assert_eq!(
            fs::read_to_string(dir.join("keep")).expect("read keep"),
            "keep",
            "a rolled back file was overwritten by another member of the batch: {:?}",
            result.failures
        );
        assert_eq!(
            fs::read_to_string(dir.join("mover")).expect("read mover"),
            "mover"
        );
        assert_eq!(result.success_count(), 0);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_final_move_does_not_overwrite_a_completed_rename() {
        let dir = temp_dir("final-move-after");
        write_file(&dir, "keep", "keep");
        write_file(&dir, "mover", "mover");

        // Same pair, opposite order: mover -> keep lands first, and keep's own move
        // then fails and is rolled back onto the file mover just became. Routed through
        // a sentinel so the second rule does not rewrite mover's result.
        let (previews, files) = preview(
            &dir,
            &["mover", "keep"],
            vec![
                replace_rule("keep", "SENTINEL"),
                replace_rule("mover", "keep"),
                replace_rule("SENTINEL", "renamed"),
            ],
        );

        let mut plan = plan_renames(&previews, &by_id(&files)).expect("plan");
        break_final_move(&mut plan, &dir, "keep");
        let result = execute_rename_plan(plan);

        // Both files are back under the names the user knows them by: neither destroyed
        // by the rollback, nor stranded under a staging name it can never be found by.
        assert_eq!(
            fs::read_to_string(dir.join("keep")).expect("read keep"),
            "keep",
            "keep is not where the user left it, batch reported {} success(es): {:?}",
            result.success_count(),
            result.failures
        );
        assert_eq!(
            fs::read_to_string(dir.join("mover")).expect("read mover"),
            "mover",
            "mover is not where the user left it, batch reported {} success(es): {:?}",
            result.success_count(),
            result.failures
        );
        // A batch that could not finish reports nothing to undo.
        assert_eq!(result.success_count(), 0);
        assert!(result.batch.is_none());
        assert!(leftover_staging_files(&dir).is_empty());

        fs::remove_dir_all(dir).ok();
    }

    /// Files abandoned under the hidden staging prefix. Their contents survive, but the
    /// user cannot find them by any name they ever chose.
    fn leftover_staging_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".bulk-renamer-"))
            .collect()
    }

    #[test]
    fn a_target_on_a_filtered_out_file_is_refused() {
        let dir = temp_dir("filtered-out");
        write_file(&dir, "keep.log", "keep");
        write_file(&dir, "mover.txt", "mover");

        // Only .txt files are in the batch, so keep.log is never vacated even though it
        // sits in the queue the user handed over.
        let files = entries(&dir, &["keep.log", "mover.txt"]);
        let mut config = config_with(vec![replace_rule("mover.txt", "keep.log")]);
        config.separate_extension = false;
        config.filter = Some(FilterConfig {
            mode: FilterMode::Include,
            rules: vec![FilterRule {
                field: FilterField::Extension,
                operator: FilterOperator::Equals,
                value: "txt".to_string(),
            }],
        });
        let mut engine = RenameEngine::new(config);
        let previews = engine.generate_previews(&files);

        assert_eq!(previews.len(), 1, "only the .txt file is in the batch");
        assert_eq!(previews[0].new_name, "keep.log");
        assert_eq!(
            previews[0].status,
            RenameStatus::Conflict,
            "a file outside the batch is not vacated by it"
        );
        assert!(execute_renames(&previews, &by_id(&files)).is_err());
        assert_eq!(
            fs::read_to_string(dir.join("keep.log")).expect("read keep"),
            "keep"
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_target_on_an_unchanged_file_is_refused() {
        let dir = temp_dir("unchanged-target");
        write_file(&dir, "keep.txt", "keep");
        write_file(&dir, "mover.txt", "mover");

        // keep.txt is in the batch but its rules leave it alone, so nothing moves it out
        // of the way of mover.txt.
        let (previews, files) = preview(
            &dir,
            &["keep.txt", "mover.txt"],
            vec![replace_rule("mover", "keep")],
        );

        let unchanged = previews
            .iter()
            .find(|preview| preview.original_name == "keep.txt")
            .expect("keep preview");
        assert_eq!(unchanged.status, RenameStatus::Unchanged);

        assert!(
            execute_renames(&previews, &by_id(&files)).is_err(),
            "an unchanged file is not vacated and must not be renamed over"
        );
        assert_eq!(
            fs::read_to_string(dir.join("keep.txt")).expect("read keep"),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(dir.join("mover.txt")).expect("read mover"),
            "mover"
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn four_name_cycle_survives_the_whole_pipeline() {
        let dir = temp_dir("four-cycle");
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            write_file(&dir, name, &name[..1]);
        }

        // a -> b -> c -> d -> a.
        let (previews, files) = preview(
            &dir,
            &["a.txt", "b.txt", "c.txt", "d.txt"],
            vec![
                replace_rule("d", "X"),
                replace_rule("c", "d"),
                replace_rule("b", "c"),
                replace_rule("a", "b"),
                replace_rule("X", "a"),
            ],
        );

        assert!(
            previews
                .iter()
                .all(|preview| preview.status == RenameStatus::WillRename),
            "a four way rotation is not a conflict: {:?}",
            previews
        );

        let result = execute_renames(&previews, &by_id(&files)).expect("execute cycle");
        assert_eq!(result.success_count(), 4);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "d");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "a");
        assert_eq!(fs::read_to_string(dir.join("c.txt")).expect("read c"), "b");
        assert_eq!(fs::read_to_string(dir.join("d.txt")).expect("read d"), "c");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_cycle_with_an_invalid_member_moves_nothing() {
        let dir = temp_dir("invalid-cycle");
        write_file(&dir, "a.txt", "a");
        write_file(&dir, "b.txt", "b");
        write_file(&dir, "c.txt", "c");

        // a -> b -> c -> a, except c's new name is empty and cannot be used. The other
        // two are only safe because the whole rotation happens.
        let files = entries(&dir, &["a.txt", "b.txt", "c.txt"]);
        let mut config = config_with(vec![
            replace_rule("c.txt", ""),
            replace_rule("b.txt", "c.txt"),
            replace_rule("a.txt", "b.txt"),
        ]);
        // An emptied stem would otherwise be rescued by the re-added extension.
        config.separate_extension = false;
        let mut engine = RenameEngine::new(config);
        let previews = engine.generate_previews(&files);

        assert!(
            execute_renames(&previews, &by_id(&files)).is_err(),
            "one unusable member must fail the whole rotation: {:?}",
            previews
        );
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "a");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "b");
        assert_eq!(fs::read_to_string(dir.join("c.txt")).expect("read c"), "c");
        assert!(leftover_staging_files(&dir).is_empty());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_same_source_queued_twice_keeps_the_file() {
        let dir = temp_dir("duplicate-source");
        write_file(&dir, "a.txt", "a");

        // The same path added to the queue twice, each copy given its own number, so the
        // two entries claim different destinations and neither is an internal conflict.
        let entry = FileEntry::from_path(dir.join("a.txt"), 0).expect("entry");
        let mut twin = FileEntry::from_path(dir.join("a.txt"), 0).expect("twin");
        twin.id = Uuid::new_v4();
        let files = vec![entry, twin];

        let mut engine = RenameEngine::new(config_with(vec![RuleType::Numbering(
            NumberingRule {
                start: 1,
                padding: 0,
                prefix: "-".to_string(),
                ..NumberingRule::default()
            },
        )]));
        let previews = engine.generate_previews(&files);
        assert_eq!(previews.len(), 2);
        assert_ne!(previews[0].new_path, previews[1].new_path);

        let result = execute_renames(&previews, &by_id(&files)).expect("execute");

        // Whatever the batch decides, the one real file on disk still exists and still
        // holds its contents under exactly one name.
        let surviving: Vec<PathBuf> = ["a.txt", "a-1.txt", "a-2.txt"]
            .iter()
            .map(|name| dir.join(name))
            .filter(|path| path.exists())
            .collect();
        assert_eq!(
            surviving.len(),
            1,
            "one file went in, so one file comes out: {:?}, failures {:?}",
            surviving,
            result.failures
        );
        assert_eq!(
            fs::read_to_string(&surviving[0]).expect("read survivor"),
            "a"
        );
        assert!(leftover_staging_files(&dir).is_empty());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn staging_keeps_a_long_name_within_the_filename_limit() {
        let dir = temp_dir("long-name");
        // Legal on ext4, but no room is left for the staging prefix.
        let original = format!("{}.txt", "n".repeat(250));
        write_file(&dir, &original, "long");

        let (previews, files) = preview(
            &dir,
            &[&original],
            vec![RuleType::Remove(RemoveRule {
                target: RemoveTarget::FirstN(1),
            })],
        );

        let result = execute_renames(&previews, &by_id(&files)).expect("execute");
        assert_eq!(
            result.success_count(),
            1,
            "staging failed: {:?}",
            result.failures
        );
        assert!(dir.join(format!("{}.txt", "n".repeat(249))).exists());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    // Needs a case-sensitive filesystem to mean anything.
    #[cfg(target_os = "linux")]
    fn targets_differing_only_in_case_are_not_a_conflict() {
        let dir = temp_dir("case-distinct");
        write_file(&dir, "one.txt", "one");
        write_file(&dir, "two.txt", "two");

        let (previews, files) = preview(
            &dir,
            &["one.txt", "two.txt"],
            vec![replace_rule("one", "A"), replace_rule("two", "a")],
        );

        let result = execute_renames(&previews, &by_id(&files)).expect("execute");
        assert_eq!(result.success_count(), 2);
        assert_eq!(fs::read_to_string(dir.join("A.txt")).expect("read A"), "one");
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "two");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    // Needs a case-sensitive filesystem to mean anything.
    #[cfg(target_os = "linux")]
    fn a_file_that_only_folds_onto_a_queued_source_is_not_overwritten() {
        let dir = temp_dir("case-collision");
        write_file(&dir, "A.txt", "A");
        write_file(&dir, "keep.txt", "keep");

        // A.txt -> B.txt and keep.txt -> a.txt.
        let (previews, files) = preview(
            &dir,
            &["A.txt", "keep.txt"],
            vec![replace_rule("A", "B"), replace_rule("keep", "a")],
        );

        // An unrelated a.txt appears between preview and execution, which is what the
        // pre-execution exists check is there to catch. Only a case-folded comparison
        // mistakes it for the queued A.txt.
        write_file(&dir, "a.txt", "unrelated");

        assert!(execute_renames(&previews, &by_id(&files)).is_err());
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).expect("read a"),
            "unrelated"
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[cfg(unix)]
    fn a_symlinked_parent_does_not_hide_a_collision() {
        let root = temp_dir("symlinked-parent");
        let real = root.join("real");
        fs::create_dir_all(&real).expect("create real dir");
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        write_file(&real, "keep.txt", "keep");
        write_file(&real, "other.txt", "other");

        // One directory, queued under both of its names, the way adding a folder and a
        // symlink to it does. Both files are real/keep.txt and real/other.txt.
        let files = vec![
            FileEntry::from_path(real.join("keep.txt"), 0).expect("entry keep"),
            FileEntry::from_path(link.join("other.txt"), 0).expect("entry other"),
        ];
        let mut engine = RenameEngine::new(config_with(vec![
            replace_rule("keep", "merged"),
            replace_rule("other", "merged"),
        ]));
        let previews = engine.generate_previews(&files);

        // Both land on real/merged.txt, so this must never execute.
        let result = execute_renames(&previews, &by_id(&files));
        assert!(
            result.is_err(),
            "two files renamed onto one path were allowed to run: {:?}",
            result.map(|batch| batch.success_count())
        );
        assert_eq!(
            fs::read_to_string(real.join("keep.txt")).expect("read keep"),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(real.join("other.txt")).expect("read other"),
            "other"
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_parent_component_in_a_name_does_not_hide_a_collision() {
        let root = temp_dir("parent-component");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).expect("create sub dir");
        write_file(&sub, "a.txt", "a");
        write_file(&root, "b.txt", "b");

        // sub/a.txt -> sub/../out.txt and b.txt -> out.txt are the same destination
        // spelled two ways.
        let files = vec![
            FileEntry::from_path(sub.join("a.txt"), 0).expect("entry a"),
            FileEntry::from_path(root.join("b.txt"), 0).expect("entry b"),
        ];
        let mut engine = RenameEngine::new(config_with(vec![
            replace_rule("a", "../out"),
            replace_rule("b", "out"),
        ]));
        let previews = engine.generate_previews(&files);

        let result = execute_renames(&previews, &by_id(&files));
        assert!(
            result.is_err(),
            "two files renamed onto one path were allowed to run: {:?}",
            result.map(|batch| batch.success_count())
        );
        assert_eq!(fs::read_to_string(sub.join("a.txt")).expect("read a"), "a");
        assert_eq!(fs::read_to_string(root.join("b.txt")).expect("read b"), "b");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn two_numbering_rules_keep_separate_counters() {
        let dir = temp_dir("numbering");
        write_file(&dir, "f1.txt", "");
        write_file(&dir, "f2.txt", "");
        write_file(&dir, "f3.txt", "");

        let numbering = |start: i64, prefix: &str| {
            RuleType::Numbering(NumberingRule {
                start,
                padding: 0,
                prefix: prefix.to_string(),
                ..NumberingRule::default()
            })
        };

        let (previews, files) = preview(
            &dir,
            &["f1.txt", "f2.txt", "f3.txt"],
            vec![numbering(1, "-a"), numbering(100, "-b")],
        );

        let names: Vec<&str> = previews
            .iter()
            .map(|preview| preview.new_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["f1-a1-b100.txt", "f2-a2-b101.txt", "f3-a3-b102.txt"]
        );

        let result = execute_renames(&previews, &by_id(&files)).expect("execute");
        assert_eq!(result.success_count(), 3);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn expression_counters_advance_once_per_file() {
        let dir = temp_dir("expression-counter");
        write_file(&dir, "x.txt", "");
        write_file(&dir, "y.txt", "");
        write_file(&dir, "z.txt", "");

        let names = ["x.txt", "y.txt", "z.txt"];
        let files = entries(&dir, &names);
        let mut engine = RenameEngine::new(config_with(vec![RuleType::Expression(
            ExpressionRule {
                expression: "${name}-${index}-of-${total}".to_string(),
            },
        )]));

        let previews = engine.generate_previews(&files);
        let new_names: Vec<&str> = previews
            .iter()
            .map(|preview| preview.new_name.as_str())
            .collect();
        assert_eq!(new_names, ["x-1-of-3.txt", "y-2-of-3.txt", "z-3-of-3.txt"]);

        // A second pass restarts rather than continuing from the first.
        let previews = engine.generate_previews(&files);
        assert_eq!(previews[0].new_name, "x-1-of-3.txt");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[cfg(unix)]
    fn one_name_windows_would_reject_does_not_block_the_batch() {
        let dir = temp_dir("colon");
        write_file(&dir, "meeting.txt", "meeting");
        write_file(&dir, "notes.txt", "notes");

        let (previews, files) = preview(
            &dir,
            &["meeting.txt", "notes.txt"],
            vec![
                replace_rule("meeting", "12:30 meeting"),
                replace_rule("notes", "notes-ok"),
            ],
        );

        let result = execute_renames(&previews, &by_id(&files)).expect("execute");
        assert_eq!(result.success_count(), 2);
        assert!(dir.join("12:30 meeting.txt").exists());
        assert!(dir.join("notes-ok.txt").exists());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_marked_internal_conflict_does_not_block_the_rest_of_the_batch() {
        let dir = temp_dir("internal-conflict");
        write_file(&dir, "a1.txt", "a1");
        write_file(&dir, "a2.txt", "a2");
        write_file(&dir, "solo.txt", "solo");

        // a1 and a2 both collapse onto "same"; solo is independent.
        let (mut previews, files) = preview(
            &dir,
            &["a1.txt", "a2.txt", "solo.txt"],
            vec![
                replace_rule("a1", "same"),
                replace_rule("a2", "same"),
                replace_rule("solo", "alone"),
            ],
        );
        let files_by_id = by_id(&files);

        // The window marks conflicts on the previews it validated, and the executor
        // then validates those same previews a second time.
        let validator = RenameValidator::new();
        for error in validator.validate_batch_with_files(&previews, &files_by_id) {
            if let Some(preview) = previews.get_mut(error.file_index) {
                preview.status = match error.error_type {
                    ValidationErrorType::Conflict => RenameStatus::InternalConflict,
                    _ => RenameStatus::Error,
                };
            }
        }

        let result = execute_renames(&previews, &files_by_id).expect("execute");
        assert_eq!(result.success_count(), 1);
        assert!(dir.join("alone.txt").exists());
        assert!(dir.join("a1.txt").exists());
        assert!(dir.join("a2.txt").exists());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn literal_case_insensitive_replace_matches_the_regex_behaviour() {
        let dir = temp_dir("literal-ci");
        write_file(&dir, "Ärger a.A.txt", "");

        let (previews, _) = preview(
            &dir,
            &["Ärger a.A.txt"],
            vec![RuleType::Replace(ReplaceRule {
                find: "a".to_string(),
                replace: "-".to_string(),
                case_sensitive: false,
                ..ReplaceRule::default()
            })],
        );

        // '.' and 'Ä' are literal, both cases of 'a' match, and the extension is
        // untouched because it is processed separately.
        assert_eq!(previews[0].new_name, "Ärger -.-.txt");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[ignore = "performance measurement; run with --ignored --nocapture"]
    fn bench_preview_over_10k_files() {
        let dir = temp_dir("bench");
        let files: Vec<FileEntry> = (0..10_000)
            .map(|i| {
                let name = format!("IMG_{:05}.jpg", i);
                FileEntry {
                    id: Uuid::new_v4(),
                    path: dir.join(&name),
                    original_name: name,
                    extension: Some("jpg".to_string()),
                    is_directory: false,
                    size: 0,
                    modified: None,
                    created: None,
                    accessed: None,
                    depth: 0,
                    parent_name: None,
                    metadata_cache: None,
                }
            })
            .collect();

        let mut regex_engine = RenameEngine::new(config_with(vec![RuleType::Replace(
            ReplaceRule {
                find: r"IMG_(\d+)".to_string(),
                replace: "Photo-$1".to_string(),
                use_regex: true,
                ..ReplaceRule::default()
            },
        )]));
        let start = std::time::Instant::now();
        let previews = regex_engine.generate_previews(&files);
        let regex_elapsed = start.elapsed();
        assert_eq!(previews[0].new_name, "Photo-00000.jpg");

        let mut literal_engine = RenameEngine::new(config_with(vec![RuleType::Replace(
            ReplaceRule {
                find: "img".to_string(),
                replace: "Photo".to_string(),
                case_sensitive: false,
                ..ReplaceRule::default()
            },
        )]));
        let start = std::time::Instant::now();
        let previews = literal_engine.generate_previews(&files);
        let literal_elapsed = start.elapsed();
        assert_eq!(previews[0].new_name, "Photo_00000.jpg");

        println!("10k files, regex replace:              {:?}", regex_elapsed);
        println!("10k files, case-insensitive literal:   {:?}", literal_elapsed);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_stage_failure_on_the_last_item_still_puts_the_first_one_back() {
        let dir = temp_dir("stage-failure-last");
        write_file(&dir, "a.txt", "AAA");
        write_file(&dir, "b.txt", "BBB");

        // a -> c and b -> a, so the item that fails to stage is the one whose
        // destination is the path the staged item has to be put back on.
        let (previews, files) = preview(
            &dir,
            &["a.txt", "b.txt"],
            vec![
                replace_rule("a", "X"),
                replace_rule("b", "a"),
                replace_rule("X", "c"),
            ],
        );

        let mut plan = plan_renames(&previews, &by_id(&files)).expect("plan");
        for item in &mut plan.items {
            if item.original_path == dir.join("b.txt") {
                item.temp_path = dir.join("no-such-dir").join("staged");
            }
        }

        let result = execute_rename_plan(plan);

        assert_eq!(result.success_count(), 0, "{:?}", result.successes);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "AAA");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "BBB");
        assert!(!dir.join("c.txt").exists());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    #[cfg(unix)]
    fn a_dangling_symlink_on_a_destination_is_not_written_over() {
        let dir = temp_dir("dangling-target");
        write_file(&dir, "a.txt", "AAA");
        // A link to something not mounted yet. `exists` follows it and calls the name
        // free, so nothing before the final move has any reason to stop.
        std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("c.txt")).expect("symlink");

        let (previews, files) = preview(&dir, &["a.txt"], vec![replace_rule("a", "c")]);
        let plan = plan_renames(&previews, &by_id(&files)).expect("plan");
        let result = execute_rename_plan(plan);

        assert_eq!(result.success_count(), 0, "{:?}", result.successes);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "AAA");
        assert!(
            fs::symlink_metadata(dir.join("c.txt"))
                .expect("stat c")
                .file_type()
                .is_symlink(),
            "the link was replaced by the renamed file"
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_rolled_back_file_is_not_eaten_by_the_next_rename() {
        let dir = temp_dir("rollback-into-pending");
        write_file(&dir, "a.txt", "AAA");
        write_file(&dir, "b.txt", "BBB");

        // a -> c and b -> a, so b's destination is the path a rolls back to.
        let (previews, files) = preview(
            &dir,
            &["a.txt", "b.txt"],
            vec![
                replace_rule("a", "X"),
                replace_rule("b", "a"),
                replace_rule("X", "c"),
            ],
        );

        let plan = plan_renames(&previews, &by_id(&files)).expect("plan");
        // Validation has passed and phase 2 is about to run. A directory arriving on a
        // destination is one of the ways a move fails there, and it is the failure that
        // sends a.txt back to its original path with b.txt still queued behind it.
        fs::create_dir(dir.join("c.txt")).expect("create obstacle");

        let result = execute_rename_plan(plan);

        assert_eq!(result.success_count(), 0, "{:?}", result.failures);
        // Both files still hold their own contents. Putting a.txt back must not turn it
        // into a target for b.txt, which is a silent overwrite of a file the batch has
        // just reported it could not rename.
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "AAA");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "BBB");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_rollback_does_not_overwrite_a_rename_the_batch_already_completed() {
        let dir = temp_dir("rollback-over-success");
        write_file(&dir, "a.txt", "AAA");
        write_file(&dir, "b.txt", "BBB");

        // a -> b and b -> c, so a lands on the path b would roll back to.
        let (previews, files) = preview(
            &dir,
            &["a.txt", "b.txt"],
            vec![
                replace_rule("b", "X"),
                replace_rule("a", "b"),
                replace_rule("X", "c"),
            ],
        );

        let plan = plan_renames(&previews, &by_id(&files)).expect("plan");
        fs::create_dir(dir.join("c.txt")).expect("create obstacle");

        let result = execute_rename_plan(plan);

        // a -> b had already landed when b -> c failed. Taking it back has to route
        // through staging, because moving b.txt straight back to a.txt would drop it on
        // top of the file that is still waiting there.
        assert_eq!(result.success_count(), 0, "{:?}", result.successes);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).expect("read a"), "AAA");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).expect("read b"), "BBB");

        // And no file is left behind under a staging name.
        let mut left: Vec<String> = fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["a.txt", "b.txt", "c.txt"]);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_directory_and_its_contents_are_not_renamed_in_one_batch() {
        let dir = temp_dir("nested");
        fs::create_dir(dir.join("d")).expect("create d");
        write_file(&dir.join("d"), "f.txt", "FFF");

        // "Add folder" walks into subdirectories, and sorting the list by path descending
        // is what puts the contents ahead of the directory that holds them.
        let files = entries(&dir, &["d/f.txt", "d"]);
        let mut engine = RenameEngine::new(config_with(vec![
            replace_rule("f", "g"),
            replace_rule("d", "dnew"),
        ]));
        engine.set_target(RenameTarget::Both);
        let previews = engine.generate_previews(&files);
        assert_eq!(previews.len(), 2);

        // Staging f.txt inside d and then renaming d moves the staged file out from under
        // its own temp path: neither its move nor its rollback can find it again, and it
        // is left inside the renamed directory under the staging name.
        let planned = plan_renames(&previews, &by_id(&files));
        assert!(planned.is_err(), "the batch cannot be executed as planned");

        // Nothing moved, and f.txt still has the name the user knows it by.
        assert_eq!(
            fs::read_to_string(dir.join("d").join("f.txt")).expect("read f"),
            "FFF"
        );
        assert!(!dir.join("dnew").exists());

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

    #[test]
    fn a_clean_batch_leaves_no_recovery_journal() {
        let dir = temp_dir("journal-clean");
        let journal_dir = dir.join("journal");
        let file = write_file(&dir, "a.txt", "data");

        let entry = FileEntry::from_path(file, 0).expect("entry");
        let preview = RenamePreview {
            file_id: entry.id,
            original_name: entry.original_name.clone(),
            new_name: "b.txt".to_string(),
            new_path: dir.join("b.txt"),
            status: RenameStatus::WillRename,
            message: None,
        };
        let files: HashMap<Uuid, FileEntry> = std::iter::once((entry.id, entry)).collect();

        let result = execute_renames_with(
            std::slice::from_ref(&preview),
            &files,
            Some(&journal_dir),
            |_, _| {},
            &std::sync::atomic::AtomicBool::new(false),
        )
        .expect("plan executes");

        assert_eq!(result.success_count(), 1);
        assert!(dir.join("b.txt").exists());
        assert_eq!(
            pending_recovery_count(&journal_dir),
            0,
            "a finished batch must not look like it needs recovery"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&journal_dir)
            .map(|entries| entries.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();
        assert!(leftovers.is_empty(), "journal must be deleted after a clean run");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn recovery_restores_files_stranded_under_staging_names() {
        let dir = temp_dir("journal-recover");
        let journal_dir = dir.join("journal");

        // Simulate a crash between the phases: the file sits at its staging
        // path, the original is vacated, and the journal records the mapping.
        let original = dir.join("photo.jpg");
        let temp = dir.join(".bulk-renamer-stage-test");
        fs::write(&temp, "pixels").expect("staged file");

        let plan = RenamePlan {
            items: vec![RenamePlanItem {
                file_id: Uuid::new_v4(),
                original_path: original.clone(),
                temp_path: temp.clone(),
                new_path: dir.join("renamed.jpg"),
                was_directory: false,
            }],
            skipped: Vec::new(),
        };
        RecoveryJournal::write(&journal_dir, &plan).expect("journal written");
        assert_eq!(pending_recovery_count(&journal_dir), 1);

        let outcome = recover_interrupted(&journal_dir);

        assert_eq!(outcome.restored, vec![original.clone()]);
        assert!(outcome.failed.is_empty());
        assert!(original.exists(), "the stranded file is back on its original path");
        assert!(!temp.exists());
        assert_eq!(pending_recovery_count(&journal_dir), 0, "journal removed once resolved");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn recovery_refuses_to_overwrite_an_occupied_original() {
        let dir = temp_dir("journal-occupied");
        let journal_dir = dir.join("journal");

        let original = write_file(&dir, "doc.txt", "someone else");
        let temp = dir.join(".bulk-renamer-stage-occupied");
        fs::write(&temp, "stranded").expect("staged file");

        let plan = RenamePlan {
            items: vec![RenamePlanItem {
                file_id: Uuid::new_v4(),
                original_path: original.clone(),
                temp_path: temp.clone(),
                new_path: dir.join("renamed.txt"),
                was_directory: false,
            }],
            skipped: Vec::new(),
        };
        RecoveryJournal::write(&journal_dir, &plan).expect("journal written");

        let outcome = recover_interrupted(&journal_dir);

        assert!(outcome.restored.is_empty());
        assert_eq!(outcome.failed.len(), 1);
        assert!(temp.exists(), "the stranded file must not be destroyed");
        assert_eq!(
            fs::read_to_string(&original).expect("original readable"),
            "someone else",
            "the occupying file must not be overwritten"
        );
        assert_eq!(
            pending_recovery_count(&journal_dir),
            1,
            "an unresolved journal is kept for a later attempt"
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_case_only_rename_of_the_same_file_is_not_a_conflict() {
        let dir = temp_dir("case-only");
        let file = write_file(&dir, "photo.jpg", "pixels");

        // Hardlink the source so the target path names the same inode, which is
        // exactly what a case-insensitive filesystem reports for Photo.jpg.
        let target = dir.join("PHOTO.jpg");
        fs::hard_link(&file, &target).expect("hard link");

        assert!(paths_are_same_file(&file, &target));
        assert!(!paths_are_same_file(&file, &dir.join("other.jpg")));

        let entry = FileEntry::from_path(file, 0).expect("entry");
        let config = RenameConfig {
            rules: vec![RenameRule::new(RuleType::Replace(ReplaceRule {
                find: "photo".to_string(),
                replace: "PHOTO".to_string(),
                ..Default::default()
            }))],
            ..Default::default()
        };
        let mut engine = RenameEngine::new(config);
        let previews = engine.generate_previews(std::slice::from_ref(&entry));

        assert_eq!(previews.len(), 1);
        assert!(
            matches!(previews[0].status, RenameStatus::WillRename),
            "a target that is the source itself under another name must not              block the rename, got {:?} ({:?})",
            previews[0].status,
            previews[0].message
        );

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conflict_resolution_appends_counters_until_free() {
        let dir = temp_dir("resolve-append");
        write_file(&dir, "taken.txt", "existing");
        write_file(&dir, "taken (1).txt", "also existing");
        let source = write_file(&dir, "source.txt", "content");

        let entry = FileEntry::from_path(source, 0).expect("entry");
        let mut previews = vec![RenamePreview {
            file_id: entry.id,
            original_name: entry.original_name.clone(),
            new_name: "taken.txt".to_string(),
            new_path: dir.join("taken.txt"),
            status: RenameStatus::Conflict,
            message: None,
        }];

        resolve_preview_conflicts(&mut previews, ConflictResolution::AppendCounter);

        assert_eq!(previews[0].new_name, "taken (2).txt");
        assert_eq!(previews[0].new_path, dir.join("taken (2).txt"));
        assert!(matches!(previews[0].status, RenameStatus::WillRename));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conflict_resolution_untangles_internal_collisions() {
        let dir = temp_dir("resolve-internal");
        let a = write_file(&dir, "a.txt", "1");
        let b = write_file(&dir, "b.txt", "2");

        let entry_a = FileEntry::from_path(a, 0).expect("entry a");
        let entry_b = FileEntry::from_path(b, 0).expect("entry b");
        let mut previews = vec![
            RenamePreview {
                file_id: entry_a.id,
                original_name: entry_a.original_name.clone(),
                new_name: "same.txt".to_string(),
                new_path: dir.join("same.txt"),
                status: RenameStatus::WillRename,
                message: None,
            },
            RenamePreview {
                file_id: entry_b.id,
                original_name: entry_b.original_name.clone(),
                new_name: "same.txt".to_string(),
                new_path: dir.join("same.txt"),
                status: RenameStatus::InternalConflict,
                message: None,
            },
        ];

        resolve_preview_conflicts(&mut previews, ConflictResolution::AppendCounter);

        assert_eq!(previews[1].new_name, "same (1).txt");
        assert!(matches!(previews[1].status, RenameStatus::WillRename));
        assert_ne!(previews[0].new_path, previews[1].new_path);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conflict_resolution_can_skip_instead() {
        let dir = temp_dir("resolve-skip");
        write_file(&dir, "taken.txt", "existing");
        let source = write_file(&dir, "source.txt", "content");

        let entry = FileEntry::from_path(source, 0).expect("entry");
        let mut previews = vec![RenamePreview {
            file_id: entry.id,
            original_name: entry.original_name.clone(),
            new_name: "taken.txt".to_string(),
            new_path: dir.join("taken.txt"),
            status: RenameStatus::Conflict,
            message: None,
        }];

        resolve_preview_conflicts(&mut previews, ConflictResolution::Skip);

        assert!(matches!(previews[0].status, RenameStatus::Skipped));
        assert_eq!(previews[0].new_name, "taken.txt", "a skipped name is untouched");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn natural_ordering_compares_digit_runs_as_numbers() {
        use std::cmp::Ordering;
        assert_eq!(natural_cmp("file2.txt", "file10.txt"), Ordering::Less);
        assert_eq!(natural_cmp("file10.txt", "file2.txt"), Ordering::Greater);
        assert_eq!(natural_cmp("File7", "file7"), Ordering::Equal.then(Ordering::Equal));
        assert_eq!(natural_cmp("IMG_9.jpg", "img_10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Greater);
        assert_eq!(natural_cmp("alpha", "beta"), Ordering::Less);
    }
}
