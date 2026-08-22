//! Gmsh MSH v4.1 bridge for bevyistr.
//!
//! Provides two entry points:
//!
//! * [`load_msh_file`] — parses an existing `.msh` (ASCII v4.1) directly into
//!   a [`FemMesh`] without invoking the Gmsh executable.
//!
//! * [`run_gmsh`] — invokes the Gmsh CLI on a `.geo` or `.msh` source file to
//!   produce a `.msh` output (the "executable bridge" pattern), then delegates
//!   to [`load_msh_file`].  Requires `gmsh` to be on `$PATH`.
//!
//! # Gmsh element type mapping
//!
//! | Gmsh type | Description           | fem_core `ElementType` |
//! |-----------|-----------------------|------------------------|
//! | 1         | 2-node line           | `Rod2`                 |
//! | 2         | 3-node triangle       | `Tri3`                 |
//! | 3         | 4-node quadrilateral  | `Quad4`                |
//! | 4         | 4-node tetrahedron    | `Tet4`                 |
//! | 5         | 8-node hexahedron     | `Hex8`                 |
//! | 6         | 6-node prism          | `Prism6`               |
//! | 9         | 6-node triangle       | `Tri6`                 |
//! | 10        | 9-node quad           | `Quad8` (corner nodes) |
//! | 11        | 10-node tetrahedron   | `Tet10`                |
//! | 13        | 15-node prism         | `Prism15`              |
//! | 20        | 20-node hexahedron    | `Hex20`                |
//!
//! Point elements (type 15) are silently skipped.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use bevy::prelude::Vec3;
use fem_core::{
    ElementId, ElementType, FemElement, FemMesh, FemNode, NodeId,
};

// ─── public API ──────────────────────────────────────────────────────────────

/// Runs the Gmsh CLI on `input` (`.geo` or `.msh`) and writes the meshed
/// output to a temporary `.msh` file beside the input, then loads that file
/// via [`load_msh_file`].
///
/// `mesh_size_factor` is forwarded as Gmsh's `-clscale` option (default 1.0).
/// Pass `None` to omit it.
///
/// Requires `gmsh` to be on `$PATH`. Returns an error if the subprocess fails
/// or the output file cannot be read.
pub fn run_gmsh(
    input: &Path,
    mesh_size_factor: Option<f32>,
) -> Result<FemMesh, GmshError> {
    let output = input.with_extension("msh");

    let mut cmd = Command::new("gmsh");
    cmd.arg(input)
        .arg("-3")            // generate 3-D mesh
        .arg("-format").arg("msh41")
        .arg("-o").arg(&output);

    if let Some(scale) = mesh_size_factor {
        cmd.arg("-clscale").arg(scale.to_string());
    }

    let status = cmd
        .status()
        .map_err(|err| GmshError::Io(err))?;

    if !status.success() {
        return Err(GmshError::GmshFailed {
            code: status.code(),
        });
    }

    load_msh_file(&output)
}

/// Parses a Gmsh MSH v4.1 ASCII file and returns a [`FemMesh`].
///
/// Binary MSH files (indicated by `1` in the `$MeshFormat` header) are
/// rejected with [`GmshError::BinaryNotSupported`].
pub fn load_msh_file(path: &Path) -> Result<FemMesh, GmshError> {
    let text = fs::read_to_string(path)
        .map_err(GmshError::Io)?;

    parse_msh41(&text)
}

// ─── error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum GmshError {
    Io(io::Error),

    /// The `.msh` file uses binary encoding, which is not yet supported.
    BinaryNotSupported,

    /// The `.msh` version string is not `4.1`.
    UnsupportedVersion(String),

    /// A required section (`$Nodes`, `$Elements`) is missing.
    MissingSection(&'static str),

    /// A line inside a section could not be parsed.
    Parse {
        section: &'static str,
        line: String,
    },

    /// `gmsh` process exited with a non-zero status.
    GmshFailed { code: Option<i32> },
}

impl std::fmt::Display for GmshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err)                    => write!(f, "IO error: {err}"),
            Self::BinaryNotSupported         => write!(f, "Binary MSH files are not supported; re-export with ASCII encoding"),
            Self::UnsupportedVersion(ver)    => write!(f, "Unsupported MSH version {ver}; only 4.1 is supported"),
            Self::MissingSection(name)       => write!(f, "Missing required MSH section ${name}"),
            Self::Parse { section, line }    => write!(f, "Parse error in ${section}: {line:?}"),
            Self::GmshFailed { code: Some(c) } => write!(f, "gmsh exited with code {c}"),
            Self::GmshFailed { code: None }  => write!(f, "gmsh was terminated by a signal"),
        }
    }
}

impl std::error::Error for GmshError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(err) = self {
            Some(err)
        } else {
            None
        }
    }
}

// ─── parser ──────────────────────────────────────────────────────────────────

struct MshSections<'a> {
    mesh_format: &'a str,
    nodes: &'a str,
    elements: &'a str,
}

fn extract_sections<'a>(text: &'a str) -> Result<MshSections<'a>, GmshError> {
    Ok(MshSections {
        mesh_format: extract_section(text, "MeshFormat")
            .ok_or(GmshError::MissingSection("MeshFormat"))?,
        nodes: extract_section(text, "Nodes")
            .ok_or(GmshError::MissingSection("Nodes"))?,
        elements: extract_section(text, "Elements")
            .ok_or(GmshError::MissingSection("Elements"))?,
    })
}

/// Returns the text between `$<name>` and `$End<name>`, exclusive.
fn extract_section<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start_tag = format!("${name}");
    let end_tag   = format!("$End{name}");

    let start = text.find(&start_tag)? + start_tag.len();
    let end   = text[start..].find(&end_tag)? + start;

    Some(text[start..end].trim())
}

fn parse_msh41(text: &str) -> Result<FemMesh, GmshError> {
    let sections = extract_sections(text)?;

    check_version(sections.mesh_format)?;

    let (nodes, tag_to_index) = parse_nodes(sections.nodes)?;
    let elements = parse_elements(sections.elements, &tag_to_index)?;

    let mut mesh = FemMesh::default();
    mesh.nodes    = nodes;
    mesh.elements = elements;

    Ok(mesh)
}

fn check_version(mesh_format: &str) -> Result<(), GmshError> {
    let mut tokens = mesh_format.split_whitespace();

    let version = tokens.next().unwrap_or("");
    let is_binary: u8 = tokens.next().and_then(|t| t.parse().ok()).unwrap_or(0);

    if is_binary != 0 {
        return Err(GmshError::BinaryNotSupported);
    }

    if !version.starts_with("4.1") {
        return Err(GmshError::UnsupportedVersion(version.to_string()));
    }

    Ok(())
}

/// Parses `$Nodes … $EndNodes` and returns the node list plus a map from
/// Gmsh node tags (1-based, not necessarily contiguous) to slice indices.
fn parse_nodes(section: &str) -> Result<(Vec<FemNode>, HashMap<u64, usize>), GmshError> {
    let mut lines = section.lines();

    // header: numEntityBlocks numNodes minNodeTag maxNodeTag
    let _header = lines.next().ok_or(GmshError::MissingSection("Nodes"))?;

    let mut nodes: Vec<FemNode>         = Vec::new();
    let mut tag_to_index: HashMap<u64, usize> = HashMap::new();

    while let Some(line) = lines.next() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // Entity block header: entityDim entityTag parametric numNodesInBlock
        let block_header: Vec<&str> = line.split_whitespace().collect();

        if block_header.len() < 4 {
            return Err(GmshError::Parse { section: "Nodes", line: line.to_string() });
        }

        let count: usize = block_header[3]
            .parse()
            .map_err(|_| GmshError::Parse { section: "Nodes", line: line.to_string() })?;

        // First `count` lines are node tags.
        let mut tags: Vec<u64> = Vec::with_capacity(count);

        for _ in 0..count {
            let tag_line = lines.next().unwrap_or("").trim().to_string();
            let tag: u64 = tag_line
                .parse()
                .map_err(|_| GmshError::Parse { section: "Nodes", line: tag_line.clone() })?;

            tags.push(tag);
        }

        // Next `count` lines are x y z coordinates.
        for tag in tags {
            let coord_line = lines.next().unwrap_or("").trim().to_string();
            let parts: Vec<f64> = coord_line
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();

            if parts.len() < 3 {
                return Err(GmshError::Parse { section: "Nodes", line: coord_line });
            }

            let index = nodes.len();
            tag_to_index.insert(tag, index);

            nodes.push(FemNode {
                id: NodeId(tag as u32),
                position: Vec3::new(parts[0] as f32, parts[1] as f32, parts[2] as f32),
            });
        }
    }

    Ok((nodes, tag_to_index))
}

/// Parses `$Elements … $EndElements` using `tag_to_index` to validate that
/// every referenced node was seen in `$Nodes`.
fn parse_elements(
    section: &str,
    tag_to_index: &HashMap<u64, usize>,
) -> Result<Vec<FemElement>, GmshError> {
    let mut lines = section.lines();

    // header: numEntityBlocks numElements minElementTag maxElementTag
    let _header = lines.next().ok_or(GmshError::MissingSection("Elements"))?;

    let mut elements: Vec<FemElement> = Vec::new();

    while let Some(line) = lines.next() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // Entity block header: entityDim entityTag elementType numElementsInBlock
        let block_header: Vec<&str> = line.split_whitespace().collect();

        if block_header.len() < 4 {
            return Err(GmshError::Parse { section: "Elements", line: line.to_string() });
        }

        let gmsh_type: u32 = block_header[2]
            .parse()
            .map_err(|_| GmshError::Parse { section: "Elements", line: line.to_string() })?;

        let count: usize = block_header[3]
            .parse()
            .map_err(|_| GmshError::Parse { section: "Elements", line: line.to_string() })?;

        let Some(element_type) = gmsh_type_to_element_type(gmsh_type) else {
            // Skip unsupported or degenerate types (e.g. point elements, type 15).
            for _ in 0..count {
                lines.next();
            }
            continue;
        };

        let expected_nodes = element_node_count(element_type.clone());

        if expected_nodes == 0 {
            // Unsupported element with unknown node count — skip
            for _ in 0..count {
                lines.next();
            }
            continue;
        }

        for _ in 0..count {
            let elem_line = lines.next().unwrap_or("").trim().to_string();
            let parts: Vec<u64> = elem_line
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();

            // parts[0] = element tag; parts[1..] = node tags
            if parts.len() < 1 + expected_nodes {
                return Err(GmshError::Parse { section: "Elements", line: elem_line });
            }

            let elem_id = ElementId(parts[0] as u32);

            let nodes: Vec<NodeId> = parts[1..1 + expected_nodes]
                .iter()
                .map(|&tag| NodeId(tag as u32))
                .collect();

            // Validate: all referenced node tags must exist.
            for node_id in &nodes {
                if !tag_to_index.contains_key(&(node_id.0 as u64)) {
                    return Err(GmshError::Parse {
                        section: "Elements",
                        line: format!("unknown node tag {} in element {}", node_id.0, elem_id.0),
                    });
                }
            }

            elements.push(FemElement {
                id: elem_id,
                element_type: element_type.clone(),
                nodes,
            });
        }
    }

    Ok(elements)
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn gmsh_type_to_element_type(gmsh_type: u32) -> Option<ElementType> {
    match gmsh_type {
        1  => Some(ElementType::Rod2),
        2  => Some(ElementType::Tri3),
        3  => Some(ElementType::Quad4),
        4  => Some(ElementType::Tet4),
        5  => Some(ElementType::Hex8),
        6  => Some(ElementType::Prism6),
        9  => Some(ElementType::Tri6),
        10 => Some(ElementType::Quad8),
        11 => Some(ElementType::Tet10),
        13 => Some(ElementType::Prism15),
        20 => Some(ElementType::Hex20),
        // 15 = point element → skip
        _  => None,
    }
}

fn element_node_count(element_type: ElementType) -> usize {
    match element_type {
        ElementType::Rod2    => 2,
        ElementType::Tri3    => 3,
        ElementType::Quad4   => 4,
        ElementType::Tet4    => 4,
        ElementType::Hex8    => 8,
        ElementType::Prism6  => 6,
        ElementType::Tri6    => 6,
        ElementType::Quad8   => 8,
        ElementType::Tet10   => 10,
        ElementType::Prism15 => 15,
        ElementType::Hex20   => 20,
        ElementType::Beam611 | ElementType::Beam641 => 2,
        ElementType::Unsupported(_) => 0,
    }
}
