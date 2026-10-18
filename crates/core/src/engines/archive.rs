//! Archives through bsdtar (libarchive ships with macOS and reads ZIP, RAR,
//! 7z, TAR, ISO…), plus a pure-Rust path for bare `.gz` files.

use super::{exec, scratch};
use crate::kind::ext;
use crate::output::{stem, unique_path};
use crate::{output_path, Options, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const TAR: &str = "/usr/bin/tar";

/// Pack `inputs` (files or folders) into one `.zip` or `.tar.gz`.
pub fn create(inputs: &[PathBuf], format: &str, opts: &Options) -> Result<PathBuf> {
    let out = if inputs.len() == 1 {
        output_path(&inputs[0], "", format, opts)
    } else {
        let dir = opts
            .out_dir
            .clone()
            .or_else(|| inputs[0].parent().map(Path::to_path_buf))
            .unwrap_or_default();
        unique_path(&dir.join(format!("Archive.{format}")))
    };
    let mut args: Vec<OsString> = vec![
        "--no-mac-metadata".into(),
        "--exclude".into(),
        ".DS_Store".into(),
    ];
    args.push(if format == "zip" {
        "--format=zip".into()
    } else {
        "-z".into()
    });
    args.push("-cf".into());
    args.push(out.clone().into());
    for input in inputs {
        let input = input.canonicalize()?;
        let parent = input.parent().unwrap_or(Path::new("/"));
        args.push("-C".into());
        args.push(parent.into());
        args.push(input.file_name().unwrap_or_default().into());
    }
    exec(TAR, &args)?;
    Ok(out)
}

/// Extract next to the archive. A lone top-level entry is moved up, so
/// `photos.zip` containing `photos/` yields `photos/`, not `photos/photos/`.
pub fn extract(input: &Path, opts: &Options) -> Result<PathBuf> {
    let parent = opts
        .out_dir
        .clone()
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let e = ext(input);
    if e == "gz" {
        let out = unique_path(&parent.join(stem(input)));
        let mut decoder = flate2::read::GzDecoder::new(std::fs::File::open(input)?);
        std::io::copy(&mut decoder, &mut std::fs::File::create(&out)?)?;
        return Ok(out);
    }
    let dest = unique_path(&parent.join(stem(input)));
    std::fs::create_dir_all(&dest)?;
    if let Err(e) = exec(
        TAR,
        [
            OsString::from("-xf"),
            input.into(),
            "-C".into(),
            dest.clone().into(),
        ],
    ) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(&dest)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n != "__MACOSX" && n != ".DS_Store")
        })
        .collect();
    if let [only] = entries.as_slice() {
        let name = only.file_name().unwrap_or_default().to_owned();
        let tmp = unique_path(&parent.join(format!(".satsuma-{}", std::process::id())));
        std::fs::rename(only, &tmp)?;
        std::fs::remove_dir_all(&dest)?;
        let lifted = unique_path(&parent.join(name));
        std::fs::rename(&tmp, &lifted)?;
        return Ok(lifted);
    }
    Ok(dest)
}

/// Re-pack an archive in another format, e.g. RAR → ZIP.
pub fn repack(input: &Path, format: &str, opts: &Options) -> Result<PathBuf> {
    let tmp = scratch()?;
    let inner = Options {
        out_dir: Some(tmp.path().to_path_buf()),
        ..opts.clone()
    };
    let extracted = extract(input, &inner)?;
    let packed = create(&[extracted], format, &inner)?;
    let out = output_path(input, "", format, opts);
    std::fs::copy(&packed, &out)?;
    Ok(out)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn zip_extract_and_repack() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("project");
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        std::fs::write(folder.join("a.txt"), "alpha").unwrap();
        std::fs::write(folder.join("sub/b.txt"), "beta").unwrap();
        let opts = Options::default();

        let zip = create(std::slice::from_ref(&folder), "zip", &opts).unwrap();
        assert_eq!(zip, dir.path().join("project.zip"));
        let tgz = repack(&zip, "tar.gz", &opts).unwrap();
        assert!(tgz.ends_with("project.tar.gz"));

        std::fs::remove_dir_all(&folder).unwrap();
        let extracted = extract(&tgz, &opts).unwrap();
        assert_eq!(extracted, folder);
        assert_eq!(
            std::fs::read_to_string(folder.join("sub/b.txt")).unwrap(),
            "beta"
        );

        let many = create(&[folder.join("a.txt"), folder.join("sub")], "zip", &opts).unwrap();
        assert!(many.ends_with("Archive.zip"));
        let out = extract(&many, &opts).unwrap();
        assert!(out.join("a.txt").exists() && out.join("sub/b.txt").exists());
    }
}
