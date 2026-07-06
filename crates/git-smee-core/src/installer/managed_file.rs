use std::{fs, io::Read, path::Path};

use super::Error;

/// Marker string used to identify files managed by git-smee.
pub const MANAGED_FILE_MARKER: &str = "THIS FILE IS MANAGED BY git-smee";
const MANAGED_FILE_SCAN_BYTES: usize = 8 * 1024;
const MANAGED_FILE_SCAN_LINES: usize = 32;

/// Prefixes content with a managed marker using `#` comments.
///
/// If content starts with a shebang (`#!`), the marker is inserted after the shebang
/// so script executability is preserved.
pub fn with_managed_header(content: &str) -> String {
    with_managed_header_with_prefix(content, "#")
        .expect("default managed header prefix should always be supported")
}

/// Prefixes content with a managed marker using the provided comment prefix.
///
/// If content starts with a shebang (`#!`), the marker is inserted after the shebang
/// so script executability is preserved.
///
/// Supported prefixes are `#` (Unix-style) and `REM` (Windows batch).
pub fn with_managed_header_with_prefix(
    content: &str,
    comment_prefix: &str,
) -> Result<String, Error> {
    if !matches!(comment_prefix, "#" | "REM") {
        return Err(Error::UnsupportedManagedHeaderPrefix {
            prefix: comment_prefix.to_string(),
        });
    }
    let marker_line = format!("{comment_prefix} {MANAGED_FILE_MARKER}");
    if content.starts_with("#!") {
        if let Some(shebang_end) = content.find('\n') {
            let (shebang, rest) = content.split_at(shebang_end + 1);
            return Ok(format!("{shebang}{marker_line}\n\n{rest}"));
        }

        return Ok(format!("{content}\n{marker_line}\n\n"));
    }

    Ok(format!("{marker_line}\n\n{content}"))
}

pub(crate) fn ensure_can_write_config_file(
    config_file: &Path,
    force_overwrite: bool,
) -> Result<(), Error> {
    ensure_not_symlink(config_file)?;

    if config_file.exists() && !config_file.is_file() {
        return Err(Error::ConfigPathNotAFile {
            path: config_file.to_string_lossy().to_string(),
        });
    }

    if !config_file.exists() || force_overwrite {
        return Ok(());
    }

    let path = config_file.to_string_lossy().to_string();
    if is_managed_file(config_file)? {
        return Err(Error::RefusingToOverwriteManagedConfigFile { path });
    }

    Err(Error::RefusingToOverwriteUnmanagedConfigFile { path })
}

pub(crate) fn ensure_not_symlink(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::RefusingToWriteSymlink {
            path: path.to_string_lossy().to_string(),
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::FailedToReadExistingFile {
            path: path.to_string_lossy().to_string(),
            source,
        }),
    }
}

/// Returns true when a file has git-smee's managed marker in its header.
///
/// The marker must appear in the same header position accepted by installer
/// overwrite/pruning logic; marker text later in a hook body is treated as
/// user-owned content.
pub fn has_managed_header(path: &Path) -> Result<bool, Error> {
    is_managed_file(path)
}

pub(crate) fn is_managed_file(path: &Path) -> Result<bool, Error> {
    let mut file = fs::File::open(path).map_err(|source| Error::FailedToReadExistingFile {
        path: path.to_string_lossy().to_string(),
        source,
    })?;
    let mut header_buf = [0_u8; MANAGED_FILE_SCAN_BYTES];
    let bytes_read =
        file.read(&mut header_buf)
            .map_err(|source| Error::FailedToReadExistingFile {
                path: path.to_string_lossy().to_string(),
                source,
            })?;
    let header = &header_buf[..bytes_read];
    let marker_hash = format!("# {MANAGED_FILE_MARKER}");
    let marker_rem = format!("REM {MANAGED_FILE_MARKER}");

    for line in header
        .split(|byte| *byte == b'\n')
        .take(MANAGED_FILE_SCAN_LINES)
    {
        let normalized_line = line.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(line);
        let normalized_line = normalized_line
            .strip_suffix(b"\r")
            .unwrap_or(normalized_line);
        if normalized_line.is_empty() {
            continue;
        }
        if normalized_line == marker_hash.as_bytes() || normalized_line == marker_rem.as_bytes() {
            return Ok(true);
        }
    }

    Ok(false)
}
