pub mod cnt;
pub mod cnt_writer;
pub mod ctrl_reader;
pub mod ctrl_writer;
pub mod frd;
pub mod inp;
pub mod msh_writer;
pub mod project_writer;
pub mod result;
pub mod series;
pub mod vtu;

pub use cnt::{CntData, CntError, load_cnt_file};
pub use cnt_writer::{write_cnt_file, write_cnt_file_with_contacts};
pub use ctrl_reader::{HecmwCtrlContent, load_hecmw_ctrl, resolve_paths};
pub use ctrl_writer::{HecmwCtrlParams, write_hecmw_ctrl, write_parallel_hecmw_ctrl};
pub use frd::{FrdError, load_frd_file};
pub use inp::{InpError, load_inp_file};
pub use msh_writer::{
    assembly_id_offsets, remap_element, remap_node, write_msh_assembly,
    write_msh_assembly_with_setup, write_msh_file, write_msh_file_with_setup,
};
pub use project_writer::{
    FrontistrExportError, FrontistrExportSummary, FrontistrValidationIssue,
    FrontistrValidationReport, FrontistrValidationSeverity, remap_setup_for_assembly,
    validate_frontistr_project, write_frontistr_project,
};
pub use result::{ResultLoadError, load_result_file, parse_result_str};
pub use series::{detect_series, load_series};
pub use vtu::{VtuError, load_vtu_file};

use std::fmt;
use std::fs;
use std::path::Path;

use fem_core::{
    ElementFaceRef, ElementId, ElementType, FemElement, FemElementSet, FemMesh, FemNode,
    FemNodeSet, FemSurfaceSet, LocalFaceId, NodeId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HecmwParseError {
    pub line: usize,

    pub message: String,
}

impl HecmwParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for HecmwParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for HecmwParseError {}

#[derive(Debug)]
pub enum HecmwLoadError {
    Io(std::io::Error),

    Parse(HecmwParseError),
}

/// A `!CONTACT PAIR` declaration from a HEC-MW mesh file. The data line is
/// stored in FrontISTR order (`slave, master`) and resolved to surface-set
/// indices by the caller after the mesh has been inserted into a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HecmwContactPairDefinition {
    pub name: String,

    pub pair_type: HecmwContactPairType,

    pub slave_group_name: String,

    pub master_surface_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HecmwContactPairType {
    /// FrontISTR's default: slave node group against a master surface group.
    #[default]
    NodeSurface,

    SurfaceSurface,
}

impl fmt::Display for HecmwLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for HecmwLoadError {}

impl From<std::io::Error> for HecmwLoadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<HecmwParseError> for HecmwLoadError {
    fn from(error: HecmwParseError) -> Self {
        Self::Parse(error)
    }
}

enum Section {
    Ignore,

    Node,

    Element(ElementType),

    NodeGroup { index: usize, generate: bool },

    ElementGroup { index: usize, generate: bool },

    SurfaceGroup { index: usize },
}

pub fn load_mesh_file(path: impl AsRef<Path>) -> Result<FemMesh, HecmwLoadError> {
    let source = fs::read_to_string(path)?;

    parse_mesh_str(&source).map_err(Into::into)
}

/// Loads a `.msh` file and also extracts any `!MATERIAL` / `!SECTION`
/// blocks embedded in it (which FrontISTR allows alongside node/element data).
/// Returns `(FemMesh, Vec<FemMaterial>, Vec<Section>)`.
pub fn load_mesh_file_with_setup(
    path: impl AsRef<Path>,
) -> Result<(FemMesh, Vec<fem_core::FemMaterial>, Vec<fem_core::Section>), HecmwLoadError> {
    let (mesh, materials, sections, _, _) = load_mesh_file_with_setup_and_contacts(path)?;
    Ok((mesh, materials, sections))
}

/// Extended project loader that also preserves `!CONTACT PAIR` declarations.
/// This is used by Open Project; the shorter [`load_mesh_file_with_setup`]
/// remains available to callers that only need geometry and assignments.
pub fn load_mesh_file_with_setup_and_contacts(
    path: impl AsRef<Path>,
) -> Result<
    (
        FemMesh,
        Vec<fem_core::FemMaterial>,
        Vec<fem_core::Section>,
        Vec<HecmwContactPairDefinition>,
        Vec<fem_core::MpcEquation>,
    ),
    HecmwLoadError,
> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)?;
    let mesh = parse_mesh_str(&source).map_err(HecmwLoadError::from)?;
    let (materials, sections) = parse_msh_setup(&source, 0);
    let contacts = parse_msh_contact_pairs(&source);
    let equation_source = expand_msh_equation_inputs(&source, path)?;
    let equations = parse_msh_equations(&equation_source, &mesh);
    Ok((mesh, materials, sections, contacts, equations))
}

#[derive(Debug, Clone)]
struct RawMpcTerm {
    target: String,
    dof: u8,
    coefficient: f32,
}

pub(crate) const MPC_GROUP_COMMENT_PREFIX: &str = "@bevyistr-group:";

fn expand_msh_equation_inputs(source: &str, mesh_path: &Path) -> std::io::Result<String> {
    let base_dir = mesh_path.parent().unwrap_or_else(|| Path::new("."));
    expand_msh_equation_inputs_with(source, base_dir, |path| fs::read_to_string(path))
}

fn expand_msh_equation_inputs_with(
    source: &str,
    base_dir: &Path,
    mut read_include: impl FnMut(&Path) -> std::io::Result<String>,
) -> std::io::Result<String> {
    let mut expanded = String::with_capacity(source.len());

    for line in source.lines() {
        let trimmed = line.trim();
        let input = trimmed
            .to_ascii_uppercase()
            .starts_with("!EQUATION")
            .then(|| keyword_parameter(trimmed, "INPUT"))
            .flatten();

        if let Some(input) = input {
            let input = input.trim_matches(|c| c == '\'' || c == '"');
            let include_path = base_dir.join(input);
            let included = read_include(&include_path)?;

            // The HEC-MW lexer switches the input stream immediately after
            // the !EQUATION header, so the external file contains only data
            // rows rather than another !EQUATION keyword. Replacing the
            // INPUT header with an ordinary block reproduces that stream for
            // the existing parser while preserving following mesh keywords.
            expanded.push_str("!EQUATION\n");
            expanded.push_str(&included);
            if !included.ends_with('\n') && !included.ends_with('\r') {
                expanded.push('\n');
            }
        } else {
            expanded.push_str(line);
            expanded.push('\n');
        }
    }

    Ok(expanded)
}

/// Parses numeric and node-group forms of the official HEC-MW `!EQUATION`
/// block. Group equations expand pairwise to explicit node equations so the
/// in-memory representation remains unambiguous and viewport-addressable.
fn parse_msh_equations(source: &str, mesh: &FemMesh) -> Vec<fem_core::MpcEquation> {
    let lines: Vec<_> = source.lines().collect();
    let mut equations = Vec::new();
    let mut index = 0usize;
    let mut serial = 1usize;

    while index < lines.len() {
        let line = lines[index].trim();
        if !line.to_ascii_uppercase().starts_with("!EQUATION") {
            index += 1;
            continue;
        }
        index += 1;
        let mut pending_name = None;
        let mut pending_group = None;

        while index < lines.len() {
            let line = lines[index].trim();
            if line.starts_with('!') && !line.starts_with("!!") {
                break;
            }
            if line.is_empty() || line.starts_with('#') {
                index += 1;
                continue;
            }
            if let Some(comment) = line.strip_prefix("!!") {
                let comment = comment.trim();
                if let Some(group) = comment.strip_prefix(MPC_GROUP_COMMENT_PREFIX) {
                    pending_group = Some(group.trim().to_string());
                } else {
                    pending_name = Some(comment.to_string());
                }
                index += 1;
                continue;
            }

            let header: Vec<_> = line.split(',').map(str::trim).collect();
            if header[0].eq_ignore_ascii_case("LINK") {
                let linked_nodes = header.get(1..3).and_then(|items| {
                    let first = items[0].parse::<u32>().ok().map(fem_core::NodeId)?;
                    let second = items[1].parse::<u32>().ok().map(fem_core::NodeId)?;
                    (mesh.node_position(first).is_some() && mesh.node_position(second).is_some())
                        .then_some((first, second))
                });
                index += 1;

                if let Some((first, second)) = linked_nodes {
                    let name = pending_name
                        .take()
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| format!("MPC_{serial}"));
                    let group = pending_group
                        .take()
                        .filter(|group| !group.is_empty())
                        .unwrap_or_else(|| name.clone());
                    serial += 1;

                    equations.extend((1..=3).map(|dof| {
                        fem_core::MpcEquation::new(
                            format!("{name}_DOF{dof}"),
                            0.0,
                            vec![
                                fem_core::MpcTerm::new(0, first, dof, 1.0),
                                fem_core::MpcTerm::new(0, second, dof, -1.0),
                            ],
                        )
                        .with_group(group.clone())
                    }));
                }

                continue;
            }
            let Ok(term_count) = header[0].parse::<usize>() else {
                index += 1;
                continue;
            };
            let constant = header
                .get(1)
                .and_then(|value| value.parse::<f32>().ok())
                .unwrap_or(0.0);
            index += 1;
            let mut raw_terms = Vec::with_capacity(term_count);

            while index < lines.len() && raw_terms.len() < term_count {
                let term_line = lines[index].trim();
                if term_line.is_empty() || term_line.starts_with('#') || term_line.starts_with("!!")
                {
                    // HEC-MW's lexer accepts both `#` and `!!` full-line
                    // comments anywhere in an !EQUATION block. FrontISTR's
                    // own MPC regression meshes put separator comments
                    // between NEQ/CONST and the first coefficient line.
                    index += 1;
                    continue;
                }
                if term_line.starts_with('!') {
                    break;
                }
                let values: Vec<_> = term_line.split(',').map(str::trim).collect();
                for term in values.chunks_exact(3) {
                    let (Ok(dof), Ok(coefficient)) =
                        (term[1].parse::<u8>(), term[2].parse::<f32>())
                    else {
                        continue;
                    };
                    raw_terms.push(RawMpcTerm {
                        target: term[0].to_string(),
                        dof,
                        coefficient,
                    });
                    if raw_terms.len() == term_count {
                        break;
                    }
                }
                index += 1;
            }

            if raw_terms.len() != term_count {
                continue;
            }
            let name = pending_name
                .take()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("MPC_{serial}"));
            serial += 1;
            let mut expanded = expand_mpc_groups(mesh, &name, constant, &raw_terms);
            let group = pending_group
                .take()
                .filter(|group| !group.is_empty())
                .or_else(|| (expanded.len() > 1).then(|| name.clone()));
            if let Some(group) = group {
                for equation in &mut expanded {
                    equation.group = Some(group.clone());
                }
            }
            equations.extend(expanded);
        }
    }

    equations
}

fn expand_mpc_groups(
    mesh: &FemMesh,
    name: &str,
    constant: f32,
    raw_terms: &[RawMpcTerm],
) -> Vec<fem_core::MpcEquation> {
    let group_nodes = |target: &str| {
        mesh.node_sets
            .iter()
            .find(|group| group.name.eq_ignore_ascii_case(target))
            .map(|group| group.nodes.as_slice())
    };
    let uses_groups = raw_terms
        .first()
        .is_some_and(|term| term.target.parse::<u32>().is_err());

    // HEC-MW deliberately rejects equations that mix numeric node IDs and
    // node-group names (HECMW-IO-HEC-E0702). Keep the same boundary here so
    // a file accepted by bevyistr will not fail later in FrontISTR.
    if raw_terms
        .iter()
        .any(|term| term.target.parse::<u32>().is_err() != uses_groups)
    {
        return Vec::new();
    }

    let group_len = if uses_groups {
        let Some(group_len) = raw_terms
            .first()
            .and_then(|term| group_nodes(&term.target))
            .map(<[_]>::len)
        else {
            return Vec::new();
        };

        if raw_terms
            .iter()
            .any(|term| group_nodes(&term.target).is_none_or(|nodes| nodes.len() != group_len))
        {
            return Vec::new();
        }

        group_len
    } else {
        1
    };
    let all_translational_dofs = raw_terms.iter().any(|term| term.dof == 0);

    (0..group_len)
        .flat_map(|group_index| {
            let Some(terms) = raw_terms
                .iter()
                .map(|term| {
                    let node = if let Ok(id) = term.target.parse::<u32>() {
                        let node = fem_core::NodeId(id);
                        mesh.node_position(node).map(|_| node)?
                    } else {
                        *group_nodes(&term.target)?.get(group_index)?
                    };
                    Some(fem_core::MpcTerm::new(0, node, term.dof, term.coefficient))
                })
                .collect::<Option<Vec<_>>>()
            else {
                return Vec::new();
            };
            let base_name = if group_len == 1 {
                name.to_string()
            } else {
                format!("{name}_{}", group_index + 1)
            };

            if all_translational_dofs {
                (1..=3)
                    .filter_map(|dof| {
                        let terms = terms
                            .iter()
                            .cloned()
                            .map(|mut term| {
                                term.dof = dof;
                                term
                            })
                            .collect();
                        let equation = fem_core::MpcEquation::new(
                            format!("{base_name}_DOF{dof}"),
                            constant,
                            terms,
                        );
                        equation.is_valid().then_some(equation)
                    })
                    .collect::<Vec<_>>()
            } else {
                let equation = fem_core::MpcEquation::new(base_name, constant, terms);
                equation
                    .is_valid()
                    .then_some(equation)
                    .into_iter()
                    .collect()
            }
        })
        .collect()
}

fn parse_msh_contact_pairs(source: &str) -> Vec<HecmwContactPairDefinition> {
    let lines: Vec<&str> = source.lines().collect();
    let mut contacts = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index].trim();
        if !line.to_ascii_uppercase().starts_with("!CONTACT PAIR") {
            index += 1;
            continue;
        }

        let name = keyword_parameter(line, "NAME").unwrap_or_default();
        let pair_type = match keyword_parameter(line, "TYPE")
            .map(|value| value.to_ascii_uppercase())
            .as_deref()
        {
            Some("SURF-SURF") => HecmwContactPairType::SurfaceSurface,
            _ => HecmwContactPairType::NodeSurface,
        };
        index += 1;

        while index < lines.len() {
            let data_line = lines[index].trim();
            if data_line.starts_with('!') {
                break;
            }
            if data_line.is_empty() || data_line.starts_with('#') {
                index += 1;
                continue;
            }

            let surfaces: Vec<&str> = data_line
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .collect();
            if !name.is_empty() && surfaces.len() >= 2 {
                contacts.push(HecmwContactPairDefinition {
                    name: name.clone(),
                    pair_type,
                    slave_group_name: surfaces[0].to_string(),
                    master_surface_name: surfaces[1].to_string(),
                });
            }
            break;
        }
    }

    contacts
}

fn keyword_parameter(line: &str, requested: &str) -> Option<String> {
    line.split(',').skip(1).find_map(|part| {
        let (key, value) = part.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(requested)
            .then(|| value.trim().to_string())
    })
}

/// Extracts `!MATERIAL` / `!SECTION` blocks from a `.msh` source string.
fn parse_msh_setup(
    source: &str,
    mesh_index: usize,
) -> (Vec<fem_core::FemMaterial>, Vec<fem_core::Section>) {
    use fem_core::{FemMaterial, Section as FemSection, SectionKind};

    let mut materials: Vec<FemMaterial> = Vec::new();
    let mut sections: Vec<FemSection> = Vec::new();

    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if line.is_empty() || line.starts_with('#') || line.starts_with("!!") {
            i += 1;
            continue;
        }

        if !line.starts_with('!') {
            i += 1;
            continue;
        }

        let upper = line.to_ascii_uppercase();

        // ── !MATERIAL ──────────────────────────────────────────────────────
        if upper.starts_with("!MATERIAL") {
            let name = keyword_parameter(line, "NAME").unwrap_or_else(|| "MAT".to_string());

            let mut mat = FemMaterial::new(name);
            i += 1;

            // Parse !ITEM=1 / !ELASTIC / !ITEM=2 / !DENSITY sub-blocks
            while i < lines.len() {
                let sub = lines[i].trim();
                let sub_upper = sub.to_ascii_uppercase();

                if sub.is_empty() || sub.starts_with("!!") || sub.starts_with('#') {
                    i += 1;
                    continue;
                }

                if sub.starts_with('!')
                    && !sub_upper.starts_with("!ITEM")
                    && !sub_upper.starts_with("!ELASTIC")
                    && !sub_upper.starts_with("!DENSITY")
                    && !sub_upper.starts_with("!PLASTIC")
                    && !sub_upper.starts_with("!HYPERELASTIC")
                    && !sub_upper.starts_with("!VISCOSITY")
                    && !sub_upper.starts_with("!ISOTROPIC")
                {
                    break; // next top-level keyword
                }

                let item =
                    keyword_assignment_value(sub).and_then(|value| value.parse::<u32>().ok());
                if sub_upper.starts_with("!ELASTIC") || item == Some(1) {
                    i += 1;
                    if i < lines.len() {
                        let vals: Vec<f32> = lines[i]
                            .trim()
                            .split(',')
                            .filter_map(|v| v.trim().parse::<f32>().ok())
                            .collect();
                        if let Some(&e) = vals.first() {
                            mat.young_modulus = Some(e);
                        }
                        if let Some(&nu) = vals.get(1) {
                            mat.poisson_ratio = Some(nu);
                        }
                    }
                } else if sub_upper.starts_with("!DENSITY") || item == Some(2) {
                    i += 1;
                    if i < lines.len() {
                        if let Ok(rho) = lines[i]
                            .trim()
                            .split(',')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .parse::<f32>()
                        {
                            mat.density = Some(rho);
                        }
                    }
                }

                i += 1;
            }

            materials.push(mat);
            continue;
        }

        // ── !SECTION ───────────────────────────────────────────────────────
        if upper.starts_with("!SECTION") {
            let sec_type = keyword_parameter(line, "TYPE")
                .map(|value| value.to_ascii_uppercase())
                .unwrap_or_default();

            let egrp = keyword_parameter(line, "EGRP");

            let mat_name = keyword_parameter(line, "MATERIAL").unwrap_or_default();

            let kind = match sec_type.as_str() {
                "SHELL" => {
                    // Next data line may have thickness
                    i += 1;
                    let thickness = if i < lines.len() {
                        let next = lines[i].trim();
                        if !next.starts_with('!') {
                            next.split(',')
                                .next()
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(1.0)
                        } else {
                            i -= 1;
                            1.0
                        }
                    } else {
                        1.0
                    };
                    SectionKind::Shell { thickness }
                }
                "BEAM" => {
                    i += 1;
                    let area = if i < lines.len() {
                        let next = lines[i].trim();
                        if !next.starts_with('!') {
                            next.split(',')
                                .next()
                                .and_then(|v| v.trim().parse::<f32>().ok())
                                .unwrap_or(1.0)
                        } else {
                            i -= 1;
                            1.0
                        }
                    } else {
                        1.0
                    };
                    SectionKind::Beam { area }
                }
                _ => SectionKind::Solid,
            };

            sections.push(FemSection {
                name: egrp.clone().unwrap_or_else(|| "ALL".to_string()),
                mesh_index,
                material_name: mat_name,
                element_set_name: egrp,
                kind,
            });
        }

        i += 1;
    }

    (materials, sections)
}

fn keyword_assignment_value(line: &str) -> Option<&str> {
    let first = line.trim_start_matches('!').split(',').next()?;
    let (_, value) = first.split_once('=')?;
    Some(value.trim())
}

pub fn parse_mesh_str(source: &str) -> Result<FemMesh, HecmwParseError> {
    let mut mesh = FemMesh::default();
    let mut section = Section::Ignore;

    for (line_index, raw_line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('!') {
            section = parse_section(line, line_number, &mut mesh)?;
            continue;
        }

        match &section {
            Section::Ignore => {}
            Section::Node => mesh.nodes.push(parse_node(line, line_number)?),
            Section::Element(element_type) => {
                mesh.elements
                    .push(parse_element(line, line_number, element_type.clone())?)
            }
            Section::NodeGroup { index, generate } => {
                let nodes = parse_node_group_line(line, line_number, *generate)?;

                if let Some(set) = mesh.node_sets.get_mut(*index) {
                    set.nodes.extend(nodes);
                }
            }
            Section::ElementGroup { index, generate } => {
                let elements = parse_element_group_line(line, line_number, *generate)?;

                if let Some(set) = mesh.element_sets.get_mut(*index) {
                    set.elements.extend(elements);
                }
            }
            Section::SurfaceGroup { index } => {
                let surfaces = parse_surface_group_line(line, line_number)?;

                if let Some(set) = mesh.surface_sets.get_mut(*index) {
                    set.surfaces.extend(surfaces);
                }
            }
        }
    }

    mesh.rebuild_topology_cache();

    Ok(mesh)
}

fn parse_section(
    line: &str,
    line_number: usize,
    mesh: &mut FemMesh,
) -> Result<Section, HecmwParseError> {
    let command = line.to_ascii_uppercase();

    if command.starts_with("!NODE") {
        return Ok(Section::Node);
    }

    if command.starts_with("!ELEMENT") {
        let Some(raw_type) = parse_attribute(line, "TYPE") else {
            return Err(HecmwParseError::new(
                line_number,
                "missing TYPE=... in !ELEMENT section",
            ));
        };

        return Ok(Section::Element(parse_element_type(&raw_type)));
    }

    if command.starts_with("!NGROUP") {
        let name = parse_attribute(line, "NGRP")
            .ok_or_else(|| HecmwParseError::new(line_number, "missing NGRP=... in !NGROUP"))?;
        let generate = has_flag(line, "GENERATE");
        let index = mesh.node_sets.len();

        mesh.node_sets.push(FemNodeSet::new(name));

        return Ok(Section::NodeGroup { index, generate });
    }

    if command.starts_with("!EGROUP") {
        let name = parse_attribute(line, "EGRP")
            .ok_or_else(|| HecmwParseError::new(line_number, "missing EGRP=... in !EGROUP"))?;
        let generate = has_flag(line, "GENERATE");
        let index = mesh.element_sets.len();

        mesh.element_sets.push(FemElementSet::new(name));

        return Ok(Section::ElementGroup { index, generate });
    }

    if command.starts_with("!SGROUP") {
        let name = parse_attribute(line, "SGRP")
            .ok_or_else(|| HecmwParseError::new(line_number, "missing SGRP=... in !SGROUP"))?;
        let index = mesh.surface_sets.len();

        mesh.surface_sets.push(FemSurfaceSet::new(name));

        return Ok(Section::SurfaceGroup { index });
    }

    Ok(Section::Ignore)
}

fn parse_node(line: &str, line_number: usize) -> Result<FemNode, HecmwParseError> {
    let fields = split_fields(line);

    if fields.len() < 4 {
        return Err(HecmwParseError::new(
            line_number,
            "node line must contain id, x, y, z",
        ));
    }

    Ok(FemNode::from_xyz(
        NodeId(parse_u32(fields[0], line_number, "node id")?),
        parse_f32(fields[1], line_number, "x")?,
        parse_f32(fields[2], line_number, "y")?,
        parse_f32(fields[3], line_number, "z")?,
    ))
}

fn parse_element(
    line: &str,
    line_number: usize,
    element_type: ElementType,
) -> Result<FemElement, HecmwParseError> {
    let fields = split_fields(line);

    if fields.len() < 2 {
        return Err(HecmwParseError::new(
            line_number,
            "element line must contain id and at least one node id",
        ));
    }

    let id = ElementId(parse_u32(fields[0], line_number, "element id")?);
    let nodes = fields[1..]
        .iter()
        .map(|field| parse_u32(field, line_number, "element node id").map(NodeId))
        .collect::<Result<Vec<_>, _>>()?;

    if let Some(expected) = element_type.node_count() {
        if nodes.len() != expected {
            return Err(HecmwParseError::new(
                line_number,
                format!(
                    "element type requires {expected} nodes, but {} were provided",
                    nodes.len()
                ),
            ));
        }
    }

    Ok(FemElement::new(id, element_type, nodes))
}

fn parse_node_group_line(
    line: &str,
    line_number: usize,
    generate: bool,
) -> Result<Vec<NodeId>, HecmwParseError> {
    parse_group_ids(line, line_number, generate, "node group id")
        .map(|ids| ids.into_iter().map(NodeId).collect())
}

fn parse_element_group_line(
    line: &str,
    line_number: usize,
    generate: bool,
) -> Result<Vec<ElementId>, HecmwParseError> {
    parse_group_ids(line, line_number, generate, "element group id")
        .map(|ids| ids.into_iter().map(ElementId).collect())
}

fn parse_surface_group_line(
    line: &str,
    line_number: usize,
) -> Result<Vec<ElementFaceRef>, HecmwParseError> {
    let fields = split_fields(line);

    if fields.len() % 2 != 0 {
        return Err(HecmwParseError::new(
            line_number,
            "surface group line must contain element id / face id pairs",
        ));
    }

    fields
        .chunks_exact(2)
        .map(|pair| {
            Ok(ElementFaceRef::new(
                ElementId(parse_u32(pair[0], line_number, "surface element id")?),
                LocalFaceId(parse_u32(pair[1], line_number, "surface local face id")?),
            ))
        })
        .collect()
}

fn parse_group_ids(
    line: &str,
    line_number: usize,
    generate: bool,
    label: &str,
) -> Result<Vec<u32>, HecmwParseError> {
    let fields = split_fields(line);

    if !generate {
        return fields
            .iter()
            .map(|field| parse_u32(field, line_number, label))
            .collect();
    }

    if fields.len() < 2 {
        return Err(HecmwParseError::new(
            line_number,
            "GENERATE group line must contain at least start and end",
        ));
    }

    let mut ids = Vec::new();
    let mut index = 0;

    while index + 1 < fields.len() {
        let start = parse_u32(fields[index], line_number, "generate start")?;
        let end = parse_u32(fields[index + 1], line_number, "generate end")?;
        let remaining = fields.len() - index;
        let step = if remaining >= 3 {
            parse_u32(fields[index + 2], line_number, "generate step")?
        } else {
            1
        };

        if step == 0 {
            return Err(HecmwParseError::new(
                line_number,
                "GENERATE step must not be zero",
            ));
        }

        push_generated_range(&mut ids, start, end, step);
        index += if remaining >= 3 { 3 } else { 2 };
    }

    if index != fields.len() {
        return Err(HecmwParseError::new(
            line_number,
            "dangling value in GENERATE group line",
        ));
    }

    Ok(ids)
}

fn parse_element_type(raw_type: &str) -> ElementType {
    let key = raw_type.trim().trim_matches('"').to_ascii_uppercase();

    match key.as_str() {
        "LINE2" | "111" => ElementType::Rod2,
        "LINE3" | "112" => ElementType::Rod3,
        "CTRIA3" | "TRI3" | "231" => ElementType::Tri3,
        "CTRIA6" | "TRI6" | "232" => ElementType::Tri6,
        "CQUAD4" | "QUAD4" | "241" => ElementType::Quad4,
        "CQUAD8" | "QUAD8" | "242" => ElementType::Quad8,
        "CROD" | "ROD2" | "TRUSS2" | "301" => ElementType::Truss2,
        "CTETR4" | "CTETRA4" | "C3D4" | "TET4" | "341" => ElementType::Tet4,
        "CTETR10" | "CTETRA10" | "C3D10" | "TET10" | "342" => ElementType::Tet10,
        "CPENTA6" | "CPRISM6" | "PENTA6" | "PRISM6" | "351" => ElementType::Prism6,
        "CPENTA15" | "CPRISM15" | "PENTA15" | "PRISM15" | "352" => ElementType::Prism15,
        "CHEXA8" | "C3D8" | "HEX8" | "361" => ElementType::Hex8,
        "CHEXA20" | "C3D20" | "HEX20" | "362" => ElementType::Hex20,
        "CONNECTOR2" | "511" => ElementType::Connector2,
        "INTERFACE_QUAD4" | "541" => ElementType::InterfaceQuad4,
        "INTERFACE_QUAD8" | "542" => ElementType::InterfaceQuad8,
        "611" | "BEAM611" => ElementType::Beam611,
        "641" | "BEAM641" | "CBEAM" => ElementType::Beam641,
        "731" | "SHELL_TRI3" => ElementType::ShellTri3,
        "732" | "SHELL_TRI6" => ElementType::ShellTri6,
        "741" | "SHELL_QUAD4" => ElementType::ShellQuad4,
        "743" | "SHELL_QUAD9" => ElementType::ShellQuad9,
        "761" | "SHELL_TRI3_MIXED" => ElementType::ShellTri3Mixed,
        "781" | "SHELL_QUAD4_MIXED" => ElementType::ShellQuad4Mixed,
        _ => ElementType::Unsupported(key),
    }
}

fn parse_attribute(line: &str, key: &str) -> Option<String> {
    keyword_parameter(line, key).map(|value| value.trim_matches('"').to_string())
}

fn has_flag(line: &str, flag: &str) -> bool {
    line.split(',')
        .any(|part| part.trim().eq_ignore_ascii_case(flag))
}

fn push_generated_range(ids: &mut Vec<u32>, start: u32, end: u32, step: u32) {
    if start <= end {
        let mut value = start;

        while value <= end {
            ids.push(value);

            match value.checked_add(step) {
                Some(next) => value = next,
                None => break,
            }
        }
    } else {
        let mut value = start;

        while value >= end {
            ids.push(value);

            let Some(next) = value.checked_sub(step) else {
                break;
            };

            value = next;
        }
    }
}

fn split_fields(line: &str) -> Vec<&str> {
    line.split([',', ' ', '\t'])
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .collect()
}

fn parse_u32(field: &str, line_number: usize, label: &str) -> Result<u32, HecmwParseError> {
    field.parse::<u32>().map_err(|_| {
        HecmwParseError::new(
            line_number,
            format!("failed to parse {label} as unsigned integer: {field}"),
        )
    })
}

fn parse_f32(field: &str, line_number: usize, label: &str) -> Result<f32, HecmwParseError> {
    field.parse::<f32>().map_err(|_| {
        HecmwParseError::new(
            line_number,
            format!("failed to parse {label} as float: {field}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use fem_core::{
        ContactType, ElementType, FaceId, FemEntityId, FemEntityRef, FemModel, SurfaceSetRef,
    };

    use super::*;

    fn documented_element_types() -> Vec<ElementType> {
        vec![
            ElementType::Rod2,
            ElementType::Rod3,
            ElementType::Tri3,
            ElementType::Tri6,
            ElementType::Quad4,
            ElementType::Quad8,
            ElementType::Truss2,
            ElementType::Tet4,
            ElementType::Tet10,
            ElementType::Prism6,
            ElementType::Prism15,
            ElementType::Hex8,
            ElementType::Hex20,
            ElementType::Connector2,
            ElementType::InterfaceQuad4,
            ElementType::InterfaceQuad8,
            ElementType::Beam611,
            ElementType::Beam641,
            ElementType::ShellTri3,
            ElementType::ShellTri6,
            ElementType::ShellQuad4,
            ElementType::ShellQuad9,
            ElementType::ShellTri3Mixed,
            ElementType::ShellQuad4Mixed,
        ]
    }

    #[test]
    fn parses_every_documented_frontistr_element_for_visualization() {
        let mesh = parse_mesh_str(include_str!("../tests/fixtures/element_library.msh")).unwrap();
        let actual_types: Vec<_> = mesh
            .elements
            .iter()
            .map(|element| element.element_type.clone())
            .collect();

        assert_eq!(actual_types, documented_element_types());
        assert_eq!(mesh.element_sets.len(), 24);

        for element in &mesh.elements {
            assert_eq!(
                element.element_type.node_count(),
                Some(element.nodes.len()),
                "wrong connectivity count for {:?}",
                element.element_type
            );

            if element.element_type.is_beam() {
                assert!(
                    !element.edge_node_ids().is_empty(),
                    "missing display segments for {:?}",
                    element.element_type
                );
            } else {
                assert!(
                    !element.face_node_ids().is_empty(),
                    "missing display faces for {:?}",
                    element.element_type
                );
            }
        }
    }

    #[test]
    fn rejects_wrong_documented_element_connectivity_count() {
        let error = parse_mesh_str(
            r#"
!NODE
 1, 0.0, 0.0, 0.0
 2, 1.0, 0.0, 0.0
 3, 0.0, 1.0, 0.0
 4, 0.0, 0.0, 1.0
!ELEMENT, TYPE=342
 1, 1, 2, 3, 4
"#,
        )
        .unwrap_err();

        assert!(error.message.contains("requires 10 nodes"));
    }

    #[test]
    fn parses_named_hecmw_mesh() {
        let mesh = parse_mesh_str(
            r#"
!NODE
 1, 0.0, 0.0, 0.0
 2, 1.0, 0.0, 0.0
 3, 1.0, 1.0, 0.0
 4, 0.0, 1.0, 0.0
!ELEMENT, TYPE=CQUAD4
 10, 1, 2, 3, 4
"#,
        )
        .unwrap();

        assert_eq!(mesh.nodes.len(), 4);
        assert_eq!(mesh.elements.len(), 1);
        assert_eq!(mesh.elements[0].id, ElementId(10));
        assert_eq!(mesh.elements[0].element_type, ElementType::Quad4);
    }

    #[test]
    fn parses_numeric_frontistr_element_type() {
        let mesh = parse_mesh_str(
            r#"
!NODE
 1001, 0.0, 0.0, 0.0
 1002, 1.0, 0.0, 0.0
 1003, 0.0, 1.0, 0.0
 1005, 0.0, 0.0, 1.0
!ELEMENT, TYPE=341
 1, 1001, 1002, 1003, 1005
"#,
        )
        .unwrap();

        assert_eq!(mesh.elements[0].element_type, ElementType::Tet4);
        assert_eq!(mesh.derived_edges().len(), 6);
        assert_eq!(mesh.boundary_faces().len(), 4);
        assert_eq!(mesh.boundary_edges().len(), 6);
        assert_eq!(mesh.cached_edges().len(), 6);
        assert_eq!(mesh.cached_boundary_faces().len(), 4);
        assert_eq!(mesh.cached_boundary_edges().len(), 6);
        assert_eq!(mesh.cached_boundary_faces()[0].element, Some(ElementId(1)));
        assert_eq!(
            mesh.cached_boundary_faces()[0].local_face,
            Some(LocalFaceId(1))
        );
    }

    #[test]
    fn creates_surface_set_from_selected_faces() {
        let mut mesh = parse_mesh_str(
            r#"
!NODE
 1001, 0.0, 0.0, 0.0
 1002, 1.0, 0.0, 0.0
 1003, 0.0, 1.0, 0.0
 1005, 0.0, 0.0, 1.0
!ELEMENT, TYPE=341
 1, 1001, 1002, 1003, 1005
"#,
        )
        .unwrap();

        let count = mesh.push_surface_set_from_targets("PICKED", &[FemEntityId::Face(FaceId(0))]);

        assert_eq!(count, 1);
        assert_eq!(mesh.surface_sets.len(), 1);
        assert_eq!(mesh.surface_sets[0].name, "PICKED");
        assert_eq!(
            mesh.surface_sets[0].surfaces,
            vec![ElementFaceRef::new(ElementId(1), LocalFaceId(1))]
        );
    }

    #[test]
    fn creates_surface_set_from_selected_element_boundary_faces() {
        let mut mesh = parse_mesh_str(
            r#"
!NODE
 1001, 0.0, 0.0, 0.0
 1002, 1.0, 0.0, 0.0
 1003, 0.0, 1.0, 0.0
 1005, 0.0, 0.0, 1.0
!ELEMENT, TYPE=341
 1, 1001, 1002, 1003, 1005
"#,
        )
        .unwrap();

        let count = mesh.push_surface_set_from_targets(
            "ELEMENT_SURFACE",
            &[FemEntityId::Element(ElementId(1))],
        );

        assert_eq!(count, 4);
        assert_eq!(mesh.surface_sets.len(), 1);
        assert_eq!(mesh.surface_sets[0].surfaces.len(), 4);
    }

    #[test]
    fn creates_contact_pair_from_recent_surface_sets() {
        let mesh = parse_mesh_str(
            r#"
!NODE
 1001, 0.0, 0.0, 0.0
 1002, 1.0, 0.0, 0.0
 1003, 0.0, 1.0, 0.0
 1005, 0.0, 0.0, 1.0
!ELEMENT, TYPE=341
 1, 1001, 1002, 1003, 1005
"#,
        )
        .unwrap();
        let mut model = FemModel::single_mesh("tet", mesh);

        assert_eq!(
            model.create_surface_set_from_targets("MASTER", &[FemEntityRef::face(0, FaceId(0))]),
            1
        );
        assert_eq!(
            model.create_surface_set_from_targets("SLAVE", &[FemEntityRef::face(0, FaceId(1))]),
            1
        );

        let contact_index =
            model.create_contact_pair_from_recent_surface_sets("CONTACT_1", ContactType::Tied);

        assert_eq!(contact_index, Some(0));
        assert_eq!(model.contacts.len(), 1);
        assert_eq!(model.contacts[0].name, "CONTACT_1");
        assert_eq!(model.contacts[0].master, SurfaceSetRef::new(0, 0));
        assert_eq!(
            model.contacts[0].slave,
            fem_core::ContactSlaveRef::Surface(SurfaceSetRef::new(0, 1))
        );
        assert_eq!(
            model.surface_set_name(model.contacts[0].master),
            Some("MASTER")
        );
        assert_eq!(
            model.contact_slave_name(model.contacts[0].slave),
            Some("SLAVE")
        );
    }

    #[test]
    fn parses_contact_pair_in_slave_master_order() {
        let contacts = parse_msh_contact_pairs(
            "!CONTACT PAIR, NAME = CP1\n  slave_surface, master_surface\n!END\n",
        );

        assert_eq!(
            contacts,
            vec![HecmwContactPairDefinition {
                name: "CP1".to_string(),
                pair_type: HecmwContactPairType::NodeSurface,
                slave_group_name: "slave_surface".to_string(),
                master_surface_name: "master_surface".to_string(),
            }]
        );
    }

    #[test]
    fn parses_explicit_surface_to_surface_contact_pair() {
        let contacts = parse_msh_contact_pairs(
            "!CONTACT PAIR, NAME=CP2, TYPE=SURF-SURF\n slave,master\n!END\n",
        );

        assert_eq!(contacts[0].pair_type, HecmwContactPairType::SurfaceSurface);
        assert_eq!(contacts[0].slave_group_name, "slave");
    }

    #[test]
    fn parses_spaced_keyword_assignments_used_by_contact_tutorials() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
 3,0,1,0
 4,0,0,1
!ELEMENT, TYPE = 341
 1,1,2,3,4
!EGROUP, EGRP = E1
 1
!NGROUP, NGRP = slave
 1,2
!SGROUP, SGRP = master
 1,1
!MATERIAL, NAME = M1, ITEM = 1
!ITEM = 1, SUBITEM = 2
 2.1e5,0.3
!SECTION, TYPE = SOLID, EGRP = E1, MATERIAL = M1
!CONTACT PAIR, NAME = CP1
 slave,master
!END
"#;

        let mesh = parse_mesh_str(source).expect("tutorial-style mesh");
        assert_eq!(mesh.node_sets[0].name, "slave");
        assert_eq!(mesh.surface_sets[0].name, "master");

        let (materials, sections) = parse_msh_setup(source, 0);
        assert_eq!(materials[0].name, "M1");
        assert_eq!(materials[0].young_modulus, Some(2.1e5));
        assert_eq!(sections[0].element_set_name.as_deref(), Some("E1"));
        assert_eq!(sections[0].material_name, "M1");

        let contacts = parse_msh_contact_pairs(source);
        assert_eq!(contacts[0].pair_type, HecmwContactPairType::NodeSurface);
        assert_eq!(contacts[0].slave_group_name, "slave");
        assert_eq!(contacts[0].master_surface_name, "master");
    }

    #[test]
    fn parses_hecmw_groups() {
        let mesh = parse_mesh_str(
            r#"
!NODE
 1, 0.0, 0.0, 0.0
 2, 1.0, 0.0, 0.0
 3, 1.0, 1.0, 0.0
 4, 0.0, 1.0, 0.0
!ELEMENT, TYPE=CQUAD4
 10, 1, 2, 3, 4
!NGROUP, NGRP=FIXED, GENERATE
 1, 4, 1
!EGROUP, EGRP=PLATE
 10
!SGROUP, SGRP=TOP
 10, 1
"#,
        )
        .unwrap();

        assert_eq!(mesh.node_sets.len(), 1);
        assert_eq!(mesh.node_sets[0].name, "FIXED");
        assert_eq!(
            mesh.node_sets[0].nodes,
            vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4)]
        );

        assert_eq!(mesh.element_sets.len(), 1);
        assert_eq!(mesh.element_sets[0].name, "PLATE");
        assert_eq!(mesh.element_sets[0].elements, vec![ElementId(10)]);

        assert_eq!(mesh.surface_sets.len(), 1);
        assert_eq!(mesh.surface_sets[0].name, "TOP");
        assert_eq!(
            mesh.surface_sets[0].surfaces,
            vec![ElementFaceRef::new(ElementId(10), LocalFaceId(1))]
        );
    }

    #[test]
    fn parses_generate_groups_with_omitted_steps_and_multiple_ranges() {
        let mesh = parse_mesh_str(
            r#"
!NGROUP, NGRP=FIX, GENERATE
 2, 2, 1
 3, 3, 1
 1, 1, 1
 69, 69, 1
 67, 67, 1
!EGROUP, EGRP=EA04, GENERATE
 301, 309, 2
 311, 313
"#,
        )
        .unwrap();

        assert_eq!(
            mesh.node_sets[0].nodes,
            [2, 3, 1, 69, 67].map(NodeId).to_vec()
        );
        assert_eq!(
            mesh.element_sets[0].elements,
            [301, 303, 305, 307, 309, 311, 312, 313]
                .map(ElementId)
                .to_vec()
        );
    }

    #[test]
    fn parses_numeric_and_group_equations() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
 3,0,1,0
 4,1,1,0
!NGROUP,NGRP=A
 1,2
!NGROUP,NGRP=B
 3,4
!EQUATION
!! DIRECT
 2,0.0
 1,1,1.0,3,1,-1.0
!! GROUP_PAIR
 2
 A,2,1.0,B,2,-1.0
!END
"#;
        let mesh = parse_mesh_str(source).unwrap();
        let equations = parse_msh_equations(source, &mesh);

        assert_eq!(equations.len(), 3);
        assert_eq!(equations[0].name, "DIRECT");
        assert_eq!(equations[1].name, "GROUP_PAIR_1");
        assert_eq!(equations[2].terms[1].node, NodeId(4));
    }

    #[test]
    fn parses_frontistr_equations_with_separator_comments() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
!EQUATION
 2,0.0
# FrontISTR regression meshes use separators here
 1,1,1.0
 2,1,-1.0
###
 2,0.5
!! comments are also legal between the header and terms
 1,2,1.0,2,2,-1.0
!END
"#;
        let mesh = parse_mesh_str(source).unwrap();
        let equations = parse_msh_equations(source, &mesh);

        assert_eq!(equations.len(), 2);
        assert_eq!(equations[0].terms.len(), 2);
        assert_eq!(equations[1].constant, 0.5);
        assert_eq!(equations[1].terms[0].dof, 2);
    }

    #[test]
    fn expands_frontistr_dof_zero_to_three_translation_equations() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
!EQUATION
!! ALL_TRANSLATIONS
 2,0.0
 1,0,1.0,2,0,-1.0
!END
"#;
        let mesh = parse_mesh_str(source).unwrap();
        let equations = parse_msh_equations(source, &mesh);

        assert_eq!(equations.len(), 3);
        assert_eq!(
            equations
                .iter()
                .map(|equation| equation.terms[0].dof)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(equations.iter().all(|equation| {
            equation
                .terms
                .iter()
                .all(|term| term.dof == equation.terms[0].dof)
        }));
        assert!(
            equations
                .iter()
                .all(|equation| equation.group.as_deref() == Some("ALL_TRANSLATIONS"))
        );
    }

    #[test]
    fn rejects_equations_that_mix_node_ids_and_node_groups() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
!NGROUP,NGRP=B
 2
!EQUATION
 2,0.0
 1,1,1.0,B,1,-1.0
!END
"#;
        let mesh = parse_mesh_str(source).unwrap();

        assert!(parse_msh_equations(source, &mesh).is_empty());
    }

    #[test]
    fn expands_frontistr_link_shorthand_to_three_translation_equations() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
!EQUATION
!! RIGID_LINK
 LINK,1,2
!END
"#;
        let mesh = parse_mesh_str(source).unwrap();
        let equations = parse_msh_equations(source, &mesh);

        assert_eq!(equations.len(), 3);
        for (index, equation) in equations.iter().enumerate() {
            let dof = index as u8 + 1;
            assert_eq!(equation.name, format!("RIGID_LINK_DOF{dof}"));
            assert_eq!(equation.group.as_deref(), Some("RIGID_LINK"));
            assert_eq!(
                equation.terms[0],
                fem_core::MpcTerm::new(0, NodeId(1), dof, 1.0)
            );
            assert_eq!(
                equation.terms[1],
                fem_core::MpcTerm::new(0, NodeId(2), dof, -1.0)
            );
        }
    }

    #[test]
    fn preserves_bevyistr_mpc_groups_across_explicit_equations() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
!EQUATION
!! @bevyistr-group: SPIDER_1
!! SPIDER_1_U1
 2,0.0
 1,1,1.0,2,1,-1.0
!! @bevyistr-group: SPIDER_1
!! SPIDER_1_U2
 2,0.0
 1,2,1.0,2,2,-1.0
!END
"#;
        let mesh = parse_mesh_str(source).unwrap();
        let equations = parse_msh_equations(source, &mesh);

        assert_eq!(equations.len(), 2);
        assert_eq!(equations[0].name, "SPIDER_1_U1");
        assert_eq!(equations[1].name, "SPIDER_1_U2");
        assert!(
            equations
                .iter()
                .all(|equation| equation.group.as_deref() == Some("SPIDER_1"))
        );
    }

    #[test]
    fn resolves_equation_input_relative_to_the_mesh_file() {
        let source = r#"
!NODE
 1,0,0,0
 2,1,0,0
 3,2,0,0
!EQUATION
!! DIRECT
 2,0.0
 1,1,1.0,2,1,-1.0
!EQUATION, INPUT=constraints/links.eqn
!END
"#;
        let included = "!! INCLUDED\nLINK,2,3\n";
        let base_dir = Path::new("project");
        let mut requested_path = None;
        let expanded = expand_msh_equation_inputs_with(source, base_dir, |path| {
            requested_path = Some(path.to_path_buf());
            Ok(included.to_string())
        })
        .unwrap();
        let mesh = parse_mesh_str(source).unwrap();
        let equations = parse_msh_equations(&expanded, &mesh);
        let expected_path = Path::new("project").join("constraints/links.eqn");

        assert_eq!(requested_path.as_deref(), Some(expected_path.as_path()));
        assert_eq!(equations.len(), 4);
        assert_eq!(equations[0].name, "DIRECT");
        assert_eq!(equations[1].name, "INCLUDED_DOF1");
        assert_eq!(equations[3].terms[1].node, NodeId(3));
    }
}
