//! Standalone ASCII VTK result geometry, never installed into the pre-model.
use crate::vtu::{arrays, attr_value, parse_vtu, section};
use fem_core::{
    ElementId, ElementType, FemElement, FemMesh, FemModel, FemNode, NodeId, ResultField, StepResult,
};
use std::path::Path;

pub struct ResultScene {
    pub model: FemModel,
    pub steps: Vec<Vec<StepResult>>,
}

fn integers(source: &str, name: &str) -> Result<Vec<usize>, String> {
    for tail in source.split("<DataArray").skip(1) {
        let (head, body) = tail.split_once('>').ok_or("Invalid DataArray")?;
        if attr_value(head, "Name") != Some(name) {
            continue;
        }
        if attr_value(head, "format").is_some_and(|s| s != "ascii") {
            return Err("Only ASCII VTK is supported".into());
        }
        return body
            .split("</DataArray>")
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(|s| s.parse().map_err(|_| format!("Invalid {name} integer {s}")))
            .collect();
    }
    Err(format!("Missing {name}"))
}

fn parse_scene_piece(source: &str) -> Result<(FemMesh, StepResult), String> {
    let point_block = section(source, "Points")
        .map_err(|e| e.to_string())?
        .ok_or("Missing Points")?;
    let coords = arrays(&point_block.replacen("<DataArray", "<DataArray Name=\"Coordinates\"", 1))
        .map_err(|e| e.to_string())?;
    let [coords] = coords.as_slice() else {
        return Err("Expected one coordinate array".into());
    };
    if coords.n_comp != 3 || coords.values.len() % 3 != 0 {
        return Err("Invalid coordinates".into());
    }
    let nodes: Vec<_> = coords
        .values
        .chunks_exact(3)
        .enumerate()
        .map(|(i, p)| FemNode::from_xyz(NodeId(i as u32), p[0], p[1], p[2]))
        .collect();
    let cells = section(source, "Cells")
        .map_err(|e| e.to_string())?
        .ok_or("Missing Cells")?;
    let connectivity = integers(cells, "connectivity")?;
    let offsets = integers(cells, "offsets")?;
    let types = integers(cells, "types")?;
    if offsets.len() != types.len() || offsets.last().copied() != Some(connectivity.len()) {
        return Err("Invalid VTK cell offsets/count".into());
    }
    let mut elements = Vec::new();
    let mut start = 0;
    for (i, (&end, &kind)) in offsets.iter().zip(&types).enumerate() {
        let ty = match kind {
            3 => ElementType::Rod2,
            5 => ElementType::Tri3,
            9 => ElementType::Quad4,
            10 => ElementType::Tet4,
            12 => ElementType::Hex8,
            13 => ElementType::Prism6,
            21 => ElementType::Rod3,
            22 => ElementType::Tri6,
            23 => ElementType::Quad8,
            24 => ElementType::Tet10,
            25 => ElementType::Hex20,
            26 => ElementType::Prism15,
            28 => ElementType::ShellQuad9,
            _ => {
                return Err(format!(
                    "Unsupported VTK cell type {kind}; no partial model was loaded"
                ));
            }
        };
        let cell = connectivity
            .get(start..end)
            .ok_or("Invalid connectivity offsets")?;
        if Some(cell.len()) != ty.node_count() || cell.iter().any(|&id| id >= nodes.len()) {
            return Err("Invalid VTK cell connectivity".into());
        }
        let mut ids: Vec<_> = cell.iter().map(|&id| NodeId(id as u32)).collect();
        if kind == 24 {
            // Invert FrontISTR's table342 permutation.
            ids[4] = NodeId(cell[5] as u32);
            ids[5] = NodeId(cell[6] as u32);
            ids[6] = NodeId(cell[4] as u32);
        }
        elements.push(FemElement {
            id: ElementId(i as u32),
            element_type: ty,
            nodes: ids,
        });
        start = end;
    }
    let ids = nodes.iter().map(|n| n.id).collect::<Vec<_>>();
    let mut step = parse_vtu(source, &ids).map_err(|e| e.to_string())?;
    if let Some(block) = section(source, "CellData").map_err(|e| e.to_string())? {
        for raw in arrays(block).map_err(|e| e.to_string())? {
            if raw.values.len() != elements.len() * raw.n_comp {
                return Err(format!("Invalid cell field {}", raw.name));
            }
            for c in 0..raw.n_comp {
                let values: Vec<_> = raw.values.chunks_exact(raw.n_comp).map(|v| v[c]).collect();
                let name = if raw.n_comp == 1 {
                    format!("{} (element)", raw.name)
                } else {
                    format!("{}[{}] (element)", raw.name, c + 1)
                };
                let min = values.iter().copied().fold(f32::INFINITY, f32::min);
                let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                step.fields.push(ResultField::ElementScalar {
                    name,
                    values,
                    min,
                    max,
                });
            }
        }
    }
    Ok((FemMesh::new(nodes, elements), step))
}

fn read_parts(path: &Path) -> Result<Vec<(FemMesh, StepResult)>, String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pvtu"))
    {
        let pieces: Vec<_> = source
            .split("<Piece ")
            .skip(1)
            .filter_map(|s| s.split('>').next().and_then(|s| attr_value(s, "Source")))
            .collect();
        if pieces.is_empty() {
            return Err("PVTU has no pieces".into());
        }
        if pieces.len() != 1 {
            return Err("Multi-piece PVTU needs partition/ghost handling and is not supported yet. Open native MPI results through Solve, or use ParaView.".into());
        }
        pieces
            .iter()
            .map(|s| {
                let text = std::fs::read_to_string(path.parent().unwrap_or(Path::new(".")).join(s))
                    .map_err(|e| e.to_string())?;
                parse_scene_piece(&text)
            })
            .collect()
    } else {
        Ok(vec![parse_scene_piece(&source)?])
    }
}

pub fn load_scene(path: &Path) -> Result<ResultScene, String> {
    let mut scene: Option<ResultScene> = None;
    for (number, file) in crate::result_series::detect_result_series(path) {
        let parts = read_parts(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if let Some(scene) = &mut scene {
            if scene.model.meshes.len() != parts.len() {
                return Err("VTK piece count changes across steps".into());
            }
            for (i, (mesh, mut step)) in parts.into_iter().enumerate() {
                let original = &scene.model.meshes[i];
                if original.nodes != mesh.nodes || original.elements != mesh.elements {
                    return Err(
                        "VTK geometry/topology changes across steps; load a fixed-mesh series"
                            .into(),
                    );
                }
                step.step = number;
                scene.steps[i].push(step);
            }
        } else {
            let mut parts = parts.into_iter();
            let (mesh, mut step) = parts.next().ok_or("No VTK geometry")?;
            step.step = number;
            let mut model = FemModel::single_mesh("VTK piece 0", mesh);
            let mut steps = vec![vec![step]];
            for (i, (mesh, mut step)) in parts.enumerate() {
                model.add_mesh(format!("VTK piece {}", i + 1), mesh);
                step.step = number;
                steps.push(vec![step]);
            }
            scene = Some(ResultScene { model, steps });
        }
    }
    scene.ok_or("No result steps".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> String {
        "<VTKFile><UnstructuredGrid><Piece NumberOfPoints=\"4\" NumberOfCells=\"1\"><Points><DataArray NumberOfComponents=\"3\">0 0 0 1 0 0 0 1 0 0 0 1</DataArray></Points><Cells><DataArray Name=\"connectivity\">0 1 2 3</DataArray><DataArray Name=\"offsets\">4</DataArray><DataArray Name=\"types\">10</DataArray></Cells><PointData><DataArray Name=\"TEMP\">10 20 30 40</DataArray></PointData><CellData><DataArray Name=\"Damage\">0.2</DataArray></CellData></Piece></UnstructuredGrid></VTKFile>".into()
    }
    #[test]
    fn standalone_geometry_and_point_cell_fields() {
        let (mesh, step) = parse_scene_piece(&fixture()).unwrap();
        assert_eq!(mesh.nodes.len(), 4);
        assert_eq!(mesh.elements.len(), 1);
        assert_eq!(mesh.cached_boundary_faces().len(), 4);
        assert!(step.field_by_name("TEMP").is_some());
        assert!(step.field_by_name("Damage (element)").is_some());
        assert!(
            parse_scene_piece(&fixture().replace("0 1 2 3</DataArray>", "0 1 2 99</DataArray>"))
                .is_err()
        );
        assert!(
            parse_scene_piece(&fixture().replace(">10</DataArray>", ">42</DataArray>")).is_err()
        );
    }
    #[test]
    #[ignore = "Requires BEVYISTR_TUTORIAL_DIR; standalone opening without pre-model"]
    fn tutorial_standalone_vtk_scene() {
        let root = std::path::PathBuf::from(std::env::var_os("BEVYISTR_TUTORIAL_DIR").unwrap());
        for (file, nodes, elements) in [
            ("01_elastic_hinge/hinge_vis_psf.0001.pvtu", 84056, 49871),
            ("19_conrod/vis_out/conrod_psf.0001.pvtu", 94047, 56115),
            ("16_heat_block/block_vis_psf.0001.pvtu", 37386, 32160),
        ] {
            let scene = load_scene(&root.join(file)).unwrap();
            assert_eq!(scene.model.meshes.len(), 1);
            assert_eq!(scene.model.meshes[0].nodes.len(), nodes);
            assert_eq!(scene.model.meshes[0].elements.len(), elements);
            assert!(!scene.model.meshes[0].cached_boundary_faces().is_empty());
            assert!(!scene.steps[0].is_empty());
            if !file.contains("heat") {assert_eq!(scene.steps[0].len(),2);}
        }
    }
}
