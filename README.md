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
- `DOCX <-> Markdown` for headings, paragraphs, tables, nested bulleted/numbered lists, bold/italic/strikethrough/underline text, inline and fenced code, blockquotes, horizontal rules, footnotes, bookmarks, and `PAGEREF` links
- `DOCX tables -> CSV / XLSX / JSON / YAML / TOML`, plus `CSV / XLSX -> DOCX`
- `PDF -> Markdown` for documents with an embedded text layer
- `PPTX <-> Markdown` for slide titles and body text/bullets
- `HTML <-> Markdown` for headings, emphasis, links, images, lists, blockquotes, code blocks, and tables
- CLI `convert` with text stdin/stdout and `batch` with glob patterns and progress bars
- GUI conversion queue with file picker, drag-and-drop, selectable target format, themes, and a format-aware preview (rendered Markdown, collapsible JSON/YAML/TOML tree, or plain text)
- Unit, integration, and property-based core tests covering DOCX and XLSX round-trips

### Still pending

- DOCX tables, links, images, and footnotes
- PPTX shape layout, embedded images/charts, and speaker notes are not preserved (only slide titles and body text/bullets)

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

PDF text extraction uses `pdfium-render`. The matching Pdfium native library is downloaded and embedded at build time, adding roughly 30 MB to each application binary. On first PDF conversion it is extracted to the user's cache; no network access or separately installed Pdfium library is required at runtime.

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

The source format is detected from the input extension and the target format from `--to` or the output extension. Use `--from` when the source format cannot be inferred. When `-o` is omitted, the converted file is written next to the source file with the target extension. If the destination file already exists, the command fails with an error unless `--overwrite` is given.

```powershell
# CSV to a Markdown table; creates .\report.md
cargo run -p markoff_cli -- convert .\report.csv --to md

# Markdown to DOCX with an explicit output path
cargo run -p markoff_cli -- convert .\notes.md -o .\output\notes.docx

# DOCX to Markdown
cargo run -p markoff_cli -- convert .\report.docx --to markdown

# PDF to Markdown
cargo run -p markoff_cli -- convert .\report.pdf --to markdown

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

Supported format identifiers are `docx`, `pdf`, `md`/`markdown`, `xlsx`/`xlsm`, `json`, `csv`, `yaml`/`yml`, `toml`, `pptx`, and `html`/`htm`. PDF is supported only as an input to Markdown; scanned PDF files without an embedded text layer require OCR and are not currently supported. PPTX conversion covers slide titles and body text/bullets only (shape layout, images, charts, and speaker notes are not preserved).

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

The pattern is relative to the supplied directory. The progress bar counts matching files; conversion stops and returns an error when an individual input is unsupported or invalid.

## Graphical application

Start the GUI directly during development or via the CLI command:

```powershell
cargo run -p markoff_gui
cargo run -p markoff_cli -- gui
```

1. Select **Add files** or drag files into the application window.
2. Choose the desired target format in **Convert to**.
3. Select **Apply format** to refresh output filenames for queued files.
4. Choose a file in the queue and select **Convert selected**.
5. Inspect the source and result previews. The converted file is saved beside the original source using the selected target extension.

The toolbar also switches between dark and light themes and opens build information in **About**. Files are converted one at a time from the selected queue entry; adding the same source path twice does not create a duplicate job.

## Format behavior and limitations

- DOCX and Markdown preserve headings, paragraphs, nested ordered and bulleted lists, tables, bold/italic/strikethrough/underline text, inline and fenced code blocks, blockquotes, horizontal rules, footnotes, bookmarks, and `PAGEREF` links. A fenced-code language identifier is not preserved.
- PDF to Markdown extracts the document text layer with Pdfium and embedded raster images with `lopdf` (saved as files in an `image` folder next to the Markdown output, same convention as DOCX below). Page layout, vector graphics, tables, and scanned text are not preserved, and images are appended after the text rather than placed at their original position.
- Markdown tables convert to and from XLSX. Each `## Sheet name` heading represents a workbook sheet; the first table row becomes the frozen header row in XLSX.
- `DOCX`/`Markdown <-> JSON/YAML/TOML` preserve the whole document structure (headings, paragraphs, list items, tables, code blocks, blockquotes, horizontal rules), not just tables, as an ordered `blocks` array; each block keeps inline Markdown formatting as raw text, and literal list numbers are not preserved. This round-trips in both directions: DOCX/Markdown can be converted to JSON/YAML/TOML and back.
- Images embedded in a DOCX or PDF are extracted and saved as files in an `image` folder next to the Markdown output, referenced with standard `![alt](image/file.ext)` syntax. Converting to JSON/YAML/TOML instead embeds each image inline as base64 (self-contained, no external files). Converting Markdown/JSON/YAML/TOML back to Markdown restores the image files from base64. Converting back to DOCX does not yet re-embed images as OOXML pictures; an image reference degrades to literal escaped text in that direction.
- JSON, CSV, YAML, and TOML convert to XLSX as tabular data. JSON/YAML/TOML input for this route must be an array of objects; this is a separate, table-only convention from the whole-document `blocks` schema above.
- Ordinary hyperlinks, advanced Word table layout, Excel formulas/styles/charts, and PDF output are not yet semantically preserved. PPTX conversion is limited to slide titles and body text/bullets (no shape layout, images, charts, or speaker notes).

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

## Verification

The current core implementation is verified with:

```bash
cargo test -p markoff_core --quiet
```

and is passing successfully at the moment.

## Notes

This project is no longer a blank scaffold. The core engine now performs real conversions for the documented text-based formats, and the next step is to turn that engine into a richer CLI experience and then into the graphical workflow described in the TЗ.
