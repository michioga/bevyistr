//! Parser for FrontISTR analysis control files (`.cnt`).
//!
//! `.cnt` files use a keyword syntax (`!KEYWORD, PARAM=value, ...` headers
//! followed by data lines, terminated by the next `!` line) covering
//! boundary conditions, loads, materials, and sections — see
//! `https://manual.frontistr.com/en/intro/00_cheat_sheet.html` for the full
//! reference. This parser handles the subset most relevant to a prepost
//! tool: `!BOUNDARY`, `!CLOAD`, `!DLOAD`, `!MATERIAL` (+ `!ITEM=n`
//! sub-blocks), and `!NGROUP`. A mesh-style `!SECTION,EGRP=...,MATERIAL=...`
//! found in `.cnt` is accepted for compatibility, while the documented CNT
//! `!SECTION` formulation card is left untouched. Unrecognized keywords (solver
//! settings, step control, output control, etc.) are skipped without error
//! — this is a setup *viewer*, not a solver front-end, so being lenient
//! about keywords we don't display is more useful than rejecting the file.
//!
//! # Node group resolution
//!
//! `!BOUNDARY`/`!CLOAD`/`!DLOAD` data lines reference nodes (or surfaces)
//! either directly by id or by a node-group (`NGRP`) / element-group
//! (`EGRP`) / surface-group (`SGRP`) name. Group names are resolved against
//! `!NGROUP` blocks defined in the `.cnt` file itself first, then against
//! the [`fem_core::FemMesh`]'s own `node_sets`/`element_sets`/`surface_sets`
//! (most FrontISTR models define groups in the `.msh` file and merely
//! reference them from `.cnt`).

use std::collections::HashMap;
use std::io;
use std::path::Path;

use bevy::prelude::Vec3;
use fem_core::{
    AnalysisSetup, AnalysisType, BoundaryCondition, ContactPair, ContactType, DistributedLoad,
    DistributedLoadKind, ElementId, FemMaterial, FemMesh, LinearSolverMethod, NodalLoad, NodeId,
    RotationCenter, Section, SectionKind, SolverSettings,
};

// ─── public API ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum CntError {
    Io(io::Error),
}

impl std::fmt::Display for CntError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO: {e}"),
        }
    }
}

impl std::error::Error for CntError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        let Self::Io(e) = self;

        Some(e)
    }
}

impl From<io::Error> for CntError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Parsed contents of a `.cnt` file: everything [`load_cnt_file`] could
/// extract, ready to be merged into an [`fem_core::AnalysisSetup`] by the
/// caller (which assigns `mesh_index`).
#[derive(Debug, Clone, Default)]
pub struct CntData {
    pub boundary_conditions: Vec<BoundaryCondition>,
    pub nodal_loads: Vec<NodalLoad>,
    pub distributed_loads: Vec<DistributedLoad>,
    pub materials: Vec<FemMaterial>,
    pub sections: Vec<Section>,

    pub contact_settings: Vec<CntContactSettings>,

    pub solver: Option<SolverSettings>,
}

/// Solver-side settings attached to a mesh `!CONTACT PAIR` by name.
#[derive(Debug, Clone, PartialEq)]
pub struct CntContactSettings {
    pub pair_name: String,

    pub contact_type: ContactType,

    pub friction_coefficient: f32,

    pub penalty_factor: Option<f32>,
}

impl CntData {
    /// Merges analysis-control data into an existing setup. Material
    /// definitions in `.cnt` take precedence over same-named definitions
    /// loaded from `.msh`, as specified by FrontISTR.
    pub fn merge_into(self, setup: &mut AnalysisSetup) {
        setup.boundary_conditions.extend(self.boundary_conditions);
        setup.nodal_loads.extend(self.nodal_loads);
        setup.distributed_loads.extend(self.distributed_loads);

        for material in self.materials {
            if let Some(existing) = setup
                .materials
                .iter_mut()
                .find(|existing| existing.name == material.name)
            {
                *existing = material;
            } else {
                setup.materials.push(material);
            }
        }

        // `!SECTION,EGRP=...,MATERIAL=...` is a mesh-data card. Retain
        // support for old project files that placed that form in `.cnt`,
        // replacing an identical assignment instead of duplicating it.
        for section in self.sections {
            if let Some(existing) = setup.sections.iter_mut().find(|existing| {
                existing.mesh_index == section.mesh_index
                    && existing.element_set_name == section.element_set_name
            }) {
                *existing = section;
            } else {
                setup.sections.push(section);
            }
        }

        if let Some(solver) = self.solver {
            setup.solver = solver;
        }
    }

    /// Applies `.cnt` interaction types and coefficients to contact pairs
    /// that were read from the associated `.msh` file.
    pub fn apply_contact_settings(&self, contacts: &mut [ContactPair]) -> usize {
        let mut applied = 0usize;
        for settings in &self.contact_settings {
            let Some(contact) = contacts
                .iter_mut()
                .find(|contact| contact.name.eq_ignore_ascii_case(&settings.pair_name))
            else {
                continue;
            };
            contact.contact_type = settings.contact_type;
            contact.friction_coefficient = settings.friction_coefficient;
            contact.penalty_factor = settings.penalty_factor;
            applied += 1;
        }
        applied
    }
}

/// Loads a FrontISTR `.cnt` file and resolves its node/element/surface
/// group references against `mesh`.
///
/// `mesh_index` is stamped onto every produced
/// [`BoundaryCondition`]/[`NodalLoad`]/[`DistributedLoad`]/[`Section`] so
/// the caller can merge the result into a multi-part
/// [`fem_core::AnalysisSetup`].
pub fn load_cnt_file(
    path: impl AsRef<Path>,
    mesh: &FemMesh,
    mesh_index: usize,
) -> Result<CntData, CntError> {
    let text = std::fs::read_to_string(path.as_ref())?;

    Ok(parse_cnt(&text, mesh, mesh_index))
}

// Writing a `.cnt` file back out is handled by [`crate::cnt_writer::write_cnt_file`],
// which (unlike an earlier version of this module) covers solver settings
// (`!SOLUTION`/`!STEP`/`!SOLVER`) and `!DLOAD` as well as boundary
// conditions, loads, and material models. Mesh section assignments are
// written by `msh_writer`. This module only reads.

// ─── parser ──────────────────────────────────────────────────────────────────

/// One `!KEYWORD, PARAM=value, ...` header line, split into the keyword
/// name and a map of its parameters.
struct KeywordHeader {
    name: String,
    params: HashMap<String, String>,
}

fn parse_keyword_header(line: &str) -> KeywordHeader {
    let body = line.trim_start_matches('!');
    let mut parts = body.split(',');

    let name = parts.next().unwrap_or("").trim().to_uppercase();
    let mut params = HashMap::new();

    for part in parts {
        let part = part.trim();

        if let Some(eq) = part.find('=') {
            let key = part[..eq].trim().to_uppercase();
            let value = part[eq + 1..].trim().to_string();

            params.insert(key, value);
        }
    }

    KeywordHeader { name, params }
}

/// Strips a trailing `# comment` from a `.cnt` data line, if present.
fn strip_comment(line: &str) -> &str {
    line.find('#').map(|i| &line[..i]).unwrap_or(line).trim()
}

fn parse_cnt(text: &str, mesh: &FemMesh, mesh_index: usize) -> CntData {
    let mut ngroups: HashMap<String, Vec<NodeId>> = HashMap::new();
    let mut data = CntData::default();

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    // First pass: collect any !NGROUP blocks defined inline in the .cnt
    // file, so later passes can resolve names defined either here or in
    // the mesh file.
    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("!NGROUP") || trimmed.starts_with("!NGRP") {
            let header = parse_keyword_header(trimmed);
            let name = header.params.get("NGRP").cloned().unwrap_or_default();
            i += 1;

            let mut nodes = Vec::new();

            while i < lines.len() && !lines[i].trim_start().starts_with('!') {
                let line = strip_comment(lines[i]);

                nodes.extend(
                    line.split(',')
                        .filter_map(|t| t.trim().parse::<u32>().ok())
                        .map(NodeId),
                );

                i += 1;
            }

            if !name.is_empty() {
                ngroups.insert(name, nodes);
            }
        } else {
            i += 1;
        }
    }

    // Second pass: process the keywords we care about.
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if !trimmed.starts_with('!') {
            i += 1;
            continue;
        }

        let header = parse_keyword_header(trimmed);

        match header.name.as_str() {
            "SOLUTION" => {
                let solver = data.solver.get_or_insert_with(SolverSettings::default);
                if let Some(solution_type) = header.params.get("TYPE") {
                    solver.analysis_type = match solution_type.to_ascii_uppercase().as_str() {
                        "NLSTATIC" => AnalysisType::NlStatic,
                        "DYNAMIC" => AnalysisType::Dynamic,
                        "EIGEN" | "EIGENVALUE" => AnalysisType::Eigen,
                        _ => AnalysisType::Static,
                    };
                }
                i += 1;
            }

            "STEP" => {
                let solver = data.solver.get_or_insert_with(SolverSettings::default);
                if let Some(value) = header
                    .params
                    .get("SUBSTEPS")
                    .and_then(|value| value.parse::<u32>().ok())
                {
                    solver.substeps = value;
                }
                if let Some(value) = header
                    .params
                    .get("MAXITER")
                    .or_else(|| header.params.get("ITMAX"))
                    .and_then(|value| value.parse::<u32>().ok())
                {
                    solver.max_iterations = value;
                }
                if let Some(value) = header
                    .params
                    .get("CONVERG")
                    .and_then(|value| value.parse::<f32>().ok())
                {
                    solver.convergence_tol = value;
                }
                i += 1;
            }

            "SOLVER" => {
                let solver = data.solver.get_or_insert_with(SolverSettings::default);
                if let Some(method) = header.params.get("METHOD") {
                    solver.solver_method = match method.to_ascii_uppercase().as_str() {
                        "GMRES" => LinearSolverMethod::Gmres,
                        "MUMPS" => LinearSolverMethod::Mumps,
                        "DIRECT" | "DIRECTMKL" | "MKL" | "PARDISO" | "MKL_PARDISO" => {
                            LinearSolverMethod::Direct
                        }
                        _ => LinearSolverMethod::Cg,
                    };
                }
                i += 1;
            }

            "CONTACT" => {
                let contact_type = match header
                    .params
                    .get("INTERACTION")
                    .map(|value| value.to_ascii_uppercase())
                    .as_deref()
                {
                    Some("TIED") => ContactType::Tied,
                    Some("FSLID") | Some("GLUED") => ContactType::FiniteSliding,
                    _ => ContactType::SmallSliding,
                };
                i += 1;

                while i < lines.len() && !lines[i].trim_start().starts_with('!') {
                    let line = strip_comment(lines[i]);
                    i += 1;
                    if line.is_empty() {
                        continue;
                    }

                    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
                    let Some(pair_name) = parts.first().filter(|name| !name.is_empty()) else {
                        continue;
                    };
                    data.contact_settings.push(CntContactSettings {
                        pair_name: (*pair_name).to_string(),
                        contact_type,
                        friction_coefficient: parts
                            .get(1)
                            .and_then(|value| value.parse::<f32>().ok())
                            .unwrap_or(0.0),
                        penalty_factor: parts.get(2).and_then(|value| value.parse::<f32>().ok()),
                    });
                }
            }

            "BOUNDARY" => {
                let rotation_center = header
                    .params
                    .get("ROT_CENTER")
                    .map(|token| parse_rotation_center(token, &ngroups, mesh, mesh_index));
                i += 1;

                while i < lines.len() && !lines[i].trim_start().starts_with('!') {
                    let line = strip_comment(lines[i]);
                    i += 1;

                    if line.is_empty() {
                        continue;
                    }

                    let parts: Vec<&str> = line.split(',').map(str::trim).collect();

                    if parts.len() < 4 {
                        continue;
                    }

                    let ngrp = if parts[0].parse::<u32>().is_err() {
                        Some(parts[0].to_string())
                    } else {
                        None
                    };
                    let nodes = resolve_node_group(parts[0], &ngroups, mesh);
                    let dof_start: u8 = parts[1].parse().unwrap_or(1);
                    let dof_end: u8 = parts[2].parse().unwrap_or(dof_start);
                    let value: f32 = parts[3].parse().unwrap_or(0.0);

                    if nodes.is_empty() {
                        continue;
                    }

                    data.boundary_conditions.push(BoundaryCondition {
                        name: parts[0].to_string(),
                        mesh_index,
                        nodes,
                        ngrp_name: ngrp,
                        rotation_center: rotation_center.clone(),
                        dof_start,
                        dof_end,
                        value,
                    });
                }
            }

            "CLOAD" => {
                let rotation_center = header
                    .params
                    .get("ROT_CENTER")
                    .map(|token| parse_rotation_center(token, &ngroups, mesh, mesh_index));
                i += 1;

                while i < lines.len() && !lines[i].trim_start().starts_with('!') {
                    let line = strip_comment(lines[i]);
                    i += 1;

                    if line.is_empty() {
                        continue;
                    }

                    let parts: Vec<&str> = line.split(',').map(str::trim).collect();

                    if parts.len() < 3 {
                        continue;
                    }

                    let ngrp = if parts[0].parse::<u32>().is_err() {
                        Some(parts[0].to_string())
                    } else {
                        None
                    };
                    let nodes = resolve_node_group(parts[0], &ngroups, mesh);
                    let dof: u8 = parts[1].parse().unwrap_or(1);
                    let value: f32 = parts[2].parse().unwrap_or(0.0);

                    for node in nodes {
                        data.nodal_loads.push(NodalLoad {
                            name: parts[0].to_string(),
                            mesh_index,
                            node,
                            ngrp_name: ngrp.clone(),
                            rotation_center: rotation_center.clone(),
                            dof,
                            value,
                        });
                    }
                }
            }

            "DLOAD" => {
                i += 1;

                while i < lines.len() && !lines[i].trim_start().starts_with('!') {
                    let line = strip_comment(lines[i]);
                    i += 1;

                    if line.is_empty() {
                        continue;
                    }

                    let parts: Vec<&str> = line.split(',').map(str::trim).collect();

                    if parts.len() < 3 {
                        continue;
                    }

                    let load_type = parts[1].to_uppercase();
                    let value: f32 = parts[2].parse().unwrap_or(0.0);

                    let elements = resolve_element_group(parts[0], mesh);

                    // "P<n>" (P1..P6) names a specific local face — the same
                    // face index applies to every element this line/group
                    // resolves to, matching FrontISTR's convention of one
                    // !DLOAD data line per (group, face) combination.
                    // Anything else (GRAV, BX/BY/BZ, CENT, ...) is a
                    // whole-element body force.
                    if let Some(face) = load_type
                        .strip_prefix('P')
                        .and_then(|digits| digits.parse::<u32>().ok())
                    {
                        let faces: Vec<fem_core::ElementFaceRef> = elements
                            .iter()
                            .map(|&element| {
                                fem_core::ElementFaceRef::new(element, fem_core::LocalFaceId(face))
                            })
                            .collect();

                        if !faces.is_empty() {
                            data.distributed_loads.push(DistributedLoad {
                                name: parts[0].to_string(),
                                mesh_index,
                                target: fem_core::DistributedLoadTarget::Faces(faces),
                                kind: DistributedLoadKind::Pressure,
                                value,
                                direction: None,
                            });
                        }
                    } else if !elements.is_empty() {
                        let direction = match load_type.as_str() {
                            "GRAV" => {
                                let components = parts
                                    .get(3..6)
                                    .map(|values| {
                                        values
                                            .iter()
                                            .filter_map(|value| value.parse::<f32>().ok())
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                if components.len() == 3 {
                                    Some(Vec3::new(components[0], components[1], components[2]))
                                } else {
                                    Some(Vec3::NEG_Y)
                                }
                            }
                            "BX" => Some(Vec3::X),
                            "BY" => Some(Vec3::Y),
                            "BZ" => Some(Vec3::Z),
                            _ => Some(Vec3::NEG_Y),
                        };
                        data.distributed_loads.push(DistributedLoad {
                            name: parts[0].to_string(),
                            mesh_index,
                            target: fem_core::DistributedLoadTarget::Elements(elements),
                            kind: DistributedLoadKind::Gravity,
                            value,
                            direction,
                        });
                    }
                }
            }

            "MATERIAL" => {
                let name = header
                    .params
                    .get("NAME")
                    .cloned()
                    .unwrap_or_else(|| "MATERIAL".to_string());
                let mut material = FemMaterial::new(name);
                i += 1;

                // Sub-blocks: !ITEM=1 (Young's modulus, Poisson ratio),
                // !ITEM=2 (density), !ITEM=3 (thermal expansion — ignored).
                while i < lines.len() {
                    let sub_trimmed = lines[i].trim();

                    if sub_trimmed.starts_with("!ITEM") {
                        let sub_header = parse_keyword_header(sub_trimmed);
                        let item_num = sub_header
                            .params
                            .get("ITEM")
                            .and_then(|v| v.parse::<u32>().ok())
                            .or_else(|| {
                                // Some files write `!ITEM=1` as the keyword
                                // name itself rather than a parameter.
                                sub_trimmed
                                    .trim_start_matches('!')
                                    .split('=')
                                    .nth(1)
                                    .and_then(|v| v.split(',').next())
                                    .and_then(|v| v.trim().parse::<u32>().ok())
                            })
                            .unwrap_or(0);

                        i += 1;

                        let Some(data_line) = lines.get(i).map(|l| strip_comment(l)) else {
                            break;
                        };
                        let values: Vec<f32> = data_line
                            .split(',')
                            .filter_map(|t| t.trim().parse::<f32>().ok())
                            .collect();

                        match item_num {
                            1 => {
                                if let Some(&young) = values.first() {
                                    material.young_modulus = Some(young);
                                }
                                if let Some(&poisson) = values.get(1) {
                                    material.poisson_ratio = Some(poisson);
                                }
                            }
                            2 => {
                                if let Some(&density) = values.first() {
                                    material.density = Some(density);
                                }
                            }
                            _ => {}
                        }

                        i += 1;
                    } else if sub_trimmed.starts_with("!DENSITY") {
                        // Older/simpler files use a standalone !DENSITY
                        // block instead of !MATERIAL's !ITEM=2.
                        i += 1;
                        if let Some(&density) = lines
                            .get(i)
                            .map(|l| strip_comment(l))
                            .and_then(|l| l.split(',').next())
                            .and_then(|v| v.trim().parse::<f32>().ok())
                            .as_ref()
                        {
                            material.density = Some(density);
                        }
                        i += 1;
                    } else if sub_trimmed.starts_with("!ELASTIC") {
                        i += 1;
                        if let Some(line) = lines.get(i).map(|l| strip_comment(l)) {
                            let values: Vec<f32> = line
                                .split(',')
                                .filter_map(|t| t.trim().parse::<f32>().ok())
                                .collect();
                            if let Some(&young) = values.first() {
                                material.young_modulus = Some(young);
                            }
                            if let Some(&poisson) = values.get(1) {
                                material.poisson_ratio = Some(poisson);
                            }
                        }
                        i += 1;
                    } else if sub_trimmed.starts_with('!') {
                        // Next top-level keyword — material block is done.
                        break;
                    } else {
                        // Stray data line outside a recognized sub-block.
                        i += 1;
                    }
                }

                data.materials.push(material);
            }

            "SECTION" => {
                // The analysis-control `!SECTION` card configures element
                // formulation/orientation and does not assign EGRP or
                // MATERIAL. Parse only the legacy mesh-style form that some
                // existing projects place in `.cnt` for compatibility.
                if !header.params.contains_key("EGRP") && !header.params.contains_key("MATERIAL") {
                    i += 1;
                    continue;
                }
                let section_type = header
                    .params
                    .get("TYPE")
                    .cloned()
                    .unwrap_or_default()
                    .to_uppercase();
                let egrp = header.params.get("EGRP").cloned();
                let material_name = header.params.get("MATERIAL").cloned().unwrap_or_default();
                let thickness = header
                    .params
                    .get("THICKNESS")
                    .and_then(|v| v.parse::<f32>().ok());

                let kind = match section_type.as_str() {
                    "SHELL" => SectionKind::Shell {
                        thickness: thickness.unwrap_or(1.0),
                    },
                    "BEAM" => SectionKind::Beam {
                        area: thickness.unwrap_or(1.0),
                    },
                    _ => SectionKind::Solid,
                };

                data.sections.push(Section {
                    name: egrp.clone().unwrap_or_else(|| "SECTION".to_string()),
                    mesh_index,
                    material_name,
                    element_set_name: egrp,
                    kind,
                });

                i += 1;
            }

            _ => {
                // Unrecognized / out-of-scope keyword — skip its header
                // line only; we don't know its data-line shape so we let
                // the outer loop re-scan from the next line and treat any
                // non-`!` lines as already-consumed data for *something*.
                i += 1;
            }
        }
    }

    data
}

// ─── group resolution ──────────────────────────────────────────────────────

/// Resolves a `.cnt` data-line token to a node list: a bare integer is a
/// direct node id; otherwise it's looked up first against `ngroups`
/// (groups defined inline in the `.cnt` file), then against the mesh's own
/// `node_sets` (groups defined in the `.msh` file).
fn resolve_node_group(
    token: &str,
    ngroups: &HashMap<String, Vec<NodeId>>,
    mesh: &FemMesh,
) -> Vec<NodeId> {
    if let Ok(id) = token.parse::<u32>() {
        return vec![NodeId(id)];
    }

    if let Some(nodes) = ngroups.get(token) {
        return nodes.clone();
    }

    if let Some(set) = mesh
        .node_sets
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(token))
    {
        return set.nodes.clone();
    }

    Vec::new()
}

fn parse_rotation_center(
    token: &str,
    ngroups: &HashMap<String, Vec<NodeId>>,
    mesh: &FemMesh,
    mesh_index: usize,
) -> RotationCenter {
    if let Ok(id) = token.parse::<u32>() {
        RotationCenter::from_node(mesh_index, NodeId(id))
    } else {
        let resolved_node = resolve_node_group(token, ngroups, mesh).into_iter().next();
        RotationCenter::from_group(mesh_index, token, resolved_node)
    }
}

/// Resolves a `.cnt` data-line token to an element list, analogous to
/// [`resolve_node_group`] but against the mesh's `element_sets`.
fn resolve_element_group(token: &str, mesh: &FemMesh) -> Vec<ElementId> {
    if let Ok(id) = token.parse::<u32>() {
        return vec![ElementId(id)];
    }

    if let Some(set) = mesh
        .element_sets
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(token))
    {
        return set.elements.clone();
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_gravity_direction_cosines() {
        let model = fem_core::FemModel::demo_hex8();
        let data = parse_cnt(
            "!DLOAD\n 0,GRAV,9.81,0.0,0.0,-1.0\n!END\n",
            &model.meshes[0],
            0,
        );

        assert_eq!(data.distributed_loads.len(), 1);
        assert_eq!(data.distributed_loads[0].direction, Some(Vec3::NEG_Z));
    }

    #[test]
    fn ignores_analysis_control_section_formulation_card() {
        let model = fem_core::FemModel::demo_hex8();
        let data = parse_cnt(
            "!SECTION,SECNUM=1,FORM361=FBAR\n!END\n",
            &model.meshes[0],
            0,
        );

        assert!(data.sections.is_empty());
    }

    #[test]
    fn parses_rot_center_for_boundary_and_cload() {
        let model = fem_core::FemModel::demo_hex8();
        let data = parse_cnt(
            "!NGROUP, NGRP=CENTER\n 0\n\
             !BOUNDARY, ROT_CENTER=CENTER\n 1,1,3,0.25\n\
             !CLOAD, ROT_CENTER=0\n 2,2,-4.0\n\
             !END\n",
            &model.meshes[0],
            0,
        );

        assert_eq!(
            data.boundary_conditions[0].rotation_center,
            Some(RotationCenter::from_group(0, "CENTER", Some(NodeId(0))))
        );
        assert_eq!(
            data.nodal_loads[0].rotation_center,
            Some(RotationCenter::from_node(0, NodeId(0)))
        );
    }

    #[test]
    fn cnt_material_overrides_same_named_mesh_material() {
        let mut setup = AnalysisSetup::default();
        setup.materials.push(FemMaterial {
            name: "STEEL".into(),
            young_modulus: Some(1.0),
            poisson_ratio: Some(0.1),
            density: None,
        });
        CntData {
            materials: vec![FemMaterial {
                name: "STEEL".into(),
                young_modulus: Some(210_000.0),
                poisson_ratio: Some(0.3),
                density: Some(7.85e-9),
            }],
            ..Default::default()
        }
        .merge_into(&mut setup);

        assert_eq!(setup.materials.len(), 1);
        assert_eq!(setup.materials[0].young_modulus, Some(210_000.0));
    }

    #[test]
    fn parses_contact_tutorial_solver_and_finite_sliding_settings() {
        let model = fem_core::FemModel::demo_hex8();
        let data = parse_cnt(
            "!SOLUTION, TYPE=NLSTATIC\n\
             !CONTACT_ALGO, TYPE=SLAGRANGE\n\
             !CONTACT, GRPID=1, INTERACTION=FSLID\n\
              CP1, 0.1, 1.0e+5\n\
             !STEP, SUBSTEPS=100, CONVERG=1.0e-4, MAXITER=1000\n\
              CONTACT, 1\n\
             !SOLVER, METHOD=MUMPS\n\
             !END\n",
            &model.meshes[0],
            0,
        );

        assert_eq!(data.contact_settings.len(), 1);
        assert_eq!(data.contact_settings[0].pair_name, "CP1");
        assert_eq!(
            data.contact_settings[0].contact_type,
            ContactType::FiniteSliding
        );
        assert_eq!(data.contact_settings[0].friction_coefficient, 0.1);
        assert_eq!(data.contact_settings[0].penalty_factor, Some(1.0e5));

        let solver = data.solver.as_ref().expect("solver settings");
        assert_eq!(solver.analysis_type, AnalysisType::NlStatic);
        assert_eq!(solver.substeps, 100);
        assert_eq!(solver.max_iterations, 1000);
        assert_eq!(solver.convergence_tol, 1.0e-4);
        assert_eq!(solver.solver_method, LinearSolverMethod::Mumps);

        let mut contacts = vec![ContactPair::new(
            "cp1",
            fem_core::SurfaceSetRef::new(0, 0),
            fem_core::SurfaceSetRef::new(0, 1),
            ContactType::SmallSliding,
        )];
        assert_eq!(data.apply_contact_settings(&mut contacts), 1);
        assert_eq!(contacts[0].contact_type, ContactType::FiniteSliding);
        assert_eq!(contacts[0].friction_coefficient, 0.1);
        assert_eq!(contacts[0].penalty_factor, Some(1.0e5));
    }
}
