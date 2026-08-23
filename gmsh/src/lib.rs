//! Gmsh MSH 4.x bridge for bevyistr.
//!
//! The reader accepts ASCII MSH 4.x revisions 4.1 and newer, preserves Gmsh
//! physical groups, and converts supported element connectivity to the HECMW
//! ordering used by [`fem_core`]. Binary MSH is deliberately rejected with a
//! clear error so a caller can ask Gmsh to re-export as ASCII.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use bevy::prelude::Vec3;
use fem_core::{
    ElementFaceRef, ElementId, ElementType, FemElement, FemElementSet, FemMesh, FemNode,
    FemNodeSet, FemSurfaceSet, LocalFaceId, NodeId,
};

type EntityKey = (u8, u64);
type PhysicalKey = (u8, u64);

/// Runs the Gmsh CLI on a geometry input and loads the generated ASCII MSH 4.1
/// mesh. `mesh_size_factor` is forwarded as Gmsh's `-clscale` option.
pub fn run_gmsh(input: &Path, mesh_size_factor: Option<f32>) -> Result<FemMesh, GmshError> {
    let output = input.with_extension("msh");

    let mut cmd = Command::new("gmsh");
    cmd.arg(input)
        .arg("-3")
        .arg("-format")
        .arg("msh41")
        .arg("-o")
        .arg(&output);

    if let Some(scale) = mesh_size_factor {
        cmd.arg("-clscale").arg(scale.to_string());
    }

    let status = cmd.status().map_err(GmshError::Io)?;
    if !status.success() {
        return Err(GmshError::GmshFailed {
            code: status.code(),
        });
    }

    load_msh_file(&output)
}

/// Parses an ASCII Gmsh MSH 4.x file (revision 4.1 or newer).
pub fn load_msh_file(path: &Path) -> Result<FemMesh, GmshError> {
    let bytes = fs::read(path).map_err(GmshError::Io)?;
    let prefix_len = bytes.len().min(256);
    let prefix = String::from_utf8_lossy(&bytes[..prefix_len]);
    if let Some(mesh_format) = extract_section(&prefix, "MeshFormat") {
        check_version(mesh_format)?;
    }
    let text = String::from_utf8(bytes)
        .map_err(|error| GmshError::Io(io::Error::new(io::ErrorKind::InvalidData, error)))?;
    parse_msh41(&text)
}

#[derive(Debug)]
pub enum GmshError {
    Io(io::Error),
    BinaryNotSupported,
    UnsupportedVersion(String),
    MissingSection(&'static str),
    Parse { section: &'static str, line: String },
    UnsupportedElementType { gmsh_type: u32, entity_dim: u8 },
    TagOutOfRange { kind: &'static str, tag: u64 },
    UnmatchedSurfaceElement { element_tag: u64 },
    GmshFailed { code: Option<i32> },
}

impl std::fmt::Display for GmshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "IO error: {err}"),
            Self::BinaryNotSupported => write!(
                f,
                "binary MSH files are not supported; re-export with ASCII encoding"
            ),
            Self::UnsupportedVersion(version) => write!(
                f,
                "unsupported MSH version {version}; expected an ASCII MSH 4.x revision >= 4.1"
            ),
            Self::MissingSection(name) => write!(f, "missing required MSH section ${name}"),
            Self::Parse { section, line } => {
                write!(f, "parse error in ${section}: {line}")
            }
            Self::UnsupportedElementType {
                gmsh_type,
                entity_dim,
            } => write!(
                f,
                "Gmsh element type {gmsh_type} is unsupported for a {entity_dim}-D analysis mesh"
            ),
            Self::TagOutOfRange { kind, tag } => {
                write!(f, "{kind} tag {tag} exceeds bevyistr's 32-bit ID range")
            }
            Self::UnmatchedSurfaceElement { element_tag } => write!(
                f,
                "physical surface element {element_tag} does not match any analysis-element face"
            ),
            Self::GmshFailed { code: Some(code) } => write!(f, "gmsh exited with code {code}"),
            Self::GmshFailed { code: None } => write!(f, "gmsh was terminated by a signal"),
        }
    }
}

impl std::error::Error for GmshError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

struct MshSections<'a> {
    mesh_format: &'a str,
    physical_names: Option<&'a str>,
    entities: Option<&'a str>,
    nodes: &'a str,
    elements: &'a str,
}

fn extract_sections(text: &str) -> Result<MshSections<'_>, GmshError> {
    Ok(MshSections {
        mesh_format: extract_section(text, "MeshFormat")
            .ok_or(GmshError::MissingSection("MeshFormat"))?,
        physical_names: extract_section(text, "PhysicalNames"),
        entities: extract_section(text, "Entities"),
        nodes: extract_section(text, "Nodes").ok_or(GmshError::MissingSection("Nodes"))?,
        elements: extract_section(text, "Elements").ok_or(GmshError::MissingSection("Elements"))?,
    })
}

/// Returns the text between `$<name>` and `$End<name>`, exclusive.
fn extract_section<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start_tag = format!("${name}");
    let end_tag = format!("$End{name}");
    let start = text.find(&start_tag)? + start_tag.len();
    let end = text[start..].find(&end_tag)? + start;
    Some(text[start..end].trim())
}

fn parse_msh41(text: &str) -> Result<FemMesh, GmshError> {
    let sections = extract_sections(text)?;
    check_version(sections.mesh_format)?;

    let physical_names = sections
        .physical_names
        .map(parse_physical_names)
        .transpose()?
        .unwrap_or_default();
    let entities = sections
        .entities
        .map(parse_entities)
        .transpose()?
        .unwrap_or_default();
    let parsed_nodes = parse_nodes(sections.nodes)?;
    let blocks = parse_elements(sections.elements, &parsed_nodes.node_tags)?;

    build_mesh(parsed_nodes, blocks, &entities, &physical_names)
}

fn check_version(mesh_format: &str) -> Result<(), GmshError> {
    let mut tokens = mesh_format.split_whitespace();
    let version = tokens.next().unwrap_or("");
    let file_type = parse_token::<u8>(tokens.next(), "MeshFormat", "file type")?;

    if file_type != 0 {
        return Err(GmshError::BinaryNotSupported);
    }

    let mut components = version.split('.');
    let major = components.next().and_then(|part| part.parse::<u32>().ok());
    let minor = components.next().and_then(|part| part.parse::<u32>().ok());
    if major != Some(4) || minor.is_none_or(|minor| minor < 1) {
        return Err(GmshError::UnsupportedVersion(version.to_string()));
    }

    Ok(())
}

fn parse_physical_names(section: &str) -> Result<HashMap<PhysicalKey, String>, GmshError> {
    let mut lines = nonempty_lines(section);
    let count_line = lines
        .next()
        .ok_or(GmshError::MissingSection("PhysicalNames"))?;
    let count = parse_str::<usize>(count_line, "PhysicalNames")?;
    let mut names = HashMap::with_capacity(count);

    for _ in 0..count {
        let line = lines
            .next()
            .ok_or_else(|| parse_error("PhysicalNames", "unexpected EOF"))?;
        let (dim_text, rest) =
            split_first_field(line).ok_or_else(|| parse_error("PhysicalNames", line))?;
        let (tag_text, name_text) =
            split_first_field(rest).ok_or_else(|| parse_error("PhysicalNames", line))?;
        let dim = parse_str::<u8>(dim_text, "PhysicalNames")?;
        let tag = parse_str::<u64>(tag_text, "PhysicalNames")?;
        let name = name_text.trim().trim_matches('"').to_string();
        names.insert((dim, tag), name);
    }

    if let Some(extra) = lines.next() {
        return Err(parse_error("PhysicalNames", extra));
    }

    Ok(names)
}

fn parse_entities(section: &str) -> Result<HashMap<EntityKey, Vec<u64>>, GmshError> {
    let mut lines = nonempty_lines(section);
    let header = lines.next().ok_or(GmshError::MissingSection("Entities"))?;
    let counts = parse_fields::<usize>(header, "Entities")?;
    if counts.len() != 4 {
        return Err(parse_error("Entities", header));
    }

    let mut result = HashMap::new();
    for (dim, count) in counts.into_iter().enumerate() {
        for _ in 0..count {
            let line = lines
                .next()
                .ok_or_else(|| parse_error("Entities", "unexpected EOF"))?;
            let fields: Vec<&str> = line.split_whitespace().collect();
            let physical_count_index = if dim == 0 { 4 } else { 7 };
            if fields.len() <= physical_count_index {
                return Err(parse_error("Entities", line));
            }

            let tag = parse_str::<u64>(fields[0], "Entities")?;
            let physical_count = parse_str::<usize>(fields[physical_count_index], "Entities")?;
            let physical_start = physical_count_index + 1;
            let physical_end = physical_start + physical_count;
            if fields.len() < physical_end {
                return Err(parse_error("Entities", line));
            }

            let physical_tags = fields[physical_start..physical_end]
                .iter()
                .map(|field| parse_str::<u64>(field, "Entities"))
                .collect::<Result<Vec<_>, _>>()?;
            result.insert((dim as u8, tag), physical_tags);
        }
    }

    if let Some(extra) = lines.next() {
        return Err(parse_error("Entities", extra));
    }

    Ok(result)
}

struct ParsedNodes {
    nodes: Vec<FemNode>,
    node_tags: HashSet<u64>,
    entity_nodes: HashMap<EntityKey, BTreeSet<NodeId>>,
}

fn parse_nodes(section: &str) -> Result<ParsedNodes, GmshError> {
    let mut tokens = section.split_whitespace();
    let block_count = next_token::<usize>(&mut tokens, "Nodes", "entity block count")?;
    let declared_node_count = next_token::<usize>(&mut tokens, "Nodes", "node count")?;
    let _min_tag = next_token::<u64>(&mut tokens, "Nodes", "minimum node tag")?;
    let _max_tag = next_token::<u64>(&mut tokens, "Nodes", "maximum node tag")?;

    let mut nodes = Vec::with_capacity(declared_node_count);
    let mut node_tags = HashSet::with_capacity(declared_node_count);
    let mut entity_nodes = HashMap::<EntityKey, BTreeSet<NodeId>>::new();

    for _ in 0..block_count {
        let entity_dim = next_token::<u8>(&mut tokens, "Nodes", "entity dimension")?;
        let entity_tag = next_token::<u64>(&mut tokens, "Nodes", "entity tag")?;
        let parametric = next_token::<u8>(&mut tokens, "Nodes", "parametric flag")?;
        let count = next_token::<usize>(&mut tokens, "Nodes", "block node count")?;
        let mut tags = Vec::with_capacity(count);

        for _ in 0..count {
            tags.push(next_token::<u64>(&mut tokens, "Nodes", "node tag")?);
        }

        for tag in tags {
            let x = next_token::<f64>(&mut tokens, "Nodes", "x coordinate")?;
            let y = next_token::<f64>(&mut tokens, "Nodes", "y coordinate")?;
            let z = next_token::<f64>(&mut tokens, "Nodes", "z coordinate")?;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(parse_error("Nodes", "non-finite coordinate"));
            }

            if parametric != 0 {
                for _ in 0..entity_dim {
                    let _ = next_token::<f64>(&mut tokens, "Nodes", "parametric coordinate")?;
                }
            }

            let id = NodeId(to_u32("node", tag)?);
            if !node_tags.insert(tag) {
                return Err(parse_error("Nodes", format!("duplicate node tag {tag}")));
            }

            let position = Vec3::new(x as f32, y as f32, z as f32);
            if !position.is_finite() {
                return Err(parse_error("Nodes", "coordinate exceeds f32 range"));
            }
            nodes.push(FemNode { id, position });
            entity_nodes
                .entry((entity_dim, entity_tag))
                .or_default()
                .insert(id);
        }
    }

    if nodes.len() != declared_node_count {
        return Err(parse_error(
            "Nodes",
            format!(
                "header declares {declared_node_count} nodes but {} were read",
                nodes.len()
            ),
        ));
    }
    if let Some(extra) = tokens.next() {
        return Err(parse_error("Nodes", format!("unexpected token {extra}")));
    }

    Ok(ParsedNodes {
        nodes,
        node_tags,
        entity_nodes,
    })
}

#[derive(Debug)]
struct RawElementBlock {
    entity_dim: u8,
    entity_tag: u64,
    gmsh_type: u32,
    elements: Vec<RawElement>,
}

#[derive(Debug)]
struct RawElement {
    tag: u64,
    nodes: Vec<u64>,
}

fn parse_elements(
    section: &str,
    node_tags: &HashSet<u64>,
) -> Result<Vec<RawElementBlock>, GmshError> {
    let mut lines = nonempty_lines(section);
    let header = lines.next().ok_or(GmshError::MissingSection("Elements"))?;
    let header_fields = parse_fields::<u64>(header, "Elements")?;
    if header_fields.len() != 4 {
        return Err(parse_error("Elements", header));
    }
    let block_count =
        usize::try_from(header_fields[0]).map_err(|_| parse_error("Elements", header))?;
    let declared_element_count =
        usize::try_from(header_fields[1]).map_err(|_| parse_error("Elements", header))?;

    let mut blocks = Vec::with_capacity(block_count);
    let mut read_count = 0usize;
    let mut element_tags = HashSet::with_capacity(declared_element_count);

    for _ in 0..block_count {
        let block_line = lines
            .next()
            .ok_or_else(|| parse_error("Elements", "unexpected EOF in block header"))?;
        let block_fields = parse_fields::<u64>(block_line, "Elements")?;
        if block_fields.len() != 4 {
            return Err(parse_error("Elements", block_line));
        }
        let entity_dim =
            u8::try_from(block_fields[0]).map_err(|_| parse_error("Elements", block_line))?;
        let entity_tag = block_fields[1];
        let gmsh_type =
            u32::try_from(block_fields[2]).map_err(|_| parse_error("Elements", block_line))?;
        let count =
            usize::try_from(block_fields[3]).map_err(|_| parse_error("Elements", block_line))?;
        let info = gmsh_element_info(gmsh_type);
        let mut elements = Vec::with_capacity(count);

        for _ in 0..count {
            let line = lines
                .next()
                .ok_or_else(|| parse_error("Elements", "unexpected EOF in element block"))?;
            let fields = parse_fields::<u64>(line, "Elements")?;
            if fields.len() < 2 {
                return Err(parse_error("Elements", line));
            }
            if let Some(info) = info {
                if fields.len() != info.node_count + 1 {
                    return Err(parse_error(
                        "Elements",
                        format!("type {gmsh_type} expects {} nodes: {line}", info.node_count),
                    ));
                }
                if info.dimension != entity_dim {
                    return Err(parse_error(
                        "Elements",
                        format!(
                            "type {gmsh_type} has dimension {} but is stored in dimension {entity_dim}",
                            info.dimension
                        ),
                    ));
                }
            }

            let tag = fields[0];
            if !element_tags.insert(tag) {
                return Err(parse_error(
                    "Elements",
                    format!("duplicate element tag {tag}"),
                ));
            }
            for node_tag in &fields[1..] {
                if !node_tags.contains(node_tag) {
                    return Err(parse_error(
                        "Elements",
                        format!("unknown node tag {node_tag} in element {tag}"),
                    ));
                }
            }
            elements.push(RawElement {
                tag,
                nodes: fields[1..].to_vec(),
            });
        }

        read_count += elements.len();
        blocks.push(RawElementBlock {
            entity_dim,
            entity_tag,
            gmsh_type,
            elements,
        });
    }

    if read_count != declared_element_count {
        return Err(parse_error(
            "Elements",
            format!("header declares {declared_element_count} elements but {read_count} were read"),
        ));
    }
    if let Some(extra) = lines.next() {
        return Err(parse_error("Elements", extra));
    }

    Ok(blocks)
}

fn build_mesh(
    parsed_nodes: ParsedNodes,
    blocks: Vec<RawElementBlock>,
    entities: &HashMap<EntityKey, Vec<u64>>,
    physical_names: &HashMap<PhysicalKey, String>,
) -> Result<FemMesh, GmshError> {
    let analysis_dim = blocks
        .iter()
        .filter(|block| !block.elements.is_empty())
        .map(|block| block.entity_dim)
        .max()
        .ok_or_else(|| parse_error("Elements", "mesh contains no elements"))?;

    let mut elements = Vec::new();
    let mut physical_elements = BTreeMap::<PhysicalKey, BTreeSet<ElementId>>::new();

    for block in blocks
        .iter()
        .filter(|block| block.entity_dim == analysis_dim)
    {
        let conversion =
            element_conversion(block.gmsh_type).ok_or(GmshError::UnsupportedElementType {
                gmsh_type: block.gmsh_type,
                entity_dim: block.entity_dim,
            })?;
        let physical_tags = entities
            .get(&(block.entity_dim, block.entity_tag))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        for raw in &block.elements {
            let id = ElementId(to_u32("element", raw.tag)?);
            let nodes = conversion
                .hecmw_order
                .iter()
                .map(|index| to_u32("node", raw.nodes[*index]).map(NodeId))
                .collect::<Result<Vec<_>, GmshError>>()?;
            elements.push(FemElement {
                id,
                element_type: conversion.element_type.clone(),
                nodes,
            });
            for physical_tag in physical_tags {
                physical_elements
                    .entry((analysis_dim, *physical_tag))
                    .or_default()
                    .insert(id);
            }
        }
    }

    // Complete Gmsh second-order elements (Quad9/Hex27/Prism18) are reduced
    // to FrontISTR's serendipity forms. Do not leave their discarded
    // face/volume-centre nodes in the HECMW mesh as isolated solver DOFs.
    let analysis_node_ids: HashSet<NodeId> = elements
        .iter()
        .flat_map(|element| element.nodes.iter().copied())
        .collect();
    let nodes = parsed_nodes
        .nodes
        .into_iter()
        .filter(|node| analysis_node_ids.contains(&node.id))
        .collect();
    let mut mesh = FemMesh::new(nodes, elements);

    let mut physical_nodes = BTreeMap::<PhysicalKey, BTreeSet<NodeId>>::new();
    for (entity, nodes) in parsed_nodes
        .entity_nodes
        .iter()
        .filter(|(entity, _)| entity.0 < analysis_dim)
    {
        for physical_tag in entities.get(entity).into_iter().flatten() {
            physical_nodes
                .entry((entity.0, *physical_tag))
                .or_default()
                .extend(
                    nodes
                        .iter()
                        .copied()
                        .filter(|node| analysis_node_ids.contains(node)),
                );
        }
    }
    for block in blocks
        .iter()
        .filter(|block| block.entity_dim < analysis_dim)
    {
        let physical_tags = entities
            .get(&(block.entity_dim, block.entity_tag))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for physical_tag in physical_tags {
            let set = physical_nodes
                .entry((block.entity_dim, *physical_tag))
                .or_default();
            for raw in &block.elements {
                for tag in &raw.nodes {
                    let node = NodeId(to_u32("node", *tag)?);
                    if analysis_node_ids.contains(&node) {
                        set.insert(node);
                    }
                }
            }
        }
    }

    let mut physical_surfaces = BTreeMap::<PhysicalKey, BTreeSet<ElementFaceRef>>::new();
    if analysis_dim == 3 {
        let face_lookup = analysis_face_lookup(&mesh);
        for block in blocks.iter().filter(|block| block.entity_dim == 2) {
            let physical_tags = entities
                .get(&(block.entity_dim, block.entity_tag))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if physical_tags.is_empty() {
                continue;
            }
            let Some(info) = gmsh_element_info(block.gmsh_type) else {
                continue;
            };
            for raw in &block.elements {
                let mut corner_key = raw.nodes[..info.corner_count]
                    .iter()
                    .map(|tag| to_u32("node", *tag).map(NodeId))
                    .collect::<Result<Vec<_>, GmshError>>()?;
                corner_key.sort();
                let Some(faces) = face_lookup.get(&corner_key) else {
                    return Err(GmshError::UnmatchedSurfaceElement {
                        element_tag: raw.tag,
                    });
                };
                for physical_tag in physical_tags {
                    physical_surfaces
                        .entry((2, *physical_tag))
                        .or_default()
                        .extend(faces.iter().copied());
                }
            }
        }
    }

    mesh.node_sets = make_node_sets(physical_nodes, physical_names);
    mesh.element_sets = make_element_sets(physical_elements, physical_names);
    mesh.surface_sets = make_surface_sets(physical_surfaces, physical_names);
    Ok(mesh)
}

fn analysis_face_lookup(mesh: &FemMesh) -> BTreeMap<Vec<NodeId>, Vec<ElementFaceRef>> {
    let mut lookup = BTreeMap::<Vec<NodeId>, Vec<ElementFaceRef>>::new();
    for element in &mesh.elements {
        for (index, mut nodes) in element.face_node_ids().into_iter().enumerate() {
            nodes.sort();
            lookup.entry(nodes).or_default().push(ElementFaceRef::new(
                element.id,
                LocalFaceId((index + 1) as u32),
            ));
        }
    }
    lookup
}

fn make_node_sets(
    groups: BTreeMap<PhysicalKey, BTreeSet<NodeId>>,
    names: &HashMap<PhysicalKey, String>,
) -> Vec<FemNodeSet> {
    let mut used = HashSet::new();
    groups
        .into_iter()
        .filter(|(_, nodes)| !nodes.is_empty())
        .map(|(key, nodes)| FemNodeSet {
            name: unique_group_name(key, names, &mut used),
            nodes: nodes.into_iter().collect(),
        })
        .collect()
}

fn make_element_sets(
    groups: BTreeMap<PhysicalKey, BTreeSet<ElementId>>,
    names: &HashMap<PhysicalKey, String>,
) -> Vec<FemElementSet> {
    let mut used = HashSet::new();
    groups
        .into_iter()
        .filter(|(_, elements)| !elements.is_empty())
        .map(|(key, elements)| FemElementSet {
            name: unique_group_name(key, names, &mut used),
            elements: elements.into_iter().collect(),
        })
        .collect()
}

fn make_surface_sets(
    groups: BTreeMap<PhysicalKey, BTreeSet<ElementFaceRef>>,
    names: &HashMap<PhysicalKey, String>,
) -> Vec<FemSurfaceSet> {
    let mut used = HashSet::new();
    groups
        .into_iter()
        .filter(|(_, surfaces)| !surfaces.is_empty())
        .map(|(key, surfaces)| FemSurfaceSet {
            name: unique_group_name(key, names, &mut used),
            surfaces: surfaces.into_iter().collect(),
        })
        .collect()
}

fn unique_group_name(
    key: PhysicalKey,
    names: &HashMap<PhysicalKey, String>,
    used: &mut HashSet<String>,
) -> String {
    let raw = names
        .get(&key)
        .cloned()
        .unwrap_or_else(|| format!("PHYSICAL_{}_{}", key.0, key.1));
    let mut base = sanitize_group_name(&raw);
    if base.is_empty() {
        base = format!("PHYSICAL_{}_{}", key.0, key.1);
    }
    if base.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        base.insert_str(0, "G_");
    }
    if used.insert(base.clone()) {
        return base;
    }

    let unique = format!("{base}_D{}_P{}", key.0, key.1);
    used.insert(unique.clone());
    unique
}

fn sanitize_group_name(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut previous_underscore = false;
    for character in raw.trim().chars() {
        let output = if character.is_ascii_alphanumeric() || character == '_' {
            character
        } else {
            '_'
        };
        if output == '_' && previous_underscore {
            continue;
        }
        previous_underscore = output == '_';
        result.push(output);
    }
    result.trim_matches('_').to_string()
}

#[derive(Clone, Copy)]
struct GmshElementInfo {
    dimension: u8,
    node_count: usize,
    corner_count: usize,
}

fn gmsh_element_info(gmsh_type: u32) -> Option<GmshElementInfo> {
    let (dimension, node_count, corner_count) = match gmsh_type {
        1 => (1, 2, 2),
        2 => (2, 3, 3),
        3 => (2, 4, 4),
        4 => (3, 4, 4),
        5 => (3, 8, 8),
        6 => (3, 6, 6),
        7 => (3, 5, 5),
        8 => (1, 3, 2),
        9 => (2, 6, 3),
        10 => (2, 9, 4),
        11 => (3, 10, 4),
        12 => (3, 27, 8),
        13 => (3, 18, 6),
        14 => (3, 14, 5),
        15 => (0, 1, 1),
        16 => (2, 8, 4),
        17 => (3, 20, 8),
        18 => (3, 15, 6),
        19 => (3, 13, 5),
        20 => (2, 9, 3),
        21 => (2, 10, 3),
        22 => (2, 12, 3),
        23 => (2, 15, 3),
        24 => (2, 15, 3),
        25 => (2, 21, 3),
        26 => (1, 4, 2),
        27 => (1, 5, 2),
        28 => (1, 6, 2),
        29 => (3, 20, 4),
        30 => (3, 35, 4),
        31 => (3, 56, 4),
        _ => return None,
    };
    Some(GmshElementInfo {
        dimension,
        node_count,
        corner_count,
    })
}

struct ElementConversion {
    element_type: ElementType,
    /// Indices into the Gmsh connectivity, in HECMW input order.
    hecmw_order: &'static [usize],
}

const DIRECT_2: &[usize] = &[0, 1];
const DIRECT_3: &[usize] = &[0, 1, 2];
const DIRECT_4: &[usize] = &[0, 1, 2, 3];
const DIRECT_6: &[usize] = &[0, 1, 2, 3, 4, 5];
const DIRECT_8: &[usize] = &[0, 1, 2, 3, 4, 5, 6, 7];
const TRI6_HECMW: &[usize] = &[0, 1, 2, 4, 5, 3];
const TET10_HECMW: &[usize] = &[0, 1, 2, 3, 5, 6, 4, 7, 9, 8];
const PRISM15_HECMW: &[usize] = &[0, 1, 2, 3, 4, 5, 9, 7, 6, 14, 13, 12, 8, 10, 11];
const HEX20_HECMW: &[usize] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 13, 9, 16, 17, 19, 18, 10, 12, 14, 15,
];

fn element_conversion(gmsh_type: u32) -> Option<ElementConversion> {
    let (element_type, hecmw_order) = match gmsh_type {
        1 => (ElementType::Rod2, DIRECT_2),
        2 => (ElementType::Tri3, DIRECT_3),
        3 => (ElementType::Quad4, DIRECT_4),
        4 => (ElementType::Tet4, DIRECT_4),
        5 => (ElementType::Hex8, DIRECT_8),
        6 => (ElementType::Prism6, DIRECT_6),
        9 => (ElementType::Tri6, TRI6_HECMW),
        // Complete second-order Gmsh elements are reduced to the serendipity
        // forms supported by FrontISTR by dropping face/volume interior nodes.
        10 | 16 => (ElementType::Quad8, DIRECT_8),
        11 => (ElementType::Tet10, TET10_HECMW),
        12 | 17 => (ElementType::Hex20, HEX20_HECMW),
        13 | 18 => (ElementType::Prism15, PRISM15_HECMW),
        _ => return None,
    };
    Some(ElementConversion {
        element_type,
        hecmw_order,
    })
}

fn nonempty_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|line| !line.is_empty())
}

fn split_first_field(line: &str) -> Option<(&str, &str)> {
    let end = line.find(char::is_whitespace)?;
    Some((&line[..end], line[end..].trim_start()))
}

fn parse_fields<T>(line: &str, section: &'static str) -> Result<Vec<T>, GmshError>
where
    T: std::str::FromStr,
{
    line.split_whitespace()
        .map(|field| parse_str(field, section))
        .collect()
}

fn parse_str<T>(field: &str, section: &'static str) -> Result<T, GmshError>
where
    T: std::str::FromStr,
{
    field
        .parse()
        .map_err(|_| parse_error(section, format!("invalid value {field:?}")))
}

fn parse_token<T>(
    token: Option<&str>,
    section: &'static str,
    description: &str,
) -> Result<T, GmshError>
where
    T: std::str::FromStr,
{
    let token = token.ok_or_else(|| parse_error(section, format!("missing {description}")))?;
    parse_str(token, section)
}

fn next_token<'a, T>(
    tokens: &mut impl Iterator<Item = &'a str>,
    section: &'static str,
    description: &str,
) -> Result<T, GmshError>
where
    T: std::str::FromStr,
{
    parse_token(tokens.next(), section, description)
}

fn parse_error(section: &'static str, message: impl Into<String>) -> GmshError {
    GmshError::Parse {
        section,
        line: message.into(),
    }
}

fn to_u32(kind: &'static str, tag: u64) -> Result<u32, GmshError> {
    u32::try_from(tag).map_err(|_| GmshError::TagOutOfRange { kind, tag })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHYSICAL_TETRA: &str = r#"
$MeshFormat
4.1 0 8
$EndMeshFormat
$PhysicalNames
2
2 11 "fixed face"
3 21 "steel solid"
$EndPhysicalNames
$Entities
0 0 1 1
1 0 0 0 1 1 0 1 11 0
1 0 0 0 1 1 1 1 21 1 1
$EndEntities
$Nodes
1 4 1 4
3 1 0 4
1 2 3 4
0 0 0
1 0 0
0 1 0
0 0 1
$EndNodes
$Elements
2 2 10 20
2 1 2 1
10 1 2 3
3 1 4 1
20 1 2 3 4
$EndElements
"#;

    const GMSH_414_PHYSICAL_BOX: &str = include_str!("../tests/fixtures/physical_box.msh");

    #[test]
    fn imports_physical_groups_as_hecmw_sets() {
        let mesh = parse_msh41(PHYSICAL_TETRA).unwrap();

        assert_eq!(mesh.nodes.len(), 4);
        assert_eq!(mesh.elements.len(), 1);
        assert_eq!(mesh.elements[0].id, ElementId(20));
        assert_eq!(mesh.elements[0].element_type, ElementType::Tet4);
        assert_eq!(mesh.cached_boundary_faces().len(), 4);

        let fixed_nodes = mesh
            .node_sets
            .iter()
            .find(|set| set.name == "fixed_face")
            .unwrap();
        assert_eq!(fixed_nodes.nodes, vec![NodeId(1), NodeId(2), NodeId(3)]);
        assert_eq!(mesh.node_sets.len(), 1);
        assert_eq!(mesh.element_sets[0].name, "steel_solid");
        assert_eq!(mesh.element_sets[0].elements, vec![ElementId(20)]);
        assert_eq!(mesh.surface_sets[0].name, "fixed_face");
        assert_eq!(
            mesh.surface_sets[0].surfaces,
            vec![ElementFaceRef::new(ElementId(20), LocalFaceId(1))]
        );
    }

    #[test]
    fn loads_real_gmsh_414_physical_box_fixture() {
        let mesh = parse_msh41(GMSH_414_PHYSICAL_BOX).unwrap();

        assert_eq!(mesh.nodes.len(), 45);
        assert_eq!(mesh.elements.len(), 100);
        assert!(
            mesh.elements
                .iter()
                .all(|element| element.element_type == ElementType::Tet4)
        );
        assert_eq!(mesh.element_sets[0].name, "steel_solid");
        assert_eq!(mesh.element_sets[0].elements.len(), 100);
        assert_eq!(mesh.surface_sets[0].name, "fixed_face");
        assert_eq!(mesh.surface_sets[0].surfaces.len(), 14);
        assert_eq!(mesh.node_sets[0].name, "fixed_face");
        assert_eq!(mesh.node_sets[0].nodes.len(), 12);
    }

    #[test]
    fn converts_high_order_connectivity_to_hecmw_order() {
        let cases = [
            (9, TRI6_HECMW),
            (11, TET10_HECMW),
            (13, PRISM15_HECMW),
            (17, HEX20_HECMW),
        ];

        for (gmsh_type, expected) in cases {
            assert_eq!(element_conversion(gmsh_type).unwrap().hecmw_order, expected);
        }
        assert_eq!(element_conversion(10).unwrap().hecmw_order.len(), 8);
        assert_eq!(element_conversion(12).unwrap().hecmw_order.len(), 20);
        assert_eq!(element_conversion(13).unwrap().hecmw_order.len(), 15);
    }

    #[test]
    fn removes_interior_nodes_when_reducing_quad9_to_quad8() {
        let mut tags = String::new();
        let mut coordinates = String::new();
        for tag in 1..=9 {
            tags.push_str(&format!("{tag}\n"));
            coordinates.push_str(&format!("{tag} 0 0\n"));
        }
        let text = format!(
            "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n\
             $Nodes\n1 9 1 9\n2 1 0 9\n{tags}{coordinates}$EndNodes\n\
             $Elements\n1 1 1 1\n2 1 10 1\n1 1 2 3 4 5 6 7 8 9\n$EndElements\n"
        );

        let mesh = parse_msh41(&text).unwrap();
        assert_eq!(mesh.elements[0].element_type, ElementType::Quad8);
        assert_eq!(mesh.elements[0].nodes.len(), 8);
        assert_eq!(mesh.nodes.len(), 8);
        assert!(mesh.nodes.iter().all(|node| node.id != NodeId(9)));
    }

    #[test]
    fn rejects_type_20_instead_of_misreading_it_as_hex20() {
        let mut nodes = String::new();
        for tag in 1..=9 {
            nodes.push_str(&format!("{tag}\n"));
        }
        for tag in 0..9 {
            nodes.push_str(&format!("{tag} 0 0\n"));
        }
        let text = format!(
            "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n\
             $Nodes\n1 9 1 9\n2 1 0 9\n{nodes}$EndNodes\n\
             $Elements\n1 1 1 1\n2 1 20 1\n1 1 2 3 4 5 6 7 8 9\n$EndElements\n"
        );

        assert!(matches!(
            parse_msh41(&text),
            Err(GmshError::UnsupportedElementType {
                gmsh_type: 20,
                entity_dim: 2
            })
        ));
    }

    #[test]
    fn accepts_future_msh4_revisions_but_not_other_major_versions() {
        assert!(check_version("4.1 0 8").is_ok());
        assert!(check_version("4.2 0 8").is_ok());
        assert!(matches!(
            check_version("4.0 0 8"),
            Err(GmshError::UnsupportedVersion(_))
        ));
        assert!(matches!(
            check_version("5.0 0 8"),
            Err(GmshError::UnsupportedVersion(_))
        ));
        assert!(matches!(
            check_version("4.1 1 8"),
            Err(GmshError::BinaryNotSupported)
        ));
    }

    #[test]
    fn reads_parametric_nodes_and_sparse_tags() {
        let text = r#"
$MeshFormat
4.1 0 8
$EndMeshFormat
$Nodes
1 3 10 900
2 5 1 3
10 40 900
0 0 0 0.1 0.2
1 0 0 0.2 0.3
0 1 0 0.3 0.4
$EndNodes
$Elements
1 1 77 77
2 5 2 1
77 10 40 900
$EndElements
"#;
        let mesh = parse_msh41(text).unwrap();
        assert_eq!(
            mesh.elements[0].nodes,
            vec![NodeId(10), NodeId(40), NodeId(900)]
        );
    }

    #[test]
    fn exports_imported_physical_groups_to_hecmw() {
        let mesh = parse_msh41(PHYSICAL_TETRA).unwrap();
        let model = fem_core::FemModel::single_mesh("tetra", mesh);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bevyistr-gmsh-{}-{unique}.msh", std::process::id()));

        hecmw::write_msh_file(&path, &model, 0).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(path);

        assert!(text.contains("!ELEMENT,TYPE=341\n 20,1,2,3,4\n"));
        assert!(text.contains("!NGROUP,NGRP=fixed_face\n 1,2,3\n"));
        assert!(text.contains("!EGROUP,EGRP=steel_solid\n 20\n"));
        assert!(text.contains("!SGROUP,SGRP=fixed_face\n 20,1\n"));
    }
}
