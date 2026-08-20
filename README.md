# markoff

markoff is a Rust workspace for a bi-directional converter between Microsoft Office documents and Markdown / structured data formats. The workspace, conversion baseline, GUI workflow, quality gates, benchmarks, and release pipeline are in place.

## Current status

The project has moved beyond the initial stub stage:

- Workspace structure is complete
- Core crate contains the format model and conversion pipeline
- Text and tabular conversion paths are available, including XLSX
- CLI supports single-file, batch, and text stream workflows
- GUI provides a local conversion queue with previews and drag-and-drop

### Implemented

- Cargo workspace with three crates: `markoff_core`, `markoff_cli`, and `markoff_gui`
- `vergen 10` build metadata in CLI `--version` and GUI About dialog
- GitHub Actions CI on Windows and Linux for formatting, Clippy, tests, and release builds
- Shared `Format` enum and file-extension detection
- Conversion request validation and typed error handling
- Real text conversion support for:
  - `JSON -> Markdown`
  - `Markdown -> JSON`
  - `CSV -> Markdown`
  - `Markdown -> CSV`
- `Markdown Tables <-> XLSX`, including multi-sheet workbooks, frozen header rows, and fitted column widths
- `JSON / CSV / YAML / TOML <-> XLSX` for arrays of objects and tabular sheets
- `DOCX <-> Markdown` for headings, paragraphs, bulleted/numbered lists, and bold/italic inline text
- CLI `convert` with text stdin/stdout and `batch` with glob patterns and progress bars
- GUI conversion queue with file picker, drag-and-drop, selectable target format, themes, and dual-pane text preview
- Unit, integration, and property-based core tests covering DOCX and XLSX round-trips

### Still pending

- DOCX tables, links, images, and footnotes
- PPTX conversion support

## Roadmap & progress

- [x] Phase 1: Architecture & project setup
  - [x] Cargo workspace and crate initialization
  - [x] Vergen metadata integration
  - [x] CI/CD pipeline setup
- [x] Phase 2: Core conversion engine
  - [x] DOCX ⇄ Markdown basic parser and generator
  - [x] Markdown table ⇄ XLSX conversion engine
  - [x] Data format bridge (JSON, CSV, YAML, TOML) ⇄ XLSX
  - [x] Text conversion bridge for JSON/CSV ↔ Markdown
  - [x] Round-trip and property-based testing
- [x] Phase 3: CLI interface
  - [x] Production command workflows and streaming I/O
  - [x] Batch processing with progress bars
- [x] Phase 4: GUI
  - [x] Egui responsive layout and theme engine
  - [x] Platform backends: glow (Windows) and wgpu (Linux)
  - [x] Dual-pane live preview
  - [x] Drag-and-drop queue
- [x] Phase 5: Benchmarks, polish, and release
  - [x] Criterion benchmark suite
  - [x] Full rustdoc coverage
  - [x] Binary release packaging

## Quick start

### Build

```bash
cargo build --workspace
```

### Run CLI

```bash
cargo run -p markoff_cli -- --help
cargo run -p markoff_cli -- convert sample.csv --to md
```

### Run GUI

```bash
cargo run -p markoff_gui
```

### Run benchmark

```bash
cargo bench -p markoff_core --bench conversion
```

## Verification

The current core implementation is verified with:

```bash
cargo test -p markoff_core --quiet
```

and is passing successfully at the moment.

## Notes

This project is no longer a blank scaffold. The core engine now performs real conversions for the documented text-based formats, and the next step is to turn that engine into a richer CLI experience and then into the graphical workflow described in the TЗ.
