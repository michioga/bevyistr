//! FrontISTR project export orchestration.
//!
//! This module owns the cross-file concerns of an export: writing
//! `hecmw_ctrl.dat`, flattening a multi-part model into one mesh, and
//! remapping analysis targets into the flattened ID space. Keeping this out
//! of the UI makes export behaviour testable without Bevy systems.

use std::{fmt, io, path::Path};

use fem_core::{
    AnalysisSetup, BoundaryCondition, DistributedLoad, DistributedLoadTarget, ElementFaceRef,
    FemModel, NodalLoad,
};

use crate::msh_writer::part_group_prefix;
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
}

#[derive(Debug)]
pub struct FrontistrExportError {
    artifact: String,
    source: io::Error,
}

impl FrontistrExportError {
    fn new(artifact: impl Into<String>, source: io::Error) -> Self {
        Self {
            artifact: artifact.into(),
            source,
        }
    }
}

impl fmt::Display for FrontistrExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.artifact, self.source)
    }
}

impl std::error::Error for FrontistrExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
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
            ..condition.clone()
        })
        .collect();

    remapped.nodal_loads = setup
        .nodal_loads
        .iter()
        .map(|load| NodalLoad {
            node: remap_node(offsets, load.mesh_index, load.node),
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
    }
    for load in &mut setup.nodal_loads {
        if let Some(group) = &mut load.ngrp_name {
            *group = format!("{}_{}", part_group_prefix(model, load.mesh_index), group);
        }
    }
}

#[cfg(test)]
mod tests {
    use fem_core::{
        BoundaryCondition, ContactPair, ContactType, DistributedLoad, DistributedLoadKind,
        DistributedLoadTarget, ElementFaceRef, ElementId, FemSurfaceSet, LocalFaceId, NodalLoad,
        NodeId, SectionKind, SurfaceSetRef,
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
                dof_start: 1,
                dof_end: 3,
                value: 0.0,
            }],
            nodal_loads: vec![NodalLoad {
                name: "force".into(),
                mesh_index: 1,
                node: NodeId(3),
                ngrp_name: None,
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
                dof_start: 1,
                dof_end: 3,
                value: 0.0,
            }],
            nodal_loads: vec![NodalLoad {
                name: "force".into(),
                mesh_index: 1,
                node: NodeId(0),
                ngrp_name: Some("LOAD".into()),
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

        let summary =
            write_frontistr_project(&dir, "contact", &model, &AnalysisSetup::default()).unwrap();
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
