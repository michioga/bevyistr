//! FrontISTR project export orchestration.
//!
//! This module owns the cross-file concerns of an export: writing
//! `hecmw_ctrl.dat`, flattening a multi-part model into one mesh, and
//! remapping analysis targets into the flattened ID space. Keeping this out
//! of the UI makes export behaviour testable without Bevy systems.

use std::{collections::BTreeSet, fmt, io, path::Path};

use fem_core::{
    AnalysisSetup, AnalysisType, BoundaryCondition, ContactSlaveRef, DistributedLoad,
    DistributedLoadKind, DistributedLoadTarget, ElementFaceRef, FemModel, NodalLoad,
    RotationCenter, SectionKind,
};

use crate::msh_writer::{element_type_code, part_group_prefix};
use crate::{
    HecmwCtrlParams, assembly_id_offsets, remap_element, remap_node, write_cnt_file_with_contacts,
    write_hecmw_ctrl, write_msh_assembly_with_setup, write_msh_file_with_setup,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrontistrExportSummary {
    pub node_count: usize,
    pub element_count: usize,
    pub part_count: usize,
    pub boundary_condition_count: usize,
    pub load_count: usize,
    pub material_count: usize,
    pub section_count: usize,
    pub contact_count: usize,
    pub mpc_equation_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontistrValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontistrValidationIssue {
    pub severity: FrontistrValidationSeverity,
    pub location: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontistrValidationReport {
    pub issues: Vec<FrontistrValidationIssue>,
}

impl FrontistrValidationReport {
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == FrontistrValidationSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == FrontistrValidationSeverity::Warning)
            .count()
    }

    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    pub fn summary(&self, max_issues: usize) -> String {
        let mut lines = vec![format!(
            "Preflight: {} error(s), {} warning(s)",
            self.error_count(),
            self.warning_count()
        )];
        for issue in self.issues.iter().take(max_issues) {
            let severity = match issue.severity {
                FrontistrValidationSeverity::Error => "ERROR",
                FrontistrValidationSeverity::Warning => "WARN",
            };
            lines.push(format!("{severity} {}: {}", issue.location, issue.message));
        }
        if self.issues.len() > max_issues {
            lines.push(format!(
                "... and {} more issue(s)",
                self.issues.len() - max_issues
            ));
        }
        lines.join("\n")
    }

    fn push(
        &mut self,
        severity: FrontistrValidationSeverity,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.issues.push(FrontistrValidationIssue {
            severity,
            location: location.into(),
            message: message.into(),
        });
    }

    fn error(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.push(FrontistrValidationSeverity::Error, location, message);
    }

    fn warning(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.push(FrontistrValidationSeverity::Warning, location, message);
    }
}

#[derive(Debug)]
pub struct FrontistrExportError {
    artifact: String,
    source: Option<io::Error>,

    detail: Option<String>,
}

impl FrontistrExportError {
    fn new(artifact: impl Into<String>, source: io::Error) -> Self {
        Self {
            artifact: artifact.into(),
            source: Some(source),
            detail: None,
        }
    }

    fn validation(report: &FrontistrValidationReport) -> Self {
        Self {
            artifact: "project preflight".to_string(),
            source: None,
            detail: Some(report.summary(12)),
        }
    }
}

impl fmt::Display for FrontistrExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(source) = &self.source {
            write!(formatter, "{}: {source}", self.artifact)
        } else if let Some(detail) = &self.detail {
            write!(formatter, "{}: {detail}", self.artifact)
        } else {
            formatter.write_str(&self.artifact)
        }
    }
}

impl std::error::Error for FrontistrExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

/// Checks solver-facing references and numeric values before any project
/// files are written. Warnings describe incomplete but still exportable
/// setups; errors identify data that would be invalid or silently omitted.
pub fn validate_frontistr_project(
    model: &FemModel,
    setup: &AnalysisSetup,
) -> FrontistrValidationReport {
    let mut report = FrontistrValidationReport::default();

    if model.meshes.is_empty() {
        report.error("Model", "no meshes are loaded");
        return report;
    }

    for (mesh_index, mesh) in model.meshes.iter().enumerate() {
        let location = format!("Mesh[{}]", mesh_index + 1);
        if mesh.nodes.is_empty() {
            report.error(&location, "contains no nodes");
        }
        if mesh.elements.is_empty() {
            report.warning(&location, "contains no elements");
        }

        let mut node_ids = BTreeSet::new();
        for node in &mesh.nodes {
            if !node_ids.insert(node.id) {
                report.error(&location, format!("duplicate node ID {}", node.id.0));
            }
            if !node.position.is_finite() {
                report.error(
                    &location,
                    format!("node {} has a non-finite coordinate", node.id.0),
                );
            }
        }

        let mut element_ids = BTreeSet::new();
        for element in &mesh.elements {
            let element_location = format!("{location}/Element {}", element.id.0);
            if !element_ids.insert(element.id) {
                report.error(&location, format!("duplicate element ID {}", element.id.0));
            }
            if element_type_code(&element.element_type).is_none() {
                report.error(
                    &element_location,
                    format!(
                        "element type {:?} cannot be written as a FrontISTR element",
                        element.element_type
                    ),
                );
            }
            if element.nodes.is_empty() {
                report.error(&element_location, "has empty connectivity");
            }
            if let Some(expected) = element.element_type.node_count() {
                if element.nodes.len() != expected {
                    report.error(
                        &element_location,
                        format!(
                            "requires {expected} connectivity nodes, but has {}",
                            element.nodes.len()
                        ),
                    );
                }
            }
            for node in &element.nodes {
                if !node_ids.contains(node) {
                    report.error(
                        &element_location,
                        format!("references missing node {}", node.0),
                    );
                }
            }
        }

        for set in &mesh.node_sets {
            let set_location = format!("{location}/NGRP {}", set.name);
            for node in &set.nodes {
                if !node_ids.contains(node) {
                    report.error(&set_location, format!("contains missing node {}", node.0));
                }
            }
        }
        for set in &mesh.element_sets {
            let set_location = format!("{location}/EGRP {}", set.name);
            for element in &set.elements {
                if !element_ids.contains(element) {
                    report.error(
                        &set_location,
                        format!("contains missing element {}", element.0),
                    );
                }
            }
        }
        for set in &mesh.surface_sets {
            let set_location = format!("{location}/SGRP {}", set.name);
            for surface in &set.surfaces {
                let Some(element) = mesh
                    .elements
                    .iter()
                    .find(|element| element.id == surface.element)
                else {
                    report.error(
                        &set_location,
                        format!("contains missing element {}", surface.element.0),
                    );
                    continue;
                };
                let face_count = element.face_node_ids().len() as u32;
                if surface.local_face.0 == 0 || surface.local_face.0 > face_count {
                    report.error(
                        &set_location,
                        format!(
                            "element {} has no local face {}",
                            surface.element.0, surface.local_face.0
                        ),
                    );
                }
            }
        }
    }

    validate_analysis_setup(model, setup, &mut report);
    validate_contacts(model, setup, &mut report);
    report
}

fn validate_analysis_setup(
    model: &FemModel,
    setup: &AnalysisSetup,
    report: &mut FrontistrValidationReport,
) {
    for (index, condition) in setup.boundary_conditions.iter().enumerate() {
        let location = format!("BC[{}] {}", index + 1, condition.name);
        let Some(mesh) = model.meshes.get(condition.mesh_index) else {
            report.error(
                &location,
                format!("missing mesh {}", condition.mesh_index + 1),
            );
            continue;
        };
        validate_node_target(
            report,
            &location,
            mesh,
            &condition.nodes,
            condition.ngrp_name.as_deref(),
        );
        let max_dof = if condition.rotation_center.is_some() {
            3
        } else {
            6
        };
        if condition.dof_start == 0
            || condition.dof_start > condition.dof_end
            || condition.dof_end > max_dof
        {
            report.error(
                &location,
                format!(
                    "invalid DOF range {}..={} (allowed 1..={max_dof})",
                    condition.dof_start, condition.dof_end
                ),
            );
        }
        if !condition.value.is_finite() {
            report.error(&location, "prescribed value is not finite");
        }
        if let Some(center) = &condition.rotation_center {
            validate_rotation_center(report, &location, model, center);
        }
    }

    for (index, load) in setup.nodal_loads.iter().enumerate() {
        let location = format!("CLOAD[{}] {}", index + 1, load.name);
        let Some(mesh) = model.meshes.get(load.mesh_index) else {
            report.error(&location, format!("missing mesh {}", load.mesh_index + 1));
            continue;
        };
        validate_node_target(
            report,
            &location,
            mesh,
            &[load.node],
            load.ngrp_name.as_deref(),
        );
        let max_dof = if load.rotation_center.is_some() { 3 } else { 6 };
        if load.dof == 0 || load.dof > max_dof {
            report.error(
                &location,
                format!("invalid DOF {} (allowed 1..={max_dof})", load.dof),
            );
        }
        if !load.value.is_finite() {
            report.error(&location, "load value is not finite");
        }
        if let Some(center) = &load.rotation_center {
            validate_rotation_center(report, &location, model, center);
        }
    }

    for (index, load) in setup.distributed_loads.iter().enumerate() {
        let location = format!("DLOAD[{}] {}", index + 1, load.name);
        let Some(mesh) = model.meshes.get(load.mesh_index) else {
            report.error(&location, format!("missing mesh {}", load.mesh_index + 1));
            continue;
        };
        if load.target.is_empty() {
            report.error(&location, "has no target elements or faces");
        }
        if !load.value.is_finite() {
            report.error(&location, "load value is not finite");
        }
        if load.kind == DistributedLoadKind::Pressure
            && matches!(&load.target, DistributedLoadTarget::Elements(_))
        {
            report.warning(
                &location,
                "pressure has no local-face data; export will use P1",
            );
        }
        if load.kind == DistributedLoadKind::Gravity {
            if let Some(direction) = load.direction {
                if !direction.is_finite() || direction.length_squared() <= f32::EPSILON {
                    report.error(&location, "gravity direction must be finite and non-zero");
                }
            }
        }
        for element_id in load.target.element_ids() {
            if !mesh.elements.iter().any(|element| element.id == element_id) {
                report.error(
                    &location,
                    format!("references missing element {}", element_id.0),
                );
            }
        }
        if let DistributedLoadTarget::Faces(faces) = &load.target {
            for face in faces {
                if let Some(element) = mesh.elements.iter().find(|item| item.id == face.element) {
                    let face_count = element.face_node_ids().len() as u32;
                    if face.local_face.0 == 0 || face.local_face.0 > face_count {
                        report.error(
                            &location,
                            format!(
                                "element {} has no local face P{}",
                                face.element.0, face.local_face.0
                            ),
                        );
                    }
                }
            }
        }
    }

    let mut material_names = BTreeSet::new();
    for (index, material) in setup.materials.iter().enumerate() {
        let location = format!("Material[{}] {}", index + 1, material.name);
        if material.name.trim().is_empty() {
            report.error(&location, "name is empty");
        } else if !material_names.insert(material.name.as_str()) {
            report.error(&location, "material name is duplicated");
        }
        match material.young_modulus {
            Some(value) if value.is_finite() && value > 0.0 => {}
            Some(_) => report.error(&location, "Young's modulus must be finite and positive"),
            None => report.error(&location, "Young's modulus is not defined"),
        }
        match material.poisson_ratio {
            Some(value) if value.is_finite() && value > -1.0 && value < 0.5 => {}
            Some(_) => report.error(
                &location,
                "Poisson ratio must be finite and between -1 and 0.5",
            ),
            None => report.warning(&location, "Poisson ratio is not defined; export uses 0"),
        }
        if let Some(density) = material.density {
            if !density.is_finite() || density <= 0.0 {
                report.error(&location, "density must be finite and positive");
            }
        }
    }

    for (index, section) in setup.sections.iter().enumerate() {
        let location = format!("Section[{}] {}", index + 1, section.name);
        let Some(mesh) = model.meshes.get(section.mesh_index) else {
            report.error(
                &location,
                format!("missing mesh {}", section.mesh_index + 1),
            );
            continue;
        };
        if !setup
            .materials
            .iter()
            .any(|material| material.name == section.material_name)
        {
            report.error(
                &location,
                format!("references missing material {}", section.material_name),
            );
        }
        if let Some(group) = section.element_set_name.as_deref() {
            if !mesh.element_sets.iter().any(|set| set.name == group) {
                report.error(
                    &location,
                    format!("references missing element group {group}"),
                );
            }
        }
        match section.kind {
            SectionKind::Solid => {}
            SectionKind::Shell { thickness } => {
                if !thickness.is_finite() || thickness <= 0.0 {
                    report.error(&location, "shell thickness must be finite and positive");
                }
            }
            SectionKind::Beam { area } => {
                if !area.is_finite() || area <= 0.0 {
                    report.error(&location, "beam area must be finite and positive");
                }
            }
        }
    }

    for (index, equation) in setup.mpc_equations.iter().enumerate() {
        let location = format!("MPC[{}] {}", index + 1, equation.name);
        if !equation.is_valid() {
            report.error(
                &location,
                "requires at least two finite terms, valid DOFs, and a finite constant",
            );
        }
        for (term_index, term) in equation.terms.iter().enumerate() {
            let exists = model
                .meshes
                .get(term.mesh_index)
                .is_some_and(|mesh| mesh.node_position(term.node).is_some());
            if !exists {
                report.error(
                    &location,
                    format!(
                        "term {} references missing part {} / node {}",
                        term_index + 1,
                        term.mesh_index + 1,
                        term.node.0
                    ),
                );
            }
        }
    }

    if setup.solver.substeps == 0 {
        report.error("Solver", "substeps must be at least 1");
    }
    if setup.solver.max_iterations == 0 {
        report.error("Solver", "maximum iterations must be at least 1");
    }
    if !setup.solver.convergence_tol.is_finite() || setup.solver.convergence_tol <= 0.0 {
        report.error(
            "Solver",
            "convergence tolerance must be finite and positive",
        );
    }

    if setup.boundary_conditions.is_empty() {
        report.warning("Setup", "no boundary conditions are defined");
    }
    if setup.materials.is_empty() {
        report.warning("Setup", "no materials are defined");
    }
    if setup.sections.is_empty() {
        report.warning("Setup", "no sections are assigned");
    }
}

fn validate_node_target(
    report: &mut FrontistrValidationReport,
    location: &str,
    mesh: &fem_core::FemMesh,
    nodes: &[fem_core::NodeId],
    group: Option<&str>,
) {
    if let Some(group) = group {
        if group.trim().is_empty() {
            report.error(location, "node group name is empty");
        } else if !mesh.node_sets.iter().any(|set| set.name == group) {
            report.error(location, format!("references missing node group {group}"));
        }
        return;
    }
    if nodes.is_empty() {
        report.error(location, "has no target nodes");
    }
    for node in nodes {
        if mesh.node_position(*node).is_none() {
            report.error(location, format!("references missing node {}", node.0));
        }
    }
}

fn validate_rotation_center(
    report: &mut FrontistrValidationReport,
    location: &str,
    model: &FemModel,
    center: &RotationCenter,
) {
    let Some(mesh) = model.meshes.get(center.mesh_index) else {
        report.error(
            location,
            format!(
                "rotation center references missing mesh {}",
                center.mesh_index + 1
            ),
        );
        return;
    };
    if let Some(group) = center.ngrp_name.as_deref() {
        if group.trim().is_empty() || !mesh.node_sets.iter().any(|set| set.name == group) {
            report.error(
                location,
                format!("rotation center references missing node group {group}"),
            );
        }
    } else if let Some(node) = center.node {
        if mesh.node_position(node).is_none() {
            report.error(
                location,
                format!("rotation center references missing node {}", node.0),
            );
        }
    } else {
        report.error(location, "rotation center has no node or node group");
    }
}

fn validate_contacts(
    model: &FemModel,
    setup: &AnalysisSetup,
    report: &mut FrontistrValidationReport,
) {
    let mut names = BTreeSet::new();
    for (index, contact) in model.contacts.iter().enumerate() {
        let location = format!("Contact[{}] {}", index + 1, contact.name);
        if contact.name.trim().is_empty() {
            report.error(&location, "name is empty");
        } else if !names.insert(contact.name.as_str()) {
            report.error(&location, "contact name is duplicated");
        }
        validate_surface_ref(report, &location, model, contact.master, "master");
        match contact.slave {
            ContactSlaveRef::Surface(reference) => {
                validate_surface_ref(report, &location, model, reference, "slave")
            }
            ContactSlaveRef::Nodes(reference) => {
                let valid = model
                    .meshes
                    .get(reference.mesh_index)
                    .and_then(|mesh| mesh.node_sets.get(reference.node_set_index));
                match valid {
                    Some(set) if !set.nodes.is_empty() => {}
                    Some(_) => report.error(&location, "slave node group is empty"),
                    None => report.error(&location, "slave node group does not exist"),
                }
            }
        }
        if !contact.friction_coefficient.is_finite() || contact.friction_coefficient < 0.0 {
            report.error(
                &location,
                "friction coefficient must be finite and non-negative",
            );
        }
        if let Some(penalty) = contact.penalty_factor {
            if !penalty.is_finite() || penalty <= 0.0 {
                report.error(&location, "penalty factor must be finite and positive");
            }
        }
    }
    if !model.contacts.is_empty() && setup.solver.analysis_type != AnalysisType::NlStatic {
        report.error(
            "Solver",
            "contact definitions require Nonlinear static analysis",
        );
    }
}

fn validate_surface_ref(
    report: &mut FrontistrValidationReport,
    location: &str,
    model: &FemModel,
    reference: fem_core::SurfaceSetRef,
    side: &str,
) {
    let valid = model
        .meshes
        .get(reference.mesh_index)
        .and_then(|mesh| mesh.surface_sets.get(reference.surface_set_index));
    match valid {
        Some(set) if !set.surfaces.is_empty() => {}
        Some(_) => report.error(location, format!("{side} surface group is empty")),
        None => report.error(location, format!("{side} surface group does not exist")),
    }
}

/// Writes a complete FrontISTR input set into `dir`.
///
/// Multi-part models are flattened into one HEC-MW mesh. Analysis targets
/// are remapped using the same offsets as the mesh writer, so this function
/// is the single boundary where assembly numbering is coordinated.
pub fn write_frontistr_project(
    dir: impl AsRef<Path>,
    stem: &str,
    model: &FemModel,
    setup: &AnalysisSetup,
) -> Result<FrontistrExportSummary, FrontistrExportError> {
    let dir = dir.as_ref();
    let validation = validate_frontistr_project(model, setup);
    if validation.has_errors() {
        return Err(FrontistrExportError::validation(&validation));
    }

    write_hecmw_ctrl(
        dir,
        &HecmwCtrlParams {
            mesh_name: stem,
            cnt_name: stem,
            result_name: stem,
        },
    )
    .map_err(|error| FrontistrExportError::new("hecmw_ctrl.dat", error))?;

    let part_count = model.meshes.len();
    let mut remapped_setup;
    let setup = if part_count > 1 {
        remapped_setup = remap_setup_for_assembly(setup, &assembly_id_offsets(model));
        prefix_assembly_group_references(&mut remapped_setup, model);
        &remapped_setup
    } else {
        setup
    };

    let msh_name = format!("{stem}.msh");
    let msh_path = dir.join(&msh_name);
    let (node_count, element_count, part_count) = if part_count > 1 {
        write_msh_assembly_with_setup(&msh_path, model, setup)
            .map_err(|error| FrontistrExportError::new(&msh_name, error))?
    } else {
        let (nodes, elements) = write_msh_file_with_setup(&msh_path, model, 0, setup)
            .map_err(|error| FrontistrExportError::new(&msh_name, error))?;
        (nodes, elements, model.meshes.len())
    };

    let cnt_name = format!("{stem}.cnt");
    let cnt_path = dir.join(&cnt_name);
    let (boundary_condition_count, load_count, material_count, section_count, contact_count) =
        write_cnt_file_with_contacts(&cnt_path, setup, &model.contacts)
            .map_err(|error| FrontistrExportError::new(&cnt_name, error))?;

    Ok(FrontistrExportSummary {
        node_count,
        element_count,
        part_count,
        boundary_condition_count,
        load_count,
        material_count,
        section_count,
        contact_count,
        mpc_equation_count: setup.mpc_equations.len(),
    })
}

/// Clones an analysis setup and remaps every mesh-scoped node/element target
/// into the flattened assembly ID space.
pub fn remap_setup_for_assembly(setup: &AnalysisSetup, offsets: &[(u32, u32)]) -> AnalysisSetup {
    let mut remapped = setup.clone();

    remapped.boundary_conditions = setup
        .boundary_conditions
        .iter()
        .map(|condition| BoundaryCondition {
            nodes: condition
                .nodes
                .iter()
                .map(|&node| remap_node(offsets, condition.mesh_index, node))
                .collect(),
            rotation_center: condition
                .rotation_center
                .as_ref()
                .map(|center| remap_rotation_center(center, offsets)),
            ..condition.clone()
        })
        .collect();

    remapped.nodal_loads = setup
        .nodal_loads
        .iter()
        .map(|load| NodalLoad {
            node: remap_node(offsets, load.mesh_index, load.node),
            rotation_center: load
                .rotation_center
                .as_ref()
                .map(|center| remap_rotation_center(center, offsets)),
            ..load.clone()
        })
        .collect();

    remapped.distributed_loads = setup
        .distributed_loads
        .iter()
        .map(|load| {
            let target = match &load.target {
                DistributedLoadTarget::Elements(elements) => DistributedLoadTarget::Elements(
                    elements
                        .iter()
                        .map(|&element| remap_element(offsets, load.mesh_index, element))
                        .collect(),
                ),
                DistributedLoadTarget::Faces(faces) => DistributedLoadTarget::Faces(
                    faces
                        .iter()
                        .map(|face| {
                            ElementFaceRef::new(
                                remap_element(offsets, load.mesh_index, face.element),
                                face.local_face,
                            )
                        })
                        .collect(),
                ),
            };

            DistributedLoad {
                target,
                ..load.clone()
            }
        })
        .collect();

    remapped
}

fn remap_rotation_center(center: &RotationCenter, offsets: &[(u32, u32)]) -> RotationCenter {
    RotationCenter {
        node: center
            .node
            .map(|node| remap_node(offsets, center.mesh_index, node)),
        ..center.clone()
    }
}

/// Keeps compact node-group references in `.cnt` aligned with the part
/// prefixes used by the flattened assembly's `.msh` groups.
fn prefix_assembly_group_references(setup: &mut AnalysisSetup, model: &FemModel) {
    for condition in &mut setup.boundary_conditions {
        if let Some(group) = &mut condition.ngrp_name {
            *group = format!(
                "{}_{}",
                part_group_prefix(model, condition.mesh_index),
                group
            );
        }
        prefix_rotation_center_group(condition.rotation_center.as_mut(), model);
    }
    for load in &mut setup.nodal_loads {
        if let Some(group) = &mut load.ngrp_name {
            *group = format!("{}_{}", part_group_prefix(model, load.mesh_index), group);
        }
        prefix_rotation_center_group(load.rotation_center.as_mut(), model);
    }
}

fn prefix_rotation_center_group(center: Option<&mut RotationCenter>, model: &FemModel) {
    let Some(center) = center else {
        return;
    };
    if let Some(group) = &mut center.ngrp_name {
        *group = format!("{}_{}", part_group_prefix(model, center.mesh_index), group);
    }
}

#[cfg(test)]
mod tests {
    use fem_core::{
        BoundaryCondition, ContactPair, ContactType, DistributedLoad, DistributedLoadKind,
        DistributedLoadTarget, ElementFaceRef, ElementId, FemMesh, FemSurfaceSet, LocalFaceId,
        MpcEquation, MpcTerm, NodalLoad, NodeId, SectionKind, SurfaceSetRef,
    };

    use super::*;

    #[test]
    fn remaps_every_mesh_scoped_analysis_target() {
        let setup = AnalysisSetup {
            boundary_conditions: vec![BoundaryCondition {
                name: "fixed".into(),
                mesh_index: 1,
                nodes: vec![NodeId(2)],
                ngrp_name: None,
                rotation_center: Some(RotationCenter::from_node(1, NodeId(4))),
                dof_start: 1,
                dof_end: 3,
                value: 0.0,
            }],
            nodal_loads: vec![NodalLoad {
                name: "force".into(),
                mesh_index: 1,
                node: NodeId(3),
                ngrp_name: None,
                rotation_center: Some(RotationCenter::from_node(1, NodeId(5))),
                dof: 1,
                value: 10.0,
            }],
            distributed_loads: vec![DistributedLoad {
                name: "pressure".into(),
                mesh_index: 1,
                target: DistributedLoadTarget::Faces(vec![ElementFaceRef::new(
                    ElementId(4),
                    LocalFaceId(2),
                )]),
                kind: DistributedLoadKind::Pressure,
                value: 2.0,
                direction: None,
            }],
            ..Default::default()
        };

        let remapped = remap_setup_for_assembly(&setup, &[(0, 0), (100, 200)]);

        assert_eq!(remapped.boundary_conditions[0].nodes, vec![NodeId(102)]);
        assert_eq!(remapped.nodal_loads[0].node, NodeId(103));
        assert_eq!(
            remapped.boundary_conditions[0]
                .rotation_center
                .as_ref()
                .and_then(|center| center.node),
            Some(NodeId(104))
        );
        assert_eq!(
            remapped.nodal_loads[0]
                .rotation_center
                .as_ref()
                .and_then(|center| center.node),
            Some(NodeId(105))
        );
        assert_eq!(
            remapped.distributed_loads[0].target,
            DistributedLoadTarget::Faces(vec![
                ElementFaceRef::new(ElementId(204), LocalFaceId(2),)
            ])
        );
    }

    #[test]
    fn prefixes_compact_group_references_for_assembly() {
        let mut model = FemModel::demo_hex8();
        let second_mesh = model.meshes[0].clone();
        model.add_mesh("SECOND", second_mesh);
        let mut setup = AnalysisSetup {
            boundary_conditions: vec![BoundaryCondition {
                name: "fixed".into(),
                mesh_index: 1,
                nodes: vec![NodeId(0)],
                ngrp_name: Some("FIX".into()),
                rotation_center: Some(RotationCenter::from_group(1, "CENTER", Some(NodeId(0)))),
                dof_start: 1,
                dof_end: 3,
                value: 0.0,
            }],
            nodal_loads: vec![NodalLoad {
                name: "force".into(),
                mesh_index: 1,
                node: NodeId(0),
                ngrp_name: Some("LOAD".into()),
                rotation_center: None,
                dof: 1,
                value: 1.0,
            }],
            ..Default::default()
        };

        prefix_assembly_group_references(&mut setup, &model);

        assert_eq!(
            setup.boundary_conditions[0].ngrp_name.as_deref(),
            Some("SECOND_FIX")
        );
        assert_eq!(
            setup.nodal_loads[0].ngrp_name.as_deref(),
            Some("SECOND_LOAD")
        );
        assert_eq!(
            setup.boundary_conditions[0]
                .rotation_center
                .as_ref()
                .and_then(|center| center.ngrp_name.as_deref()),
            Some("SECOND_CENTER")
        );
    }

    #[test]
    fn writes_coordinated_contact_definitions_to_mesh_and_control_files() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bevyistr_frontistr_export_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir(&dir).unwrap();

        let mut model = FemModel::demo_hex8();
        model.meshes[0].surface_sets = vec![
            FemSurfaceSet {
                name: "MASTER".into(),
                surfaces: vec![ElementFaceRef::new(ElementId(0), LocalFaceId(1))],
            },
            FemSurfaceSet {
                name: "SLAVE".into(),
                surfaces: vec![ElementFaceRef::new(ElementId(0), LocalFaceId(2))],
            },
        ];
        model.contacts.push(ContactPair::new(
            "CP1",
            SurfaceSetRef::new(0, 0),
            SurfaceSetRef::new(0, 1),
            ContactType::Tied,
        ));

        let mut setup = AnalysisSetup::default();
        setup.solver.analysis_type = AnalysisType::NlStatic;
        let summary = write_frontistr_project(&dir, "contact", &model, &setup).unwrap();
        let mesh_text = std::fs::read_to_string(dir.join("contact.msh")).unwrap();
        let control_text = std::fs::read_to_string(dir.join("contact.cnt")).unwrap();

        assert_eq!(summary.contact_count, 1);
        assert!(mesh_text.contains("!CONTACT PAIR, NAME=CP1, TYPE=SURF-SURF"));
        assert!(mesh_text.contains(" SLAVE,MASTER"));
        assert!(control_text.contains("!CONTACT, GRPID=1, INTERACTION=TIED"));
        assert!(control_text.contains(" CP1"));

        for file in ["hecmw_ctrl.dat", "contact.msh", "contact.cnt"] {
            std::fs::remove_file(dir.join(file)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn preflight_reports_invalid_solver_references_and_values() {
        let model = FemModel::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.boundary_conditions.push(BoundaryCondition {
            name: "BAD_BC".into(),
            mesh_index: 0,
            nodes: vec![NodeId(999)],
            ngrp_name: None,
            rotation_center: None,
            dof_start: 0,
            dof_end: 7,
            value: f32::NAN,
        });
        setup.mpc_equations.push(MpcEquation::new(
            "BAD_MPC",
            0.0,
            vec![
                MpcTerm::new(0, NodeId(0), 1, 1.0),
                MpcTerm::new(1, NodeId(1), 1, -1.0),
            ],
        ));
        setup.solver.substeps = 0;

        let report = validate_frontistr_project(&model, &setup);

        assert!(report.has_errors());
        let summary = report.summary(20);
        assert!(summary.contains("BC[1] BAD_BC: references missing node 999"));
        assert!(summary.contains("invalid DOF range 0..=7"));
        assert!(summary.contains("MPC[1] BAD_MPC: term 2 references missing part 2"));
        assert!(summary.contains("Solver: substeps must be at least 1"));
    }

    #[test]
    fn failed_preflight_does_not_write_partial_project_files() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bevyistr_frontistr_invalid_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir(&dir).unwrap();
        let model = FemModel::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.mpc_equations.push(MpcEquation::new(
            "INVALID",
            0.0,
            vec![MpcTerm::new(0, NodeId(0), 1, 1.0)],
        ));

        let error = write_frontistr_project(&dir, "invalid", &model, &setup).unwrap_err();

        assert!(error.to_string().contains("project preflight"));
        assert!(!dir.join("hecmw_ctrl.dat").exists());
        assert!(!dir.join("invalid.msh").exists());
        assert!(!dir.join("invalid.cnt").exists());
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn assembly_export_offsets_mpc_nodes_exactly_once() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bevyistr_frontistr_mpc_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir(&dir).unwrap();

        let mut model = FemModel::demo_hex8();
        model.add_mesh("SECOND", FemMesh::demo_hex8());
        let mut setup = AnalysisSetup::default();
        setup.mpc_equations.push(MpcEquation::new(
            "PART_LINK",
            0.0,
            vec![
                MpcTerm::new(0, NodeId(1), 1, 1.0),
                MpcTerm::new(1, NodeId(1), 1, -1.0),
            ],
        ));

        let summary = write_frontistr_project(&dir, "mpc", &model, &setup).unwrap();
        let mesh_text = std::fs::read_to_string(dir.join("mpc.msh")).unwrap();

        assert_eq!(summary.mpc_equation_count, 1);
        assert!(mesh_text.contains(" 1,1,1.000000000e0,9,1,-1.000000000e0"));

        for file in ["hecmw_ctrl.dat", "mpc.msh", "mpc.cnt"] {
            std::fs::remove_file(dir.join(file)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn separates_mesh_assignments_from_analysis_control() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bevyistr_frontistr_sections_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir(&dir).unwrap();

        let model = FemModel::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.add_material("STEEL", Some(210_000.0), Some(0.3), Some(7.85e-9));
        setup.add_section(0, "STEEL", None, SectionKind::Solid);

        write_frontistr_project(&dir, "solid", &model, &setup).unwrap();
        let mesh_text = std::fs::read_to_string(dir.join("solid.msh")).unwrap();
        let control_text = std::fs::read_to_string(dir.join("solid.cnt")).unwrap();

        assert!(mesh_text.contains("!EGROUP,EGRP=ALL"));
        assert!(mesh_text.contains("!MATERIAL,NAME=STEEL,ITEM=2"));
        assert!(mesh_text.contains("!SECTION,TYPE=SOLID,EGRP=ALL,MATERIAL=STEEL"));
        assert!(control_text.contains("!MATERIAL, NAME=STEEL"));
        assert!(!control_text.contains("!SECTION"));
        assert!(control_text.contains("!output_type=VTK"));

        for file in ["hecmw_ctrl.dat", "solid.msh", "solid.cnt"] {
            std::fs::remove_file(dir.join(file)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
