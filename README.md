# Bulk Renamer

A powerful, GNOME-native bulk file renaming application written entirely in Rust.


<img width="1454" height="849" alt="bulk-renamer-white" src="https://github.com/user-attachments/assets/647a5424-549b-4a15-92d7-636923ca4506" />

## Features

### Current Status

Implemented and wired today: 13 rule types with full UI editors, safe batch
validation, two-phase rename execution for swaps, undo/redo with a rename
history browser and per-batch undo, a crash-recovery journal, rename
logging/export, presets, CSV-driven direct renames, preview export as CSV,
undo shell-script export, settings persistence, folder scanning options,
metadata loading for previews, live preview status counts, and headless CLI
subcommands. Metadata (EXIF/ID3) is read for renaming; writing metadata back
to files is not implemented.

### Rule Types

All 13 rule types are available from the rule editor:

- **Find & Replace**: Simple text replacement with optional case sensitivity, or full regex with capture groups
- **Insert Text**: Add text at any position (start, end, specific index, before/after patterns)
- **Remove Text**: Remove characters by position, range, pattern, or brackets
- **Case Conversion**: Lowercase, uppercase, title case, sentence case, camel case, snake case, kebab case
- **Numbering**: Sequential numbers with configurable format (decimal, hex, roman, letters)
- **Trim**: Remove whitespace or specific characters from either end
- **Pad**: Pad names to a fixed length
- **Date/Time**: Insert formatted dates from the file or the clock
- **Expression**: Template-based renaming with the expression engine
- **Rearrange**: Reorder parts of the filename
- **Metadata**: Use EXIF (photos) and ID3 (music) tags in filenames
- **Cleanup**: Remove special characters, normalize whitespace, fix encoding issues
- **Transliterate**: Convert between scripts (e.g. Greek/Cyrillic to Latin)

### Advanced Features

- **Multiple Rules**: Chain rename operations, toggle individual rules on/off, or clear them all at once
- **Live Preview**: Virtualized file list and preview that stay fast with thousands of files
- **Safe Execution**: Batch validation, automatic conflict resolution, two-phase renames for swaps, and a crash-recovery journal
- **Undo System**: Full undo/redo, a rename history browser with per-batch undo, and shell script export
- **Background Execution**: Renames, undo/redo, and metadata loading run off the main thread with progress and cancellation
- **File List Control**: Natural sorting, name search and extension filters, per-file exclusion, and inline new-name overrides
- **Presets**: Save and load rename configurations (also usable from the CLI)
- **CSV**: Import old-name/new-name pairs for direct renames; export the current preview as CSV
- **Headless CLI**: `preview`, `apply`, and `list-presets` subcommands that run without GTK
- **Drag & Drop**: Add files by dragging into the window
- **Quick Actions**: One-shot lowercase, uppercase, title case, numbering, and Tidy Up Names cleanup

### Expression Engine

The expression engine provides a powerful template language:

```
${stem}_${num(counter, 4)}_${date("%Y%m%d")}.${ext}
```

Available variables:
- `${name}` - Full filename with extension
- `${stem}` - Filename without extension
- `${ext}` - File extension
- `${parent}` / `${grandparent}` - Parent and grandparent directory names
- `${dir}` / `${path}` - Full directory and file paths
- `${counter}` / `${index}` - Sequential counter; `${total}` - number of files
- `${year}`, `${month}`, `${day}`, `${hour}`, `${minute}`, `${second}` - Current date/time parts
- `${created}`, `${modified}`, `${accessed}` - File dates (as `YYYYMMDD`)
- `${size}` - File size in bytes
- `${sha256}`, `${sha1}`, `${md5}`, `${crc32}` - Content hashes (lowercase hex)
- `${width}`, `${height}`, `${camera}`, `${taken}` - Image/EXIF metadata
- `${artist}`, `${album}`, `${title}`, `${track}`, `${genre}` - Audio/ID3 metadata

Available functions:
- String: `upper()`, `lower()`, `title()`, `camel()`, `snake()`, `kebab()`, `trim()`, `replace()`, `regex()`, `substr()`, `left()`, `right()`, `pad()`
- Numeric: `num()`, `hex()`, `roman()`, `letter()`
- Date: `date("%Y-%m-%d")` for the current date, `filedate(source, format)` for file dates
- Conditional: `if()`, `coalesce()`, `default()`
- Meta: `len()`, `ext()`, `stem()`, `dir()`, `concat()`

### Metadata Support

#### EXIF (Images)
- Date taken
- Camera model
- Dimensions
- GPS coordinates
- Exposure settings

#### ID3 (Audio)
- Artist
- Album
- Title
- Track number
- Year
- Genre

## Installation

### From Source

Requirements:
- Rust 1.85 or later
- GTK4 4.12 or later
- libadwaita 1.5 or later
- `glib-compile-resources` (used by `build.rs` to compile the GResource bundle)

```bash
# Install dependencies (Fedora)
sudo dnf install gtk4-devel libadwaita-devel glib2-devel

# Install dependencies (Ubuntu/Debian)
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libglib2.0-dev-bin

# Install dependencies (Arch)
sudo pacman -S gtk4 libadwaita glib2-devel

# Build
cargo build --release

# Install
sudo install -Dm755 target/release/bulk-renamer /usr/local/bin/
sudo install -Dm644 data/com.chrisdaggas.bulk-renamer.desktop /usr/share/applications/
sudo install -Dm644 data/icons/hicolor/scalable/apps/com.chrisdaggas.bulk-renamer.svg /usr/share/icons/hicolor/scalable/apps/
sudo install -Dm644 data/com.chrisdaggas.bulk-renamer.metainfo.xml /usr/share/metainfo/
```

### Flatpak

The Flatpak is available for local build; Flathub submission is in progress.

```bash
flatpak-builder --user --install --force-clean build-dir com.chrisdaggas.bulk-renamer.yml
flatpak run com.chrisdaggas.bulk-renamer
```

### Packages

The `scripts/` directory contains packaging helpers:

```bash
./scripts/package-deb.sh       # Debian package (needs cargo-deb)
./scripts/package-rpm.sh       # RPM package (needs rpmbuild)
./scripts/package-appimage.sh  # AppImage
```

Note on the AppImage: it bundles only the application binary, not the GTK
stack. The host system must provide GTK4 >= 4.12 and libadwaita >= 1.5 at
runtime.

## Usage

### Basic Usage

1. Launch Bulk Renamer
2. Add files using the "+" button or drag & drop
3. Configure rename rules in the left panel
4. Preview changes in the right panel
5. Click "Rename" to apply changes

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+O` | Add files |
| `Ctrl+Shift+O` | Add folder |
| `Ctrl+Enter` | Execute rename |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` | Redo |
| `Ctrl+Shift+Delete` | Clear file list |
| `Ctrl+L` | Load preset |
| `Ctrl+S` | Save preset |
| `Ctrl+,` | Preferences |
| `Ctrl+1` | Quick lowercase |
| `Ctrl+2` | Quick uppercase |
| `Ctrl+3` | Quick title case |
| `Ctrl+4` | Quick numbering |
| `Ctrl+5` | Tidy Up Names (quick cleanup) |
| `Ctrl+Shift+K` | Clear all rules |
| `Ctrl+?` | Keyboard shortcuts window |
| `Ctrl+Q` | Quit |

### Command Line

The binary doubles as a headless tool: `preview`, `apply`, and `list-presets`
run entirely without GTK.

```bash
# Open the GUI with files or a directory
bulk-renamer file1.txt file2.txt
bulk-renamer ~/Documents/Photos/

# List available presets
bulk-renamer list-presets

# Show what a preset would rename (nothing is touched)
bulk-renamer preview --preset "Lowercase All" ~/Documents/Photos/

# Apply a preset; prompts for confirmation unless --yes is given
bulk-renamer apply --preset "Lowercase All" --yes ~/Documents/Photos/
```

Options for `preview` and `apply`:

| Option | Meaning |
|--------|---------|
| `--preset <NAME>` | Preset to use (see `list-presets`) |
| `-r`, `--recursive` | Recurse into subdirectories (default: one level) |
| `--hidden` | Include hidden files (dotfiles) |
| `-y`, `--yes` | `apply` only: skip the confirmation prompt |
| `--` | Treat every following argument as a path |

Exit codes: `0` success, `1` apply aborted or some renames failed, `2` invalid
usage, unknown preset, or a plan with conflicts/errors.

## Configuration

Settings are stored in `~/.config/bulk-renamer/settings.toml`.

Application data lives in `~/.local/share/bulk-renamer/`:

- `presets/` - User presets
- `undo/` - Persisted undo/redo batches and the crash-recovery journal
- `logs/rename.log` - Rename history log

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run
```

### Project Structure

```
src/
├── main.rs              # Entry point
├── app.rs               # Application setup
├── cli.rs               # Headless CLI (preview/apply/list-presets)
├── lib.rs               # Library crate
├── core/
│   ├── mod.rs
│   ├── error.rs         # Error types
│   ├── types.rs         # Core data types and settings
│   └── rules.rs         # Rename rule definitions
├── engine/
│   ├── mod.rs
│   ├── engine.rs        # Rename engine, executor, recovery journal
│   ├── transformer.rs   # String transformations
│   └── validator.rs     # Filename and batch validation
├── expression/
│   ├── mod.rs
│   └── evaluator.rs     # Expression template evaluation
├── metadata/
│   ├── mod.rs
│   ├── exif.rs          # EXIF handling
│   ├── id3.rs           # ID3 tag handling
│   └── attributes.rs    # File attributes
├── presets.rs           # Preset management
├── ui/
│   ├── mod.rs
│   ├── window.rs        # Main window
│   ├── file_item.rs     # File list items (exclusion, overrides)
│   ├── rule_dialogs.rs  # Rule editor dialogs
│   ├── dialogs.rs       # Common dialogs
│   ├── menu.rs          # Menus
│   ├── execution.rs     # Background execution with progress
│   ├── csv_io.rs        # CSV import/export, undo script export
│   ├── history_dialog.rs    # Rename history browser
│   ├── presets_dialog.rs    # Preset management dialog
│   ├── preferences_dialog.rs # Preferences
│   └── util.rs          # UI helpers
└── undo/
    ├── mod.rs
    ├── undo.rs          # Undo manager with on-disk persistence
    └── logging.rs       # Rename logging and CSV export
data/
└── resources/
    ├── bulk-renamer.gresource.xml
    └── style.css        # Custom styles (compiled by build.rs)
```

## Contributing

Contributions are welcome! Please read our contributing guidelines before submitting pull requests.

### Code Style

- Use `cargo clippy` for linting (CI runs it as a non-blocking check)
- Write tests for new functionality
- Follow GNOME Human Interface Guidelines for UI

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- [GTK4](https://gtk.org/) - GNOME widget toolkit
- [libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/) - GNOME design patterns
- [kamadak-exif](https://crates.io/crates/kamadak-exif) - EXIF parsing
- [id3](https://crates.io/crates/id3) - ID3 tag handling
