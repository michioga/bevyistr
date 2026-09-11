//! Inline ASCII VTU fields. Single-piece data must follow the supplied mesh order.
//! Multi-piece PVTU requires global-node mapping; concatenation is unsafe.
use bevy::prelude::Vec3;
use fem_core::{NodeId, ResultField, StepResult};
use std::{collections::HashMap, io, path::Path};

#[derive(Debug)]
pub enum VtuError {
    Io(io::Error),
    UnsupportedFormat(String),
    Parse(String),
}
impl std::fmt::Display for VtuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO: {e}"),
            Self::UnsupportedFormat(e) => write!(f, "Unsupported format: {e}"),
            Self::Parse(e) => write!(f, "Parse error: {e}"),
        }
    }
}
impl std::error::Error for VtuError {}
impl From<io::Error> for VtuError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// PointData must follow the input mesh's node order. Unsupported multi-piece
/// data is rejected instead of silently assigning values to different nodes.
pub fn load_vtu_file(path: impl AsRef<Path>, node_ids: &[NodeId]) -> Result<StepResult, VtuError> {
    parse_vtu(&read_piece(path.as_ref())?, node_ids)
}

pub(crate) fn read_piece(path: &Path) -> Result<String, VtuError> {
    let source = std::fs::read_to_string(path)?;
    let source = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pvtu"))
    {
        let pieces: Vec<_> = source
            .split("<Piece ")
            .skip(1)
            .filter_map(|s| s.split('>').next().and_then(|s| attr_value(s, "Source")))
            .collect();
        if pieces.len() != 1 {
            return Err(VtuError::UnsupportedFormat(
                "Multi-piece PVTU requires global node mapping. Use native .res results through the Solve result handoff, or inspect this PVTU in ParaView.".into()));
        }
        std::fs::read_to_string(path.parent().unwrap_or(Path::new(".")).join(pieces[0]))?
    } else {
        source
    };
    Ok(source)
}

/// FrontISTR omits nodes unused by elements from its VTK output. Validate the
/// coordinates before accepting either the full or the compacted node order.
pub fn load_vtu_for_mesh(
    path: impl AsRef<Path>,
    mesh: &fem_core::FemMesh,
) -> Result<StepResult, VtuError> {
    let source = read_piece(path.as_ref())?;
    parse_vtu_for_mesh(&source, mesh)
}

fn parse_vtu_for_mesh(source: &str, mesh: &fem_core::FemMesh) -> Result<StepResult, VtuError> {
    let points = section(&source, "Points")?
        .ok_or_else(|| VtuError::Parse("Missing Points; cannot verify result geometry".into()))?;
    // VTK coordinate arrays need not have a Name.
    let named = points.replacen("<DataArray", "<DataArray Name=\"Coordinates\"", 1);
    let coordinates = arrays(&named)?;
    let [coordinates] = coordinates.as_slice() else {
        return Err(VtuError::Parse("Expected one coordinate array".into()));
    };
    if coordinates.n_comp != 3 || coordinates.values.len() % 3 != 0 {
        return Err(VtuError::Parse("Invalid point coordinates".into()));
    }
    let used: std::collections::HashSet<_> = mesh
        .elements
        .iter()
        .flat_map(|e| e.nodes.iter().copied())
        .collect();
    let count = coordinates.values.len() / 3;
    let indices: Vec<_> = mesh
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| count == mesh.nodes.len() || used.contains(&n.id))
        .map(|(i, _)| i)
        .collect();
    if indices.len() != count {
        return Err(VtuError::Parse(format!(
            "Result has {count} points; current mesh has {} nodes ({} used by elements). Open the matching project first.",
            mesh.nodes.len(),
            used.len()
        )));
    }
    for (&index, point) in indices.iter().zip(coordinates.values.chunks_exact(3)) {
        let expected = mesh.nodes[index].position.to_array();
        // FrontISTR ASCII VTK writes seven significant digits.
        if expected
            .iter()
            .zip(point)
            .any(|(a, b)| (a - b).abs() > 1e-6 * a.abs().max(1.0))
        {
            return Err(VtuError::Parse(format!(
                "Result geometry/order differs at mesh node {}. Open the matching project; reordered VTK points are not supported yet.",
                mesh.nodes[index].id.0
            )));
        }
    }
    let ids: Vec<_> = indices.iter().map(|&i| mesh.nodes[i].id).collect();
    let mut step = parse_vtu(&source, &ids)?;
    if count != mesh.nodes.len() {
        for field in &mut step.fields {
            match field {
                ResultField::NodeScalar { values, .. } => {
                    let mut expanded = vec![f32::NAN; mesh.nodes.len()];
                    for (&i, &v) in indices.iter().zip(values.iter()) {
                        expanded[i] = v;
                    }
                    *values = expanded;
                }
                ResultField::NodeVector { values, .. } => {
                    let mut expanded = vec![Vec3::splat(f32::NAN); mesh.nodes.len()];
                    for (&i, &v) in indices.iter().zip(values.iter()) {
                        expanded[i] = v;
                    }
                    *values = expanded;
                }
                _ => {}
            }
        }
    }
    Ok(step)
}

pub(crate) struct RawDataArray {
    pub(crate) name: String,
    pub(crate) n_comp: usize,
    pub(crate) values: Vec<f32>,
}
pub(crate) fn arrays(block: &str) -> Result<Vec<RawDataArray>, VtuError> {
    let error = |s: &str| VtuError::Parse(s.into());
    let mut remaining = block;
    let mut result = Vec::new();
    while let Some(start) = remaining.find("<DataArray") {
        remaining = &remaining[start + "<DataArray".len()..];
        let end = remaining
            .find('>')
            .ok_or_else(|| error("Unclosed DataArray header"))?;
        let header = &remaining[..end];
        if attr_value(header, "format").is_some_and(|f| f != "ascii") {
            return Err(VtuError::UnsupportedFormat(
                "Only inline ASCII DataArray is supported".into(),
            ));
        }
        let name = attr_value(header, "Name")
            .ok_or_else(|| error("Missing DataArray Name"))?
            .to_string();
        let n_comp = attr_value(header, "NumberOfComponents")
            .unwrap_or("1")
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0 && *n <= 1024)
            .ok_or_else(|| error("Invalid component count"))?;
        remaining = &remaining[end + 1..];
        if header.trim_end().ends_with('/') {
            return Err(error("Empty result DataArray"));
        }
        let end = remaining
            .find("</DataArray>")
            .ok_or_else(|| error("Unclosed DataArray"))?;
        let values = remaining[..end]
            .split_whitespace()
            .map(|s| {
                s.replace(['D', 'd'], "E")
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| VtuError::Parse(format!("Invalid value in {name}: {s}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        remaining = &remaining[end + "</DataArray>".len()..];
        if result.iter().any(|a: &RawDataArray| a.name == name) {
            return Err(error("Duplicate field name"));
        }
        result.push(RawDataArray {
            name,
            n_comp,
            values,
        });
    }
    Ok(result)
}

pub(crate) fn section<'a>(source: &'a str, tag: &str) -> Result<Option<&'a str>, VtuError> {
    let Some(start) = source.find(&format!("<{tag}")) else {
        return Ok(None);
    };
    let rest = &source[start..];
    let end = rest
        .find('>')
        .ok_or_else(|| VtuError::Parse(format!("Unclosed {tag}")))?;
    if rest[..end].trim_end().ends_with('/') {
        return Ok(Some(""));
    }
    let body = &rest[end + 1..];
    let close = body
        .find(&format!("</{tag}>"))
        .ok_or_else(|| VtuError::Parse(format!("Unclosed {tag}")))?;
    Ok(Some(&body[..close]))
}

pub(crate) fn parse_vtu(source: &str, nodes: &[NodeId]) -> Result<StepResult, VtuError> {
    if source.matches("<Piece ").count() != 1 {
        return Err(VtuError::UnsupportedFormat(
            "Expected one UnstructuredGrid Piece".into(),
        ));
    }
    let block =
        section(source, "PointData")?.ok_or_else(|| VtuError::Parse("Missing PointData".into()))?;
    let raws = arrays(block)?;
    if raws.is_empty() {
        return Err(VtuError::Parse("No PointData fields".into()));
    }
    let mut fields = Vec::new();
    for raw in raws {
        let expected = nodes
            .len()
            .checked_mul(raw.n_comp)
            .ok_or_else(|| VtuError::Parse("Result size overflow".into()))?;
        if raw.values.len() != expected {
            return Err(VtuError::Parse(format!(
                "{}: expected {expected} values for {} mesh nodes, found {}",
                raw.name,
                nodes.len(),
                raw.values.len()
            )));
        }
        // A three-component array may contain principal values, not a vector.
        let vector = [
            "DISPLACEMENT",
            "VELOCITY",
            "ACCELERATION",
            "REACTION_FORCE",
            "ROTATION",
        ]
        .iter()
        .any(|name| raw.name.eq_ignore_ascii_case(name));
        if vector && matches!(raw.n_comp, 2 | 3) {
            let map: HashMap<_, _> = nodes
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let v = &raw.values[i * raw.n_comp..(i + 1) * raw.n_comp];
                    (*id, Vec3::new(v[0], v[1], v.get(2).copied().unwrap_or(0.0)))
                })
                .collect();
            let name = if raw.name.eq_ignore_ascii_case("DISPLACEMENT") {
                "Displacement"
            } else {
                &raw.name
            };
            fields.push(ResultField::node_vector(name, nodes, &map));
        }
        for component in 0..raw.n_comp {
            let name = if raw.n_comp == 1 {
                raw.name.clone()
            } else {
                format!("{}[{}]", raw.name, component + 1)
            };
            let values = nodes
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, raw.values[i * raw.n_comp + component]))
                .collect();
            fields.push(ResultField::node_scalar(name, nodes, &values));
        }
    }
    let mut time = 0.0;
    if let Some(block) = section(source, "FieldData")? {
        for raw in arrays(block)? {
            if matches!(raw.name.as_str(), "TimeValue" | "TOTALTIME") && raw.values.len() == 1 {
                time = raw.values[0];
            }
        }
    }
    Ok(StepResult {
        step: 1,
        time,
        fields,
    })
}

pub(crate) fn attr_value<'a>(header: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = header.find(&needle)? + needle.len();
    Some(&header[start..start + header[start..].find('"')?])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(data: &str) -> String {
        format!(
            "<VTKFile><UnstructuredGrid><Piece NumberOfPoints=\"2\"><PointData>{data}</PointData></Piece></UnstructuredGrid></VTKFile>"
        )
    }
    #[test]
    fn tensor_and_unknown_components_are_preserved_in_order() {
        let src = file(
            "<DataArray Name=\"NodalSTRESS\" NumberOfComponents=\"6\" format=\"ascii\">1 2 3 4 5 6 11 12 13 14 15 16</DataArray><DataArray Name=\"CustomPair\" NumberOfComponents=\"2\">7 8 9 10</DataArray>",
        );
        let step = parse_vtu(&src, &[NodeId(8), NodeId(2)]).unwrap();
        assert_eq!(step.fields.len(), 8);
        let Some(ResultField::NodeScalar { values, .. }) = step.field_by_name("NodalSTRESS[6]")
        else {
            panic!()
        };
        assert_eq!(values, &[6., 16.]);
        let Some(ResultField::NodeScalar { values, .. }) = step.field_by_name("CustomPair[2]")
        else {
            panic!()
        };
        assert_eq!(values, &[8., 10.]);
    }
    #[test]
    fn malformed_or_incomplete_arrays_do_not_become_zero_results() {
        for data in ["1", "1 invalid", "1 NaN", "1 2 3"] {
            assert!(
                parse_vtu(
                    &file(&format!("<DataArray Name=\"TEMP\">{data}</DataArray>")),
                    &[NodeId(1), NodeId(2)]
                )
                .is_err()
            );
        }
        assert!(matches!(
            parse_vtu(
                &file("<DataArray Name=\"TEMP\" format=\"binary\">AAAA</DataArray>"),
                &[NodeId(1), NodeId(2)]
            ),
            Err(VtuError::UnsupportedFormat(_))
        ));
    }
    #[test]
    fn uppercase_displacement_supports_shape_toggle_and_components() {
        let step = parse_vtu(
            &file(
                "<DataArray Name=\"DISPLACEMENT\" NumberOfComponents=\"3\">1 2 3 4 5 6</DataArray>",
            ),
            &[NodeId(1), NodeId(2)],
        )
        .unwrap();
        assert!(matches!(
            step.field_by_name("Displacement"),
            Some(ResultField::NodeVector { .. })
        ));
        assert!(step.field_by_name("DISPLACEMENT[3]").is_some());
    }

    #[test]
    fn compacted_unused_nodes_preserve_ranges_and_reject_wrong_geometry() {
        let mut mesh = fem_core::FemMesh::demo_hex8();
        let coords = mesh
            .nodes
            .iter()
            .map(|n| format!("{} {} {}", n.position.x, n.position.y, n.position.z))
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "<Piece NumberOfPoints=\"8\"><Points><DataArray NumberOfComponents=\"3\">{coords}</DataArray></Points><PointData><DataArray Name=\"TEMP\">5 6 7 8 9 10 11 12</DataArray></PointData></Piece>"
        );
        mesh.nodes
            .insert(1, fem_core::FemNode::new(NodeId(999), Vec3::splat(99.)));
        let step = parse_vtu_for_mesh(&source, &mesh).unwrap();
        let ResultField::NodeScalar {
            values, min, max, ..
        } = &step.fields[0]
        else {
            panic!()
        };
        assert_eq!((*min, *max), (5., 12.));
        assert!(values[1].is_nan());
        assert_eq!(values[2], 6.);
        mesh.nodes[0].position.x += 1.;
        assert!(parse_vtu_for_mesh(&source, &mesh).is_err());
        mesh.nodes[0].position.x -= 1.;
        mesh.elements[0].nodes.push(NodeId(999));
        assert!(parse_vtu_for_mesh(&source, &mesh).is_err());
    }

    #[test]
    #[ignore = "Requires BEVYISTR_TUTORIAL_DIR containing existing FrontISTR outputs; read-only"]
    fn tutorial_conrod_compacted_points() {
        let root = std::path::PathBuf::from(std::env::var_os("BEVYISTR_TUTORIAL_DIR").unwrap())
            .join("19_conrod");
        let mesh = crate::load_mesh_file(root.join("conrod.msh")).unwrap();
        let path = root.join("vis_out/conrod_psf.0001/conrod_psf.0001.0.vtu");
        let vtk = load_vtu_for_mesh(&path, &mesh).unwrap();
        let used: std::collections::HashSet<_> =
            mesh.elements.iter().flat_map(|e| e.nodes.iter()).collect();
        let nodes: Vec<_> = mesh
            .nodes
            .iter()
            .filter(|n| used.contains(&n.id))
            .map(|n| n.id)
            .collect();
        let elements: Vec<_> = mesh.elements.iter().map(|e| e.id).collect();
        let source = std::fs::read_to_string(root.join("fstrRES/STEP1/conrod.res.0.1")).unwrap();
        let native = crate::native_result::NativeResult::parse(&source)
            .unwrap()
            .step(&nodes, &elements, 1)
            .unwrap();
        let ResultField::NodeVector { values: a, .. } =
            native.field_by_name("Displacement").unwrap()
        else {
            panic!()
        };
        let ResultField::NodeVector { values: b, .. } = vtk.field_by_name("Displacement").unwrap()
        else {
            panic!()
        };
        let mut expected = a.iter();
        let mut missing = 0;
        for (node, b) in mesh.nodes.iter().zip(b) {
            if used.contains(&node.id) {
                let a = expected.next().unwrap();
                assert!(b.is_finite());
                assert!(
                    (*a - *b).length() <= 2e-5 * a.length().max(1.0),
                    "node {:?}",
                    node.id
                );
            } else {
                assert!(b.is_nan());
                missing += 1;
            }
        }
        assert_eq!(missing, 2281);
        let mut wrong_mesh = mesh.clone();
        wrong_mesh.nodes[0].position.x += 1.0;
        assert!(load_vtu_for_mesh(&path, &wrong_mesh).is_err());
    }

    #[test]
    #[ignore = "Requires BEVYISTR_TUTORIAL_DIR containing existing FrontISTR outputs; read-only"]
    fn tutorial_eigen_heat_and_flow_outputs() {
        let root = std::path::PathBuf::from(
            std::env::var_os("BEVYISTR_TUTORIAL_DIR").expect("tutorial root"),
        );
        for (dir, stem, step, field) in [
            ("15_eigen_spring", "spring", 1, "Displacement"),
            ("16_heat_block", "block", 1, "TEMPERATURE"),
        ] {
            let mesh = crate::load_mesh_file(root.join(dir).join(format!("{stem}.msh"))).unwrap();
            let nodes: Vec<_> = mesh.nodes.iter().map(|n| n.id).collect();
            let elements: Vec<_> = mesh.elements.iter().map(|e| e.id).collect();
            let native =
                std::fs::read_to_string(root.join(dir).join(format!("{stem}.res.0.{step}")))
                    .unwrap();
            let native = crate::native_result::NativeResult::parse(&native)
                .unwrap()
                .step(&nodes, &elements, step)
                .unwrap();
            let vtk = load_vtu_for_mesh(
                root.join(dir)
                    .join(format!("{stem}_vis_psf.{step:04}.pvtu")),
                &mesh,
            )
            .unwrap();
            let a = native.field_by_name(field).unwrap();
            let b = vtk.field_by_name(field).unwrap();
            match (a, b) {
                (
                    ResultField::NodeVector { values: a, .. },
                    ResultField::NodeVector { values: b, .. },
                ) => {
                    assert!(
                        a.iter()
                            .zip(b)
                            .all(|(a, b)| (*a - *b).length() <= 2e-5 * a.length().max(1.0)),
                        "{dir}: node order/value mismatch"
                    );
                }
                (
                    ResultField::NodeScalar { values: a, .. },
                    ResultField::NodeScalar { values: b, .. },
                ) => {
                    assert!(
                        a.iter()
                            .zip(b)
                            .all(|(a, b)| (a - b).abs() <= 2e-5 * a.abs().max(1.0)),
                        "{dir}: node order/value mismatch"
                    );
                }
                _ => panic!("Different field types"),
            }
        }
        let flow = root.join("18_cavity_flow/cavityflow_vis_psf.0000.pvtu");
        assert!(matches!(
            load_vtu_file(flow, &[]),
            Err(VtuError::UnsupportedFormat(_))
        ));
    }
}
