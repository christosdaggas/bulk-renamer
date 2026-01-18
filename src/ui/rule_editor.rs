//! Rule editor components.

use crate::core::*;
use libadwaita as adw;
use adw::prelude::*;
use gtk4 as gtk;

/// Create a rule row for the rules list.
pub fn create_rule_row(rule: &RenameRule) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(&get_rule_title(&rule.rule_type))
        .subtitle(&get_rule_subtitle(&rule.rule_type))
        .show_enable_switch(true)
        .enable_expansion(true)
        .build();

    // Set enabled state
    row.set_expanded(false);

    // Add rule-specific configuration widgets
    let content = create_rule_content(&rule.rule_type);
    row.add_row(&content);

    // Add remove button
    let remove_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(vec!["flat", "circular"])
        .tooltip_text("Remove rule")
        .build();

    row.add_suffix(&remove_btn);

    // Add drag handle
    let drag_handle = gtk::Image::builder()
        .icon_name("list-drag-handle-symbolic")
        .css_classes(vec!["dim-label"])
        .build();

    row.add_prefix(&drag_handle);

    row
}

/// Get the title for a rule type.
fn get_rule_title(rule_type: &RuleType) -> String {
    match rule_type {
        RuleType::Replace(_) => "Replace".to_string(),
        RuleType::Insert(_) => "Insert".to_string(),
        RuleType::Remove(_) => "Remove".to_string(),
        RuleType::ChangeCase(_) => "Change Case".to_string(),
        RuleType::Numbering(_) => "Numbering".to_string(),
        RuleType::Trim(_) => "Trim".to_string(),
        RuleType::Pad(_) => "Pad".to_string(),
        RuleType::Expression(_) => "Expression".to_string(),
        RuleType::Rearrange(_) => "Rearrange".to_string(),
        RuleType::DateTime(_) => "Date/Time".to_string(),
        RuleType::Metadata(_) => "Metadata".to_string(),
        RuleType::Cleanup(_) => "Cleanup".to_string(),
        RuleType::Transliterate(_) => "Transliterate".to_string(),
    }
}

/// Get a subtitle/description for a rule.
fn get_rule_subtitle(rule_type: &RuleType) -> String {
    match rule_type {
        RuleType::Replace(r) => {
            if r.use_regex {
                format!("Regex: {} → {}", r.find, r.replace)
            } else {
                format!("{} → {}", r.find, r.replace)
            }
        }
        RuleType::Insert(r) => {
            let text = match &r.text {
                InsertText::Fixed(t) => t.clone(),
                InsertText::ParentFolder => "[parent folder]".to_string(),
                InsertText::GrandparentFolder => "[grandparent folder]".to_string(),
                InsertText::CurrentDate(f) => format!("[date: {}]", f),
                InsertText::FileDate { format, .. } => format!("[file date: {}]", format),
                InsertText::Counter(_) => "[counter]".to_string(),
                InsertText::Clipboard => "[clipboard]".to_string(),
                InsertText::Expression(e) => format!("[{}]", e),
            };
            let pos = match &r.position {
                InsertPosition::Prefix => "at start",
                InsertPosition::Suffix => "at end",
                InsertPosition::Position(p) => return format!("{} at position {}", text, p),
                InsertPosition::BeforeText(t) => return format!("{} before '{}'", text, t),
                InsertPosition::AfterText(t) => return format!("{} after '{}'", text, t),
                _ => "",
            };
            format!("{} {}", text, pos)
        }
        RuleType::Remove(r) => match &r.target {
            RemoveTarget::Text { text, .. } => format!("Remove '{}'", text),
            RemoveTarget::Pattern(p) => format!("Pattern: {}", p),
            RemoveTarget::Range { start, end } => format!("Characters {} to {}", start, end),
            RemoveTarget::FirstN(n) => format!("First {} characters", n),
            RemoveTarget::LastN(n) => format!("Last {} characters", n),
            RemoveTarget::Digits => "Remove all digits".to_string(),
            RemoveTarget::Letters => "Remove all letters".to_string(),
            RemoveTarget::Symbols => "Remove symbols".to_string(),
            RemoveTarget::Whitespace => "Remove whitespace".to_string(),
            RemoveTarget::Characters(c) => format!("Remove [{}]", c),
            RemoveTarget::Words(w) => format!("Remove words: {}", w.join(", ")),
            RemoveTarget::Bracketed(_) => "Remove bracketed content".to_string(),
            RemoveTarget::Duplicates => "Remove duplicate characters".to_string(),
            RemoveTarget::LeadingZeros => "Remove leading zeros".to_string(),
            RemoveTarget::BeforeAfter { marker, remove_before, .. } => {
                if *remove_before {
                    format!("Remove before '{}'", marker)
                } else {
                    format!("Remove after '{}'", marker)
                }
            }
        },
        RuleType::ChangeCase(r) => match r.case_type {
            CaseType::Lower => "lowercase".to_string(),
            CaseType::Upper => "UPPERCASE".to_string(),
            CaseType::Title => "Title Case".to_string(),
            CaseType::Sentence => "Sentence case".to_string(),
            CaseType::Camel => "camelCase".to_string(),
            CaseType::Pascal => "PascalCase".to_string(),
            CaseType::Snake => "snake_case".to_string(),
            CaseType::Kebab => "kebab-case".to_string(),
            CaseType::Constant => "CONSTANT_CASE".to_string(),
            CaseType::Toggle => "tOGGLE cASE".to_string(),
            CaseType::Capitalize => "Capitalize first".to_string(),
            CaseType::Alternating => "aLtErNaTiNg".to_string(),
            CaseType::Random => "RaNdOm".to_string(),
        },
        RuleType::Numbering(r) => format!(
            "Start: {}, Padding: {}, Increment: {}",
            r.start, r.padding, r.increment
        ),
        RuleType::Trim(r) => match r.trim_type {
            TrimType::Both => "Trim whitespace".to_string(),
            TrimType::Start => "Trim start".to_string(),
            TrimType::End => "Trim end".to_string(),
            TrimType::Characters => format!("Trim [{}]", r.characters.as_deref().unwrap_or("")),
            TrimType::MaxLength(n) => format!("Limit to {} characters", n),
        },
        RuleType::Pad(r) => format!(
            "Pad to {} with '{}'",
            r.length,
            r.pad_char
        ),
        RuleType::Expression(r) => r.expression.clone(),
        RuleType::Rearrange(r) => format!(
            "Split by '{}', order: {:?}",
            r.separator, r.order
        ),
        RuleType::DateTime(r) => format!(
            "{:?}: {}",
            r.source, r.format
        ),
        RuleType::Metadata(r) => format!("{:?}", r.field),
        RuleType::Cleanup(r) => {
            let mut parts = Vec::new();
            if r.collapse_spaces { parts.push("spaces"); }
            if r.remove_special { parts.push("special chars"); }
            if r.remove_diacritics { parts.push("diacritics"); }
            format!("Clean: {}", parts.join(", "))
        }
        RuleType::Transliterate(r) => match r.mapping {
            TransliterationMapping::CyrillicToLatin => "Cyrillic → Latin".to_string(),
            TransliterationMapping::GreekToLatin => "Greek → Latin".to_string(),
            TransliterationMapping::RemoveDiacritics => "Remove diacritics".to_string(),
            TransliterationMapping::NormalizeUnicode => "Normalize Unicode".to_string(),
        },
    }
}

/// Create the content widget for a rule type.
fn create_rule_content(rule_type: &RuleType) -> gtk::Widget {
    match rule_type {
        RuleType::Replace(r) => create_replace_content(r),
        RuleType::Insert(r) => create_insert_content(r),
        RuleType::Remove(r) => create_remove_content(r),
        RuleType::ChangeCase(r) => create_case_content(r),
        RuleType::Numbering(r) => create_numbering_content(r),
        RuleType::Expression(r) => create_expression_content(r),
        _ => gtk::Label::new(Some("Configuration not yet implemented")).into(),
    }
}

fn create_replace_content(rule: &ReplaceRule) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();

    let find_row = adw::EntryRow::builder()
        .title("Find")
        .text(&rule.find)
        .build();
    group.add(&find_row);

    let replace_row = adw::EntryRow::builder()
        .title("Replace with")
        .text(&rule.replace)
        .build();
    group.add(&replace_row);

    let regex_row = adw::SwitchRow::builder()
        .title("Use Regular Expression")
        .active(rule.use_regex)
        .build();
    group.add(&regex_row);

    let case_row = adw::SwitchRow::builder()
        .title("Case Sensitive")
        .active(rule.case_sensitive)
        .build();
    group.add(&case_row);

    let all_row = adw::SwitchRow::builder()
        .title("Replace All Occurrences")
        .active(rule.replace_all)
        .build();
    group.add(&all_row);

    group.into()
}

fn create_insert_content(_rule: &InsertRule) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();

    // Text source dropdown
    let text_options = ["Fixed text", "Parent folder", "Date", "Counter", "Clipboard", "Expression"];
    let text_dropdown = adw::ComboRow::builder()
        .title("Insert")
        .model(&gtk::StringList::new(&text_options))
        .build();
    group.add(&text_dropdown);

    // Text entry (for fixed text)
    let text_entry = adw::EntryRow::builder()
        .title("Text")
        .build();
    group.add(&text_entry);

    // Position dropdown
    let pos_options = ["At start", "At end", "At position", "Before text", "After text"];
    let pos_dropdown = adw::ComboRow::builder()
        .title("Position")
        .model(&gtk::StringList::new(&pos_options))
        .build();
    group.add(&pos_dropdown);

    group.into()
}

fn create_remove_content(_rule: &RemoveRule) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();

    let remove_options = [
        "Specific text", "Pattern (regex)", "Character range",
        "First N characters", "Last N characters",
        "All digits", "All letters", "All symbols", "Whitespace",
        "Bracketed content", "Duplicate characters"
    ];

    let type_dropdown = adw::ComboRow::builder()
        .title("Remove")
        .model(&gtk::StringList::new(&remove_options))
        .build();
    group.add(&type_dropdown);

    let text_entry = adw::EntryRow::builder()
        .title("Text/Pattern")
        .build();
    group.add(&text_entry);

    group.into()
}

fn create_case_content(rule: &CaseRule) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();

    let case_options = [
        "lowercase", "UPPERCASE", "Title Case", "Sentence case",
        "camelCase", "PascalCase", "snake_case", "kebab-case",
        "CONSTANT_CASE", "tOGGLE cASE", "Capitalize", "aLtErNaTiNg"
    ];

    let case_dropdown = adw::ComboRow::builder()
        .title("Case Style")
        .model(&gtk::StringList::new(&case_options))
        .selected(match rule.case_type {
            CaseType::Lower => 0,
            CaseType::Upper => 1,
            CaseType::Title => 2,
            CaseType::Sentence => 3,
            CaseType::Camel => 4,
            CaseType::Pascal => 5,
            CaseType::Snake => 6,
            CaseType::Kebab => 7,
            CaseType::Constant => 8,
            CaseType::Toggle => 9,
            CaseType::Capitalize => 10,
            CaseType::Alternating => 11,
            CaseType::Random => 11,
        })
        .build();
    group.add(&case_dropdown);

    let ext_row = adw::SwitchRow::builder()
        .title("Include Extension")
        .active(rule.include_extension)
        .build();
    group.add(&ext_row);

    group.into()
}

fn create_numbering_content(rule: &NumberingRule) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();

    let start_row = adw::SpinRow::builder()
        .title("Start at")
        .adjustment(&gtk::Adjustment::new(
            rule.start as f64, 0.0, 999999.0, 1.0, 10.0, 0.0
        ))
        .build();
    group.add(&start_row);

    let increment_row = adw::SpinRow::builder()
        .title("Increment")
        .adjustment(&gtk::Adjustment::new(
            rule.increment as f64, -1000.0, 1000.0, 1.0, 10.0, 0.0
        ))
        .build();
    group.add(&increment_row);

    let padding_row = adw::SpinRow::builder()
        .title("Padding (digits)")
        .adjustment(&gtk::Adjustment::new(
            rule.padding as f64, 0.0, 10.0, 1.0, 1.0, 0.0
        ))
        .build();
    group.add(&padding_row);

    let format_options = ["Decimal", "Hexadecimal", "Roman", "Letter"];
    let format_dropdown = adw::ComboRow::builder()
        .title("Format")
        .model(&gtk::StringList::new(&format_options))
        .build();
    group.add(&format_dropdown);

    let prefix_row = adw::EntryRow::builder()
        .title("Prefix")
        .text(&rule.prefix)
        .build();
    group.add(&prefix_row);

    let suffix_row = adw::EntryRow::builder()
        .title("Suffix")
        .text(&rule.suffix)
        .build();
    group.add(&suffix_row);

    let reset_row = adw::SwitchRow::builder()
        .title("Reset per folder")
        .active(rule.reset_per_folder)
        .build();
    group.add(&reset_row);

    group.into()
}

fn create_expression_content(rule: &ExpressionRule) -> gtk::Widget {
    let box_ = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();

    let label = gtk::Label::builder()
        .label("Expression template:")
        .xalign(0.0)
        .build();
    box_.append(&label);

    let entry = gtk::Entry::builder()
        .text(&rule.expression)
        .placeholder_text("${name}_${num(counter, 3)}")
        .build();
    box_.append(&entry);

    let help_label = gtk::Label::builder()
        .label("Variables: ${name}, ${stem}, ${ext}, ${parent}, ${counter}, ${date('format')}")
        .xalign(0.0)
        .css_classes(vec!["dim-label", "caption"])
        .wrap(true)
        .build();
    box_.append(&help_label);

    box_.into()
}

/// Create a dialog for adding a new rule.
pub fn create_add_rule_dialog(parent: &impl IsA<gtk::Window>) -> adw::MessageDialog {
    let dialog = adw::MessageDialog::new(
        Some(parent),
        Some("Add Rename Rule"),
        None,
    );

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(vec!["boxed-list"])
        .build();

    let rule_types = [
        ("text-x-generic-symbolic", "Replace Text", "Find and replace text or patterns"),
        ("format-text-direction-symbolic", "Change Case", "Convert to uppercase, lowercase, title case, etc."),
        ("list-add-symbolic", "Insert Text", "Add text, dates, numbers at any position"),
        ("list-remove-symbolic", "Remove Text", "Remove characters, patterns, or ranges"),
        ("view-list-ordered-symbolic", "Numbering", "Add sequential numbers"),
        ("x-office-calendar-symbolic", "Date/Time", "Insert file or current date"),
        ("applications-science-symbolic", "Expression", "Use template expressions"),
        ("edit-clear-symbolic", "Cleanup", "Remove special characters and normalize"),
    ];

    for (icon, title, subtitle) in &rule_types {
        let row = adw::ActionRow::builder()
            .title(*title)
            .subtitle(*subtitle)
            .activatable(true)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(*icon));
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        list.append(&row);
    }

    dialog.set_extra_child(Some(&list));
    dialog.add_response("cancel", "Cancel");

    dialog
}
