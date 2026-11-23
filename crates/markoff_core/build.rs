use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const PDFIUM_VERSION: &str = "8125";
const DOWNLOAD_BASE: &str = "https://github.com/bblanchon/pdfium-binaries/releases/download";

struct Platform {
    archive: &'static str,
    library_in_archive: &'static str,
    library_name: &'static str,
    archive_sha256: &'static str,
}

// SHA-256 values are pinned from the matching upstream GitHub release assets.
fn platform(os: &str, arch: &str) -> Option<Platform> {
    let (archive, library_in_archive, library_name, archive_sha256) = match (os, arch) {
        ("windows", "x86_64") => (
            "pdfium-win-x64.tgz",
            "bin/pdfium.dll",
            "pdfium.dll",
            "9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b",
        ),
        ("windows", "aarch64") => (
            "pdfium-win-arm64.tgz",
            "bin/pdfium.dll",
            "pdfium.dll",
            "d394ba61991fb53f4d87f70988035f337d9e73f2bd2c3c1b966df8c43fcabb05",
        ),
        ("windows", "x86") => (
            "pdfium-win-x86.tgz",
            "bin/pdfium.dll",
            "pdfium.dll",
            "5aa1eb3e7de0589534245a181cb51def90d1dc76e987aea4be524a8cb3db1069",
        ),
        ("linux", "x86_64") => (
            "pdfium-linux-x64.tgz",
            "lib/libpdfium.so",
            "libpdfium.so",
            "e1c3ce6e270cb5e0c20816b379ad9f2c1309972697c505f508de92bf5e7d6659",
        ),
        ("linux", "aarch64") => (
            "pdfium-linux-arm64.tgz",
            "lib/libpdfium.so",
            "libpdfium.so",
            "8b8818f2ec57d1b213d952ca6d7f0ffeca9d0110487d0e20ad83ba918bc92efa",
        ),
        ("macos", "x86_64") => (
            "pdfium-mac-x64.tgz",
            "lib/libpdfium.dylib",
            "libpdfium.dylib",
            "8a5193535b80c7215d8932f6647ae5097465b6dba2811602c21e120c2af112b0",
        ),
        ("macos", "aarch64") => (
            "pdfium-mac-arm64.tgz",
            "lib/libpdfium.dylib",
            "libpdfium.dylib",
            "4939bd935ac83ddc09a3a4f038bc43a19cea1660316a3667f26cb07a070e139d",
        ),
        _ => return None,
    };

    Some(Platform {
        archive,
        library_in_archive,
        library_name,
        archive_sha256,
    })
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn cargo_home() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_HOME") {
        return PathBuf::from(path);
    }

    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".cargo")
}

fn download_archive(path: &Path, expected_sha256: &str) -> Result<(), Box<dyn Error>> {
    let filename = path
        .file_name()
        .ok_or("PDFium archive cache path has no filename")?
        .to_string_lossy();
    let url = format!("{DOWNLOAD_BASE}/chromium%2F{PDFIUM_VERSION}/{filename}");
    println!("cargo:warning=Downloading PDFium {PDFIUM_VERSION} for this target");

    let mut response = ureq::get(&url).call()?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()?;
    let actual_sha256 = sha256(&bytes);
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "PDFium archive SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        )
        .into());
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), timestamp));
    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if fs::read(path).is_ok_and(|bytes| sha256(&bytes) == expected_sha256) {
            return Ok(());
        }
        return Err(error.into());
    }
    Ok(())
}

fn verified_archive(cache_dir: &Path, platform: &Platform) -> Result<PathBuf, Box<dyn Error>> {
    fs::create_dir_all(cache_dir)?;
    let path = cache_dir.join(platform.archive);

    if path.is_file() {
        let bytes = fs::read(&path)?;
        if sha256(&bytes) == platform.archive_sha256 {
            return Ok(path);
        }
        fs::remove_file(&path)?;
    }

    download_archive(&path, platform.archive_sha256)?;
    Ok(path)
}

fn extract_library(
    archive_path: &Path,
    expected_path: &str,
    destination: &Path,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let file = fs::File::open(archive_path)?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().replace('\\', "/");
        if path == expected_path && entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                return Err("PDFium library in the verified archive is empty".into());
            }
            fs::write(destination, &bytes)?;
            return Ok(bytes);
        }
    }

    Err(format!("'{expected_path}' not found in PDFium archive").into())
}

fn run() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=CARGO_HOME");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?);
    let target = std::env::var("TARGET")?;
    let os = std::env::var("CARGO_CFG_TARGET_OS")?;
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH")?;
    let platform = platform(&os, &arch)
        .ok_or_else(|| format!("PDFium {PDFIUM_VERSION} is unsupported on target {target}"))?;

    let library_path = out_dir.join("pdfium.bin");
    let library_hash = if std::env::var_os("DOCS_RS").is_some() {
        fs::write(&library_path, [])?;
        sha256(&[])
    } else {
        let cache_dir = cargo_home()
            .join("markoff-pdfium")
            .join(PDFIUM_VERSION)
            .join(&target);
        let archive = verified_archive(&cache_dir, &platform)?;
        let bytes = extract_library(&archive, platform.library_in_archive, &library_path)?;
        sha256(&bytes)
    };

    println!("cargo:rustc-env=MARKOFF_PDFIUM_SHA256={library_hash}");
    println!("cargo:rustc-env=MARKOFF_PDFIUM_VERSION={PDFIUM_VERSION}");
    println!(
        "cargo:rustc-env=MARKOFF_PDFIUM_LIBRARY_NAME={}",
        platform.library_name
    );
    println!("cargo:rustc-env=MARKOFF_PDFIUM_TARGET={target}");
    println!("cargo:rerun-if-changed={}", library_path.display());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("markoff_core: failed to prepare PDFium {PDFIUM_VERSION}: {error}");
        std::process::exit(1);
    }
}
