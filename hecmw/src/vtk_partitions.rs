//! Keep VTK partitions independent: never weld coincident points or average
//! values without an explicit ownership mapping.
use fem_core::{FemMesh, ResultField, StepResult};
use std::collections::{HashMap, HashSet};

fn flags(field: Option<&ResultField>, len: usize, allowed: u8) -> Result<Vec<u8>, String> {
    let Some(field) = field else {
        return Ok(vec![0; len]);
    };
    let values = match field {
        ResultField::NodeScalar { values, .. } | ResultField::ElementScalar { values, .. } => {
            values
        }
        _ => return Err("vtkGhostType must be a scalar array".into()),
    };
    if values.len() != len {
        return Err("Invalid vtkGhostType length".into());
    }
    values
        .iter()
        .map(|&v| {
            if !v.is_finite()
                || v < 0.0
                || v > 255.0
                || v.fract() != 0.0
                || (v as u8) & !allowed != 0
            {
                Err(format!(
                    "Unsupported vtkGhostType flag {v}; only duplicate/hidden flags are supported"
                ))
            } else {
                Ok(v as u8)
            }
        })
        .collect()
}

pub(crate) fn filter_ghosts(
    mesh: FemMesh,
    mut step: StepResult,
) -> Result<(FemMesh, StepResult), String> {
    // VTK vtkDataSetAttributes: point duplicate=1, hidden=2;
    // cell duplicate=1, hidden=32. Other ghost semantics are not guessed.
    // https://vtk.org/doc/nightly/release/9.6/html/classvtkDataSetAttributes.html
    let nodes = flags(step.field_by_name("vtkGhostType"), mesh.nodes.len(), 1 | 2)?;
    let cells = flags(
        step.field_by_name("vtkGhostType (element)"),
        mesh.elements.len(),
        1 | 32,
    )?;
    step.fields
        .retain(|f| !matches!(f.name(), "vtkGhostType" | "vtkGhostType (element)"));
    let hidden: HashSet<_> = mesh
        .nodes
        .iter()
        .zip(nodes)
        .filter(|(_, mask)| mask & 2 != 0)
        .map(|(n, _)| n.id)
        .collect();
    let retained: Vec<_> = mesh
        .elements
        .iter()
        .enumerate()
        .filter(|(i, e)| cells[*i] & (1 | 32) == 0 && !e.nodes.iter().any(|id| hidden.contains(id)))
        .map(|(i, _)| i)
        .collect();
    if retained.len() == mesh.elements.len() {
        return Ok((mesh, step));
    }
    let elements = retained
        .iter()
        .map(|&i| mesh.elements[i].clone())
        .collect::<Vec<_>>();
    let used: HashSet<_> = elements
        .iter()
        .flat_map(|e| e.nodes.iter().copied())
        .collect();
    let node_indices: Vec<_> = mesh
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| used.contains(&n.id))
        .map(|(i, _)| i)
        .collect();
    for field in &mut step.fields {
        match field {
            ResultField::NodeScalar { values, .. } => {
                *values = node_indices.iter().map(|&i| values[i]).collect()
            }
            ResultField::NodeVector { values, .. } => {
                *values = node_indices.iter().map(|&i| values[i]).collect()
            }
            ResultField::ElementScalar { values, .. } => {
                *values = retained.iter().map(|&i| values[i]).collect()
            }
        }
    }
    Ok((
        FemMesh::new(
            node_indices
                .iter()
                .map(|&i| mesh.nodes[i].clone())
                .collect(),
            elements,
        ),
        step,
    ))
}

fn kind(field: &ResultField) -> u8 {
    match field {
        ResultField::NodeScalar { .. } => 0,
        ResultField::NodeVector { .. } => 1,
        ResultField::ElementScalar { .. } => 2,
    }
}

/// Per-frame common ranges prevent the same value having different colors on
/// different ranks. Only the ranges change; the stored values stay untouched.
pub(crate) fn validate_and_share_ranges(parts: &mut [(FemMesh, StepResult)]) -> Result<(), String> {
    let first = &parts.first().ok_or("No VTK pieces")?.1;
    let schema: HashSet<_> = first
        .fields
        .iter()
        .map(|f| (f.name().to_string(), kind(f)))
        .collect();
    let time = first.time;
    let mut ranges: HashMap<(String, u8), (f32, f32)> = HashMap::new();
    for (i, (_, step)) in parts.iter().enumerate() {
        if !first.same_eigenmode(step) {
            return Err(format!("VTK piece {i} has a different EIGENVALUE"));
        }
        let actual: HashSet<_> = step
            .fields
            .iter()
            .map(|f| (f.name().to_string(), kind(f)))
            .collect();
        if step.fields.len() != schema.len() || actual != schema {
            return Err(format!("VTK piece {i} has inconsistent point/cell fields"));
        }
        if !step.time.is_finite() || (step.time - time).abs() > 1e-6 * time.abs().max(1.0) {
            return Err(format!("VTK piece {i} has a different TimeValue"));
        }
        for field in &step.fields {
            let range = ranges
                .entry((field.name().into(), kind(field)))
                .or_insert((f32::INFINITY, f32::NEG_INFINITY));
            let mut add = |v: f32| {
                if v.is_finite() {
                    range.0 = range.0.min(v);
                    range.1 = range.1.max(v);
                }
            };
            match field {
                ResultField::NodeScalar { values, .. }
                | ResultField::ElementScalar { values, .. } => {
                    values.iter().copied().for_each(&mut add)
                }
                ResultField::NodeVector { values, .. } => {
                    values.iter().map(|v| v.length()).for_each(&mut add)
                }
            }
        }
    }
    for (_, step) in parts {
        for field in &mut step.fields {
            let (lo, hi) = ranges[&(field.name().to_string(), kind(field))];
            let (lo, hi) = if lo.is_finite() && hi.is_finite() {
                (lo, hi)
            } else {
                (f32::NAN, f32::NAN)
            };
            match field {
                ResultField::NodeScalar { min, max, .. }
                | ResultField::ElementScalar { min, max, .. } => {
                    *min = lo;
                    *max = hi;
                }
                ResultField::NodeVector {
                    min_mag, max_mag, ..
                } => {
                    *min_mag = lo;
                    *max_mag = hi;
                }
            }
        }
    }
    Ok(())
}
