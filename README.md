# markoff

[Русская версия](docs/README.ru.md)

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
- Whole-document conversion from Markdown/DOCX/PDF/PPTX/HTML to a typed
  JSON/YAML/TOML document schema, and back to Markdown/DOCX/PDF/PPTX/HTML; inline
  formatting, lists, links, footnotes, code metadata, and rich table cells are
  represented as typed nodes
- Direct conversion between JSON, YAML, and TOML values
- Real text conversion support for:
  - `JSON <-> Markdown`
  - `CSV <-> Markdown`
- `Markdown Tables <-> XLSX`, including multi-sheet workbooks, frozen header rows, and fitted column widths
- `JSON / CSV / YAML / TOML <-> XLSX` for arrays of objects and tabular sheets
- `DOCX <-> Markdown` for headings, paragraphs, tables, nested bulleted/numbered lists, bold/italic/strikethrough/underline text, inline and fenced code, blockquotes, horizontal rules, footnotes, bookmarks, and `PAGEREF` links
- `DOCX -> HTML` through Markdown, preserving supported document elements
- `DOCX tables -> CSV / XLSX / JSON / YAML / TOML`, plus `CSV / XLSX -> DOCX`
- `PDF -> Markdown / JSON / YAML / TOML` for documents with an embedded text layer, and `Markdown / DOCX / JSON / YAML / TOML -> PDF`
- `PPTX <-> Markdown` for slide titles and body text/bullets
- `HTML <-> Markdown` for headings, emphasis, links, images, lists, blockquotes, code blocks, and tables
- `PDF / PPTX / HTML -> JSON / YAML / TOML` and JSON/YAML/TOML back to Markdown, DOCX, PDF, PPTX, or HTML
- CLI `convert` with text stdin/stdout and `batch` with glob patterns and progress bars
- GUI conversion queue with file picker, drag-and-drop, selectable target format, a compatible-pairs-only **Tables only** toggle, themes, and a format-aware preview (rendered Markdown, collapsible JSON/YAML/TOML tree, or plain text)
- Unit, integration, and property-based core tests covering DOCX and XLSX round-trips

### Still pending

- Re-embedding images into DOCX when converting from Markdown or structured data
- PPTX shape layout, embedded images/charts, and speaker notes are not preserved (only slide titles and body text/bullets)

## Architecture

![markoff workspace architecture](docs/architecture/markoff-architecture.drawio.svg)

Open the [embedded SVG diagram](docs/architecture/markoff-architecture.drawio.svg) directly in the Draw.io Integration extension or [diagrams.net](https://app.diagrams.net/). The SVG retains the editable draw.io data for both pages: **Architecture** shows crate and module dependencies, while **Repository layout** maps the complete workspace structure, build assets, tests, and documentation.

## Getting started

Install the current stable [Rust toolchain](https://www.rust-lang.org/tools/install), clone the repository, and build the workspace from its root:

```powershell
cargo build --workspace
```

Run the test suite before using a locally built version:

```powershell
cargo test --workspace
```

The commands below use `cargo run` during development. A release build is created with `cargo build --release`; its executable is available at `target/release/markoff_cli` (or `markoff_cli.exe` on Windows).

PDF import and export use `pdfium-render`. The matching Pdfium native library is downloaded and embedded at build time, adding roughly 30 MB to each application binary. On first PDF conversion it is extracted to the user's cache; no network access or separately installed Pdfium library is required at runtime. PDF output embeds fonts with Cyrillic support and renders supported document structure, formatting, links, code, tables, footnotes, and raster images.

PDF output uses the embedded DejaVu fonts for broad Unicode coverage and Twemoji
graphics for emoji fallback. DejaVu fonts are distributed under their bundled
free-font license; Twemoji graphics are copyright their contributors and
licensed under CC-BY 4.0.

### Use the executable file

To use the application without `cargo run`, build the release binary once:

```powershell
cargo build --release -p markoff_cli
```

In PowerShell, run the executable from the repository root with its relative path:

```powershell
.\target\release\markoff_cli.exe --help
.\target\release\markoff_cli.exe --version
.\target\release\markoff_cli.exe convert .\report.csv --to md
```

To call `markoff_cli.exe` from any directory, copy it to a directory already listed in the `PATH` environment variable, or add `target\release` to `PATH` for the current PowerShell session:

```powershell
$env:Path += ";$PWD\target\release"
markoff_cli.exe convert .\report.csv --to md
```

On Linux and macOS, use `./target/release/markoff_cli` instead. The commands in the following sections show the development form with `cargo run`; replace `cargo run -p markoff_cli --` with the executable path to run the same command from the compiled application.

## Command line

Display the list of commands and supported arguments:

```powershell
cargo run -p markoff_cli -- --help
cargo run -p markoff_cli -- convert --help
cargo run -p markoff_cli -- --version
```

### Convert one file

The general command is:

```text
markoff convert INPUT [--from FORMAT] [--to FORMAT] [-o OUTPUT]
```

The source format is detected from the input extension and the target format from `--to` or the output extension. Use `--from` when the source format cannot be inferred. When `-o` is omitted, the converted file is written next to the source file with the target extension. If the destination file already exists, the command fails with an error unless `--overwrite` is given. The input file is never overwritten, even with `--overwrite`.

```powershell
# CSV to a Markdown table; creates .\report.md
cargo run -p markoff_cli -- convert .\report.csv --to md

# Markdown to DOCX with an explicit output path
cargo run -p markoff_cli -- convert .\notes.md -o .\output\notes.docx

# DOCX to Markdown
cargo run -p markoff_cli -- convert .\report.docx --to markdown

# PDF to Markdown
cargo run -p markoff_cli -- convert .\report.pdf --to markdown

# Markdown or DOCX to PDF
cargo run -p markoff_cli -- convert .\notes.md -o .\notes.pdf
cargo run -p markoff_cli -- convert .\report.docx --to pdf

# Structured document or data to PDF
cargo run -p markoff_cli -- convert .\report.json --to pdf
cargo run -p markoff_cli -- convert .\settings.yaml --to pdf

# Markdown table to an XLSX workbook
cargo run -p markoff_cli -- convert .\scores.md -o .\scores.xlsx

# JSON array of objects to XLSX
cargo run -p markoff_cli -- convert .\people.json --to xlsx

# XLSX to JSON
cargo run -p markoff_cli -- convert .\people.xlsx -o .\people.json

# Overwrite an existing output file
cargo run -p markoff_cli -- convert .\report.csv --to md --overwrite

# CSV with a semicolon delimiter
cargo run -p markoff_cli -- convert .\report.csv --to md --delimiter ';'

# Tab-separated values
cargo run -p markoff_cli -- convert .\report.csv --to md --delimiter tab
```

Supported format identifiers are `docx`, `pdf`, `md`/`markdown`, `xlsx`/`xlsm`, `json`, `csv`, `yaml`/`yml`, `toml`, `pptx`, and `html`/`htm`. PDF input converts to Markdown or the JSON/YAML/TOML document schema when it has an embedded text layer; PDF output is available from Markdown, DOCX, and JSON/YAML/TOML. PDF export preserves supported document elements, embeds PNG/JPEG/GIF/BMP/WebP images, makes internal links and HTTP(S)/mailto/tel links clickable, and exposes headings in the PDF outline. It reflows content onto A4 pages rather than reproducing source pagination or precise page layout. SVG images and scanned-PDF OCR are not currently supported. PPTX conversion covers slide titles and body text/bullets only (shape layout, images, charts, and speaker notes are not preserved).

`--delimiter` sets the CSV field delimiter (a single character, or the word `tab`); it defaults to a comma and applies wherever CSV is read or written (`convert` and `batch`, including through XLSX/DOCX intermediates).

### Standard input and output

Use `-` as the input path to read from standard input and as the output path to write to standard output. Both ends must be text formats: Markdown, JSON, CSV, YAML, or TOML. Specify both formats when neither filename can provide them.

```powershell
'{"name":"Ada","active":true}' | cargo run -p markoff_cli -- convert - --from json --to markdown -o -

Get-Content .\table.csv | cargo run -p markoff_cli -- convert - --from csv --to md -o -
```

Binary Office files (`.docx`, `.xlsx`, `.xlsm`) cannot be read from stdin or written to stdout.

### Batch conversion

Convert every matching file in a directory. The output directory is created by the converter when needed.

```powershell
# Convert all DOCX files from .\documents into Markdown files in .\converted
cargo run -p markoff_cli -- batch .\documents --pattern '*.docx' --to md -o .\converted

# Convert CSV files to XLSX workbooks
cargo run -p markoff_cli -- batch .\exports --pattern '*.csv' --to xlsx -o .\workbooks
```

As with `convert`, pass `--overwrite` to replace output files that already exist; otherwise a matching existing destination file stops the batch with an error.

The pattern is relative to the supplied directory. The progress indicator advances for converted files; conversion stops and returns an error when a matching file cannot be read or an individual input is unsupported or invalid.

## Graphical application

Start the GUI directly during development or via the CLI command:

```powershell
cargo run -p markoff_gui
cargo run -p markoff_cli -- gui
```

1. Select **Add files** or drag files into the application window.
2. Choose the desired target format in **Convert to**.
3. Output filenames update automatically when you change the target format.
4. Choose a file in the queue and select **Convert selected**.
5. Inspect the source and result previews. The converted file is saved beside the original source using the selected target extension; hover over its status for the output path or error.

The toolbar also offers **Tables only** for supported document ↔ JSON/YAML/TOML conversions, switches between dark and light themes, and opens build information in **About**. Files are converted one at a time from the selected queue entry; adding the same source path twice does not create a duplicate job.

## Format behavior and limitations

- DOCX and Markdown preserve headings, paragraphs, nested ordered and bulleted lists, tables, bold/italic/strikethrough/underline text, inline and fenced code blocks, blockquotes, horizontal rules, footnotes, bookmarks, and `PAGEREF` links. Structured JSON/YAML/TOML also preserve inline formatting, list numbering, table alignment, and fenced-code info strings.
- PDF to Markdown, JSON, YAML, or TOML extracts the document text layer with Pdfium and embedded raster images with `lopdf` (saved as files next to Markdown or embedded in structured output). Page layout, vector graphics, tables, and scanned text are not preserved, and images are appended after the text rather than placed at their original position.
- Markdown, DOCX, and JSON/YAML/TOML to PDF renders supported document structure, inline formatting, nested lists, aligned tables, code blocks, blockquotes, footnotes, links, bookmarks, and embedded raster images. Long code lines wrap inside their shaded blocks, including across pages. Headings are added to the PDF outline; internal links and HTTP(S)/mailto/tel links remain clickable. Other URI schemes are printed as text. Content is reflowed onto new A4 pages, so original pagination and exact page placement are not retained. JSON/YAML/TOML document schemas are rendered as documents; other valid values are printed as formatted source code. SVG images are not supported for PDF output.
- Markdown tables convert to and from XLSX. Each `## Sheet name` heading represents a workbook sheet; the first table row becomes the frozen header row in XLSX.
- Markdown/DOCX/PDF/PPTX/HTML to JSON/YAML/TOML and back preserve supported document structure as an ordered `blocks` array with typed inline `content`, nested list items, footnote definitions and references, links, bookmarks, images, and rich table `cells`. Older `text` and table `rows` documents remain readable. `--tables-only` retains table blocks.
- Images embedded in a DOCX or PDF are extracted and saved as files in an `image` folder next to the Markdown output, referenced with standard `![alt](image/file.ext)` syntax. Converting to JSON/YAML/TOML embeds image data as base64. Converting structured data back to Markdown restores image files; Markdown-to-HTML embeds local image data in the HTML. Converting back to DOCX does not yet re-embed images as OOXML pictures.
- JSON, CSV, YAML, and TOML convert to XLSX as tabular data. JSON/YAML/TOML input for this route must be an array of objects; this is a separate, table-only convention from the whole-document `blocks` schema above.
- Advanced Word table layout and Excel formulas/styles/charts are not yet semantically preserved. PDF output reflows content and does not retain source pagination, exact page placement, or advanced Word layout. Supported raster images and document elements are rendered; SVG images are not supported. PPTX conversion is limited to slide titles and body text/bullets; Markdown inline markup is converted to its visible plain text (no shape layout, images, charts, or speaker notes).

## Rust code quality

Install the standard formatting and linting components once:

```powershell
rustup component add rustfmt clippy
```

Use these commands from the workspace root for the usual development checks:

```powershell
# Fast compile check without producing binaries
cargo check --workspace --all-targets --all-features

# Format the workspace, or only verify formatting in CI
cargo fmt --all
cargo fmt --all -- --check

# Run all Clippy lints used by this workspace
cargo clippy --workspace --all-targets --all-features

# Treat every Clippy warning as an error (recommended before a commit)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run all tests, including documentation tests
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc

# Verify that API documentation builds without dependency documentation
cargo doc --workspace --all-features --no-deps
```

Cargo and Clippy can apply some suggestions automatically. Review the diff afterward; `--allow-dirty` permits changes when the working tree already contains edits:

```powershell
cargo fix --workspace --all-targets --all-features --allow-dirty
cargo clippy --fix --workspace --all-targets --all-features --allow-dirty
git diff
```

Optional third-party tools can check dependency vulnerabilities, outdated packages, and unused dependencies:

```powershell
cargo install cargo-audit cargo-outdated cargo-machete
cargo audit --deny warnings
cargo outdated --workspace
cargo machete
```

Update the installed Rust toolchain and inspect project dependencies with:

```powershell
rustup update
cargo tree --workspace
cargo update --dry-run
```

### Run benchmark

```powershell
cargo bench -p markoff_core --bench conversion
```

## Code quality checks

Run these commands from the workspace root to validate code quality with standard Rust tooling:

```powershell
# 1) Formatting check (rustfmt)
cargo fmt --all -- --check

# 2) Lints and static analysis (Clippy)
cargo clippy --workspace --all-targets

# 3) Unit/integration/doctests
cargo test --workspace

# 4) Documentation build (catches doc issues)
cargo doc --workspace --no-deps
```

For CI-grade strictness, make Clippy fail the build on warnings:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

## Verification

The current core implementation is verified with:

```bash
cargo test -p markoff_core --quiet
```

and is passing successfully at the moment.

## Notes

This project is no longer a blank scaffold. The core engine now performs real conversions for the documented text-based formats, and the next step is to turn that engine into a richer CLI experience and then into the graphical workflow described in the technical specification.
