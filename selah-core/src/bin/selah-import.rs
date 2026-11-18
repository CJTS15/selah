//! Builds a translation database from a directory of USFM files and `meta.txt`.
//!
//! Usage: selah-import <source-dir> <output.sqlite>

use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs};

use selah_core::{import, store};

fn main() -> ExitCode {
    let args: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    let [source, output] = args.as_slice() else {
        eprintln!("usage: selah-import <source-dir> <output.sqlite>");
        return ExitCode::from(2);
    };

    // Write next to the output and rename, so a failed import leaves no partial file.
    let partial = output.with_extension("partial");
    let result = fs::remove_file(&partial)
        .or_else(|err| match err.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(err),
        })
        .map_err(selah_core::Error::from)
        .and_then(|()| import::import_dir(source))
        .and_then(|(meta, books)| store::create(&partial, &meta, &books))
        .and_then(|()| fs::rename(&partial, output).map_err(Into::into));

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = fs::remove_file(&partial);
            eprintln!("selah-import: {err}");
            ExitCode::FAILURE
        }
    }
}
