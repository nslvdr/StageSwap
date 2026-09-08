//! Validation of packaged executables and their embedded media-source payload.
use super::RELEASE_PREFIX;
use anyhow::{Context, Result, bail};
use stageswap_core::release::ReleaseVersion;
use std::fs;
use std::path::Path;

pub(super) fn validate_release_outputs(artifact: &Path, version: ReleaseVersion) -> Result<()> {
    let expected_name = format!("{RELEASE_PREFIX}{version}.exe");
    if artifact.file_name().and_then(|name| name.to_str()) != Some(&expected_name) {
        bail!("packaged executable name does not match {expected_name}");
    }
    let checksum = artifact.with_file_name(format!("{expected_name}.sha256"));
    if !checksum.is_file() {
        bail!("packaging did not produce {}", checksum.display());
    }
    Ok(())
}

pub(super) fn validate_embedded_payload(executable: &Path, payload: &Path) -> Result<()> {
    let executable_bytes =
        fs::read(executable).with_context(|| format!("could not read {}", executable.display()))?;
    let payload_bytes =
        fs::read(payload).with_context(|| format!("could not read {}", payload.display()))?;
    let marker_len = payload_bytes.len().min(64);
    if marker_len == 0 {
        bail!("embedded media-source payload is empty");
    }
    let marker = &payload_bytes[..marker_len];
    let embedded = executable_bytes
        .windows(marker_len)
        .enumerate()
        .any(|(offset, candidate)| {
            candidate == marker
                && executable_bytes.get(offset..offset + payload_bytes.len())
                    == Some(payload_bytes.as_slice())
        });
    if !embedded {
        bail!("executable does not contain the media-source payload");
    }
    Ok(())
}

pub(super) fn validate_pe(path: String, architecture: String) -> Result<()> {
    validate_pe_path(Path::new(&path), &architecture)?;
    println!("validated {architecture} PE: {path}");
    Ok(())
}

pub(super) fn validate_pe_path(path: &Path, architecture: &str) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    if bytes.get(..2) != Some(b"MZ") {
        bail!("{} is not a PE file", path.display());
    }
    let pe_offset = u32::from_le_bytes(
        bytes
            .get(0x3c..0x40)
            .context("truncated DOS header")?
            .try_into()?,
    ) as usize;
    if bytes.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
        bail!("{} has no PE signature", path.display());
    }
    let machine = u16::from_le_bytes(
        bytes
            .get(pe_offset + 4..pe_offset + 6)
            .context("truncated COFF header")?
            .try_into()?,
    );
    let expected = match architecture {
        "x64" => 0x8664,
        _ => bail!("architecture must be x64"),
    };
    if machine != expected {
        bail!("PE machine 0x{machine:04x} does not match {architecture}");
    }
    Ok(())
}
