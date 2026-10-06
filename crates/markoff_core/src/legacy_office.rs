use crate::{Format, MarkoffError};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMPORARY_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct OfficeWorkspace {
    path: PathBuf,
}

impl OfficeWorkspace {
    pub(crate) fn new() -> Result<Self, MarkoffError> {
        for _ in 0..16 {
            let sequence = TEMPORARY_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("markoff-office-{}-{sequence}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "unable to create a unique temporary directory for Office conversion",
        )
        .into())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OfficeWorkspace {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                path = %self.path.display(),
                %error,
                "unable to remove temporary Office conversion workspace"
            );
        }
    }
}

pub(crate) fn modern_equivalent(format: Format) -> Option<Format> {
    match format {
        Format::Doc => Some(Format::Docx),
        Format::Xls => Some(Format::Xlsx),
        Format::Ppt => Some(Format::Pptx),
        _ => None,
    }
}

pub(crate) fn convert_to_modern(
    input: &Path,
    legacy_format: Format,
    workspace: &OfficeWorkspace,
) -> Result<PathBuf, MarkoffError> {
    let modern_format =
        modern_equivalent(legacy_format).ok_or_else(|| MarkoffError::InvalidOption {
            message: format!("{legacy_format} is not a legacy Microsoft Office format"),
        })?;
    let output_directory = workspace.path().join("modern");
    std::fs::create_dir(&output_directory)?;
    let stem = input
        .file_stem()
        .ok_or_else(|| MarkoffError::InvalidInput {
            path: input.to_string_lossy().to_string(),
        })?;
    let mut converted = output_directory.join(stem);
    converted.set_extension(modern_format.to_string());

    convert_with_office_oxide(input, &converted, legacy_format)?;
    Ok(converted)
}

fn convert_with_office_oxide(
    input: &Path,
    output: &Path,
    input_format: Format,
) -> Result<(), MarkoffError> {
    let document = office_oxide::Document::open(input).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "office_oxide could not read {} file {}: {error}",
                input_format,
                input.display()
            ),
        )
    })?;
    document.save_as(output).map_err(|error| {
        std::io::Error::other(format!(
            "office_oxide could not convert {} to {}: {error}",
            input.display(),
            output.display()
        ))
    })?;
    if !output.is_file() {
        return Err(std::io::Error::other(format!(
            "office_oxide reported success but did not create {}",
            output.display()
        ))
        .into());
    }
    Ok(())
}

pub(crate) fn convert_from_modern(
    input: &Path,
    legacy_format: Format,
    output: &Path,
    workspace: &OfficeWorkspace,
) -> Result<(), MarkoffError> {
    if modern_equivalent(legacy_format).is_none() {
        return Err(MarkoffError::InvalidOption {
            message: format!("{legacy_format} is not a legacy Microsoft Office format"),
        });
    }
    let output_directory = workspace.path().join("legacy");
    std::fs::create_dir(&output_directory)?;
    let converted = run_conversion(input, legacy_format, &output_directory)?;
    std::fs::copy(converted, output)?;
    Ok(())
}

fn run_conversion(
    input: &Path,
    output_format: Format,
    output_directory: &Path,
) -> Result<PathBuf, MarkoffError> {
    let executable = libreoffice_executable()?;
    let profile = output_directory.join("profile");
    std::fs::create_dir(&profile)?;
    let profile_url = file_url(&profile)?;
    let (extension, filter) = output_spec(output_format)?;
    let conversion = match filter {
        Some(filter) => format!("{extension}:{filter}"),
        None => extension.to_string(),
    };
    let output = Command::new(&executable)
        .arg("--headless")
        .arg("--nologo")
        .arg("--nodefault")
        .arg("--nolockcheck")
        .arg(format!("-env:UserInstallation={profile_url}"))
        .arg("--convert-to")
        .arg(conversion)
        .arg("--outdir")
        .arg(output_directory)
        .arg(input)
        .output()
        .map_err(|error| {
            MarkoffError::Io(std::io::Error::new(
                error.kind(),
                format!(
                    "failed to start LibreOffice at {}: {error}",
                    Path::new(&executable).display()
                ),
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(std::io::Error::other(format!(
            "LibreOffice failed to convert {} to .{extension}: {}",
            input.display(),
            if details.is_empty() {
                format!("process exited with {}", output.status)
            } else {
                details.to_string()
            }
        ))
        .into());
    }

    let stem = input
        .file_stem()
        .ok_or_else(|| MarkoffError::InvalidInput {
            path: input.to_string_lossy().to_string(),
        })?;
    let mut converted = output_directory.join(stem);
    converted.set_extension(extension);
    if !converted.is_file() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let details = if stderr.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", stderr.trim())
        };
        return Err(std::io::Error::other(format!(
            "LibreOffice reported success but did not create {}{details}",
            converted.display()
        ))
        .into());
    }
    Ok(converted)
}

fn libreoffice_executable() -> Result<OsString, MarkoffError> {
    if let Some(configured) = std::env::var_os("MARKOFF_LIBREOFFICE") {
        return probe_libreoffice(configured, true);
    }
    for candidate in ["soffice", "libreoffice"] {
        match probe_libreoffice(candidate.into(), false) {
            Ok(executable) => return Ok(executable),
            Err(MarkoffError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "writing legacy .doc/.xls/.ppt files requires LibreOffice; install it and add `soffice` or `libreoffice` to PATH, or set MARKOFF_LIBREOFFICE to its executable",
    )
    .into())
}

fn probe_libreoffice(executable: OsString, configured: bool) -> Result<OsString, MarkoffError> {
    match Command::new(&executable).arg("--version").output() {
        Ok(_) => Ok(executable),
        Err(error) if !configured && error.kind() == std::io::ErrorKind::NotFound => {
            Err(error.into())
        }
        Err(error) if configured && error.kind() == std::io::ErrorKind::NotFound => {
            Err(std::io::Error::new(
                error.kind(),
                format!(
                    "configured LibreOffice executable {} was not found",
                    Path::new(&executable).display()
                ),
            )
            .into())
        }
        Err(error) => Err(error.into()),
    }
}

fn output_spec(format: Format) -> Result<(&'static str, Option<&'static str>), MarkoffError> {
    match format {
        Format::Docx => Ok(("docx", None)),
        Format::Xlsx => Ok(("xlsx", None)),
        Format::Pptx => Ok(("pptx", None)),
        Format::Doc => Ok(("doc", Some("MS Word 97"))),
        Format::Xls => Ok(("xls", Some("MS Excel 97"))),
        Format::Ppt => Ok(("ppt", Some("MS PowerPoint 97"))),
        _ => Err(MarkoffError::InvalidOption {
            message: format!("legacy Office export does not support {format}"),
        }),
    }
}

fn file_url(path: &Path) -> Result<String, MarkoffError> {
    let path = path.canonicalize()?;
    let normalized = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(normalized.len());
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(format!("file:///{}", encoded.trim_start_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::{
        OfficeWorkspace, convert_with_office_oxide, modern_equivalent, output_spec,
        probe_libreoffice,
    };
    use crate::Format;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str, extension: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "markoff_office_oxide_{}_{}_{}.{}",
            std::process::id(),
            name,
            stamp,
            extension
        ))
    }

    #[test]
    fn maps_legacy_formats_to_ooxml_formats() {
        assert_eq!(modern_equivalent(Format::Doc), Some(Format::Docx));
        assert_eq!(modern_equivalent(Format::Xls), Some(Format::Xlsx));
        assert_eq!(modern_equivalent(Format::Ppt), Some(Format::Pptx));
        assert_eq!(modern_equivalent(Format::Docx), None);
    }

    #[test]
    fn selects_legacy_libreoffice_export_filters() {
        assert_eq!(
            output_spec(Format::Doc).unwrap(),
            ("doc", Some("MS Word 97"))
        );
        assert_eq!(
            output_spec(Format::Xls).unwrap(),
            ("xls", Some("MS Excel 97"))
        );
        assert_eq!(
            output_spec(Format::Ppt).unwrap(),
            ("ppt", Some("MS PowerPoint 97"))
        );
    }

    #[test]
    fn reports_a_missing_configured_libreoffice_executable() {
        let path = std::env::temp_dir().join(format!(
            "markoff-no-libreoffice-{}-{}",
            std::process::id(),
            super::TEMPORARY_DIRECTORY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let error = probe_libreoffice(path.into_os_string(), true).unwrap_err();
        assert!(error.to_string().contains("was not found"));
    }

    #[test]
    fn office_oxide_reads_and_writes_ooxml_documents() {
        let markdown = temp_path("input", "md");
        let input = temp_path("input", "docx");
        std::fs::write(
            &markdown,
            "# Legacy conversion\n\nOffice conversion test.\n",
        )
        .unwrap();
        crate::convert_file(&markdown, &input, Format::Markdown, Format::Docx).unwrap();

        let workspace = OfficeWorkspace::new().unwrap();
        let output = workspace.path().join("officeoxide-roundtrip.docx");
        convert_with_office_oxide(&input, &output, Format::Docx).unwrap();
        let reopened = office_oxide::Document::open(&output).unwrap();
        assert!(reopened.plain_text().contains("Office conversion test."));

        std::fs::remove_file(markdown).ok();
        std::fs::remove_file(input).ok();
    }
}
