# Bulk Renamer

A powerful, GNOME-native bulk file renaming application written entirely in Rust.

![Bulk Renamer](data/icons/hicolor/scalable/apps/com.chrisdaggas.bulk-renamer.svg)

## Features

### Basic Renaming
- **Find & Replace**: Simple text replacement with optional case sensitivity
- **Regular Expressions**: Advanced pattern matching with capture groups
- **Case Conversion**: Lowercase, uppercase, title case, sentence case, camel case, snake case, kebab case
- **Insert Text**: Add text at any position (start, end, specific index, before/after patterns)
- **Remove Text**: Remove characters by position, range, or pattern
- **Numbering**: Sequential numbers with configurable format (decimal, hex, roman, letters)
- **Trim/Pad**: Remove or add whitespace/characters
- **Cleanup**: Remove special characters, normalize whitespace, fix encoding issues

### Advanced Features
- **Expression Engine**: Powerful DSL for complex renaming logic
- **Metadata-based Renaming**: Use EXIF (photos) and ID3 (music) tags in filenames
- **Multiple Rules**: Chain multiple rename operations
- **Live Preview**: See results before applying changes
- **Undo System**: Full undo/redo with shell script export
- **Presets**: Save and load rename configurations
- **Drag & Drop**: Add files by dragging into the window

### Expression Engine

The expression engine provides a powerful template language:

```
${stem}_${counter:4}_${date:%Y%m%d}.${ext}
```

Available variables:
- `${name}` - Full filename with extension
- `${stem}` - Filename without extension
- `${ext}` - File extension
- `${parent}` - Parent directory name
- `${counter}` - Sequential counter
- `${date}` - Current date
- `${filedate}` - File modification date

Available functions:
- String: `upper()`, `lower()`, `title()`, `camel()`, `snake()`, `trim()`, `replace()`, `regex()`, `substr()`, `left()`, `right()`, `pad()`
- Numeric: `num()`, `hex()`, `roman()`, `letter()`
- Date: `date()`, `filedate()`
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
- Rust 1.70 or later
- GTK4 4.12 or later
- libadwaita 1.4 or later

```bash
# Install dependencies (Fedora)
sudo dnf install gtk4-devel libadwaita-devel

# Install dependencies (Ubuntu/Debian)
sudo apt install libgtk-4-dev libadwaita-1-dev

# Install dependencies (Arch)
sudo pacman -S gtk4 libadwaita

# Build
cargo build --release

# Install
sudo install -Dm755 target/release/bulk-renamer /usr/local/bin/
sudo install -Dm644 data/com.chrisdaggas.bulk-renamer.desktop /usr/share/applications/
sudo install -Dm644 data/com.chrisdaggas.bulk-renamer.svg /usr/share/icons/hicolor/scalable/apps/
```

### Flatpak (Coming Soon)

```bash
flatpak install flathub com.chrisdaggas.bulk-renamer
```

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
| `Ctrl+L` | Load preset |
| `Ctrl+S` | Save preset |
| `Ctrl+1` | Quick lowercase |
| `Ctrl+2` | Quick uppercase |
| `Ctrl+3` | Quick title case |
| `Ctrl+4` | Quick numbering |
| `Ctrl+Q` | Quit |

### Command Line

```bash
# Open with files
bulk-renamer file1.txt file2.txt

# Open with directory
bulk-renamer ~/Documents/Photos/
```

## Configuration

Configuration is stored in `~/.config/bulk-renamer/`:

- `config.toml` - Application settings
- `presets/` - User presets

Logs are stored in `~/.local/share/bulk-renamer/logs/`.

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
├── lib.rs               # Library crate
├── core/
│   ├── mod.rs
│   ├── error.rs         # Error types
│   ├── types.rs         # Core data types
│   └── rules.rs         # Rename rule definitions
├── engine/
│   ├── mod.rs
│   ├── engine.rs        # Main rename engine
│   ├── transformer.rs   # String transformations
│   └── validator.rs     # Filename validation
├── expression/
│   ├── mod.rs
│   ├── parser.rs        # Pest parser
│   ├── grammar.pest     # Expression grammar
│   └── evaluator.rs     # Expression evaluation
├── metadata/
│   ├── mod.rs
│   ├── exif.rs          # EXIF handling
│   ├── id3.rs           # ID3 tag handling
│   └── attributes.rs    # File attributes
├── presets.rs           # Preset management
├── ui/
│   ├── mod.rs
│   ├── window.rs        # Main window
│   ├── preview_panel.rs # Preview panel
│   ├── rule_editor.rs   # Rule editor
│   ├── file_list.rs     # File list
│   ├── dialogs.rs       # Dialogs
│   ├── preferences.rs   # Preferences
│   └── header.rs        # Header bar
├── undo/
│   ├── mod.rs
│   ├── undo.rs          # Undo manager
│   └── logging.rs       # Structured logging
└── style.css            # Custom styles
```

## Contributing

Contributions are welcome! Please read our contributing guidelines before submitting pull requests.

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Use `cargo clippy` for linting
- Write tests for new functionality
- Follow GNOME Human Interface Guidelines for UI

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- [GTK4](https://gtk.org/) - GNOME widget toolkit
- [libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/) - GNOME design patterns
- [Pest](https://pest.rs/) - Parser library for the expression engine
- [kamadak-exif](https://crates.io/crates/kamadak-exif) - EXIF parsing
- [id3](https://crates.io/crates/id3) - ID3 tag handling
