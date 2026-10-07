//! The `merge` and `validate` commands over the flat `results/` directory.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anstream::println;
use serde::Serialize;

use crate::schema::{self, RunFile};
use crate::ui::{BAD, BOLD, GOOD};

/// The file the website loads.
#[derive(Serialize)]
struct Site<'a> {
    generated: String,
    runs: &'a [RunFile],
}

pub fn merge(results: &Path, out: &Path) -> ExitCode {
    let files = match result_files(results) {
        Ok(files) => files,
        Err(e) => {
            println!("{BAD}error:{BAD:#} {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut runs = Vec::new();
    for file in &files {
        match schema::load(file) {
            Ok(run) => runs.push(run),
            Err(e) => {
                println!("{BAD}invalid{BAD:#} {}: {e}", file.display());
                return ExitCode::FAILURE;
            }
        }
    }
    runs.sort_by(|a, b| a.meta.date.cmp(&b.meta.date));

    let site = Site {
        generated: crate::machine::now_rfc3339(),
        runs: &runs,
    };
    let json = serde_json::to_string(&site).expect("serializable");
    if let Err(e) = std::fs::write(out, json) {
        println!("{BAD}error:{BAD:#} {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    println!(
        "Wrote {} runs to {BOLD}{}{BOLD:#}",
        runs.len(),
        out.display()
    );
    ExitCode::SUCCESS
}

pub fn validate(paths: &[PathBuf]) -> ExitCode {
    let default = [PathBuf::from("results")];
    let paths = if paths.is_empty() {
        &default[..]
    } else {
        paths
    };

    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            match result_files(path) {
                Ok(found) => files.extend(found),
                Err(e) => {
                    println!("{BAD}invalid{BAD:#} {e}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            files.push(path.clone());
        }
    }

    let mut ok = true;
    for file in &files {
        match schema::load(file) {
            Ok(_) => println!("{GOOD}valid{GOOD:#}   {}", file.display()),
            Err(e) => {
                ok = false;
                println!("{BAD}invalid{BAD:#} {}: {e}", file.display());
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Every `.json` file in `dir`, which must hold nothing else.
fn result_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() || path.extension().is_none_or(|e| e != "json") {
            return Err(format!(
                "{}: only .json files belong in {}, without subdirectories",
                path.display(),
                dir.display()
            ));
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}
