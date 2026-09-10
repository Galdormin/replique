use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use walkdir::WalkDir;

use crate::EXTENSIONS;

pub fn discover(paths: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();

    // Take . if not path were given
    let paths = if paths.is_empty() {
        &[PathBuf::from(".")][..]
    } else {
        paths
    };

    for p in paths {
        if !p.exists() {
            bail!("Cannot find path: {}", p.display());
        }

        if p.is_file() {
            if is_rep_file(p) {
                out.push(p.clone());
            }
            continue;
        }

        let walk = if recursive {
            WalkDir::new(p)
        } else {
            WalkDir::new(p).max_depth(1)
        };

        for entry in walk {
            // `WalkDir` also yields the directories: a folder named
            // `dialogue.rep` must not end up in the list.
            let entry = entry?;
            if entry.file_type().is_file() && is_rep_file(entry.path()) {
                out.push(entry.into_path());
            }
        }
    }

    out.sort();
    out.dedup();

    Ok(out)
}

/// Whether the file carries one of the [`EXTENSIONS`]
fn is_rep_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}
