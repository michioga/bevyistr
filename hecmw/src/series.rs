//! Auto-detection and batch loading of FrontISTR result file series.
//!
//! FrontISTR writes one result file per output step:
//!
//! ```text
//! job.res.0.1
//! job.res.0.2
//! job.res.0.3
//! …
//! ```
//!
//! [`detect_series`] finds all siblings of a given file that share its base
//! name and differ only in the trailing step number.  [`load_series`] loads
//! them all in order and returns a `Vec<StepResult>`.

use std::path::{Path, PathBuf};

use fem_core::{NodeId, StepResult};

use crate::{load_result_file, ResultLoadError};

/// Detects sibling files that belong to the same FrontISTR result series as
/// `path`, including `path` itself.
///
/// `path` is expected to end with a step-number suffix separated by `.`,
/// e.g. `job.res.0.3`.  The function strips the last `.N` component to form
/// the base pattern (`job.res.0`), then scans the parent directory for files
/// whose names start with that base and whose remaining suffix is a valid
/// `u32`.  The returned list is sorted in ascending step-number order.
///
/// Returns `vec![path.to_owned()]` when no siblings are found or when the
/// pattern cannot be inferred (so single-file loading still works).
pub fn detect_series(path: &Path) -> Vec<PathBuf> {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return vec![path.to_owned()];
    };

    let Some(last_dot) = file_name.rfind('.') else {
        return vec![path.to_owned()];
    };

    let base     = &file_name[..last_dot];
    let step_str = &file_name[last_dot + 1..];

    if step_str.parse::<u32>().is_err() {
        return vec![path.to_owned()];
    }

    let dir = match path.parent() {
        Some(d) => d,
        None    => Path::new("."),
    };

    let Ok(entries) = dir.read_dir() else {
        return vec![path.to_owned()];
    };

    let prefix = format!("{base}.");

    let mut series: Vec<(u32, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name();
            let name_str = name.to_str()?.to_owned();

            if !name_str.starts_with(&prefix) {
                return None;
            }

            let suffix = &name_str[prefix.len()..];
            let step   = suffix.parse::<u32>().ok()?;

            Some((step, e.path()))
        })
        .collect();

    if series.is_empty() {
        return vec![path.to_owned()];
    }

    series.sort_by_key(|(step, _)| *step);

    series.into_iter().map(|(_, p)| p).collect()
}

/// Loads every file in the series rooted at `any_file` and returns the steps
/// in ascending order.
///
/// Steps that cannot be parsed are silently skipped.  Returns an error only
/// if the *first* file in the series fails — indicating that the base format
/// is wrong (not just a gap in the sequence).
pub fn load_series(
    any_file: &Path,
    node_ids: &[NodeId],
) -> Result<Vec<StepResult>, ResultLoadError> {
    let paths = detect_series(any_file);

    let mut steps = Vec::with_capacity(paths.len());
    let mut first = true;

    for path in &paths {
        match load_result_file(path, node_ids) {
            Ok(step) => steps.push(step),
            Err(err) => {
                if first {
                    return Err(err);
                }
                // Later files may simply not exist yet (e.g. in progress) —
                // log and continue.
                bevy::log::warn!(
                    "Skipping result file {:?}: {err}",
                    path.file_name().unwrap_or_default()
                );
            }
        }
        first = false;
    }

    Ok(steps)
}
