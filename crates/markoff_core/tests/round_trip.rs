use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "round_trip/data.rs"]
mod data;
#[path = "round_trip/docx_read.rs"]
mod docx_read;
#[path = "round_trip/docx_write.rs"]
mod docx_write;
#[path = "round_trip/formats.rs"]
mod formats;
#[path = "round_trip/golden.rs"]
mod golden;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temporary_path(name: &str, extension: &str) -> PathBuf {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "markoff_{name}_{}_{}.{}",
        std::process::id(),
        sequence,
        extension
    ))
}

fn remove_files(paths: &[&PathBuf]) {
    for path in paths {
        fs::remove_file(path).ok();
    }
}
