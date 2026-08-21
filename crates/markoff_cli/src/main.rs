use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use markoff_core::{ConversionRequest, Format, MarkoffError, convert_document, detect_format};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const BUILD_INFO: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\ncommit: ",
    env!("VERGEN_GIT_SHA"),
    "\nbranch: ",
    env!("VERGEN_GIT_BRANCH"),
    "\nbuilt: ",
    env!("VERGEN_BUILD_TIMESTAMP"),
    "\ntarget: ",
    env!("VERGEN_CARGO_TARGET_TRIPLE"),
    "\nrustc: ",
    env!("VERGEN_RUSTC_SEMVER"),
);

#[derive(Parser, Debug)]
#[command(
    name = "markoff",
    version,
    long_version = BUILD_INFO,
    about = "Bi-directional Office <-> Markdown converter",
    long_about = "Convert Office documents, Markdown, and structured data files.",
    after_help = "FORMATS:\n  docx, pdf, md/markdown, xlsx/xlsm, json, csv, yaml/yml, toml, pptx,\n  html/htm\n  PDF conversion supports PDF -> Markdown for documents with a text layer.\n  PPTX (presentations) converts slide titles and body text/bullets to and\n  from Markdown headings/lists; shape layout, images, and speaker notes are\n  not preserved.\n  HTML converts to and from Markdown (headings, emphasis, links, images,\n  lists, blockquotes, code blocks, and tables); page layout/CSS and scripts\n  are not preserved.\n  JSON/YAML/TOML preserve the full document structure (headings, lists,\n  tables, footnotes, images as base64), not just tables.\n  DOCX/PDF images are extracted into an 'image/' folder next to Markdown\n  output, or embedded as base64 in JSON/YAML/TOML output.\n\nOPTIONS (convert/batch):\n  --overwrite         Overwrite the output file(s) if they already exist;\n                      otherwise an error is raised when the destination\n                      exists.\n  --delimiter <CHAR>  CSV field delimiter: a single character, or 'tab'.\n                      Defaults to ','.\n\nSTREAMING:\n  Use '-' as INPUT or --output to read/write stdin/stdout. Streaming supports\n  text formats only: Markdown, JSON, CSV, YAML, TOML, and HTML.\n\nEXAMPLES:\n  markoff convert report.csv --to md\n  markoff convert report.pdf --to md\n  markoff convert report.md -o report.docx\n  markoff convert report.md -o report.docx --overwrite\n  markoff convert report.tsv --to md --delimiter tab\n  markoff convert slides.pptx --to md\n  markoff convert report.md -o page.html\n  markoff convert - --from json --to yaml -o -\n  markoff batch documents --pattern '*.docx' --to md -o converted\n  markoff batch documents --pattern '*.docx' --to md -o converted --overwrite"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Convert a single input file to another format.
    Convert {
        /// Input file path, or '-' to read a text format from stdin.
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        /// Output path; inferred from INPUT and --to when omitted. Use '-' for stdout.
        #[arg(short, long, value_name = "OUTPUT")]
        output: Option<PathBuf>,
        /// Source format. Required when INPUT is '-'.
        #[arg(long, value_name = "FORMAT")]
        from: Option<String>,
        /// Target format; inferred from OUTPUT when --to is omitted.
        #[arg(long, value_name = "FORMAT")]
        to: Option<String>,
        /// Overwrite the output file if it already exists.
        #[arg(long)]
        overwrite: bool,
        /// CSV field delimiter (single character, or 'tab'). Defaults to ','.
        #[arg(long, value_name = "CHAR")]
        delimiter: Option<String>,
    },
    /// Convert all matching files in a directory.
    Batch {
        /// Directory containing source files.
        #[arg(value_name = "DIRECTORY")]
        directory: PathBuf,
        /// Glob pattern relative to DIRECTORY, for example '*.docx'.
        #[arg(long, value_name = "GLOB")]
        pattern: String,
        /// Target format for every matching file.
        #[arg(long, value_name = "FORMAT")]
        to: String,
        /// Directory where converted files are written.
        #[arg(short, long, value_name = "OUTPUT_DIRECTORY")]
        output: PathBuf,
        /// Overwrite output files that already exist.
        #[arg(long)]
        overwrite: bool,
        /// CSV field delimiter (single character, or 'tab'). Defaults to ','.
        #[arg(long, value_name = "CHAR")]
        delimiter: Option<String>,
    },
    /// Run the graphical application.
    Gui,
}

fn parse_format_spec(spec: &str) -> Result<Format, MarkoffError> {
    Format::from_extension(spec)
}

/// Parses a CSV delimiter option: a single character, or the word `tab`.
fn parse_delimiter(spec: Option<&str>) -> anyhow::Result<u8> {
    let Some(spec) = spec else {
        return Ok(b',');
    };
    if spec.eq_ignore_ascii_case("tab") {
        return Ok(b'\t');
    }
    let mut characters = spec.chars();
    let (Some(character), None) = (characters.next(), characters.next()) else {
        return Err(anyhow::anyhow!(
            "--delimiter must be a single ASCII character or 'tab', got {spec:?}"
        ));
    };
    u8::try_from(character).map_err(|_| {
        anyhow::anyhow!("--delimiter must be a single ASCII character, got {spec:?}")
    })
}

fn temporary_path(format: Format) -> PathBuf {
    std::env::temp_dir().join(format!(
        "markoff_{}_{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos(),
        format
    ))
}

fn target_format(to: Option<&str>, output: Option<&Path>) -> anyhow::Result<Format> {
    if let Some(value) = to {
        return Ok(parse_format_spec(value)?);
    }
    if let Some(path) = output.filter(|path| *path != Path::new("-")) {
        return Ok(detect_format(path)?);
    }
    Err(anyhow::anyhow!(
        "specify --to or an output path with a known extension"
    ))
}

fn convert_one(
    input: &Path,
    output: Option<&Path>,
    from: Option<&str>,
    to: Option<&str>,
    overwrite: bool,
    delimiter: u8,
) -> anyhow::Result<()> {
    let from = match from {
        Some(value) => parse_format_spec(value)?,
        None if input != Path::new("-") => detect_format(input)?,
        None => return Err(anyhow::anyhow!("stdin requires --from")),
    };
    let to = target_format(to, output)?;
    let reads_stdin = input == Path::new("-");
    let writes_stdout = output == Some(Path::new("-"));
    if (reads_stdin || writes_stdout) && (!from.is_text() || !to.is_text()) {
        return Err(anyhow::anyhow!(
            "stdin/stdout are available only for text formats"
        ));
    }

    let temporary_input = reads_stdin.then(|| temporary_path(from));
    let temporary_output = writes_stdout.then(|| temporary_path(to));
    if let Some(path) = &temporary_input {
        let mut source = String::new();
        std::io::stdin().read_to_string(&mut source)?;
        std::fs::write(path, source)?;
    }
    let source_path = temporary_input.as_deref().unwrap_or(input);
    let destination = temporary_output
        .as_deref()
        .map(Path::to_path_buf)
        .or_else(|| output.map(Path::to_path_buf))
        .unwrap_or_else(|| {
            let mut path = input.to_path_buf();
            path.set_extension(to.to_string());
            path
        });

    if source_path == destination {
        return Err(anyhow::anyhow!("input and output paths must differ"));
    }
    convert_document(&ConversionRequest {
        input: source_path.to_path_buf(),
        output: destination.clone(),
        from,
        to,
        overwrite: overwrite || writes_stdout,
        csv_delimiter: delimiter,
    })?;
    if writes_stdout {
        let rendered = std::fs::read_to_string(&destination)?;
        std::io::stdout().write_all(rendered.as_bytes())?;
        std::fs::remove_file(&destination).ok();
    } else {
        println!("Converted {} -> {}", input.display(), destination.display());
    }
    if let Some(path) = temporary_input {
        std::fs::remove_file(path).ok();
    }
    Ok(())
}

fn run_batch(
    directory: &Path,
    pattern: &str,
    output: &Path,
    to: Format,
    overwrite: bool,
    delimiter: u8,
) -> anyhow::Result<()> {
    let pattern = directory.join(pattern).to_string_lossy().to_string();
    let inputs = glob::glob(&pattern)?
        .filter_map(Result::ok)
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    let progress = ProgressBar::new(inputs.len() as u64);
    progress.set_style(ProgressStyle::with_template(
        "{wide_bar} {pos}/{len} {msg}",
    )?);

    for input in inputs {
        let from = detect_format(&input)?;
        let stem = input
            .file_stem()
            .ok_or_else(|| anyhow::anyhow!("invalid input filename"))?;
        let destination = output.join(stem).with_extension(to.to_string());
        progress.set_message(input.display().to_string());
        convert_document(&ConversionRequest {
            input: input.clone(),
            output: destination,
            from,
            to,
            overwrite,
            csv_delimiter: delimiter,
        })?;
        progress.inc(1);
    }
    progress.finish_with_message("complete");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Convert {
            input,
            output,
            from,
            to,
            overwrite,
            delimiter,
        }) => {
            convert_one(
                &input,
                output.as_deref(),
                from.as_deref(),
                to.as_deref(),
                overwrite,
                parse_delimiter(delimiter.as_deref())?,
            )?;
        }
        Some(Commands::Batch {
            directory,
            pattern,
            to,
            output,
            overwrite,
            delimiter,
        }) => {
            run_batch(
                &directory,
                &pattern,
                &output,
                parse_format_spec(&to)?,
                overwrite,
                parse_delimiter(delimiter.as_deref())?,
            )?;
        }
        Some(Commands::Gui) => {
            markoff_gui::run().map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        None => {
            println!(
                "markoff CLI scaffold is ready. Use `markoff convert --help` to see commands."
            );
        }
    }

    Ok(())
}
