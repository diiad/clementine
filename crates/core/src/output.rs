//! Naming of output files.

use crate::Options;
use std::path::{Path, PathBuf};

/// Build the destination for `input` with a new extension and an optional
/// suffix (`photo.png` + `-small` + `jpg` → `photo-small.jpg`). Never returns
/// a path that already exists.
pub fn output_path(input: &Path, suffix: &str, ext: &str, opts: &Options) -> PathBuf {
    let dir = opts
        .out_dir
        .clone()
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = stem(input);
    let name = if ext.is_empty() {
        format!("{stem}{suffix}")
    } else {
        format!("{stem}{suffix}.{ext}")
    };
    unique_path(&dir.join(name))
}

/// File name without any extension, `.tar.gz` style double extensions included.
pub fn stem(input: &Path) -> String {
    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    if input.is_dir() {
        return name;
    }
    let lower = name.to_lowercase();
    for double in [".tar.gz", ".tar.bz2", ".tar.xz"] {
        if lower.ends_with(double) {
            return name[..name.len() - double.len()].to_string();
        }
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name,
    }
}

/// `file.txt` → `file 2.txt` → `file 3.txt`… until the name is free.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (&name[..], ""),
    };
    (2..)
        .map(|n| dir.join(format!("{base} {n}{ext}")))
        .find(|p| !p.exists())
        .expect("infinite iterator")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_unique_names() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("photo.png");
        let opts = Options::default();
        let first = output_path(&input, "", "jpg", &opts);
        assert_eq!(first, dir.path().join("photo.jpg"));
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(
            output_path(&input, "", "jpg", &opts),
            dir.path().join("photo 2.jpg")
        );
        assert_eq!(stem(Path::new("a/b.tar.gz")), "b");
    }
}
