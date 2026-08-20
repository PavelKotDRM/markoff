use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use markoff_core::{Format, MarkoffError, convert_file, detect_format};
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
    after_help = "FORMATS:\n  docx, md/markdown, xlsx/xlsm, json, csv, yaml/yml, toml\n  PPTX is recognized but conversion is not implemented yet.\n\nSTREAMING:\n  Use '-' as INPUT or --output to read/write stdin/stdout. Streaming supports\n  text formats only: Markdown, JSON, CSV, YAML, and TOML.\n\nEXAMPLES:\n  markoff convert report.csv --to md\n  markoff convert report.md -o report.docx\n  markoff convert - --from json --to yaml -o -\n  markoff batch documents --pattern '*.docx' --to md -o converted"
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
    },
    /// Run the graphical application.
    Gui,
}

fn parse_format_spec(spec: &str) -> Result<Format, MarkoffError> {
    Format::from_extension(spec)
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
    convert_file(source_path, &destination, from, to)?;
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

fn run_batch(directory: &Path, pattern: &str, output: &Path, to: Format) -> anyhow::Result<()> {
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
        convert_file(&input, destination, from, to)?;
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
        }) => {
            convert_one(&input, output.as_deref(), from.as_deref(), to.as_deref())?;
        }
        Some(Commands::Batch {
            directory,
            pattern,
            to,
            output,
        }) => {
            run_batch(&directory, &pattern, &output, parse_format_spec(&to)?)?;
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
