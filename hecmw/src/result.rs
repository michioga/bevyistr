//! Parser for FrontISTR/HECMW ASCII result files (`.res.0.1`, `.res.0.2`, …).
//!
//! # FrontISTR result file format
//!
//! Each result file has the structure:
//!
//! ```text
//! *comment lines starting with !
//! 1  step_id  time
//! node_id  val1  val2  ...    (one line per node)
//! ...
//! ```
//!
//! A header comment block is usually present. The first non-comment, non-blank
//! line gives the step number and time. Subsequent lines are node results.
//!
//! FrontISTR typically writes one `.res.0.N` file per output step. The
//! number of value columns per node depends on the analysis type:
//! * Linear static / non-linear static: 3 columns (Ux, Uy, Uz)
//! * Heat: 1 column (temperature)
//! * Eigenvalue: N_modes × 3 columns
//!
//! This parser auto-detects the column count from the first data line and
//! exposes:
//! * a `Displacement` `NodeVector` field (first 3 columns) plus a `|U|`
//!   scalar magnitude, when exactly 3 columns are present (linear/
//!   non-linear static).
//! * one `Displacement (mode N)` / `|U| (mode N)` pair per mode when the
//!   column count is a multiple of 3 greater than 3 (eigenvalue analysis).
//! * a raw scalar `Temperature` field when exactly 1 column is found (heat
//!   analysis).

use std::collections::HashMap;
use std::io;
use std::path::Path;

use bevy::prelude::Vec3;
use fem_core::{NodeId, ResultField, StepResult};

// ─── public API ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ResultLoadError {
    Io(io::Error),

    Parse {
        line: usize,
        message: String,
    },

    /// The file was read successfully but contained no node data rows.
    Empty,
}

impl std::fmt::Display for ResultLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "IO: {err}"),
            Self::Parse { line, message } => write!(f, "line {line}: {message}"),
            Self::Empty => write!(f, "result file contains no node data"),
        }
    }
}

impl std::error::Error for ResultLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(err) = self { Some(err) } else { None }
    }
}

impl From<io::Error> for ResultLoadError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Loads one FrontISTR result file and returns a [`StepResult`].
///
/// `node_ids` must be the `FemMesh::nodes` slice (in order) so the parser
/// can build properly ordered [`ResultField`]s.
pub fn load_result_file(
    path: impl AsRef<Path>,
    node_ids: &[NodeId],
) -> Result<StepResult, ResultLoadError> {
    let text = std::fs::read_to_string(path.as_ref())?;

    parse_result_str(&text, node_ids)
}

/// Parses a FrontISTR result file from a string.
pub fn parse_result_str(
    source: &str,
    node_ids: &[NodeId],
) -> Result<StepResult, ResultLoadError> {
    let mut lines = source.lines().enumerate();

    // Skip comment / blank lines until the step header.
    let (step, time) = loop {
        let Some((line_no, line)) = lines.next() else {
            return Err(ResultLoadError::Empty);
        };

        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('!') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        if parts.len() >= 2 {
            let step: u32 = parts[0].parse().unwrap_or(0);
            let time: f32 = parts[1].parse().map_err(|_| ResultLoadError::Parse {
                line: line_no + 1,
                message: format!("expected step header, got {:?}", trimmed),
            })?;

            break (step, time);
        }
    };

    // Parse node data lines. Every node's raw values are kept (not just the
    // first 3) because a multi-mode eigenvalue result packs N_modes × 3
    // columns per node — collapsing to the first 3 would silently keep
    // only mode 1 and discard the rest, contradicting this module's own
    // documented support for eigenvalue results (see the module doc above).
    let mut node_values: HashMap<NodeId, Vec<f32>> = HashMap::new();
    let mut cols_detected: Option<usize> = None;

    for (line_no, line) in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('!') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        // First field is node id; rest are values.
        if parts.len() < 2 {
            continue;
        }

        let node_id: u32 = parts[0].parse().map_err(|_| ResultLoadError::Parse {
            line: line_no + 1,
            message: format!("expected node id, got {:?}", parts[0]),
        })?;

        let values: Vec<f32> = parts[1..]
            .iter()
            .map(|s| s.parse::<f32>().unwrap_or(0.0))
            .collect();

        if cols_detected.is_none() {
            cols_detected = Some(values.len());
        }

        node_values.insert(NodeId(node_id), values);
    }

    let col_count = cols_detected.unwrap_or(0);

    if col_count == 0 {
        return Err(ResultLoadError::Empty);
    }

    let mut fields = Vec::new();

    if col_count == 1 {
        // Heat analysis: temperature scalar.
        let temp_map: HashMap<NodeId, f32> = node_values
            .iter()
            .filter_map(|(&id, v)| v.first().map(|&t| (id, t)))
            .collect();

        fields.push(ResultField::node_scalar("Temperature", node_ids, &temp_map));
    } else if col_count >= 3 && col_count % 3 == 0 {
        // Static/non-linear (1 mode) or eigenvalue (N modes): one
        // Displacement vector + |U| magnitude per mode. Single-mode keeps
        // the plain "Displacement"/"|U|" names so existing lookups (e.g.
        // deformed-shape rendering's `displacement_field: "Displacement"`)
        // keep working unchanged.
        let n_modes = col_count / 3;

        for mode in 0..n_modes {
            let base = mode * 3;

            let disp_map: HashMap<NodeId, Vec3> = node_values
                .iter()
                .filter_map(|(&id, v)| {
                    if v.len() >= base + 3 {
                        Some((id, Vec3::new(v[base], v[base + 1], v[base + 2])))
                    } else {
                        None
                    }
                })
                .collect();

            let mag_map: HashMap<NodeId, f32> = disp_map
                .iter()
                .map(|(&id, &v)| (id, v.length()))
                .collect();

            let (disp_name, mag_name) = if n_modes == 1 {
                ("Displacement".to_string(), "|U|".to_string())
            } else {
                (format!("Displacement (mode {})", mode + 1), format!("|U| (mode {})", mode + 1))
            };

            fields.push(ResultField::node_vector(disp_name, node_ids, &disp_map));
            fields.push(ResultField::node_scalar(mag_name, node_ids, &mag_map));
        }
    } else {
        // Column count doesn't match a known layout (not 1, not a clean
        // multiple of 3) — fall back to the first 3 columns as a
        // displacement-like vector rather than failing outright, since
        // that's still usually the most useful interpretation available.
        let disp_map: HashMap<NodeId, Vec3> = node_values
            .iter()
            .filter_map(|(&id, v)| {
                if v.len() >= 3 { Some((id, Vec3::new(v[0], v[1], v[2]))) } else { None }
            })
            .collect();

        let mag_map: HashMap<NodeId, f32> = disp_map
            .iter()
            .map(|(&id, &v)| (id, v.length()))
            .collect();

        fields.push(ResultField::node_vector("Displacement", node_ids, &disp_map));
        fields.push(ResultField::node_scalar("|U|", node_ids, &mag_map));
    }

    Ok(StepResult { step, time, fields })
}
