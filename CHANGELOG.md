# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Headless command-line mode: `preview`, `apply`, and `list-presets` subcommands
  run without GTK, with clear exit codes and a confirmation prompt for `apply`.
- Rename history browser with per-batch undo.
- Export of the current preview as CSV and of the last batch as an undo shell script.
- Natural sorting plus name search and extension filters in the file list.
- Per-file exclusion and inline new-name overrides directly in the list.
- Content-hash template variables: `${sha256}`, `${sha1}`, `${md5}`, `${crc32}`.
- Toast feedback and automatic resolution of self-inflicted name conflicts.
- Per-rule enable/disable toggles, a Clear All Rules action, a one-click
  Tidy Up Names action, and a keyboard shortcuts window.
- Crash-recovery journal: interrupted rename batches can be recovered because
  the plan is written to disk before any file is touched.

### Changed
- Renames, undo/redo, and metadata loading now run off the GTK main thread,
  with progress dialogs and cancellation; the UI stays responsive on large batches.
- File list and preview migrated to virtualized `ListView`/`ColumnView`,
  keeping thousands of files fast to display.
- Rule dialogs unified into a single consistent editor; the main window code
  was split into focused modules.
- Preset and settings saves are atomic (write-then-rename) and carry a schema
  version, so a crash mid-write can no longer corrupt them.
- Case-only renames (e.g. `Photo.JPG` to `photo.jpg`) are now allowed.
- The expression parser is quote-aware inside braces and nested arguments, so
  literal braces, parentheses, and commas in quoted strings stay literal.
- Removed dead code: parallel UI modules, the pest parser, and unused dependencies.

### Fixed
- Data-loss and correctness defects in the rename engine, including an
  unreachable two-phase swap path.
- Undo persistence toggle was not respected; undo no longer adopts a file
  whose content fingerprint changed since the rename ("impostor" files).

## [1.0.0] - 2026-07-18

Initial release.

### Added
- Rule-based bulk renaming with 13 rule types: replace (plain or regex),
  insert, remove, case conversion, numbering, trim, pad, date/time,
  expression templates, rearrange, metadata, cleanup, and transliteration.
- Live preview with per-file status, batch validation, and two-phase
  execution so swap renames are safe.
- Undo/redo with on-disk persistence and rename logging.
- Presets: built-in and user-defined, stored as JSON.
- CSV import for direct old-name/new-name renames.
- Expression engine with filename, date, size, counter, and EXIF/ID3
  metadata variables plus string/numeric/conditional functions.
- GNOME-native GTK4/libadwaita interface with drag & drop, folder scanning
  options, and quick actions.
