//! Material identity is independent of part number and render entity order.
use bevy::prelude::*;
use fem_core::{AnalysisSetup, ElementId, FemMesh};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialColorMode {
    Part,
    #[default]
    Material,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MaterialIdentity {
    Unassigned,
    Invalid,
    Assigned(String),
}
pub const UNASSIGNED_MATERIAL_COLOR: Color = Color::srgb(0.48, 0.51, 0.55);
pub const INVALID_MATERIAL_COLOR: Color = Color::srgb(0.95, 0.16, 0.65);

pub fn material_identity_color(name: &str) -> Color {
    // Stable FNV-1a, not RandomState (which changes between runs). A full hue
    // range avoids a tiny modulo palette; names remain visible in the legend.
    let hash = name.bytes().fold(2166136261u32, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(16777619)
    });
    Color::hsl((hash as f64 / u32::MAX as f64 * 360.0) as f32, 0.48, 0.58)
}
impl MaterialIdentity {
    pub(crate) fn color(&self) -> Color {
        match self {
            Self::Unassigned => UNASSIGNED_MATERIAL_COLOR,
            Self::Invalid => INVALID_MATERIAL_COLOR,
            Self::Assigned(name) => material_identity_color(name),
        }
    }
}

pub(crate) fn resolve_materials(
    setup: &AnalysisSetup,
    mesh_index: usize,
    mesh: &FemMesh,
) -> BTreeMap<ElementId, MaterialIdentity> {
    let sections = setup.build_element_section_map(mesh_index, mesh);
    let mut names = BTreeMap::<&str, usize>::new();
    for material in &setup.materials {
        *names.entry(&material.name).or_default() += 1;
    }
    let mut colors: BTreeMap<_, _> = mesh
        .elements
        .iter()
        .map(|element| {
            let identity = match sections.get(&element.id) {
                None => MaterialIdentity::Unassigned,
                Some(section) if names.get(section.material_name.as_str()) == Some(&1) => {
                    MaterialIdentity::Assigned(section.material_name.clone())
                }
                _ => MaterialIdentity::Invalid,
            };
            (element.id, identity)
        })
        .collect();
    // Specific groups override whole-mesh defaults. Competing specific
    // groups with different materials must not silently look unambiguous.
    let mut owners: BTreeMap<ElementId, BTreeSet<&str>> = BTreeMap::new();
    for section in setup.sections_for_mesh(mesh_index) {
        if let Some(group) = section
            .element_set_name
            .as_ref()
            .and_then(|name| mesh.element_sets.iter().find(|group| &group.name == name))
        {
            for id in &group.elements {
                owners
                    .entry(*id)
                    .or_default()
                    .insert(&section.material_name);
            }
        }
    }
    for (id, owners) in owners {
        if owners.len() > 1 && colors.contains_key(&id) {
            colors.insert(id, MaterialIdentity::Invalid);
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{FemElementSet, SectionKind};
    #[test]
    fn identity_is_stable_and_not_based_on_part_number() {
        assert_eq!(
            material_identity_color("STEEL"),
            material_identity_color("STEEL")
        );
        assert_ne!(
            material_identity_color("STEEL"),
            material_identity_color("AL6082")
        );
        let mesh = FemMesh::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.add_material("STEEL", Some(210e9), Some(0.3), Some(7850.0));
        for index in [0, 1] {
            setup.add_section(index, "STEEL", None, SectionKind::Solid);
        }
        assert_eq!(
            resolve_materials(&setup, 0, &mesh),
            resolve_materials(&setup, 1, &mesh)
        );
        assert!(
            resolve_materials(&setup, 2, &mesh)
                .values()
                .all(|color| *color == MaterialIdentity::Unassigned)
        );
    }
    #[test]
    fn group_precedence_missing_materials_and_conflicts_are_explicit() {
        let mut mesh = FemMesh::demo_hex8();
        let mut setup = AnalysisSetup::default();
        mesh.element_sets.push(FemElementSet {
            name: "PATCH".into(),
            elements: vec![ElementId(0)],
        });
        for name in ["A", "B"] {
            setup.add_material(name, Some(1.0), Some(0.3), None);
        }
        setup.add_section(0, "A", None, SectionKind::Solid);
        setup.add_section(0, "B", Some("PATCH".into()), SectionKind::Solid);
        assert_eq!(
            resolve_materials(&setup, 0, &mesh)[&ElementId(0)],
            MaterialIdentity::Assigned("B".into())
        );
        setup.add_section(0, "A", Some("PATCH".into()), SectionKind::Solid);
        assert_eq!(
            resolve_materials(&setup, 0, &mesh)[&ElementId(0)],
            MaterialIdentity::Invalid
        );
        setup.sections.truncate(1);
        setup.materials.clear();
        assert_eq!(
            resolve_materials(&setup, 0, &mesh)[&ElementId(0)],
            MaterialIdentity::Invalid
        );
    }
}
