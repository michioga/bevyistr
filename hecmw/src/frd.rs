//! Parser for CalculiX FRD ASCII result files (`.frd`).
//!
//! FRD is CalculiX's native result format, also readable by CGX (the
//! CalculiX GraphiX post-processor) and PrePoMax.
//!
//! # Supported record types
//!
//! | Code | Description                              |
//! |------|------------------------------------------|
//! | 1    | File header / metadata (skipped)         |
//! | 2C   | Node coordinate block                    |
//! | 3C   | Element topology block (skipped here)    |
//! | 100CL| Result block header                      |
//! | -4   | Field / component definition              |
//! | -1   | Per-node result values                   |
//! | 9999 | End of file                              |
//!
//! This parser reads the `2C` block to build a node-order table that maps
//! 1-based node ids to slice indices, then reads all `100CL` result blocks.
//! Each block becomes one [`StepResult`] containing up to two fields:
//!
//! * A vector field named after the result (e.g. `DISP`) when the component
//!   count is ≥ 3, plus a derived scalar magnitude field (`|DISP|`).
//! * A scalar field when the component count is 1 (e.g. temperature `NT`).
//!
//! Fields with 6 components (stress/strain tensors like `STRESS`) produce a
//! scalar field from the Von-Mises equivalent: `√(½[(σ₁−σ₂)²+(σ₂−σ₃)²+(σ₃−σ₁)²+6(τ₁²+τ₂²+τ₃²)])`.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use bevy::prelude::Vec3;
use fem_core::{NodeId, ResultField, StepResult};

// ─── public API ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum FrdError {
    Io(io::Error),
    MissingNodeBlock,
    Parse { line: usize, message: String },
}

impl std::fmt::Display for FrdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e)                     => write!(f, "IO: {e}"),
            Self::MissingNodeBlock          => write!(f, "FRD file has no node coordinate (2C) block"),
            Self::Parse { line, message }   => write!(f, "line {line}: {message}"),
        }
    }
}

impl std::error::Error for FrdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(e) = self { Some(e) } else { None }
    }
}

impl From<io::Error> for FrdError {
    fn from(e: io::Error) -> Self { Self::Io(e) }
}

/// Loads a CalculiX `.frd` file and returns one [`StepResult`] per result
/// block found in the file.
///
/// `node_ids` must be the `FemMesh::nodes` slice so that result fields are
/// aligned with the mesh.
pub fn load_frd_file(
    path: impl AsRef<Path>,
    node_ids: &[NodeId],
) -> Result<Vec<StepResult>, FrdError> {
    let text = std::fs::read_to_string(path.as_ref())?;

    parse_frd(&text, node_ids)
}

// ─── parser ──────────────────────────────────────────────────────────────────

fn parse_frd(src: &str, node_ids: &[NodeId]) -> Result<Vec<StepResult>, FrdError> {
    let lines: Vec<&str> = src.lines().collect();
    let mut cursor = 0usize;

    // 1. Find and parse 2C node block to get ordering.
    let node_order = parse_node_order(&lines, &mut cursor)?;

    // 2. Parse all 100CL result blocks.
    let mut steps = Vec::new();
    let mut block_index = 1u32;

    while cursor < lines.len() {
        let line = lines[cursor].trim_end();

        if line.starts_with(" 9999") {
            break;
        }

        if line.starts_with("  100CL") || line.starts_with("  100 ") {
            let (step_opt, new_cursor) =
                parse_result_block(&lines, cursor, &node_order, node_ids, block_index);

            cursor = new_cursor;

            if let Some(step) = step_opt {
                steps.push(step);
                block_index += 1;
            }
        } else {
            cursor += 1;
        }
    }

    Ok(steps)
}

/// Returns `HashMap<frd_node_id, slice_index>` from the `2C` block.
fn parse_node_order(
    lines: &[&str],
    cursor: &mut usize,
) -> Result<HashMap<u32, usize>, FrdError> {
    // Find 2C block
    let mut block_start = None;
    for (i, &line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("2C") || (t.len() > 1 && &t[..2] == "2C") {
            block_start = Some(i);
            break;
        }
        // Alternative: "    2C"
        if line.starts_with("    2C") || line.starts_with("   2C") {
            block_start = Some(i);
            break;
        }
    }

    let Some(start) = block_start else {
        return Err(FrdError::MissingNodeBlock);
    };

    let mut map = HashMap::new();
    let mut line_no = start + 1;

    while line_no < lines.len() {
        let raw = lines[line_no];

        if raw.starts_with(" 2999") || raw.trim_start().starts_with("2999") {
            line_no += 1;
            break;
        }

        // Node lines: "  -1    nodeid   x   y   z"
        let t = raw.trim();
        if t.starts_with("-1") || raw.starts_with("  -1") {
            let parts: Vec<&str> = t.split_whitespace().collect();
            if parts.len() >= 5 {
                // parts[0]="-1", parts[1]=node_id, parts[2..4]=coords
                if let Ok(nid) = parts[1].parse::<u32>() {
                    let idx = map.len();
                    map.insert(nid, idx);
                }
            }
        }

        line_no += 1;
    }

    *cursor = line_no;
    Ok(map)
}

fn parse_result_block(
    lines: &[&str],
    start: usize,
    node_order: &HashMap<u32, usize>,
    node_ids: &[NodeId],
    block_index: u32,
) -> (Option<StepResult>, usize) {
    let mut cursor = start + 1;

    // Parse -4 field header line(s):  "   -4 FIELDNAME ncomp nstep"
    let mut field_name = String::from("Result");
    let mut n_comp = 0usize;
    let mut step_time = block_index as f32;

    while cursor < lines.len() {
        let raw = lines[cursor];
        let t   = raw.trim();

        if t.starts_with("-4") || raw.starts_with("   -4") {
            let parts: Vec<&str> = t.split_whitespace().collect();
            if parts.len() >= 3 {
                field_name = parts[1].to_string();
                n_comp = parts[2].parse::<usize>().unwrap_or(0);
                // 4th token is step/time when present
                if parts.len() >= 5 {
                    step_time = parts[4].parse::<f32>().unwrap_or(block_index as f32);
                }
            }
            cursor += 1;
            break;
        } else if t.starts_with("-5") || raw.starts_with("   -5") {
            // Component-name sub-headers — skip here
            cursor += 1;
        } else {
            cursor += 1;
            if cursor > start + 20 { break; } // Safety guard
        }
    }

    // Skip remaining -5 lines
    while cursor < lines.len() {
        let t = lines[cursor].trim();
        if t.starts_with("-5") || lines[cursor].starts_with("   -5") {
            cursor += 1;
        } else {
            break;
        }
    }

    // Parse -1 node-value lines
    let mut node_values: HashMap<usize, Vec<f32>> = HashMap::new();

    while cursor < lines.len() {
        let raw = lines[cursor];
        let t   = raw.trim();

        if raw.starts_with(" 9999") || raw.starts_with("  100CL") || raw.starts_with("  100 ") {
            break; // Start of next block
        }

        if t.starts_with("-1") || raw.starts_with("   -1") {
            let parts: Vec<&str> = t.split_whitespace().collect();

            // End sentinel: "  -1  0" or "-1  0"
            if parts.len() >= 2 && parts[1] == "0" {
                cursor += 1;
                break;
            }

            if parts.len() >= 2 {
                if let Ok(nid) = parts[1].parse::<u32>() {
                    if let Some(&mesh_idx) = node_order.get(&nid) {
                        let vals: Vec<f32> = parts[2..]
                            .iter()
                            .filter_map(|s| s.parse::<f32>().ok())
                            .collect();

                        node_values.insert(mesh_idx, vals);
                    }
                }
            }
        }

        cursor += 1;
    }

    if node_values.is_empty() || n_comp == 0 {
        return (None, cursor);
    }

    // Build ResultFields
    let mut fields  = Vec::new();

    match n_comp {
        1 => {
            // Scalar field
            let scalar_map: HashMap<NodeId, f32> = node_ids
                .iter()
                .enumerate()
                .map(|(i, &id)| {
                    let v = node_values.get(&i).and_then(|v| v.first()).copied().unwrap_or(0.0);
                    (id, v)
                })
                .collect();

            fields.push(ResultField::node_scalar(&field_name, node_ids, &scalar_map));
        }
        n if n >= 3 => {
            // Vector field
            let mut disp_map: HashMap<NodeId, Vec3> = HashMap::new();

            for (i, &id) in node_ids.iter().enumerate() {
                if let Some(vals) = node_values.get(&i) {
                    if vals.len() >= 3 {
                        disp_map.insert(id, Vec3::new(vals[0], vals[1], vals[2]));
                    }
                }
            }

            let mag_map: HashMap<NodeId, f32> = disp_map
                .iter()
                .map(|(&id, &v)| (id, v.length()))
                .collect();

            fields.push(ResultField::node_vector(&field_name, node_ids, &disp_map));
            fields.push(ResultField::node_scalar(format!("|{field_name}|"), node_ids, &mag_map));

            // Von-Mises for 6-component stress/strain tensors
            if n_comp == 6 {
                let vm_map: HashMap<NodeId, f32> = node_ids
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &id)| {
                        let v = node_values.get(&i)?;
                        if v.len() < 6 { return None; }
                        let (s11, s22, s33) = (v[0], v[1], v[2]);
                        let (s12, s23, s13) = (v[3], v[4], v[5]);
                        let vm = (0.5 * ((s11-s22).powi(2)
                                       +(s22-s33).powi(2)
                                       +(s33-s11).powi(2)
                                       +6.0*(s12.powi(2)+s23.powi(2)+s13.powi(2)))).sqrt();
                        Some((id, vm))
                    })
                    .collect();

                fields.push(ResultField::node_scalar(
                    format!("{field_name}_vonMises"), node_ids, &vm_map,
                ));
            }
        }
        _ => {}
    }

    let step = StepResult {
        step: block_index,
        time: step_time,
        fields,
    };

    (Some(step), cursor)
}
