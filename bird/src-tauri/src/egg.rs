//! Egg packaging — compress local save folders into zipped Eggs and unpack
//! downloaded Eggs back onto disk.

use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

use crate::error::{BirdError, BirdResult};

/// Package a save directory into a `.zip` Egg and return the bytes plus a
/// SHA-256 hash.
pub fn package_directory(path: &Path) -> BirdResult<(Vec<u8>, String)> {
    if !path.exists() {
        return Err(BirdError::SavePathNotFound(path.to_path_buf()));
    }

    let mut writer = Cursor::new(Vec::new());
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(&mut writer);

    for entry in WalkDir::new(path)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(path).unwrap_or(entry.path());
            let name = rel.to_string_lossy().replace('\\', "/");
            zip.start_file(name, options)?;

            let mut file = File::open(entry.path())?;
            let mut buf = [0u8; 8192];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n])?;
            }
        }
    }

    zip.finish()?;
    let bytes = writer.into_inner();
    let hash = hash_bytes(&bytes);
    Ok((bytes, hash))
}

/// Unpack a downloaded Egg into `dest`.
///
/// Any existing contents at `dest` are **not** removed by this function; callers
/// should rename or clear the directory before calling when replacing a save.
pub fn unpack_to_directory(zip_bytes: &[u8], dest: &Path) -> BirdResult<()> {
    let reader = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.enclosed_name().ok_or_else(|| {
            BirdError::Archive(format!("invalid path in archive: {:?}", file.name()))
        })?;
        let out_path = dest.join(name);

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&out_path)?;
            std::io::copy(&mut file, &mut out)?;
        }
    }

    Ok(())
}

/// SHA-256 hash of a byte slice, formatted as lower-case hex.
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Replace `save_path` with the contents of a downloaded Egg, keeping a
/// timestamped backup of the previous directory.
pub fn replace_with_egg(zip_bytes: &[u8], save_path: &Path) -> BirdResult<PathBuf> {
    if !save_path.exists() {
        std::fs::create_dir_all(save_path)?;
        unpack_to_directory(zip_bytes, save_path)?;
        return Ok(save_path.to_path_buf());
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let file_name = save_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "save".to_string());
    let backup = save_path
        .parent()
        .unwrap_or(save_path)
        .join(format!("{file_name}.nest-bak-{stamp}"));

    std::fs::rename(save_path, &backup)?;
    std::fs::create_dir_all(save_path)?;

    if let Err(err) = unpack_to_directory(zip_bytes, save_path) {
        // Restore the backup on extraction failure.
        let _ = std::fs::remove_dir_all(save_path);
        let _ = std::fs::rename(&backup, save_path);
        return Err(err);
    }

    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_and_unpack_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("nest-egg-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("nested/b.txt"), b"world").unwrap();

        let (bytes, hash) = package_directory(&src).unwrap();
        assert!(!bytes.is_empty());
        assert!(!hash.is_empty());

        std::fs::create_dir_all(&dst).unwrap();
        unpack_to_directory(&bytes, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            std::fs::read_to_string(dst.join("nested/b.txt")).unwrap(),
            "world"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }
}
