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

use crate::{
    HecmwCtrlParams, assembly_id_offsets, remap_element, remap_node, write_cnt_file_with_contacts,
    write_hecmw_ctrl, write_msh_assembly, write_msh_file,
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

    let msh_name = format!("{stem}.msh");
    let msh_path = dir.join(&msh_name);
    let (node_count, element_count, part_count) = if model.meshes.len() > 1 {
        write_msh_assembly(&msh_path, model)
            .map_err(|error| FrontistrExportError::new(&msh_name, error))?
    } else {
        let (nodes, elements) = write_msh_file(&msh_path, model, 0)
            .map_err(|error| FrontistrExportError::new(&msh_name, error))?;
        (nodes, elements, model.meshes.len())
    };

    let remapped_setup;
    let setup = if part_count > 1 {
        remapped_setup = remap_setup_for_assembly(setup, &assembly_id_offsets(model));
        &remapped_setup
    } else {
        setup
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

#[cfg(test)]
mod tests {
    use fem_core::{
        BoundaryCondition, ContactPair, ContactType, DistributedLoad, DistributedLoadKind,
        DistributedLoadTarget, ElementFaceRef, ElementId, FemSurfaceSet, LocalFaceId, NodalLoad,
        NodeId, SurfaceSetRef,
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
}
