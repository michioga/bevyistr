//! Writer for the HECMW mesh file format (`.msh`).
//!
//! The HECMW `.msh` format is what FrontISTR expects as its mesh input,
//! specified in `hecmw_ctrl.dat`. This writer produces the subset of keywords
//! that every non-trivial FrontISTR analysis needs:
//!
//!   `!HEADER`, `!NODE`, `!ELEMENT` (one block per element type),
//!   `!NGROUP`, `!EGROUP`, `!SGROUP`, `!MATERIAL`, `!SECTION`,
//!   `!CONTACT PAIR`, `!END`
//!
//! Coordinates are written in full `f64`-formatted `f32` values (6 decimal
//! places) so precision is not lost in a read-modify-write round-trip even
//! though `FemNode::position` is `Vec3` (f32).
//!
//! # Multi-part assembly export
//!
//! HECMW's `.msh` format has no native concept of "multiple meshes in one
//! file" the way an assembly might — everything is one flat
//! node/element/group space. So when a [`FemModel`] has more than one part
//! (built via "Add Part"), [`write_msh_file`] flattens them into a single
//! mesh by re-numbering every part's node and element IDs into disjoint
//! ranges (offsetting by the running totals of all earlier parts) and
//! prefixing every group name with the part name (`PART2_EGRP1`) so that
//! identically-named groups from different source files don't collide.
//! Contact pairs are mesh definitions in FrontISTR, so accepted contacts are
//! exported as `TYPE=NODE-SURF` or `TYPE=SURF-SURF` after their referenced
//! groups. Their interaction law is written to the analysis `.cnt` file.

use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::io;
use std::path::Path;

use fem_core::{
    AnalysisSetup, ContactSlaveRef, ElementId, ElementType, FemMaterial, FemMesh, FemModel,
    MpcEquation, NodeId, NodeSetRef, Section, SectionKind, SurfaceSetRef,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExportContactPair {
    name: String,
    node_surface: bool,
    slave_group: String,
    master_group: String,
}

// ─── public API ──────────────────────────────────────────────────────────────

/// Writes the first mesh of `model` to `path` in HECMW `.msh` format.
///
/// Returns the number of nodes and elements written on success, for the
/// caller to surface in the status bar.
///
/// For models with a single mesh, this is equivalent to (and a thin
/// wrapper over) [`write_msh_assembly`] — kept as a separate name because
/// "write the mesh" reads more naturally than "write the assembly" for the
/// overwhelmingly common single-part case, and existing callers don't need
/// to change.
pub fn write_msh_file(
    path: impl AsRef<Path>,
    model: &FemModel,
    mesh_index: usize,
) -> io::Result<(usize, usize)> {
    write_msh_file_impl(path, model, mesh_index, None)
}

/// Saves the mesh geometry and groups as a standalone HEC-MW file.
pub fn write_msh_geometry_file(
    path: impl AsRef<Path>,
    mesh: &FemMesh,
) -> io::Result<(usize, usize)> {
    let path = path.as_ref();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| crate::staged_inputs::is_project_name(name))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid mesh filename"))?;
    validate_single_mesh_all_groups(mesh)?;
    if mesh.elements.is_empty()
        || mesh
            .elements
            .iter()
            .any(|element| element_type_code(&element.element_type).is_none())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mesh must contain supported HEC-MW elements",
        ));
    }
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let staged = crate::staged_inputs::StagedInputs::new(directory)?;
    std::fs::write(staged.path().join(name), build_msh(mesh, 0, &[], None))?;
    staged.publish(&[name])?;
    Ok((mesh.nodes.len(), mesh.elements.len()))
}

/// Writes one HEC-MW mesh together with the mesh-side material and section
/// definitions required by a complete FrontISTR project.
pub fn write_msh_file_with_setup(
    path: impl AsRef<Path>,
    model: &FemModel,
    mesh_index: usize,
    setup: &AnalysisSetup,
) -> io::Result<(usize, usize)> {
    write_msh_file_impl(path, model, mesh_index, Some(setup))
}

fn write_msh_file_impl(
    path: impl AsRef<Path>,
    model: &FemModel,
    mesh_index: usize,
    setup: Option<&AnalysisSetup>,
) -> io::Result<(usize, usize)> {
    let mesh = model
        .meshes
        .get(mesh_index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "mesh index out of range"))?;

    validate_single_mesh_all_groups(mesh)?;
    let contacts = resolve_contact_pairs(model, Some(mesh_index), None)?;
    let text = build_msh(mesh, mesh_index, &contacts, setup);
    std::fs::write(path, text.as_bytes())?;

    Ok((mesh.nodes.len(), mesh.elements.len()))
}

pub(crate) fn validate_single_mesh_all_groups(mesh: &FemMesh) -> io::Result<()> {
    // HEC-MW defines ALL itself. Suppressing a partial legacy declaration
    // would redirect its references to every node or element.
    fn whole_group<T: Copy + Eq + std::hash::Hash>(
        members: &[T],
        all: impl Iterator<Item = T>,
    ) -> bool {
        members.iter().copied().collect::<HashSet<_>>() == all.collect()
    }
    for group in &mesh.node_sets {
        if group.name.eq_ignore_ascii_case("ALL")
            && !whole_group(&group.nodes, mesh.nodes.iter().map(|node| node.id))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "NGRP ALL is reserved for all nodes. Rename the partial group before export.",
            ));
        }
    }
    for group in &mesh.element_sets {
        if group.name.eq_ignore_ascii_case("ALL")
            && !whole_group(&group.elements, mesh.elements.iter().map(|element| element.id))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "EGRP ALL is reserved for all elements. Rename the partial group before export.",
            ));
        }
    }
    Ok(())
}

/// Writes **every** mesh in `model` to `path` as one flattened HECMW
/// `.msh`, renumbering node/element IDs into disjoint ranges and prefixing
/// group names by part so multi-part assemblies (built via "Add Part")
/// export correctly instead of silently dropping every part after the
/// first.
///
/// Returns `(total_nodes, total_elements, part_count)`.
pub fn write_msh_assembly(
    path: impl AsRef<Path>,
    model: &FemModel,
) -> io::Result<(usize, usize, usize)> {
    write_msh_assembly_impl(path, model, None)
}

/// Writes a flattened assembly and the mesh-side material/section
/// assignments, using the same part prefixes as the exported groups.
pub fn write_msh_assembly_with_setup(
    path: impl AsRef<Path>,
    model: &FemModel,
    setup: &AnalysisSetup,
) -> io::Result<(usize, usize, usize)> {
    write_msh_assembly_impl(path, model, Some(setup))
}

fn write_msh_assembly_impl(
    path: impl AsRef<Path>,
    model: &FemModel,
    setup: Option<&AnalysisSetup>,
) -> io::Result<(usize, usize, usize)> {
    if model.meshes.len() <= 1 {
        // Single-part fast path — no renumbering or prefixing needed.
        let (n, e) = write_msh_file_impl(&path, model, 0, setup)?;
        return Ok((n, e, model.meshes.len()));
    }

    let mut combined = String::with_capacity(1024 * 64 * model.meshes.len());
    writeln!(combined, "!HEADER").unwrap();
    writeln!(
        combined,
        " Generated by bevyistr (assembly, {} parts)",
        model.meshes.len()
    )
    .unwrap();

    // First pass: compute per-part ID offsets so every part's node/element
    // IDs land in a disjoint range in the combined file.
    let mut node_offset: u32 = 0;
    let mut elem_offset: u32 = 0;
    let mut offsets: Vec<(u32, u32)> = Vec::with_capacity(model.meshes.len());
    for mesh in &model.meshes {
        offsets.push((node_offset, elem_offset));
        node_offset += mesh.nodes.iter().map(|n| n.id.0).max().unwrap_or(0) + 1;
        elem_offset += mesh.elements.iter().map(|e| e.id.0).max().unwrap_or(0) + 1;
    }

    // ── NODE (all parts) ──
    writeln!(combined, "!NODE").unwrap();
    for (mesh, &(noff, _)) in model.meshes.iter().zip(&offsets) {
        write_mesh_nodes(&mut combined, mesh, noff);
    }

    // ── ELEMENT (all parts, grouped by type across all parts) ──
    let mut type_codes_seen: Vec<&str> = Vec::new();
    for mesh in &model.meshes {
        for el in &mesh.elements {
            if let Some(code) = element_type_code(&el.element_type) {
                if !type_codes_seen.contains(&code) {
                    type_codes_seen.push(code);
                }
            }
        }
    }
    for code in &type_codes_seen {
        writeln!(combined, "!ELEMENT,TYPE={code}").unwrap();
        for (mesh, &(noff, eoff)) in model.meshes.iter().zip(&offsets) {
            for el in mesh
                .elements
                .iter()
                .filter(|e| element_type_code(&e.element_type) == Some(code))
            {
                let node_list: Vec<String> =
                    el.nodes.iter().map(|n| (n.0 + noff).to_string()).collect();
                writeln!(combined, " {},{}", el.id.0 + eoff, node_list.join(",")).unwrap();
            }
        }
    }

    let prefixes = part_group_prefixes(model);
    // ── NGROUP / EGROUP / SGROUP (prefixed by part name to avoid collisions) ──
    for (part_index, (mesh, &(noff, eoff))) in model.meshes.iter().zip(&offsets).enumerate() {
        let part_name = &prefixes[part_index];

        for nset in &mesh.node_sets {
            write_id_group(
                &mut combined,
                "NGROUP",
                "NGRP",
                &format!("{part_name}_{}", nset.name),
                nset.nodes.iter().map(|n| n.0 + noff),
            );
        }
        for eset in &mesh.element_sets {
            write_id_group(
                &mut combined,
                "EGROUP",
                "EGRP",
                &format!("{part_name}_{}", eset.name),
                eset.elements.iter().map(|e| e.0 + eoff),
            );
        }
        if setup.is_some_and(|setup| {
            setup.sections.iter().any(|section| {
                section.mesh_index == part_index && section.element_set_name.is_none()
            }) && !mesh
                .element_sets
                .iter()
                .any(|group| group.name.eq_ignore_ascii_case("ALL"))
        }) {
            write_id_group(
                &mut combined,
                "EGROUP",
                "EGRP",
                &format!("{part_name}_ALL"),
                mesh.elements.iter().map(|element| element.id.0 + eoff),
            );
        }
        for sset in &mesh.surface_sets {
            writeln!(combined, "!SGROUP,SGRP={part_name}_{}", sset.name).unwrap();
            for surf in &sset.surfaces {
                writeln!(combined, " {},{}", surf.element.0 + eoff, surf.local_face.0).unwrap();
            }
        }
    }

    if let Some(setup) = setup {
        write_mesh_materials(&mut combined, &setup.materials);
        for section in &setup.sections {
            if section.mesh_index >= model.meshes.len() {
                continue;
            }
            let local_group = section.element_set_name.as_deref().unwrap_or("ALL");
            let exported_group = format!("{}_{}", prefixes[section.mesh_index], local_group,);
            write_mesh_section(&mut combined, section, &exported_group);
        }
        write_mpc_equations(&mut combined, &setup.mpc_equations, |mesh_index, node| {
            offsets
                .get(mesh_index)
                .map(|(node_offset, _)| node.0 + node_offset)
        });
    }

    write_contact_pairs(
        &mut combined,
        &resolve_contact_pairs(model, None, Some(&prefixes))?,
    );

    writeln!(combined, "!END").unwrap();
    std::fs::write(path, combined.as_bytes())?;

    let total_nodes: usize = model.meshes.iter().map(|m| m.nodes.len()).sum();
    let total_elements: usize = model.meshes.iter().map(|m| m.elements.len()).sum();

    Ok((total_nodes, total_elements, model.meshes.len()))
}

/// Builds the global node/element ID remap a multi-part export would use,
/// without writing anything — exposed mainly for tests and for the `.cnt`
/// writer, which needs the *same* offsets to remap boundary
/// conditions/loads/sections that reference node or element IDs from a
/// specific part's original numbering.
pub fn assembly_id_offsets(model: &FemModel) -> Vec<(u32, u32)> {
    let mut node_offset: u32 = 0;
    let mut elem_offset: u32 = 0;
    let mut offsets = Vec::with_capacity(model.meshes.len());
    for mesh in &model.meshes {
        offsets.push((node_offset, elem_offset));
        node_offset += mesh.nodes.iter().map(|n| n.id.0).max().unwrap_or(0) + 1;
        elem_offset += mesh.elements.iter().map(|e| e.id.0).max().unwrap_or(0) + 1;
    }
    offsets
}

/// Remaps a `(mesh_index, NodeId)` pair to its assembly-wide [`NodeId`]
/// using the offsets from [`assembly_id_offsets`].
pub fn remap_node(offsets: &[(u32, u32)], mesh_index: usize, id: NodeId) -> NodeId {
    let (noff, _) = offsets.get(mesh_index).copied().unwrap_or((0, 0));
    NodeId(id.0 + noff)
}

/// Remaps a `(mesh_index, ElementId)` pair to its assembly-wide
/// [`ElementId`] using the offsets from [`assembly_id_offsets`].
pub fn remap_element(offsets: &[(u32, u32)], mesh_index: usize, id: ElementId) -> ElementId {
    let (_, eoff) = offsets.get(mesh_index).copied().unwrap_or((0, 0));
    ElementId(id.0 + eoff)
}

// ─── implementation ──────────────────────────────────────────────────────────

/// Strips characters that would be ambiguous or illegal in a HECMW group
/// name (commas, whitespace) from a part's display name when using it as a
/// group-name prefix.
fn sanitize_group_prefix(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_end_matches(".msh")
        .to_string()
        .to_ascii_uppercase()
}

pub(crate) fn part_group_prefixes(model: &FemModel) -> Vec<String> {
    let numbered = || (1..=model.meshes.len()).map(|index| format!("PART{index}"));
    let mut prefixes: Vec<_> = numbered().collect();
    for part in &model.parts {
        if let Some(prefix) = prefixes.get_mut(part.mesh_index) {
            *prefix = sanitize_group_prefix(&part.name);
        }
    }
    let names: HashSet<_> = prefixes.iter().map(String::as_str).collect();
    // A_B + C and A + B_C both produce A_B_C. Fall back to
    // distinct numeric namespaces when names or their prefixes overlap.
    if names.len() != prefixes.len()
        || names.contains("")
        || prefixes.iter().any(|prefix| {
            prefix
                .match_indices('_')
                .any(|(index, _)| names.contains(&prefix[..index]))
        })
    {
        numbered().collect()
    } else {
        prefixes
    }
}

fn exported_surface_group_name(
    model: &FemModel,
    surface: SurfaceSetRef,
    prefixes: Option<&[String]>,
) -> io::Result<String> {
    let mesh = model.meshes.get(surface.mesh_index).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("contact references missing mesh {}", surface.mesh_index),
        )
    })?;
    let surface_set = mesh
        .surface_sets
        .get(surface.surface_set_index)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "contact references missing surface set {} on mesh {}",
                    surface.surface_set_index, surface.mesh_index
                ),
            )
        })?;

    if let Some(prefixes) = prefixes {
        Ok(format!(
            "{}_{}",
            prefixes[surface.mesh_index], surface_set.name
        ))
    } else {
        Ok(surface_set.name.clone())
    }
}

fn exported_node_group_name(
    model: &FemModel,
    node_set: NodeSetRef,
    prefixes: Option<&[String]>,
) -> io::Result<String> {
    let mesh_index = node_set.mesh_index;
    let node_set_index = node_set.node_set_index;
    let mesh = model.meshes.get(mesh_index).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("contact references missing mesh {mesh_index}"),
        )
    })?;
    let node_set = mesh.node_sets.get(node_set_index).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "contact references missing node set {} on mesh {}",
                node_set_index, mesh_index
            ),
        )
    })?;

    if let Some(prefixes) = prefixes {
        Ok(format!("{}_{}", prefixes[mesh_index], node_set.name))
    } else {
        Ok(node_set.name.clone())
    }
}

fn resolve_contact_pairs(
    model: &FemModel,
    selected_mesh: Option<usize>,
    prefixes: Option<&[String]>,
) -> io::Result<Vec<ExportContactPair>> {
    let mut resolved = Vec::with_capacity(model.contacts.len());

    for contact in &model.contacts {
        if let Some(mesh_index) = selected_mesh {
            let master_selected = contact.master.mesh_index == mesh_index;
            let slave_selected = contact.slave.mesh_index() == mesh_index;

            if !master_selected && !slave_selected {
                continue;
            }

            if master_selected != slave_selected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "contact '{}' crosses mesh boundaries; export the assembly instead",
                        contact.name
                    ),
                ));
            }
        }

        let (slave_group, node_surface) = match contact.slave {
            ContactSlaveRef::Nodes(reference) => {
                (exported_node_group_name(model, reference, prefixes)?, true)
            }
            ContactSlaveRef::Surface(reference) => (
                exported_surface_group_name(model, reference, prefixes)?,
                false,
            ),
        };
        resolved.push(ExportContactPair {
            name: contact.name.clone(),
            node_surface,
            slave_group,
            master_group: exported_surface_group_name(model, contact.master, prefixes)?,
        });
    }

    Ok(resolved)
}

fn write_contact_pairs(out: &mut String, contacts: &[ExportContactPair]) {
    for contact in contacts {
        let pair_type = if contact.node_surface {
            "NODE-SURF"
        } else {
            "SURF-SURF"
        };
        writeln!(
            out,
            "!CONTACT PAIR, NAME={}, TYPE={pair_type}",
            contact.name
        )
        .unwrap();
        writeln!(out, " {},{}", contact.slave_group, contact.master_group).unwrap();
    }
}

fn write_id_list(out: &mut String, ids: impl Iterator<Item = u32>) {
    let mut col = 0;
    let ids: Vec<u32> = ids.collect();
    for (i, id) in ids.iter().enumerate() {
        if col == 0 {
            write!(out, " {id}").unwrap();
        } else {
            write!(out, ",{id}").unwrap();
        }
        col += 1;
        if col == 10 || i + 1 == ids.len() {
            writeln!(out).unwrap();
            col = 0;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GenerateRange {
    start: u32,
    end: u32,
    step: u32,
}

/// Writes a node/element group using `GENERATE` when an arithmetic-range
/// representation is shorter than explicitly listing at least four IDs.
/// Group membership is set-like in HECMW, so sorting and deduplication do
/// not change its meaning.
fn write_id_group(
    out: &mut String,
    header: &str,
    name_parameter: &str,
    name: &str,
    ids: impl Iterator<Item = u32>,
) {
    let mut ids: Vec<u32> = ids.collect();
    ids.sort_unstable();
    ids.dedup();

    let ranges = generate_ranges(&ids);
    let generated_field_count: usize = ranges
        .iter()
        .map(|range| usize::from(range.step != 1) + 2)
        .sum();
    let use_generate = ids.len() >= 4 && generated_field_count < ids.len();

    if use_generate {
        writeln!(out, "!{header},{name_parameter}={name},GENERATE").unwrap();

        for range in ranges {
            if range.step == 1 {
                writeln!(out, " {},{}", range.start, range.end).unwrap();
            } else {
                writeln!(out, " {},{},{}", range.start, range.end, range.step).unwrap();
            }
        }
    } else {
        writeln!(out, "!{header},{name_parameter}={name}").unwrap();
        write_id_list(out, ids.into_iter());
    }
}

fn generate_ranges(ids: &[u32]) -> Vec<GenerateRange> {
    let mut ranges = Vec::new();
    let mut index = 0;

    while index < ids.len() {
        if index + 1 == ids.len() {
            ranges.push(GenerateRange {
                start: ids[index],
                end: ids[index],
                step: 1,
            });
            break;
        }

        let step = ids[index + 1] - ids[index];
        let mut end_index = index + 1;

        while end_index + 1 < ids.len() && ids[end_index + 1] - ids[end_index] == step {
            end_index += 1;
        }

        ranges.push(GenerateRange {
            start: ids[index],
            end: ids[end_index],
            step,
        });
        index = end_index + 1;
    }

    ranges
}

/// Mesh-side material definitions are required even when a same-named
/// material model in `.cnt` overrides their values. Writing the known
/// elastic properties here also keeps a standalone `.msh` useful.
fn write_mesh_materials(out: &mut String, materials: &[FemMaterial]) {
    for material in materials {
        if material.mesh_fluid_placeholder {
            writeln!(out, "!MATERIAL,NAME={},ITEM=3", material.name).unwrap();
            writeln!(
                out,
                "!ITEM=1,SUBITEM=2\n 0,0\n!ITEM=2,SUBITEM=1\n 0\n!ITEM=3,SUBITEM=1\n 0",
            )
            .unwrap();
            continue;
        }

        let item_count = if material.density.is_some() { 2 } else { 1 };
        writeln!(out, "!MATERIAL,NAME={},ITEM={item_count}", material.name,).unwrap();
        writeln!(out, "!ITEM=1,SUBITEM=2").unwrap();
        writeln!(
            out,
            " {},{}",
            material.young_modulus.unwrap_or(0.0),
            material.poisson_ratio.unwrap_or(0.0),
        )
        .unwrap();
        if let Some(density) = material.density {
            writeln!(out, "!ITEM=2,SUBITEM=1").unwrap();
            writeln!(out, " {density}").unwrap();
        }
    }
}

fn write_mesh_section(out: &mut String, section: &Section, element_group: &str) {
    match section.kind {
        SectionKind::Solid => {
            writeln!(
                out,
                "!SECTION,TYPE=SOLID,EGRP={element_group},MATERIAL={}",
                section.material_name,
            )
            .unwrap();
        }
        SectionKind::Shell { thickness }
        | SectionKind::ShellWithIntegration { thickness, .. } => {
            writeln!(
                out,
                "!SECTION,TYPE=SHELL,EGRP={element_group},MATERIAL={}",
                section.material_name,
            )
            .unwrap();
            let integration_points = match section.kind {
                SectionKind::ShellWithIntegration { integration_points, .. } => integration_points,
                _ => 5,
            };
            writeln!(out, " {thickness},{integration_points}").unwrap();
        }
        SectionKind::Beam { .. } | SectionKind::BeamWithProperties { .. } => {
            writeln!(
                out,
                "!SECTION,TYPE=BEAM,EGRP={element_group},MATERIAL={}",
                section.material_name,
            )
            .unwrap();
            let (axis, area, iyy, izz, torsion) = match section.kind {
                SectionKind::BeamWithProperties { axis, area, iyy, izz, torsion } => {
                    (axis, area, iyy, izz, torsion)
                }
                SectionKind::Beam { area } => {
                    // The area-only editor uses an equivalent circular section.
                    let moment = area * area / (4.0 * std::f32::consts::PI);
                    ([0.0, 1.0, 0.0], area, moment, moment, 2.0 * moment)
                }
                _ => unreachable!(),
            };
            let [vx, vy, vz] = axis;
            writeln!(out, " {vx},{vy},{vz},{area},{iyy},{izz},{torsion}").unwrap();
        }
    }
}

pub(crate) fn element_type_code(et: &ElementType) -> Option<&'static str> {
    match et {
        ElementType::Rod2 => Some("111"),
        ElementType::Rod3 => Some("112"),
        ElementType::Tri3 => Some("231"),
        ElementType::Tri6 => Some("232"),
        ElementType::Quad4 => Some("241"),
        ElementType::Quad8 => Some("242"),
        ElementType::Truss2 => Some("301"),
        ElementType::Tet4 => Some("341"),
        ElementType::FluidTet4 => Some("3414"),
        ElementType::Tet10 => Some("342"),
        ElementType::Prism6 => Some("351"),
        ElementType::Prism15 => Some("352"),
        ElementType::Hex8 => Some("361"),
        ElementType::Hex20 => Some("362"),
        ElementType::Connector2 => Some("511"),
        ElementType::InterfaceQuad4 => Some("541"),
        ElementType::InterfaceQuad8 => Some("542"),
        ElementType::Beam611 => Some("611"),
        ElementType::Beam641 => Some("641"),
        ElementType::ShellTri3 => Some("731"),
        ElementType::ShellTri6 => Some("732"),
        ElementType::ShellQuad4 => Some("741"),
        ElementType::ShellQuad9 => Some("743"),
        ElementType::ShellTri3Mixed => Some("761"),
        ElementType::ShellQuad4Mixed => Some("781"),
        ElementType::Unsupported(_) => None,
    }
}

fn write_mesh_nodes(out: &mut String, mesh: &FemMesh, offset: u32) {
    for node in &mesh.nodes {
        let position = mesh.precise_node_position(node);
        writeln!(
            out,
            " {},{:e},{:e},{:e}",
            node.id.0 + offset,
            position.x,
            position.y,
            position.z,
        )
        .unwrap();
    }
}

fn build_msh(
    mesh: &FemMesh,
    mesh_index: usize,
    contacts: &[ExportContactPair],
    setup: Option<&AnalysisSetup>,
) -> String {
    let mut out = String::with_capacity(1024 * 64);

    // ── HEADER ──
    writeln!(out, "!HEADER").unwrap();
    writeln!(out, " Generated by bevyistr").unwrap();

    // ── NODE ──
    writeln!(out, "!NODE").unwrap();
    write_mesh_nodes(&mut out, mesh, 0);

    // ── ELEMENT — one !ELEMENT block per unique element type ──
    let mut type_codes_seen: Vec<&str> = Vec::new();
    for el in &mesh.elements {
        if let Some(code) = element_type_code(&el.element_type) {
            if !type_codes_seen.contains(&code) {
                type_codes_seen.push(code);
            }
        }
    }

    for code in &type_codes_seen {
        writeln!(out, "!ELEMENT,TYPE={code}").unwrap();

        for el in mesh
            .elements
            .iter()
            .filter(|e| element_type_code(&e.element_type) == Some(code))
        {
            let node_list: Vec<String> = el.nodes.iter().map(|n| n.0.to_string()).collect();
            writeln!(out, " {},{}", el.id.0, node_list.join(",")).unwrap();
        }
    }

    // ── NGROUP ──
    for nset in mesh.node_sets.iter().filter(|set| !set.name.eq_ignore_ascii_case("ALL")) {
        write_id_group(
            &mut out,
            "NGROUP",
            "NGRP",
            &nset.name,
            nset.nodes.iter().map(|n| n.0),
        );
    }

    // ── EGROUP ──
    for eset in mesh.element_sets.iter().filter(|set| !set.name.eq_ignore_ascii_case("ALL")) {
        write_id_group(
            &mut out,
            "EGROUP",
            "EGRP",
            &eset.name,
            eset.elements.iter().map(|e| e.0),
        );
    }
    // ── SGROUP (surface groups) ──
    for sset in &mesh.surface_sets {
        writeln!(out, "!SGROUP,SGRP={}", sset.name).unwrap();
        for surf in &sset.surfaces {
            writeln!(out, " {},{}", surf.element.0, surf.local_face.0).unwrap();
        }
    }

    if let Some(setup) = setup {
        write_mesh_materials(&mut out, &setup.materials);
        for section in setup
            .sections
            .iter()
            .filter(|section| section.mesh_index == mesh_index)
        {
            write_mesh_section(
                &mut out,
                section,
                section.element_set_name.as_deref().unwrap_or("ALL"),
            );
        }
        write_mpc_equations(&mut out, &setup.mpc_equations, |term_mesh, node| {
            (term_mesh == mesh_index).then_some(node.0)
        });
    }

    write_contact_pairs(&mut out, contacts);

    writeln!(out, "!END").unwrap();

    out
}

/// Writes all valid equations whose node references can be resolved by the
/// supplied export mapping. `!EQUATION` contains a sequence of equations;
/// each data block starts with its term count and constant, followed by up
/// to seven `(node, DOF, coefficient)` terms per line.
fn write_mpc_equations(
    out: &mut String,
    equations: &[MpcEquation],
    mut exported_node: impl FnMut(usize, NodeId) -> Option<u32>,
) {
    let resolved: Vec<_> = equations
        .iter()
        .filter(|equation| equation.is_valid())
        .filter_map(|equation| {
            let terms = equation
                .terms
                .iter()
                .map(|term| {
                    Some((
                        exported_node(term.mesh_index, term.node)?,
                        term.dof,
                        term.coefficient,
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            Some((equation, terms))
        })
        .collect();

    if resolved.is_empty() {
        return;
    }

    writeln!(out, "!EQUATION").unwrap();
    for (equation, terms) in resolved {
        if let Some(group) = equation.group.as_deref().filter(|group| !group.is_empty()) {
            writeln!(out, "!! {} {}", crate::MPC_GROUP_COMMENT_PREFIX, group).unwrap();
        }
        if !equation.name.is_empty() {
            writeln!(out, "!! {}", equation.name).unwrap();
        }
        writeln!(out, " {},{:.9e}", terms.len(), equation.constant).unwrap();
        for chunk in terms.chunks(7) {
            let line = chunk
                .iter()
                .map(|(node, dof, coefficient)| format!("{node},{dof},{coefficient:.9e}"))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(out, " {line}").unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use fem_core::{
        ContactPair, ContactType, ElementFaceRef, ElementId, FemMesh, FemNodeSet, FemSurfaceSet,
        LocalFaceId, MpcTerm, NodeId, NodeSetRef, SurfaceSetRef,
    };

    use super::*;

    fn add_surface(mesh: &mut FemMesh, name: &str, local_face: u32) {
        mesh.surface_sets.push(FemSurfaceSet {
            name: name.into(),
            surfaces: vec![ElementFaceRef::new(ElementId(0), LocalFaceId(local_face))],
        });
    }

    #[test]
    fn ambiguous_part_names_use_distinct_export_namespaces() {
        for (left, right) in [("part.step", "part.step"), ("A", "A_B"), ("a-b", "a_b")] {
            let mut model = FemModel::single_mesh(left, FemMesh::demo_hex8());
            model.add_mesh(right, FemMesh::demo_hex8());
            assert_eq!(part_group_prefixes(&model), ["PART1", "PART2"]);
        }
    }

    #[test]
    fn preserves_imported_section_properties() {
        let source = "!SECTION,TYPE=SHELL,EGRP=SKIN,MATERIAL=STEEL\n!! thickness and quadrature\n0.025,3\n!SECTION,TYPE=BEAM,EGRP=FRAME,MATERIAL=STEEL\n1,0,0,2,3,4,5\n";
        let (_, sections) = crate::parse_msh_setup(source, 0);
        assert_eq!(sections[0].kind, SectionKind::ShellWithIntegration {
            thickness: 0.025,
            integration_points: 3,
        });
        assert_eq!(sections[1].kind, SectionKind::BeamWithProperties {
            axis: [1.0, 0.0, 0.0],
            area: 2.0,
            iyy: 3.0,
            izz: 4.0,
            torsion: 5.0,
        });
        let mut output = String::new();
        for section in &sections {
            write_mesh_section(&mut output, section, section.name.as_str());
        }
        let (_, reloaded) = crate::parse_msh_setup(&output, 0);
        assert_eq!(reloaded.iter().map(|s| s.kind).collect::<Vec<_>>(),
                   sections.iter().map(|s| s.kind).collect::<Vec<_>>());
    }

    #[test]
    fn preserves_mesh_coordinates_through_export() {
        use bevy::math::DVec3;

        let mesh = crate::parse_mesh_str(
            "!NODE,SYSTEM=C\n1,2,90,0\n!! keep reading nodes\n2,4,,1e-7\n!NODE\n3,100000000,1.2345678901234567,0\n4,100000001,0,0\n",
        )
        .unwrap();
        assert!(mesh.nodes[0].position.x.abs() < 1e-12);
        assert_eq!(mesh.nodes[0].position.y, 2.0);
        assert_eq!(mesh.nodes[1].position.y, 0.0);
        assert_eq!(mesh.nodes[1].position.z, 1e-7);
        assert_eq!(mesh.nodes[2].position.x, mesh.nodes[3].position.x);
        let exported = build_msh(&mesh, 0, &[], None);
        let reloaded = crate::parse_mesh_str(&exported).unwrap();
        for (actual, expected) in reloaded.nodes.iter().zip(&mesh.nodes) {
            assert_eq!(
                reloaded.precise_node_position(actual),
                mesh.precise_node_position(expected),
            );
        }
        assert_eq!(reloaded.nodes.len(), mesh.nodes.len());
        assert_eq!(
            reloaded.precise_node_position(&reloaded.nodes[2]),
            DVec3::new(100000000.0, 1.2345678901234567, 0.0),
        );
        assert_eq!(
            reloaded.precise_node_position(&reloaded.nodes[3]).x,
            100000001.0,
        );
    }

    #[test]
    fn assembly_export_preserves_precise_coordinates_through_pose_edits() {
        use bevy::prelude::{Quat, Vec3};

        let mesh = crate::parse_mesh_str(
            "!NODE\n1,100000000,0,0\n2,100000001,0,0\n3,100000000,1,0\n4,100000000,0,1\n!ELEMENT,TYPE=341\n1,1,2,3,4\n",
        )
        .unwrap();
        let mut model = FemModel::single_mesh("First part", mesh.clone());
        let second = model.add_mesh("Second part", mesh);
        assert!(model.translate_part(second, Vec3::X));
        assert!(model.rotate_part_about_centroid(
            second,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ));
        model.meshes[second].nodes[1].position.z = 4.0;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("assembly.msh");
        write_msh_assembly(&path, &model).unwrap();
        let reloaded = crate::load_mesh_file(&path).unwrap();
        let first_node = &reloaded.nodes[1];
        let second_node = &reloaded.nodes[5];
        assert_eq!(first_node.id, NodeId(2));
        assert_eq!(second_node.id, NodeId(7));
        assert_eq!(reloaded.precise_node_position(first_node).x, 100000001.0);
        let position = reloaded.precise_node_position(second_node);
        assert_eq!(position.x, 100000002.0);
        assert!((position.y - 0.5).abs() < 1e-12);
        assert_eq!(position.z, 4.0);

        assert!(model.reset_part_pose(second));
        write_msh_assembly(&path, &model).unwrap();
        let reloaded = crate::load_mesh_file(&path).unwrap();
        assert_eq!(
            reloaded.precise_node_position(&reloaded.nodes[1]),
            reloaded.precise_node_position(&reloaded.nodes[5]),
        );
    }

    #[test]
    fn writes_surface_to_surface_contact_pair_in_slave_master_order() {
        let mut model = FemModel::demo_hex8();
        add_surface(&mut model.meshes[0], "MASTER", 1);
        add_surface(&mut model.meshes[0], "SLAVE", 2);
        model.contacts.push(ContactPair::new(
            "CP1",
            SurfaceSetRef::new(0, 0),
            SurfaceSetRef::new(0, 1),
            ContactType::Tied,
        ));

        let contacts = resolve_contact_pairs(&model, Some(0), None).unwrap();
        let text = build_msh(&model.meshes[0], 0, &contacts, None);

        assert!(text.contains("!CONTACT PAIR, NAME=CP1, TYPE=SURF-SURF\n SLAVE,MASTER\n"));
    }

    #[test]
    fn prefixes_contact_surface_groups_in_assembly_export() {
        let mut model = FemModel::single_mesh("gear", FemMesh::demo_hex8());
        model.add_mesh("shaft", FemMesh::demo_hex8());
        add_surface(&mut model.meshes[0], "TEETH", 1);
        add_surface(&mut model.meshes[1], "OUTER", 2);
        model.contacts.push(ContactPair::new(
            "GEAR_SHAFT",
            SurfaceSetRef::new(0, 0),
            SurfaceSetRef::new(1, 0),
            ContactType::SmallSliding,
        ));

        let contacts =
            resolve_contact_pairs(&model, None, Some(&part_group_prefixes(&model))).unwrap();

        assert_eq!(
            contacts,
            vec![ExportContactPair {
                name: "GEAR_SHAFT".into(),
                node_surface: false,
                slave_group: "SHAFT_OUTER".into(),
                master_group: "GEAR_TEETH".into(),
            }]
        );
    }

    #[test]
    fn writes_node_to_surface_contact_pair_from_tutorial_style_groups() {
        let mut model = FemModel::demo_hex8();
        model.meshes[0].node_sets.push(FemNodeSet {
            name: "SLAVE".into(),
            nodes: vec![NodeId(0)],
        });
        add_surface(&mut model.meshes[0], "MASTER", 1);
        model.contacts.push(ContactPair::new_node_surface(
            "CP1",
            SurfaceSetRef::new(0, 0),
            NodeSetRef::new(0, 0),
            ContactType::FiniteSliding,
        ));

        let contacts = resolve_contact_pairs(&model, Some(0), None).unwrap();
        let text = build_msh(&model.meshes[0], 0, &contacts, None);

        assert!(text.contains("!CONTACT PAIR, NAME=CP1, TYPE=NODE-SURF\n SLAVE,MASTER\n"));
    }

    #[test]
    fn preserves_every_documented_frontistr_element_code() {
        let mesh =
            crate::parse_mesh_str(include_str!("../tests/fixtures/element_library.msh")).unwrap();
        let text = build_msh(&mesh, 0, &[], None);
        let reparsed = crate::parse_mesh_str(&text).unwrap();
        let original_types: Vec<_> = mesh
            .elements
            .iter()
            .map(|element| element.element_type.clone())
            .collect();
        let reparsed_types: Vec<_> = reparsed
            .elements
            .iter()
            .map(|element| element.element_type.clone())
            .collect();

        assert_eq!(reparsed_types, original_types);

        for code in [
            "111", "112", "231", "232", "241", "242", "301", "341", "342", "351", "352", "361",
            "362", "511", "541", "542", "611", "641", "731", "732", "741", "743", "761", "781",
        ] {
            assert!(
                text.contains(&format!("!ELEMENT,TYPE={code}")),
                "missing HECMW element block {code}"
            );
        }
    }

    #[test]
    fn fluid_tetrahedron_geometry_round_trips_without_changing_element_type() {
        let source = "!NODE\n1,0,0,0\n2,1,0,0\n3,0,1,0\n4,0,0,1\n5,0,0,-1\n\
                      !ELEMENT,TYPE=3414\n10,1,2,3,4\n\
                      !ELEMENT,TYPE=341\n11,1,3,2,5\n\
                      !NGROUP,NGRP=WALL\n1,2,3\n!END\n";
        let mesh = crate::parse_mesh_str(source).unwrap();
        assert_eq!(mesh.elements[0].element_type, ElementType::FluidTet4);
        assert_eq!(mesh.elements[0].edge_node_ids().len(), 6);
        assert_eq!(mesh.elements[0].face_node_ids().len(), 4);
        assert_eq!(mesh.cached_boundary_faces().len(), 6);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("flow.msh");
        assert_eq!(write_msh_geometry_file(&path, &mesh).unwrap(), (5, 2));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("!ELEMENT,TYPE=3414\n 10,1,2,3,4\n"));
        let reloaded = crate::parse_mesh_str(&saved).unwrap();
        assert_eq!(reloaded.elements, mesh.elements);
        assert_eq!(reloaded.node_sets, mesh.node_sets);

        let model = FemModel::single_mesh("Flow mesh", reloaded);
        let report = crate::validate_frontistr_project(&model, &AnalysisSetup::default());
        assert!(report.has_errors());
        assert!(report.summary(4).contains("Flow setup for element type 3414"));
    }

    #[test]
    fn writes_compact_generate_groups_and_round_trips_membership() {
        let mut text = String::new();

        write_id_group(
            &mut text,
            "NGROUP",
            "NGRP",
            "CONTIGUOUS",
            [1, 2, 3, 4, 5, 6].into_iter(),
        );
        write_id_group(
            &mut text,
            "EGROUP",
            "EGRP",
            "ODD",
            [11, 13, 15, 17].into_iter(),
        );

        assert!(text.contains("!NGROUP,NGRP=CONTIGUOUS,GENERATE\n 1,6\n"));
        assert!(text.contains("!EGROUP,EGRP=ODD,GENERATE\n 11,17,2\n"));

        let mesh = crate::parse_mesh_str(&text).unwrap();

        assert_eq!(
            mesh.node_sets[0].nodes,
            [1, 2, 3, 4, 5, 6].map(fem_core::NodeId).to_vec()
        );
        assert_eq!(
            mesh.element_sets[0].elements,
            [11, 13, 15, 17].map(ElementId).to_vec()
        );
    }

    #[test]
    fn writes_official_equation_terms_and_remaps_part_nodes() {
        let equation = MpcEquation::new(
            "SPIDER_1_UX",
            0.0,
            vec![
                MpcTerm::new(0, NodeId(2), 1, 1.0),
                MpcTerm::new(1, NodeId(3), 1, -1.0),
            ],
        )
        .with_group("SPIDER_1");
        let mut text = String::new();

        write_mpc_equations(&mut text, &[equation], |mesh_index, node| {
            Some(node.0 + [10, 100][mesh_index])
        });

        assert!(text.starts_with("!EQUATION\n"));
        assert!(text.contains("!! @bevyistr-group: SPIDER_1\n"));
        assert!(text.contains("!! SPIDER_1_UX\n 2,0.000000000e0\n"));
        assert!(text.contains(" 12,1,1.000000000e0,103,1,-1.000000000e0\n"));

        let source = format!("!NODE\n 12,0,0,0\n 103,1,0,0\n{text}!END\n");
        let mesh = crate::parse_mesh_str(&source).unwrap();
        let equations = crate::parse_msh_equations(&source, &mesh);
        assert_eq!(equations.len(), 1);
        assert_eq!(equations[0].group.as_deref(), Some("SPIDER_1"));
    }
}
