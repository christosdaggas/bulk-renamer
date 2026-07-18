//! Expression evaluator for the renamer DSL.

use crate::core::{FileEntry, RenamerError, RenamerResult};
use crate::engine::transformer::*;
use crate::core::NumberFormat;
use chrono::Local;
use regex::Regex;
use std::collections::HashMap;

/// The expression engine evaluates template expressions.
pub struct ExpressionEngine {
    /// Counter for sequential numbering.
    counter: i64,
    /// Total count of files (set externally).
    total: i64,
    /// Custom variables.
    variables: HashMap<String, String>,
}

impl ExpressionEngine {
    /// Create a new expression engine.
    pub fn new() -> Self {
        Self {
            counter: 1,
            total: 0,
            variables: HashMap::new(),
        }
    }

    /// Set the total file count.
    pub fn set_total(&mut self, total: i64) {
        self.total = total;
    }

    /// Reset the counter.
    pub fn reset_counter(&mut self) {
        self.counter = 1;
    }

    /// Set a custom variable.
    pub fn set_variable(&mut self, name: &str, value: &str) {
        self.variables.insert(name.to_string(), value.to_string());
    }

    /// Evaluate an expression template.
    pub fn evaluate(&self, template: &str, entry: &FileEntry, current_name: &str) -> RenamerResult<String> {
        let mut result = String::new();
        let mut chars = template.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' && chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                
                // Collect the expression inside ${}
                let mut expr = String::new();
                let mut depth = 1;
                
                while let Some(c) = chars.next() {
                    if c == '{' {
                        depth += 1;
                        expr.push(c);
                    } else if c == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        expr.push(c);
                    } else {
                        expr.push(c);
                    }
                }

                // Evaluate the expression
                let value = self.evaluate_expr(&expr, entry, current_name)?;
                result.push_str(&value);
            } else {
                result.push(c);
            }
        }

        Ok(result)
    }

    /// Evaluate a single expression (inside ${}).
    fn evaluate_expr(&self, expr: &str, entry: &FileEntry, current_name: &str) -> RenamerResult<String> {
        let expr = expr.trim();

        // Check if it's a function call
        if let Some(paren_pos) = expr.find('(') {
            if expr.ends_with(')') {
                let func_name = &expr[..paren_pos];
                let args_str = &expr[paren_pos + 1..expr.len() - 1];
                let args = self.parse_args(args_str, entry, current_name)?;
                return self.call_function(func_name, &args, entry, current_name);
            }
        }

        // Otherwise it's a variable
        self.get_variable(expr, entry, current_name)
    }

    /// Parse function arguments.
    fn parse_args(&self, args_str: &str, entry: &FileEntry, current_name: &str) -> RenamerResult<Vec<String>> {
        if args_str.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut args = Vec::new();
        let mut current = String::new();
        let mut depth = 0;
        let mut in_string = false;
        let mut string_char = '"';

        for c in args_str.chars() {
            match c {
                '"' | '\'' if depth == 0 => {
                    if !in_string {
                        in_string = true;
                        string_char = c;
                    } else if c == string_char {
                        in_string = false;
                    }
                    current.push(c);
                }
                '(' if !in_string => {
                    depth += 1;
                    current.push(c);
                }
                ')' if !in_string => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 && !in_string => {
                    args.push(self.evaluate_arg(&current.trim(), entry, current_name)?);
                    current.clear();
                }
                _ => {
                    current.push(c);
                }
            }
        }

        if !current.is_empty() {
            args.push(self.evaluate_arg(&current.trim(), entry, current_name)?);
        }

        Ok(args)
    }

    /// Evaluate a single argument.
    fn evaluate_arg(&self, arg: &str, entry: &FileEntry, current_name: &str) -> RenamerResult<String> {
        let arg = arg.trim();

        // String literal
        if (arg.starts_with('"') && arg.ends_with('"')) || 
           (arg.starts_with('\'') && arg.ends_with('\'')) {
            return Ok(arg[1..arg.len()-1].to_string());
        }

        // Number literal
        if arg.parse::<f64>().is_ok() {
            return Ok(arg.to_string());
        }

        // Nested expression or variable
        self.evaluate_expr(arg, entry, current_name)
    }

    /// Get a variable value.
    fn get_variable(&self, name: &str, entry: &FileEntry, current_name: &str) -> RenamerResult<String> {
        // Check custom variables first
        if let Some(value) = self.variables.get(name) {
            return Ok(value.clone());
        }

        let result = match name {
            // File name components
            "name" => current_name.to_string(),
            "stem" => entry.stem(),
            "ext" | "extension" => entry.extension.clone().unwrap_or_default(),
            
            // Path components
            "parent" => entry.parent_name.clone().unwrap_or_default(),
            "grandparent" => entry
                .path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            "path" => entry.path.to_string_lossy().to_string(),
            "dir" => entry
                .path
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),

            // File properties
            "size" => entry.size.to_string(),
            "created" => entry
                .created
                .map(|d| d.format("%Y%m%d").to_string())
                .unwrap_or_default(),
            "modified" => entry
                .modified
                .map(|d| d.format("%Y%m%d").to_string())
                .unwrap_or_default(),
            "accessed" => entry
                .accessed
                .map(|d| d.format("%Y%m%d").to_string())
                .unwrap_or_default(),
            "taken" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.exif.as_ref())
                .and_then(|e| e.date_taken)
                .map(|d| d.format("%Y%m%d").to_string())
                .unwrap_or_default(),

            // Counters
            "index" | "counter" => self.counter.to_string(),
            "total" => self.total.to_string(),

            // Current date/time
            "year" => Local::now().format("%Y").to_string(),
            "month" => Local::now().format("%m").to_string(),
            "day" => Local::now().format("%d").to_string(),
            "hour" => Local::now().format("%H").to_string(),
            "minute" => Local::now().format("%M").to_string(),
            "second" => Local::now().format("%S").to_string(),

            // Image metadata
            "width" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.dimensions)
                .map(|(w, _)| w.to_string())
                .unwrap_or_default(),
            "height" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.dimensions)
                .map(|(_, h)| h.to_string())
                .unwrap_or_default(),
            "camera" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.exif.as_ref())
                .and_then(|e| e.camera_model.clone())
                .unwrap_or_default(),
            "iso" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.exif.as_ref())
                .and_then(|e| e.iso)
                .map(|i| i.to_string())
                .unwrap_or_default(),
            "aperture" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.exif.as_ref())
                .and_then(|e| e.aperture)
                .map(|a| format!("f{:.1}", a))
                .unwrap_or_default(),
            "focal" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.exif.as_ref())
                .and_then(|e| e.focal_length)
                .map(|f| format!("{}mm", f))
                .unwrap_or_default(),

            // Audio metadata
            "artist" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.id3.as_ref())
                .and_then(|i| i.artist.clone())
                .unwrap_or_default(),
            "album" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.id3.as_ref())
                .and_then(|i| i.album.clone())
                .unwrap_or_default(),
            "title" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.id3.as_ref())
                .and_then(|i| i.title.clone())
                .unwrap_or_default(),
            "track" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.id3.as_ref())
                .and_then(|i| i.track)
                .map(|t| t.to_string())
                .unwrap_or_default(),
            "genre" => entry
                .metadata_cache
                .as_ref()
                .and_then(|m| m.id3.as_ref())
                .and_then(|i| i.genre.clone())
                .unwrap_or_default(),

            _ => {
                return Err(RenamerError::InvalidExpression(format!(
                    "Unknown variable: {}",
                    name
                )));
            }
        };

        Ok(result)
    }

    /// Call a function with arguments.
    fn call_function(
        &self,
        name: &str,
        args: &[String],
        entry: &FileEntry,
        current_name: &str,
    ) -> RenamerResult<String> {
        let result = match name {
            // String case functions
            "upper" | "uppercase" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.to_uppercase()
            }
            "lower" | "lowercase" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.to_lowercase()
            }
            "title" | "titlecase" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                titlecase::titlecase(s)
            }
            "sentence" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                transform_case(s, crate::core::CaseType::Sentence)
            }
            "camel" | "camelcase" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                transform_case(s, crate::core::CaseType::Camel)
            }
            "pascal" | "pascalcase" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                transform_case(s, crate::core::CaseType::Pascal)
            }
            "snake" | "snakecase" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                transform_case(s, crate::core::CaseType::Snake)
            }
            "kebab" | "kebabcase" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                transform_case(s, crate::core::CaseType::Kebab)
            }

            // String manipulation
            "trim" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.trim().to_string()
            }
            "ltrim" | "trimstart" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.trim_start().to_string()
            }
            "rtrim" | "trimend" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.trim_end().to_string()
            }
            "replace" => {
                if args.len() < 3 {
                    return Err(RenamerError::InvalidExpression(
                        "replace() requires 3 arguments: string, old, new".to_string(),
                    ));
                }
                args[0].replace(&args[1], &args[2])
            }
            "regex" => {
                if args.len() < 3 {
                    return Err(RenamerError::InvalidExpression(
                        "regex() requires 3 arguments: string, pattern, replacement".to_string(),
                    ));
                }
                let re = Regex::new(&args[1]).map_err(|e| {
                    RenamerError::InvalidExpression(format!("Invalid regex: {}", e))
                })?;
                re.replace_all(&args[0], &args[2] as &str).to_string()
            }
            "substr" | "substring" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                let start: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let len: usize = args
                    .get(2)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(s.len());
                s.chars().skip(start).take(len).collect()
            }
            "left" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                s.chars().take(n).collect()
            }
            "right" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let chars: Vec<char> = s.chars().collect();
                if n >= chars.len() {
                    s.to_string()
                } else {
                    chars[chars.len() - n..].iter().collect()
                }
            }
            "mid" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                let start: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let len: usize = args
                    .get(2)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(s.len());
                s.chars().skip(start).take(len).collect()
            }
            "pad" | "lpad" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                let len: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(s.len());
                let pad_char: char = args
                    .get(2)
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ');
                let current_len = s.chars().count();
                if current_len >= len {
                    s.to_string()
                } else {
                    let padding: String =
                        std::iter::repeat(pad_char).take(len - current_len).collect();
                    format!("{}{}", padding, s)
                }
            }
            "rpad" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                let len: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(s.len());
                let pad_char: char = args
                    .get(2)
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ');
                let current_len = s.chars().count();
                if current_len >= len {
                    s.to_string()
                } else {
                    let padding: String =
                        std::iter::repeat(pad_char).take(len - current_len).collect();
                    format!("{}{}", s, padding)
                }
            }
            "repeat" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or("");
                let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                s.repeat(n)
            }
            "reverse" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.chars().rev().collect()
            }
            "clean" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.chars()
                    .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
                    .collect()
            }
            "slug" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                remove_diacritics(s)
                    .to_lowercase()
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() {
                            c
                        } else if c.is_whitespace() || c == '_' {
                            '-'
                        } else {
                            '-'
                        }
                    })
                    .collect::<String>()
                    .split('-')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("-")
            }

            // Number formatting
            "num" | "number" => {
                let n: i64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
                let padding: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                format_number(n, NumberFormat::Decimal, padding)
            }
            "hex" => {
                let n: i64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
                let padding: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                format_number(n, NumberFormat::Hex, padding)
            }
            "roman" => {
                let n: i64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
                format_number(n, NumberFormat::Roman, 0)
            }
            "letter" => {
                let n: i64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(1);
                format_number(n, NumberFormat::Letter, 0)
            }
            "round" => {
                let n: f64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                n.round().to_string()
            }
            "floor" => {
                let n: f64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                n.floor().to_string()
            }
            "ceil" => {
                let n: f64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                n.ceil().to_string()
            }
            "abs" => {
                let n: f64 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                n.abs().to_string()
            }

            // Date formatting
            "date" => {
                let format = args.get(0).map(|s| s.as_str()).unwrap_or("%Y-%m-%d");
                Local::now().format(format).to_string()
            }
            "filedate" => {
                let source = args.get(0).map(|s| s.as_str()).unwrap_or("modified");
                let format = args.get(1).map(|s| s.as_str()).unwrap_or("%Y-%m-%d");
                
                let date = match source {
                    "created" => entry.created,
                    "modified" => entry.modified,
                    "accessed" => entry.accessed,
                    "exif" | "taken" => entry
                        .metadata_cache
                        .as_ref()
                        .and_then(|m| m.exif.as_ref())
                        .and_then(|e| e.date_taken),
                    // Falling back to mtime renamed photos by the wrong date without
                    // saying so, so an unknown source is an error.
                    other => {
                        return Err(RenamerError::InvalidExpression(format!(
                            "Unknown filedate source: {}",
                            other
                        )));
                    }
                };

                date.map(|d| d.format(format).to_string())
                    .unwrap_or_default()
            }

            // Conditional
            "if" => {
                if args.len() < 3 {
                    return Err(RenamerError::InvalidExpression(
                        "if() requires 3 arguments: condition, then, else".to_string(),
                    ));
                }
                let cond = &args[0];
                let is_true = !cond.is_empty() && cond != "0" && cond.to_lowercase() != "false";
                if is_true {
                    args[1].clone()
                } else {
                    args[2].clone()
                }
            }
            "coalesce" => {
                args.iter()
                    .find(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_default()
            }
            "default" => {
                let val = args.get(0).map(|s| s.as_str()).unwrap_or("");
                let default = args.get(1).map(|s| s.as_str()).unwrap_or("");
                if val.is_empty() {
                    default.to_string()
                } else {
                    val.to_string()
                }
            }

            // Meta functions
            "len" | "length" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                s.chars().count().to_string()
            }
            "ext" | "extension" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                std::path::Path::new(s)
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default()
            }
            "stem" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                std::path::Path::new(s)
                    .file_stem()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_else(|| s.to_string())
            }
            "dir" | "dirname" => {
                let path_str = entry.path.to_string_lossy().to_string();
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(&path_str);
                std::path::Path::new(s)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            }
            "filename" | "basename" => {
                let s = args.get(0).map(|s| s.as_str()).unwrap_or(current_name);
                std::path::Path::new(s)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| s.to_string())
            }

            // Concat
            "concat" | "join" => args.join(""),

            _ => {
                return Err(RenamerError::InvalidExpression(format!(
                    "Unknown function: {}",
                    name
                )));
            }
        };

        Ok(result)
    }

    /// Increment the counter and return the current value.
    pub fn next_counter(&mut self) -> i64 {
        let current = self.counter;
        self.counter += 1;
        current
    }
}

impl Default for ExpressionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ExifData, MetadataCache};
    use chrono::TimeZone;
    use std::path::PathBuf;

    /// A photo whose EXIF date differs from its mtime, so falling back to mtime shows.
    fn make_photo_entry() -> FileEntry {
        let taken = Local
            .with_ymd_and_hms(2021, 7, 4, 9, 30, 0)
            .single()
            .expect("valid date taken");
        let modified = Local
            .with_ymd_and_hms(2024, 1, 2, 3, 4, 5)
            .single()
            .expect("valid modified time");

        FileEntry {
            modified: Some(modified),
            metadata_cache: Some(MetadataCache {
                exif: Some(ExifData {
                    date_taken: Some(taken),
                    camera_make: None,
                    camera_model: None,
                    focal_length: None,
                    aperture: None,
                    iso: None,
                    exposure_time: None,
                    gps_latitude: None,
                    gps_longitude: None,
                    orientation: None,
                    width: None,
                    height: None,
                }),
                ..MetadataCache::default()
            }),
            ..make_test_entry()
        }
    }

    fn make_test_entry() -> FileEntry {
        FileEntry {
            id: uuid::Uuid::new_v4(),
            path: PathBuf::from("/home/user/photos/vacation.jpg"),
            original_name: "vacation.jpg".to_string(),
            extension: Some("jpg".to_string()),
            is_directory: false,
            size: 1024000,
            modified: None,
            created: None,
            accessed: None,
            depth: 0,
            parent_name: Some("photos".to_string()),
            metadata_cache: None,
        }
    }

    #[test]
    fn test_simple_variable() {
        let engine = ExpressionEngine::new();
        let entry = make_test_entry();
        
        let result = engine.evaluate("${name}", &entry, "vacation").unwrap();
        assert_eq!(result, "vacation");
    }

    #[test]
    fn test_function_call() {
        let engine = ExpressionEngine::new();
        let entry = make_test_entry();
        
        let result = engine.evaluate("${upper(name)}", &entry, "vacation").unwrap();
        assert_eq!(result, "VACATION");
    }

    #[test]
    fn test_template_with_literal() {
        let engine = ExpressionEngine::new();
        let entry = make_test_entry();
        
        let result = engine.evaluate("prefix_${name}_suffix", &entry, "vacation").unwrap();
        assert_eq!(result, "prefix_vacation_suffix");
    }

    #[test]
    fn test_replace_function() {
        let engine = ExpressionEngine::new();
        let entry = make_test_entry();
        
        let result = engine.evaluate("${replace(name, 'a', 'o')}", &entry, "vacation").unwrap();
        assert_eq!(result, "vocotion");
    }

    #[test]
    fn test_number_formatting() {
        let engine = ExpressionEngine::new();
        let entry = make_test_entry();

        let result = engine.evaluate("file_${num(counter, 3)}", &entry, "vacation").unwrap();
        assert_eq!(result, "file_001");
    }

    #[test]
    fn filedate_exif_reads_the_date_taken() {
        let engine = ExpressionEngine::new();
        let entry = make_photo_entry();

        // The shape the "Photo Rename (EXIF Date)" preset uses.
        let taken = engine
            .evaluate("${filedate('exif', '%Y%m%d_%H%M%S')}", &entry, "photo")
            .expect("evaluate exif date");
        assert_eq!(taken, "20210704_093000");

        let alias = engine
            .evaluate("${filedate('taken', '%Y%m%d')}", &entry, "photo")
            .expect("evaluate taken date");
        assert_eq!(alias, "20210704");

        // The other sources still read what they say they read.
        let modified = engine
            .evaluate("${filedate('modified', '%Y%m%d')}", &entry, "photo")
            .expect("evaluate modified date");
        assert_eq!(modified, "20240102");
    }

    #[test]
    fn filedate_reports_an_unknown_source() {
        let engine = ExpressionEngine::new();
        let entry = make_photo_entry();

        assert!(
            engine
                .evaluate("${filedate('modifed', '%Y%m%d')}", &entry, "photo")
                .is_err(),
            "a typo must surface instead of silently returning the mtime"
        );
    }

    #[test]
    fn taken_variable_reads_the_exif_date() {
        let engine = ExpressionEngine::new();
        let entry = make_photo_entry();

        let result = engine
            .evaluate("${taken}", &entry, "photo")
            .expect("evaluate taken");
        assert_eq!(result, "20210704");
    }

    #[test]
    fn counter_and_total_follow_the_engine() {
        let mut engine = ExpressionEngine::new();
        let entry = make_test_entry();
        engine.set_total(3);

        assert_eq!(
            engine
                .evaluate("${index}/${total}", &entry, "vacation")
                .expect("evaluate first"),
            "1/3"
        );

        engine.next_counter();
        assert_eq!(
            engine
                .evaluate("${counter}/${total}", &entry, "vacation")
                .expect("evaluate second"),
            "2/3"
        );

        engine.reset_counter();
        assert_eq!(
            engine
                .evaluate("${index}", &entry, "vacation")
                .expect("evaluate after reset"),
            "1"
        );
    }
}
