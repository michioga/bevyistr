//! Parser for VTK XML Unstructured Grid result files (`.vtu`, ASCII/inline).
//!
//! VTU is the standard interchange format for FEM post-processing tools
//! (ParaView, CalculiX, OpenFOAM, FEniCS, etc.).  This parser handles the
//! most common case: ASCII `format="ascii"` inline data arrays.
//!
//! # Parsed elements
//!
//! * `<PointData>` / `<DataArray>` — node-centred scalar and 3-component
//!   vector result fields mapped to [`ResultField::NodeScalar`] and
//!   [`ResultField::NodeVector`].
//!
//! # Limitations
//!
//! * Binary / base64 / appended data formats are not supported.
//! * `<CellData>` is currently ignored.
//! * Multi-block `.pvtu` files: the `.pvtu` XML contains `<Piece>`
//!   references, one per MPI rank in a parallel FrontISTR run. Each piece's
//!   `<PointData>` arrays are read and concatenated in `<Piece>` order to
//!   form the full field. This assumes FrontISTR/HEC-MW's usual
//!   decomposition — each rank owns a disjoint, contiguous block of the
//!   global node numbering, written in rank order — so it will *not*
//!   produce correct results for a partitioning that duplicates or
//!   reorders nodes across pieces (e.g. overlapping/ghost node layers). If
//!   results look wrong for a parallel run, prefer the merged serial
//!   `.res`/`.frd` output instead of the raw per-rank `.pvtu`/`.vtu` set.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use bevy::prelude::Vec3;
use fem_core::{NodeId, ResultField, StepResult};

// ─── public API ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum VtuError {
    Io(io::Error),
    UnsupportedFormat(String),
    Parse(String),
}

impl std::fmt::Display for VtuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e)                   => write!(f, "IO: {e}"),
            Self::UnsupportedFormat(msg)  => write!(f, "Unsupported format: {msg}"),
            Self::Parse(msg)              => write!(f, "Parse error: {msg}"),
        }
    }
}

impl std::error::Error for VtuError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(e) = self { Some(e) } else { None }
    }
}

impl From<io::Error> for VtuError {
    fn from(e: io::Error) -> Self { Self::Io(e) }
}

/// Loads a `.vtu` or `.pvtu` file and returns a single-step [`StepResult`].
///
/// `node_ids` must be the ordered `FemMesh::nodes` id list so result fields
/// align with the mesh.  VTU stores values per point in the same order as
/// the `<Points>` block; this function assumes that order matches `node_ids`.
pub fn load_vtu_file(
    path: impl AsRef<Path>,
    node_ids: &[NodeId],
) -> Result<StepResult, VtuError> {
    let path = path.as_ref();
    let text  = std::fs::read_to_string(path)?;

    // Dispatch .pvtu → collect all piece .vtu files
    let lower = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if lower == "pvtu" {
        return load_pvtu(&text, path, node_ids);
    }

    parse_vtu(&text, node_ids, 1, 0.0)
}

// ─── pvtu dispatch ───────────────────────────────────────────────────────────

/// Loads every `<Piece Source="...">` referenced by a `.pvtu` file and
/// concatenates their `<PointData>` arrays (in `<Piece>` order) into one
/// [`StepResult`] — see this module's doc comment for the assumption this
/// relies on.
fn load_pvtu(pvtu: &str, pvtu_path: &Path, node_ids: &[NodeId]) -> Result<StepResult, VtuError> {
    let dir = pvtu_path.parent().unwrap_or(Path::new("."));

    let pieces: Vec<&str> = pvtu
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.starts_with("<Piece ") {
                attr_value(t, "Source")
            } else {
                None
            }
        })
        .collect();

    if pieces.is_empty() {
        return Err(VtuError::Parse("No <Piece> found in .pvtu file".into()));
    }

    // Merge by field name: append each piece's raw values in piece order,
    // so field[k] ends up holding the values for all nodes owned by piece
    // 0, then all nodes owned by piece 1, etc. — matching the contiguous
    // per-rank numbering HEC-MW/FrontISTR uses.
    let mut merged: Vec<RawDataArray> = Vec::new();

    for piece in &pieces {
        let piece_path = dir.join(piece);
        let text = std::fs::read_to_string(&piece_path)?;
        let raws = parse_raw_data_arrays(&text);

        if merged.is_empty() {
            merged = raws;
            continue;
        }

        for raw in raws {
            match merged.iter_mut().find(|m| m.name == raw.name && m.n_comp == raw.n_comp) {
                Some(existing) => existing.values.extend(raw.values),
                // A field present in a later piece but not the first is
                // unexpected (HEC-MW writes the same field set per rank),
                // but keep it rather than silently drop data.
                None => merged.push(raw),
            }
        }
    }

    if merged.is_empty() {
        return Err(VtuError::Parse("No PointData DataArrays found in any piece".into()));
    }

    let fields = merged
        .iter()
        .flat_map(|raw| build_result_fields(&raw.name, raw.n_comp, &raw.values, node_ids))
        .collect();

    Ok(StepResult { step: 1, time: 0.0, fields })
}

// ─── vtu parser ──────────────────────────────────────────────────────────────

/// Raw `<PointData>` `<DataArray>` contents, not yet mapped to `node_ids` —
/// the intermediate form shared by single-file parsing ([`parse_vtu`]) and
/// multi-piece merging ([`load_pvtu`]).
struct RawDataArray {
    name: String,
    n_comp: usize,
    values: Vec<f32>,
}

fn parse_vtu(src: &str, node_ids: &[NodeId], step: u32, time: f32) -> Result<StepResult, VtuError> {
    let raws = parse_raw_data_arrays(src);

    if raws.is_empty() {
        return Err(VtuError::Parse("No PointData DataArrays found".into()));
    }

    let fields = raws
        .iter()
        .flat_map(|raw| build_result_fields(&raw.name, raw.n_comp, &raw.values, node_ids))
        .collect();

    Ok(StepResult { step, time, fields })
}

/// Scans every `<PointData>` block in `src` and collects each
/// `<DataArray>`'s raw values, without mapping them to node ids yet.
fn parse_raw_data_arrays(src: &str) -> Vec<RawDataArray> {
    let mut raws = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let t = lines[i].trim();

        if t.contains("<PointData") {
            i += 1;
            // Collect DataArray blocks until </PointData>
            while i < lines.len() {
                let t2 = lines[i].trim();

                if t2.contains("</PointData") {
                    break;
                }

                if t2.starts_with("<DataArray") {
                    if let Some((raw, new_i)) = parse_data_array(&lines, i) {
                        raws.push(raw);
                        i = new_i;
                        continue;
                    }
                }

                i += 1;
            }
        }

        i += 1;
    }

    raws
}

/// Parses one `<DataArray ... format="ascii">` block starting at `start`.
/// Returns `(RawDataArray, next_line_index)` or `None` on parse failure.
fn parse_data_array(lines: &[&str], start: usize) -> Option<(RawDataArray, usize)> {
    let header = lines[start].trim();

    // Only handle ascii format
    if header.contains("format=") && !header.contains("format=\"ascii\"") {
        return None;
    }

    let name       = attr_value(header, "Name").unwrap_or("Field").to_string();
    let n_comp_str = attr_value(header, "NumberOfComponents").unwrap_or("1");
    let n_comp: usize = n_comp_str.parse().unwrap_or(1);

    // Collect data lines until </DataArray>
    let mut raw_values: Vec<f32> = Vec::new();
    let mut cursor = start + 1;
    let mut inline = false;

    // Handle case where header closes on same line with data inline
    if header.ends_with('>') && !header.ends_with("/>") {
        inline = true;
    }

    if inline {
        while cursor < lines.len() {
            let t = lines[cursor].trim();

            if t.contains("</DataArray") {
                cursor += 1;
                break;
            }

            for tok in t.split_whitespace() {
                if let Ok(v) = tok.parse::<f32>() {
                    raw_values.push(v);
                }
            }

            cursor += 1;
        }
    } else {
        // Compact single-line format: data on same line after >
        let after_gt = header.find('>').map(|i| &header[i + 1..]).unwrap_or("");
        for tok in after_gt.split_whitespace() {
            if let Ok(v) = tok.parse::<f32>() {
                raw_values.push(v);
            }
        }

        while cursor < lines.len() {
            let t = lines[cursor].trim();
            if t.contains("</DataArray") {
                cursor += 1;
                break;
            }
            for tok in t.split_whitespace() {
                if let Ok(v) = tok.parse::<f32>() {
                    raw_values.push(v);
                }
            }
            cursor += 1;
        }
    }

    Some((RawDataArray { name, n_comp, values: raw_values }, cursor))
}

/// Maps a field's raw per-node values onto `node_ids`. 3+-component fields
/// (displacement, velocity, ...) produce both a [`ResultField::NodeVector`]
/// (so deformed-shape rendering — [`fem_core::ContourSettings::displacement_field`]
/// looks up a field by name and needs real X/Y/Z components, not just a
/// magnitude — and vector-aware consumers have what they need) and a
/// companion `|name|` [`ResultField::NodeScalar`] magnitude for quick
/// contour coloring, matching the convention [`crate::result`] and
/// [`crate::frd`] already use. Scalar fields (temperature, von Mises
/// stress extrapolated to nodes, ...) produce just the one `NodeScalar`.
fn build_result_fields(name: &str, n_comp: usize, raw_values: &[f32], node_ids: &[NodeId]) -> Vec<ResultField> {
    if raw_values.is_empty() {
        return vec![ResultField::node_scalar(name, node_ids, &HashMap::new())];
    }

    if n_comp >= 3 {
        let vec_map: HashMap<NodeId, Vec3> = node_ids
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| {
                let base = i * n_comp;
                if base + 2 < raw_values.len() {
                    Some((id, Vec3::new(
                        raw_values[base],
                        raw_values[base + 1],
                        raw_values[base + 2],
                    )))
                } else {
                    None
                }
            })
            .collect();

        let mag_map: HashMap<NodeId, f32> = vec_map
            .iter()
            .map(|(&id, &v)| (id, v.length()))
            .collect();

        vec![
            ResultField::node_vector(name, node_ids, &vec_map),
            ResultField::node_scalar(format!("|{name}|"), node_ids, &mag_map),
        ]
    } else {
        let scalar_map: HashMap<NodeId, f32> = node_ids
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| raw_values.get(i).map(|&v| (id, v)))
            .collect();

        vec![ResultField::node_scalar(name, node_ids, &scalar_map)]
    }
}

// ─── XML attribute helper ─────────────────────────────────────────────────────

/// Returns the value of `attr="..."` in `line`, without quotes.
fn attr_value<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start  = line.find(&needle)? + needle.len();
    let end    = line[start..].find('"')? + start;
    Some(&line[start..end])
}
