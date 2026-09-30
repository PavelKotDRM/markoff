use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

mod conversion;
mod docx;
mod structured;
mod xlsx;

fn unique_temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("markoff_{name}_{nanos}.tmp"))
}
