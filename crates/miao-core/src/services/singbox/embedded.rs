use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ruzstd::decoding::StreamingDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::{
    is_windows_sharing_violation, map_remove_embedded_error, restrict_to_owner, set_executable,
    sing_box_file_name,
};
use crate::error::{AppError, AppResult};

struct EmbeddedKernel {
    compressed: &'static [u8],
    metadata: &'static str,
}

macro_rules! embedded_kernel {
    ($name:literal) => {
        EmbeddedKernel {
            compressed: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../embedded/",
                $name,
                ".zst"
            )),
            metadata: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../embedded/",
                $name,
                ".meta.json"
            )),
        }
    };
}

#[cfg(all(windows, target_arch = "x86_64"))]
const KERNEL: EmbeddedKernel = embedded_kernel!("sing-box-windows-amd64.exe");

#[cfg(all(windows, target_arch = "aarch64"))]
compile_error!("Windows arm64 is not supported yet");

#[cfg(all(not(windows), target_arch = "x86_64"))]
const KERNEL: EmbeddedKernel = embedded_kernel!("sing-box-amd64");

#[cfg(all(not(windows), target_arch = "aarch64"))]
const KERNEL: EmbeddedKernel = embedded_kernel!("sing-box-arm64");

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("Unsupported architecture: only x86_64 and aarch64 are supported");

const IP_RULE_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/geoip-cn.srs"
));
const SITE_RULE_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/geosite-geolocation-cn.srs"
));

#[derive(Deserialize)]
struct KernelMetadata {
    schema_version: u32,
    binary: BinaryMetadata,
    compression: CompressionMetadata,
}

#[derive(Deserialize)]
struct BinaryMetadata {
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct CompressionMetadata {
    format: String,
}

/// Decode directly to a sibling file, then atomically replace the old inode.
/// Corruption, a truncated archive or an I/O failure leaves the old kernel
/// intact. Streaming also avoids allocating a second full kernel on OpenWrt.
fn unpack_kernel(compressed: &[u8], metadata: &str, path: &Path) -> AppResult<()> {
    let metadata: KernelMetadata = serde_json::from_str(metadata)
        .map_err(|err| AppError::context("Invalid embedded kernel metadata", err))?;
    if metadata.schema_version != 1
        || metadata.compression.format != "zstd"
        || metadata.binary.bytes == 0
    {
        return Err(AppError::message("Unsupported embedded kernel metadata"));
    }
    let mut expected_hash = [0u8; 32];
    hex::decode_to_slice(&metadata.binary.sha256, &mut expected_hash)
        .map_err(|err| AppError::message(format!("Invalid embedded kernel SHA-256: {err}")))?;
    let directory = path
        .parent()
        .ok_or_else(|| AppError::message("Embedded kernel has no destination directory"))?;
    let mut decoder = StreamingDecoder::new(compressed).map_err(|err| {
        AppError::context(
            "Failed to open compressed kernel",
            std::io::Error::other(err),
        )
    })?;
    let mut output = tempfile::NamedTempFile::new_in(directory)
        .map_err(|err| AppError::context("Failed to prepare embedded kernel file", err))?;
    let mut hash = Sha256::new();
    let mut written = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = decoder
            .read(&mut buffer)
            .map_err(|err| AppError::context("Failed to decompress embedded kernel", err))?;
        if count == 0 {
            break;
        }
        written += count as u64;
        if written > metadata.binary.bytes {
            return Err(AppError::message(
                "Embedded kernel exceeds its expected size",
            ));
        }
        hash.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|err| AppError::context("Failed to write embedded kernel", err))?;
    }
    let actual_hash: [u8; 32] = hash.finalize().into();
    if written != metadata.binary.bytes || actual_hash != expected_hash {
        return Err(AppError::message(
            "Embedded kernel size or SHA-256 mismatch",
        ));
    }
    if !decoder.into_inner().is_empty() {
        return Err(AppError::message(
            "Unexpected trailing data in compressed kernel",
        ));
    }
    set_executable(output.path())
        .map_err(|err| AppError::context("Failed to set permissions on embedded kernel", err))?;
    output
        .as_file()
        .sync_all()
        .map_err(|err| AppError::context("Failed to flush embedded kernel", err))?;
    output.persist(path).map_err(|err| {
        if is_windows_sharing_violation(&err.error) {
            map_remove_embedded_error(sing_box_file_name(), err.error)
        } else {
            AppError::context("Failed to install embedded kernel", err.error)
        }
    })?;
    Ok(())
}

pub fn extract_sing_box_to(directory: &Path) -> AppResult<PathBuf> {
    fs::create_dir_all(directory)
        .map_err(|err| AppError::context("Failed to create sing-box home directory", err))?;
    // Runtime configs contain subscription credentials. As before, correct
    // permissions on every extraction and warn if this filesystem cannot do so.
    if let Err(err) = restrict_to_owner(directory) {
        warn!(error = %err, path = ?directory, "Failed to restrict sing-box home permissions");
    }
    let path = directory.join(sing_box_file_name());
    info!(?path, "Extracting embedded kernel");
    unpack_kernel(KERNEL.compressed, KERNEL.metadata, &path)?;

    for (name, bytes) in [
        ("chinaip.srs", IP_RULE_BINARY),
        ("chinasite.srs", SITE_RULE_BINARY),
    ] {
        let path = directory.join(name);
        if path.exists() {
            fs::remove_file(&path).map_err(|err| map_remove_embedded_error(name, err))?;
        }
        fs::write(&path, bytes).map_err(|err| {
            AppError::context(format!("Failed to write embedded file {name}"), err)
        })?;
    }
    // Keep config/cache and selection state across extraction, as before.
    let _ = fs::remove_file(directory.join("adblock_reject.srs"));
    fs::create_dir_all(directory.join("dashboard"))
        .map_err(|err| AppError::context("Failed to create sing-box dashboard directory", err))?;
    Ok(directory.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruzstd::encoding::{compress_to_vec, CompressionLevel};

    fn fixture(bytes: &[u8]) -> (Vec<u8>, String) {
        let compressed = compress_to_vec(bytes, CompressionLevel::Fastest);
        let metadata = serde_json::json!({
            "schema_version": 1,
            "binary": {"bytes": bytes.len(), "sha256": hex::encode(Sha256::digest(bytes))},
            "compression": {"format": "zstd"}
        });
        (compressed, metadata.to_string())
    }

    #[test]
    fn corrupt_archives_leave_previous_kernel_intact_and_clean_up() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(sing_box_file_name());
        fs::write(&path, b"previous kernel").unwrap();
        let (compressed, metadata) = fixture(b"replacement kernel");
        let mut wrong_hash: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        wrong_hash["binary"]["sha256"] = serde_json::json!("00".repeat(32));
        let mut wrong_size: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        wrong_size["binary"]["bytes"] = serde_json::json!(1);
        let mut trailing = compressed.clone();
        trailing.extend_from_slice(b"unexpected second frame");
        for (bytes, manifest) in [
            (b"not zstd".as_slice(), metadata.clone()),
            (&compressed[..compressed.len() - 1], metadata.clone()),
            (compressed.as_slice(), wrong_hash.to_string()),
            (compressed.as_slice(), wrong_size.to_string()),
            (trailing.as_slice(), metadata.clone()),
            (compressed.as_slice(), "{}".to_string()),
        ] {
            assert!(unpack_kernel(bytes, &manifest, &path).is_err());
            assert_eq!(fs::read(&path).unwrap(), b"previous kernel");
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn streaming_replacement_preserves_unrelated_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(sing_box_file_name());
        fs::write(&path, b"previous kernel").unwrap();
        fs::write(directory.path().join("cache.db"), b"keep cache").unwrap();
        #[cfg(unix)]
        let mut previous_inode = fs::File::open(&path).unwrap();
        let bytes: Vec<u8> = (0..300_000).map(|i| (i % 251) as u8).collect();
        let (compressed, metadata) = fixture(&bytes);
        unpack_kernel(&compressed, &metadata, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        #[cfg(unix)]
        {
            let mut previous_bytes = Vec::new();
            previous_inode.read_to_end(&mut previous_bytes).unwrap();
            assert_eq!(previous_bytes, b"previous kernel");
        }
        assert_eq!(
            fs::read(directory.path().join("cache.db")).unwrap(),
            b"keep cache"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
    }

    #[test]
    fn bundled_asset_decodes_and_preserves_runtime_cache() {
        // CI and local builds use real kernels; fresh-clone tests may use
        // inert assets. Check Bun -> Rust interoperability without execution.
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("cache.db"), b"keep cache").unwrap();
        fs::write(directory.path().join("adblock_reject.srs"), b"obsolete").unwrap();
        extract_sing_box_to(directory.path()).unwrap();
        let metadata: KernelMetadata = serde_json::from_str(KERNEL.metadata).unwrap();
        let bytes = fs::read(directory.path().join(sing_box_file_name())).unwrap();
        assert_eq!(bytes.len() as u64, metadata.binary.bytes);
        assert_eq!(hex::encode(Sha256::digest(&bytes)), metadata.binary.sha256);
        assert_eq!(
            fs::read(directory.path().join("cache.db")).unwrap(),
            b"keep cache"
        );
        assert_eq!(
            fs::read(directory.path().join("chinaip.srs")).unwrap(),
            IP_RULE_BINARY
        );
        assert_eq!(
            fs::read(directory.path().join("chinasite.srs")).unwrap(),
            SITE_RULE_BINARY
        );
        assert!(!directory.path().join("adblock_reject.srs").exists());
    }

    #[cfg(windows)]
    #[test]
    fn locked_windows_kernel_is_preserved_with_a_clear_error() {
        use std::os::windows::fs::OpenOptionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(sing_box_file_name());
        fs::write(&path, b"previous kernel").unwrap();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let (compressed, metadata) = fixture(b"replacement kernel");
        let error = unpack_kernel(&compressed, &metadata, &path).unwrap_err();
        assert!(error.to_string().contains("残留的 sing-box 仍在运行"));
        drop(locked);
        assert_eq!(fs::read(&path).unwrap(), b"previous kernel");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
