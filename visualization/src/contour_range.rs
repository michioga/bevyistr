//! Display ranges use cached field extrema, never rewrite simulation values.
use bevy::prelude::*;
use fem_core::{FemResultSet, ResultField};

#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContourRangeMode {
    #[default]
    CurrentFrame,
    AllFrames,
}

pub(crate) fn same_kind(a: &ResultField, b: &ResultField) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

pub(crate) fn bounds(field: &ResultField) -> Option<(f32, f32)> {
    let (min, max, has_values) = match field {
        ResultField::NodeScalar {
            min, max, values, ..
        }
        | ResultField::ElementScalar {
            min, max, values, ..
        } => (*min, *max, !values.is_empty()),
        ResultField::NodeVector {
            min_mag,
            max_mag,
            values,
            ..
        } => (*min_mag, *max_mag, !values.is_empty()),
    };
    if min == max && field.constant_value().is_none() {
        return None;
    }
    (has_values && min.is_finite() && max.is_finite() && min <= max).then_some((min, max))
}

pub(crate) fn resolve(results: &FemResultSet, mode: ContourRangeMode) -> Option<(f32, f32)> {
    let active = results.active.as_ref()?;
    let selected = results.active_field()?;
    let mut range: Option<(f32, f32)> = None;
    for steps in &results.by_mesh {
        for (index, step) in steps.iter().enumerate() {
            if mode == ContourRangeMode::CurrentFrame && index != active.step_index {
                continue;
            }
            let Some(field) = step
                .field_by_name(&active.field_name)
                .filter(|f| same_kind(f, selected))
            else {
                continue;
            };
            let Some((min, max)) = bounds(field) else {
                continue;
            };
            range = Some(range.map_or((min, max), |(lo, hi)| (lo.min(min), hi.max(max))));
        }
    }
    range
}

pub(crate) fn normalize(value: f32, (min, max): (f32, f32)) -> f32 {
    if min == max {
        0.5
    } else {
        // f64 avoids overflow and preserves meaningful very small ranges.
        ((value as f64 - min as f64) / (max as f64 - min as f64)).clamp(0.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{ActiveResult, StepResult};
    fn scalar(min: f32, max: f32) -> ResultField {
        ResultField::NodeScalar {
            name: "P".into(),
            values: vec![min, max],
            min,
            max,
        }
    }
    #[test]
    fn all_frames_share_ranges_without_changing_values_or_including_other_fields() {
        let step = |fields| StepResult {
            fields,
            ..default()
        };
        let mut r = FemResultSet {
            by_mesh: vec![
                vec![step(vec![scalar(0., 1.)]), step(vec![scalar(-5., 3.)])],
                vec![step(vec![scalar(2., 4.)]), step(vec![scalar(0., 10.)])],
            ],
            active: Some(ActiveResult {
                mesh_index: 0,
                step_index: 0,
                field_name: "P".into(),
            }),
        };
        assert_eq!(resolve(&r, ContourRangeMode::CurrentFrame), Some((0., 4.)));
        assert_eq!(resolve(&r, ContourRangeMode::AllFrames), Some((-5., 10.)));
        r.active.as_mut().unwrap().step_index = 1;
        assert_eq!(resolve(&r, ContourRangeMode::AllFrames), Some((-5., 10.)));
        assert_eq!(bounds(&r.by_mesh[0][0].fields[0]), Some((0., 1.)));
        r.by_mesh[1][1].fields = vec![ResultField::ElementScalar {
            name: "P".into(),
            values: vec![999.],
            min: 999.,
            max: 999.,
        }];
        assert_eq!(resolve(&r, ContourRangeMode::AllFrames), Some((-5., 4.)));
        r.by_mesh[0][1].fields.clear();
        assert_eq!(resolve(&r, ContourRangeMode::AllFrames), None);
    }
    #[test]
    fn constants_tiny_ranges_and_vector_magnitudes_are_not_confused() {
        assert_eq!(normalize(2., (2., 2.)), 0.5);
        assert_eq!(normalize(1e-20, (0., 1e-20)), 1.);
        let vector = ResultField::NodeVector {
            name: "V".into(),
            values: vec![Vec3::X],
            min_mag: 1.,
            max_mag: 1.,
        };
        assert_eq!(bounds(&vector), Some((1., 1.)));
        assert_eq!(bounds(&scalar(f32::NAN, f32::NAN)), None);
    }

    #[test]
    fn fixed_range_maps_constant_frame_to_its_real_color_without_moving_geometry() {
        use bevy::mesh::VertexAttributeValues;
        let mesh = fem_core::FemMesh::demo_hex8();
        let step = StepResult {
            fields: vec![ResultField::NodeScalar {
                name: "P".into(),
                values: vec![0.; 8],
                min: 0.,
                max: 0.,
            }],
            ..default()
        };
        let settings = crate::ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "P".into(),
            show_deformation: false,
            displacement_field: "Displacement".into(),
            deformation_scale: 1.,
        };
        let current =
            crate::demo_mesh::build_contour_surface_mesh(&mesh, &step, &settings, None).unwrap();
        let fixed =
            crate::demo_mesh::build_contour_surface_mesh(&mesh, &step, &settings, Some((0., 10.)))
                .unwrap();
        for (surface, t) in [(&current, 0.5), (&fixed, 0.)] {
            let Some(VertexAttributeValues::Float32x4(colors)) =
                surface.attribute(Mesh::ATTRIBUTE_COLOR)
            else {
                panic!("no colors");
            };
            assert!(
                colors
                    .iter()
                    .all(|c| *c == fem_core::rainbow_color(t).to_f32_array())
            );
        }
        let Some(VertexAttributeValues::Float32x3(a)) = current.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!();
        };
        let Some(VertexAttributeValues::Float32x3(b)) = fixed.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!();
        };
        assert_eq!(a, b);
    }
}
