//! Parser for Abaqus / CalculiX `.inp` mesh files.
//!
//! Reads `*NODE`, `*ELEMENT`, `*NSET`, and `*ELSET` blocks into a
//! [`FemMesh`].  Both Abaqus and CalculiX use the same keyword syntax for
//! these fundamental blocks, so one parser handles both.
//!
//! # Element type mapping
//!
//! | Abaqus / CalculiX type | fem_core [`ElementType`] |
//! |------------------------|--------------------------|
//! | C3D4, C3D4H            | `Tet4`                   |
//! | C3D10, C3D10H          | `Tet10`                  |
//! | C3D8, C3D8R, C3D8I     | `Hex8`                   |
//! | C3D20, C3D20R          | `Hex20`                  |
//! | C3D6                   | `Prism6`                 |
//! | C3D15                  | `Prism15`                |
//! | S3, S3R, STRI3         | `Tri3`                   |
//! | S4, S4R, S4RS          | `Quad4`                  |
//! | S6, S6R, STRI65        | `Tri6`                   |
//! | S8, S8R                | `Quad8`                  |
//! | B31, B32, T3D2         | `Rod2`                   |
//!
//! Unknown element types are stored as `ElementType::Unsupported(name)`.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use bevy::prelude::Vec3;
use fem_core::{
    ElementId, ElementType, FemElement, FemElementSet, FemMesh, FemNode, FemNodeSet, NodeId,
};

// ─── public API ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum InpError {
    Io(io::Error),
    Parse { line: usize, message: String },
}

impl std::fmt::Display for InpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e)                   => write!(f, "IO: {e}"),
            Self::Parse { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl std::error::Error for InpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(e) = self { Some(e) } else { None }
    }
}

impl From<io::Error> for InpError {
    fn from(e: io::Error) -> Self { Self::Io(e) }
}

/// Loads an Abaqus / CalculiX `.inp` file and returns a [`FemMesh`].
pub fn load_inp_file(path: impl AsRef<Path>) -> Result<FemMesh, InpError> {
    let text = std::fs::read_to_string(path.as_ref())?;
    parse_inp(&text)
}

// ─── parser ──────────────────────────────────────────────────────────────────

fn parse_inp(src: &str) -> Result<FemMesh, InpError> {
    let lines: Vec<(usize, &str)> = src
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .collect();

    let mut nodes:    Vec<FemNode>       = Vec::new();
    let mut elements: Vec<FemElement>    = Vec::new();
    let mut node_sets:    Vec<FemNodeSet>    = Vec::new();
    let mut element_sets: Vec<FemElementSet> = Vec::new();

    // node_id → FemMesh slice index (reserved for future use, e.g. element
    // connectivity validation against loaded node list)
    let mut _node_index: HashMap<u32, usize> = HashMap::new();

    let mut cursor = 0usize;

    while cursor < lines.len() {
        let (_line_no, raw) = lines[cursor];
        let trimmed = raw.trim();

        // Skip blank lines and comments
        if trimmed.is_empty() || trimmed.starts_with("**") {
            cursor += 1;
            continue;
        }

        if trimmed.starts_with('*') {
            let keyword = keyword_name(trimmed);

            match keyword.to_uppercase().as_str() {
                "NODE" => {
                    cursor += 1;
                    while cursor < lines.len() {
                        let (ln, r) = lines[cursor];
                        let t = r.trim();
                        if t.is_empty() || t.starts_with("**") {
                            cursor += 1;
                            continue;
                        }
                        if t.starts_with('*') { break; }

                        let parts: Vec<&str> = t.split(',').map(str::trim).collect();
                        if parts.len() < 4 {
                            return Err(InpError::Parse { line: ln, message: format!("expected id, x, y, z — got {:?}", t) });
                        }
                        let id: u32 = parts[0].parse().map_err(|_| InpError::Parse { line: ln, message: format!("bad node id {:?}", parts[0]) })?;
                        let x: f32  = parts[1].parse().unwrap_or(0.0);
                        let y: f32  = parts[2].parse().unwrap_or(0.0);
                        let z: f32  = parts[3].parse().unwrap_or(0.0);

                        let idx = nodes.len();
                        _node_index.insert(id, idx);
                        nodes.push(FemNode { id: NodeId(id), position: Vec3::new(x, y, z) });
                        cursor += 1;
                    }
                }

                "ELEMENT" => {
                    let type_str = extract_param(trimmed, "TYPE").unwrap_or("C3D4");
                    let etype    = inp_type_to_element_type(type_str);
                    let set_name = extract_param(trimmed, "ELSET").map(str::to_string);

                    let mut elem_ids_for_set: Vec<ElementId> = Vec::new();

                    cursor += 1;
                    while cursor < lines.len() {
                        let (ln, r) = lines[cursor];
                        let t = r.trim();
                        if t.is_empty() || t.starts_with("**") {
                            cursor += 1;
                            continue;
                        }
                        if t.starts_with('*') { break; }

                        // Some INP writers split long element connectivity over
                        // multiple lines ending with ','.  Collect continuation.
                        let mut full = t.to_string();
                        cursor += 1;
                        while full.trim_end().ends_with(',') && cursor < lines.len() {
                            full.push_str(lines[cursor].1.trim());
                            cursor += 1;
                        }

                        let parts: Vec<&str> = full.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
                        if parts.is_empty() { continue; }

                        let elem_id: u32 = parts[0].parse().unwrap_or(0);
                        let nids: Vec<NodeId> = parts[1..].iter().filter_map(|s| s.parse::<u32>().ok()).map(NodeId).collect();

                        let eid = ElementId(elem_id);
                        elem_ids_for_set.push(eid);

                        elements.push(FemElement { id: eid, element_type: etype.clone(), nodes: nids });
                        let _ = ln;
                    }

                    if let Some(name) = set_name {
                        element_sets.push(FemElementSet { name, elements: elem_ids_for_set });
                    }
                }

                "NSET" => {
                    let set_name = extract_param(trimmed, "NSET").unwrap_or("NSET").to_string();
                    let generate  = trimmed.to_uppercase().contains("GENERATE");
                    let mut nids: Vec<NodeId> = Vec::new();

                    cursor += 1;
                    while cursor < lines.len() {
                        let (_, r) = lines[cursor];
                        let t = r.trim();
                        if t.is_empty() || t.starts_with("**") { cursor += 1; continue; }
                        if t.starts_with('*') { break; }

                        let parts: Vec<u32> = t.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                        if generate && parts.len() >= 2 {
                            let start = parts[0];
                            let end   = parts[1];
                            let step  = if parts.len() >= 3 { parts[2] } else { 1 };
                            let mut n = start;
                            while n <= end { nids.push(NodeId(n)); n += step; }
                        } else {
                            nids.extend(parts.into_iter().map(NodeId));
                        }
                        cursor += 1;
                    }

                    node_sets.push(FemNodeSet { name: set_name, nodes: nids });
                }

                "ELSET" => {
                    let set_name = extract_param(trimmed, "ELSET").unwrap_or("ELSET").to_string();
                    let generate  = trimmed.to_uppercase().contains("GENERATE");
                    let mut eids: Vec<ElementId> = Vec::new();

                    cursor += 1;
                    while cursor < lines.len() {
                        let (_, r) = lines[cursor];
                        let t = r.trim();
                        if t.is_empty() || t.starts_with("**") { cursor += 1; continue; }
                        if t.starts_with('*') { break; }

                        let parts: Vec<u32> = t.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                        if generate && parts.len() >= 2 {
                            let start = parts[0];
                            let end   = parts[1];
                            let step  = if parts.len() >= 3 { parts[2] } else { 1 };
                            let mut n = start;
                            while n <= end { eids.push(ElementId(n)); n += step; }
                        } else {
                            eids.extend(parts.into_iter().map(ElementId));
                        }
                        cursor += 1;
                    }

                    element_sets.push(FemElementSet { name: set_name, elements: eids });
                }

                _ => { cursor += 1; }
            }
        } else {
            cursor += 1;
        }
    }

    let mut mesh = FemMesh::default();
    mesh.nodes        = nodes;
    mesh.elements     = elements;
    mesh.node_sets    = node_sets;
    mesh.element_sets = element_sets;

    Ok(mesh)
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Returns the keyword name from a `*KEYWORD, params…` line (before any `,`).
fn keyword_name(line: &str) -> &str {
    let body = line.trim_start_matches('*');
    body.split(',').next().unwrap_or("").trim()
}

/// Finds `KEY=value` (case-insensitive) in a `*KEYWORD, KEY=value, …` line.
fn extract_param<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let key_upper = key.to_uppercase();
    for token in line.split(',').skip(1) {
        let t = token.trim();
        let upper = t.to_uppercase();
        if upper.starts_with(&key_upper) && upper[key_upper.len()..].starts_with('=') {
            return Some(&t[key.len() + 1..]);
        }
    }
    None
}

fn inp_type_to_element_type(name: &str) -> ElementType {
    let upper = name.to_uppercase();
    // Normalize: strip trailing variant codes (H, R, I, S, RS, 65)
    let base: String = upper.chars().take_while(|c| c.is_ascii_alphabetic() || c.is_ascii_digit()).collect();

    match base.as_str() {
        // Solid tetrahedral
        "C3D4" | "C3D4H"                   => ElementType::Tet4,
        "C3D10" | "C3D10H" | "C3D10M"     => ElementType::Tet10,
        // Solid hexahedral
        "C3D8" | "C3D8R" | "C3D8I"        => ElementType::Hex8,
        "C3D20" | "C3D20R"                 => ElementType::Hex20,
        // Solid prismatic/wedge
        "C3D6"                              => ElementType::Prism6,
        "C3D15"                             => ElementType::Prism15,
        // Shell triangular
        "S3" | "S3R" | "STRI3"            => ElementType::Tri3,
        "S6" | "S6R" | "STRI65"           => ElementType::Tri6,
        // Shell quadrilateral
        "S4" | "S4R" | "S4RS"             => ElementType::Quad4,
        "S8" | "S8R"                       => ElementType::Quad8,
        // Beam / truss
        "B31" | "B32" | "T3D2" | "T3D3"  => ElementType::Rod2,
        _                                  => ElementType::Unsupported(name.to_string()),
    }
}
