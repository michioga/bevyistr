use bevy::asset::RenderAssetUsages;
use bevy::math::primitives::{Cuboid, Cylinder};
use bevy::mesh::{Mesh3d, PrimitiveTopology};
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use fem_core::{
    ContactCandidate, ContactCandidateState, ContactPair, ContactSlaveRef, ElementFaceRef, FaceId,
    FemEdge, FemElement, FemEntityId, FemEntityRef, FemFace, FemMesh, FemModel, FemNode,
    FemResultSet, NodeId, SurfaceSetRef, rainbow_color,
};
use interaction::HoverResult;
use std::collections::BTreeSet;

use selection::{
    EdgeEntity, ElementEntity, FaceEntity, Hovered, NodeEntity, Selectable, Selected,
    SelectionState,
};

const NODE_SIZE: f32 = 0.12;
const EDGE_THICKNESS: f32 = 0.04;
const FACE_THICKNESS: f32 = 0.012;
const MIN_VISUAL_SIZE: f32 = 0.01;
const ENTITY_RENDER_LIMIT: usize = 30_000;
const MAX_DEFINED_CONTACT_NODE_MARKERS: usize = 20_000;

#[derive(Resource, Debug, Clone)]
pub struct VisualizationSettings {
    pub mode: VisualizationMode,

    /// When `Some`, the aggregate surface is coloured by this result field.
    pub contour: Option<ContourSettings>,
}

impl Default for VisualizationSettings {
    fn default() -> Self {
        Self {
            mode: VisualizationMode::ShadedWithEdges,
            contour: None,
        }
    }
}

/// View-only aids for reviewing an automatically detected contact pair.
///
/// Separation is deliberately expressed as a percentage of the model's
/// bounding-box diagonal and is applied only to render transforms. FEM node
/// coordinates, contact search geometry, and exported data remain unchanged.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ContactReviewSettings {
    pub active: bool,

    pub ghost_others: bool,

    pub separation_percent: f32,
}

impl Default for ContactReviewSettings {
    fn default() -> Self {
        Self {
            active: false,
            ghost_others: true,
            separation_percent: 8.0,
        }
    }
}

/// View-only selection of a contact pair already defined in the model.
///
/// This is separate from [`ContactReviewSettings`], which controls the
/// exploded review of automatically detected candidates. Defined contacts
/// never move parts: they only colour the master side blue and the slave
/// side orange while the Contact page is active.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DefinedContactPreview {
    pub selected: Option<usize>,

    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactDraftSurface {
    pub mesh_index: usize,

    pub surfaces: Vec<ElementFaceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContactDraftSlave {
    Nodes {
        mesh_index: usize,
        nodes: Vec<NodeId>,
    },

    Surface(ContactDraftSurface),
}

/// Geometry captured while a new contact pair is being assembled in the UI.
/// It intentionally stores raw members instead of creating solver groups
/// immediately, so recapturing a side does not leave orphan NGRP/SGRP entries.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub struct ContactDraftPreview {
    pub master: Option<ContactDraftSurface>,

    pub slave: Option<ContactDraftSlave>,

    pub active: bool,
}

impl ContactDraftPreview {
    pub fn clear(&mut self) {
        self.master = None;
        self.slave = None;
        self.active = false;
    }
}

#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContactReviewPose {
    active: bool,

    ghost_others: bool,

    mesh_a: usize,

    mesh_b: usize,

    offset_a: Vec3,

    offset_b: Vec3,
}

impl Default for ContactReviewPose {
    fn default() -> Self {
        Self {
            active: false,
            ghost_others: true,
            mesh_a: 0,
            mesh_b: 0,
            offset_a: Vec3::ZERO,
            offset_b: Vec3::ZERO,
        }
    }
}

/// Which result field to display as a rainbow contour, and optional
/// deformation scaling.
#[derive(Debug, Clone)]
pub struct ContourSettings {
    /// Mesh index within `FemModel::meshes`.
    pub mesh_index: usize,

    pub step_index: usize,

    pub field_name: String,

    /// If `true`, node positions are offset by `displacement_field × deformation_scale`.
    pub show_deformation: bool,

    pub displacement_field: String,

    /// Scale factor applied to the raw displacement vector before offsetting.
    pub deformation_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualizationMode {
    ShadedWithEdges,

    Shaded,

    /// Unlit solid colour — no PBR highlights, depth readable via ambient
    /// occlusion only.  Equivalent to a "clay model" look.
    Flat,

    /// GPU wireframe via [`bevy::pbr::wireframe::Wireframe`] — shows every
    /// triangle edge of the boundary surface mesh.
    Wireframe,

    /// Semi-transparent shaded surface — lets internal elements, contacts,
    /// and interior boundary faces show through. Useful for checking
    /// internal structure (ribs, cavities, contact interfaces) without
    /// switching to a section/clip view.
    Transparent,

    Edges,
}

impl VisualizationMode {
    pub const ALL: [Self; 6] = [
        Self::ShadedWithEdges,
        Self::Shaded,
        Self::Flat,
        Self::Wireframe,
        Self::Transparent,
        Self::Edges,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ShadedWithEdges => "Both",
            Self::Shaded => "Shaded",
            Self::Flat => "Flat",
            Self::Wireframe => "Wire",
            Self::Transparent => "X-ray",
            Self::Edges => "Edges",
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisualLayer {
    Shaded,

    Edge,

    Node,
}

impl VisualLayer {
    const fn visible_in(self, mode: VisualizationMode) -> bool {
        match (self, mode) {
            // Shaded surface visible in all modes that show a solid mesh.
            (
                Self::Shaded,
                VisualizationMode::Shaded
                | VisualizationMode::ShadedWithEdges
                | VisualizationMode::Flat
                | VisualizationMode::Wireframe
                | VisualizationMode::Transparent,
            ) => true,
            // Boundary-edge cuboids visible alongside shading or alone.
            // In Wireframe mode we suppress them: the GPU wireframe already
            // shows every triangle edge so adding boundary-edge cuboids on
            // top creates a double-edge artefact.
            (
                Self::Edge,
                VisualizationMode::Edges
                | VisualizationMode::ShadedWithEdges
                | VisualizationMode::Transparent,
            ) => true,
            // Nodes only in Both mode.
            (Self::Node, VisualizationMode::ShadedWithEdges) => true,
            _ => false,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopologyHighlight {
    Hover,

    Selected,
}

/// Marks the overlay entities that highlight the master/slave sides of the
/// active contact review.
///
/// Unlike [`TopologyHighlight`], each of these covers an arbitrary number of
/// The same two entities are reused for both an automatically detected
/// candidate and a contact pair already defined in the model. Their meshes
/// are rebuilt from scratch whenever the active review changes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactCandidateHighlight {
    Master,

    Slave,
}

#[derive(Component, Debug, Clone, Copy, Default)]
pub(crate) struct ContactHighlightAvailability(bool);

#[derive(Default)]
pub(crate) struct TopologyHighlightCache {
    /// The last-rendered preview group, so [`update_topology_highlights`]
    /// only rebuilds geometry when it actually changes (rebuilding walks
    /// every target's boundary face and rebuilds a full merged mesh, not
    /// free on a large model).
    hover: Vec<FemEntityRef>,

    selected: Vec<FemEntityRef>,
}

/// Marker for any entity spawned to visualize the current [`FemModel`].
///
/// All per-element/face/edge/node visuals and the aggregated boundary
/// surface/edge visuals carry this component so they can be cleanly
/// despawned and respawned when the model is reloaded.
#[derive(Component, Debug, Clone, Copy)]
pub struct FemMeshVisual;

/// Identifies which assembly mesh produced a visual entity. During a direct
/// manipulation drag, every visual for one part receives the same temporary
/// transform and the FEM node coordinates are updated only on release.
#[derive(Component, Debug, Clone, Copy)]
pub struct FemPartVisual {
    pub mesh_index: usize,
}

#[derive(Component)]
pub struct NormalMaterial(pub Handle<StandardMaterial>);

/// Unlit material used in [`VisualizationMode::Flat`].
///
/// Stored alongside [`NormalMaterial`] on every shaded entity so that
/// [`apply_visualization_mode`] can switch between them without touching
/// the asset storage.
#[derive(Component)]
pub struct FlatMaterial(pub Handle<StandardMaterial>);

/// Semi-transparent material used in [`VisualizationMode::Transparent`].
#[derive(Component)]
pub struct TransparentMaterial(pub Handle<StandardMaterial>);

#[derive(Component)]
pub struct HoverMaterial(pub Handle<StandardMaterial>);

#[derive(Component)]
pub struct SelectedMaterial(pub Handle<StandardMaterial>);

#[derive(Clone)]
struct MaterialSet {
    normal: Handle<StandardMaterial>,

    hover: Handle<StandardMaterial>,

    selected: Handle<StandardMaterial>,

    /// Unlit material used in [`VisualizationMode::Flat`].
    flat: Handle<StandardMaterial>,

    /// Semi-transparent material used in [`VisualizationMode::Transparent`].
    transparent: Handle<StandardMaterial>,
}

pub fn spawn_demo_mesh(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    analysis_setup: Option<Res<fem_core::AnalysisSetup>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let fem_model = model
        .as_deref()
        .cloned()
        .unwrap_or_else(FemModel::demo_hex8);

    if model.is_none() {
        commands.insert_resource(fem_model.clone());
    }

    spawn_model_visuals(
        &mut commands,
        &mut meshes,
        &mut materials,
        &fem_model,
        analysis_setup.as_deref(),
    );
}

/// Spawns all per-mesh visualization entities for `fem_model`.
///
/// Every spawned entity carries [`FemMeshVisual`] so that a later reload
/// can despawn exactly this set before respawning from the new model.
///
/// When the model has more than one mesh (i.e. more than one
/// [`fem_core::Part`] — an assembly built via `add_mesh`/Import), each
/// mesh's *base* colour (normal / flat / transparent) is hue-rotated by
/// [`part_hue_shift`] so parts are visually distinguishable at a glance.
/// Hover (yellow) and selected (green) colours are never tinted — those
/// convey interaction state, not part identity, and must stay consistent
/// across the whole assembly.
pub(crate) fn spawn_model_visuals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    fem_model: &FemModel,
    analysis_setup: Option<&fem_core::AnalysisSetup>,
) {
    // Selected colour: bright opaque lime-green — same hue as the
    // topology-highlight overlay so per-entity and aggregate models look
    // consistent when something is selected. Selection must write depth;
    // otherwise rear faces can show through when the camera moves.
    let selected_element = Color::srgb(0.10, 1.0, 0.45);
    let selected_face = Color::srgb(0.10, 1.0, 0.45);
    let selected_edge = Color::srgb(0.10, 1.0, 0.45);
    let selected_node = Color::srgb(0.10, 1.0, 0.45);

    let hover_element = Color::srgba(1.0, 0.75, 0.15, 0.70);
    let hover_face = Color::srgba(1.0, 0.88, 0.15, 0.70);
    let hover_edge = Color::srgb(1.0, 0.82, 0.15);
    let hover_node = Color::srgb(1.0, 0.82, 0.18);

    let multi_part = fem_model.meshes.len() > 1;
    let model_scale = model_visual_scale(fem_model);

    for (mesh_index, fem_mesh) in fem_model.meshes.iter().enumerate() {
        let hue_shift = if multi_part {
            part_hue_shift(mesh_index)
        } else {
            0.0
        };

        // Base (normal) colours, hue-shifted per part.
        let normal_element = tint_hue(Color::srgba(0.25, 0.45, 0.95, 0.22), hue_shift);
        let normal_face = tint_hue(Color::srgba(0.20, 0.70, 0.65, 0.18), hue_shift);
        let normal_edge = tint_hue(Color::srgb(0.12, 0.14, 0.16), hue_shift);
        let normal_node = tint_hue(Color::srgb(0.82, 0.88, 0.95), hue_shift);

        // Flat colours: unlit, slightly lighter than the corresponding
        // normal colour so the same object is still recognisable in Flat
        // mode, also hue-shifted per part.
        let flat_element = tint_hue(Color::srgba(0.45, 0.62, 0.92, 0.45), hue_shift);
        let flat_face = tint_hue(Color::srgba(0.46, 0.72, 0.68, 0.45), hue_shift);
        let flat_edge = tint_hue(Color::srgb(0.30, 0.34, 0.38), hue_shift);
        let flat_node = tint_hue(Color::srgb(0.90, 0.93, 0.97), hue_shift);

        let element_materials = material_set(
            materials,
            normal_element,
            hover_element,
            selected_element,
            flat_element,
            true,
        );
        let face_materials = material_set(
            materials,
            normal_face,
            hover_face,
            selected_face,
            flat_face,
            true,
        );
        let edge_materials = material_set(
            materials,
            normal_edge,
            hover_edge,
            selected_edge,
            flat_edge,
            false,
        );
        let node_materials = material_set(
            materials,
            normal_node,
            hover_node,
            selected_node,
            flat_node,
            false,
        );

        // Bright, unmistakably-not-a-normal-colour magenta for element
        // types this platform doesn't recognize (`ElementType::Unsupported`).
        // Hover/selected stay the usual yellow/green so interaction state
        // still reads consistently, but the *resting* colour deliberately
        // clashes with every other part of the model — a parser gap should
        // be obvious at a glance, not blend in as if it were ordinary solid
        // geometry.
        let warning_materials = material_set(
            materials,
            Color::srgba(0.95, 0.05, 0.85, 0.85),
            hover_element,
            selected_element,
            Color::srgba(0.95, 0.05, 0.85, 0.55),
            true,
        );

        if use_aggregate_rendering(fem_mesh) {
            spawn_aggregate_surface_visual(
                commands, meshes, materials, mesh_index, fem_mesh, hue_shift,
            );

            continue;
        }

        let section_map = analysis_setup
            .map(|setup| setup.build_element_section_map(mesh_index, fem_mesh))
            .unwrap_or_default();

        for element in &fem_mesh.elements {
            let section = section_map.get(&element.id).copied();
            let materials_for_element =
                if matches!(element.element_type, fem_core::ElementType::Unsupported(_)) {
                    &warning_materials
                } else {
                    &element_materials
                };

            spawn_element_visual(
                commands,
                meshes,
                mesh_index,
                fem_mesh,
                element,
                materials_for_element,
                section,
                model_scale,
            );
        }

        for face in fem_mesh.cached_boundary_faces() {
            spawn_face_visual(
                commands,
                meshes,
                mesh_index,
                fem_mesh,
                face,
                &face_materials,
            );
        }

        for edge in fem_mesh.cached_edges() {
            spawn_edge_visual(
                commands,
                meshes,
                mesh_index,
                fem_mesh,
                edge,
                &edge_materials,
            );
        }

        for node in &fem_mesh.nodes {
            spawn_node_visual(commands, meshes, mesh_index, node, &node_materials);
        }
    }
}

/// Hue rotation, in degrees, applied to the `mesh_index`-th part's base
/// colour so an assembly's parts are visually distinguishable.
///
/// Uses the golden-angle (~137.5°) increment, which spreads any number of
/// parts around the colour wheel with minimal adjacent-hue collisions —
/// the same technique used for well-distributed categorical palettes.
fn part_hue_shift(mesh_index: usize) -> f32 {
    const GOLDEN_ANGLE_DEG: f32 = 137.50776;

    (mesh_index as f32 * GOLDEN_ANGLE_DEG).rem_euclid(360.0)
}

/// Rotates `color`'s hue by `shift_deg` degrees, preserving saturation,
/// lightness, and alpha. A `shift_deg` of `0.0` returns `color` unchanged
/// (skips the HSLA round-trip).
fn tint_hue(color: Color, shift_deg: f32) -> Color {
    if shift_deg.abs() < 1.0e-3 {
        return color;
    }

    let hsla: Hsla = color.into();
    let new_hue = (hsla.hue + shift_deg).rem_euclid(360.0);

    Color::Hsla(Hsla {
        hue: new_hue,
        ..hsla
    })
}

fn use_aggregate_rendering(fem_mesh: &FemMesh) -> bool {
    fem_mesh.nodes.len()
        + fem_mesh.elements.len()
        + fem_mesh.cached_edges().len()
        + fem_mesh.cached_boundary_faces().len()
        > ENTITY_RENDER_LIMIT
}

fn spawn_aggregate_surface_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    mesh_index: usize,
    fem_mesh: &FemMesh,
    hue_shift: f32,
) {
    if let Some(mesh) = build_part_surface_mesh(fem_mesh) {
        let normal_mat = materials.add(StandardMaterial {
            base_color: tint_hue(Color::srgb(0.35, 0.52, 0.68), hue_shift),
            perceptual_roughness: 0.82,
            cull_mode: None,
            ..default()
        });

        // Flat / clay-model material: unlit, same hue but slightly lighter
        // so the mesh is still recognisably the same object.
        let flat_mat = materials.add(StandardMaterial {
            base_color: tint_hue(Color::srgb(0.48, 0.64, 0.76), hue_shift),
            unlit: true,
            cull_mode: None,
            ..default()
        });

        // Transparent / "X-ray" material: low, fixed alpha so internal
        // elements, contact interfaces, and overlapping parts show through.
        let transparent_mat = materials.add(StandardMaterial {
            base_color: tint_hue(Color::srgba(0.35, 0.52, 0.68, 0.18), hue_shift),
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            double_sided: true,
            ..default()
        });

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(normal_mat.clone()),
            Transform::default(),
            VisualLayer::Shaded,
            Visibility::Visible,
            NormalMaterial(normal_mat),
            FlatMaterial(flat_mat),
            TransparentMaterial(transparent_mat),
            FemPartVisual { mesh_index },
            FemMeshVisual,
            Name::new("Aggregated boundary surface"),
        ));
    }

    if let Some(mesh) = build_part_edge_mesh(fem_mesh) {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.04, 0.05, 0.055),
            unlit: true,
            ..default()
        });

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::default(),
            VisualLayer::Edge,
            Visibility::Visible,
            FemPartVisual { mesh_index },
            FemMeshVisual,
            Name::new("Aggregated boundary edges"),
        ));
    }
}

pub(crate) fn spawn_topology_highlights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Hover: warm yellow — "you can click this"
    //
    // `depth_bias` (not a geometric vertex offset) is what keeps this
    // coincident with the true surface from fighting/flickering — see
    // `build_multi_face_highlight_mesh`'s doc comment for why a vertex
    // offset causes a jagged silhouette on curved surfaces at grazing
    // viewing angles (very visible on a coplanar-selected bore or fillet)
    // and isn't used here.
    let hover_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.88, 0.15, 0.70),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        unlit: true,
        depth_bias: 2.0,
        ..default()
    });

    // Selected: bright opaque lime-green. Keeping this in the opaque render
    // pass makes it write depth, so rear selected faces and internal edges do
    // not leak through a merged multi-face highlight as the camera moves.
    // Back-face culling stays disabled for shell meshes that must remain
    // selectable and visible from either side.
    let mut selected_material = selection_material(Color::srgb(0.10, 1.0, 0.45));
    selected_material.cull_mode = None;
    selected_material.double_sided = true;
    selected_material.unlit = true;
    selected_material.depth_bias = 3.0;
    let selected_material = materials.add(selected_material);

    spawn_topology_highlight(
        &mut commands,
        &mut meshes,
        hover_material,
        TopologyHighlight::Hover,
        "Topology hover highlight",
    );
    spawn_topology_highlight(
        &mut commands,
        &mut meshes,
        selected_material,
        TopologyHighlight::Selected,
        "Topology selected highlight",
    );
}

fn spawn_topology_highlight(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    highlight: TopologyHighlight,
    name: &'static str,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.01, 0.01, 0.01))),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::Hidden,
        highlight,
        Name::new(name),
    ));
}

/// Spawns the (initially hidden) master/slave overlay entities used by
/// [`update_contact_candidate_highlights`] to preview the currently
/// selected contact candidate.
pub(crate) fn spawn_contact_candidate_highlights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let master_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.25, 0.55, 1.0, 0.55),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        unlit: true,
        depth_bias: 2.0,
        ..default()
    });
    let slave_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.55, 0.10, 0.55),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        unlit: true,
        depth_bias: 2.0,
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.01, 0.01, 0.01))),
        MeshMaterial3d(master_material),
        Transform::default(),
        Visibility::Hidden,
        ContactCandidateHighlight::Master,
        ContactHighlightAvailability::default(),
        Name::new("Contact candidate master highlight"),
    ));

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.01, 0.01, 0.01))),
        MeshMaterial3d(slave_material),
        Transform::default(),
        Visibility::Hidden,
        ContactCandidateHighlight::Slave,
        ContactHighlightAvailability::default(),
        Name::new("Contact candidate slave highlight"),
    ));
}

pub fn update_hover_materials(
    settings: Res<VisualizationSettings>,
    mut query: Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        &NormalMaterial,
        Option<&FlatMaterial>,
        Option<&TransparentMaterial>,
        &HoverMaterial,
        &SelectedMaterial,
        Option<&Hovered>,
        Option<&Selected>,
    )>,
) {
    let use_flat = matches!(settings.mode, VisualizationMode::Flat);
    let use_transparent = matches!(settings.mode, VisualizationMode::Transparent);

    for (mut material, normal, flat, transparent, hover, selected, hovered, is_selected) in
        query.iter_mut()
    {
        // Selected / hovered states always use their vivid materials,
        // regardless of the active visualization mode — selection must be
        // visible no matter what.
        if is_selected.is_some() {
            material.0 = selected.0.clone();
        } else if hovered.is_some() {
            material.0 = hover.0.clone();
        } else if use_flat {
            // In Flat mode, non-selected entities use the unlit flat material.
            // Fall back to NormalMaterial when FlatMaterial is not present
            // (e.g. edge / node entities that haven't been given one yet).
            material.0 = flat
                .map(|f| f.0.clone())
                .unwrap_or_else(|| normal.0.clone());
        } else if use_transparent {
            // Same fallback logic as Flat above, but for the X-ray material.
            // Without this branch, this system (which runs every frame)
            // would overwrite the Transparent material that
            // `apply_visualization_mode` set on mode-change frames as soon
            // as one frame passes without a mode change.
            material.0 = transparent
                .map(|t| t.0.clone())
                .unwrap_or_else(|| normal.0.clone());
        } else {
            material.0 = normal.0.clone();
        }
    }
}

/// Rebuilds the hover and selected highlight overlays whenever either
/// group of targets changes.
///
/// Both overlays cover a *set* of targets, not just one: [`TopologyHighlight::Selected`]
/// shows the model's entire current selection (every face the person has
/// clicked/box-selected, not only the most recent one), and
/// [`TopologyHighlight::Hover`] shows [`fem_core::HoverPreviewTargets`] —
/// the full Coplanar/Smooth group that would be added if the person clicked
/// right now, computed by `ui`'s `update_hover_preview_group` (or just the
/// single hovered entity in Single mode).
pub(crate) fn update_topology_highlights(
    model: Option<Res<FemModel>>,
    hover_preview: Res<fem_core::HoverPreviewTargets>,
    selection: Res<SelectionState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: Local<TopologyHighlightCache>,
    mut query: Query<
        (
            &TopologyHighlight,
            &mut Mesh3d,
            &mut Transform,
            &mut Visibility,
        ),
        Without<VisualLayer>,
    >,
) {
    let Some(model) = model else {
        hide_topology_highlights(&mut query);

        return;
    };

    // When every hover-preview target is already part of the selection
    // (the common case: hovering the thing you just selected, or —
    // with surface growth on — hovering back over the same group),
    // hide the hover overlay entirely and let only the selected (bright
    // green) overlay show. Without this both overlays render at the same
    // position and blend into a confusing colour.
    let hover_is_redundant = !hover_preview.targets.is_empty()
        && hover_preview
            .targets
            .iter()
            .all(|t| selection.targets.contains(t));

    let preview_highlights: &[FemEntityRef] = if hover_preview.highlight_targets.is_empty() {
        &hover_preview.targets
    } else {
        &hover_preview.highlight_targets
    };
    let selected_highlights: &[FemEntityRef] = if selection.highlight_targets.is_empty() {
        &selection.targets
    } else {
        &selection.highlight_targets
    };
    let hover_targets: &[FemEntityRef] = if hover_is_redundant {
        &[]
    } else {
        preview_highlights
    };

    if cache.hover.as_slice() == hover_targets && cache.selected == selected_highlights {
        return;
    }

    cache.hover = hover_targets.to_vec();
    cache.selected = selected_highlights.to_vec();

    for (highlight, mut mesh, mut transform, mut visibility) in &mut query {
        let targets: &[FemEntityRef] = match highlight {
            TopologyHighlight::Hover => hover_targets,
            TopologyHighlight::Selected => selected_highlights,
        };

        if targets.is_empty() {
            *visibility = Visibility::Hidden;
            continue;
        }

        if apply_topology_highlight(
            &model,
            targets,
            &mut meshes,
            &mut mesh,
            &mut transform,
            &mut visibility,
        )
        .is_none()
        {
            *visibility = Visibility::Hidden;
        }
    }
}

pub(crate) fn update_visual_layer_visibility(
    settings: Res<VisualizationSettings>,
    mut query: Query<(&VisualLayer, &mut Visibility), Without<TopologyHighlight>>,
) {
    if !settings.is_changed() {
        return;
    }

    for (layer, mut visibility) in &mut query {
        *visibility = if layer.visible_in(settings.mode) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

/// Switches materials between PBR and unlit (Flat mode) and toggles the
/// `Wireframe` component on boundary-surface entities when the
/// [`VisualizationMode`] changes.
///
/// * **Flat** — replaces the material handle on every `VisualLayer::Shaded`
///   entity with its stored [`FlatMaterial`], giving an unlit clay-model look.
/// * **Wireframe** — inserts Bevy's `Wireframe` component on those same
///   entities so the GPU renders their triangle edges rather than filled
///   triangles.
/// * **Any other mode** — restores the [`NormalMaterial`] and removes the
///   `Wireframe` component.
pub(crate) fn apply_visualization_mode(
    settings: Res<VisualizationSettings>,
    mut commands: Commands,
    mut query: Query<
        (
            Entity,
            &VisualLayer,
            &mut MeshMaterial3d<StandardMaterial>,
            &NormalMaterial,
            &FlatMaterial,
            &TransparentMaterial,
        ),
        With<FemMeshVisual>,
    >,
) {
    if !settings.is_changed() {
        return;
    }

    for (entity, layer, mut mat, normal, flat, transparent) in &mut query {
        if *layer != VisualLayer::Shaded {
            continue;
        }

        match settings.mode {
            VisualizationMode::Flat => {
                mat.0 = flat.0.clone();
                commands
                    .entity(entity)
                    .remove::<bevy::pbr::wireframe::Wireframe>();
            }
            VisualizationMode::Wireframe => {
                mat.0 = normal.0.clone();
                commands
                    .entity(entity)
                    .insert(bevy::pbr::wireframe::Wireframe);
            }
            VisualizationMode::Transparent => {
                mat.0 = transparent.0.clone();
                commands
                    .entity(entity)
                    .remove::<bevy::pbr::wireframe::Wireframe>();
            }
            _ => {
                mat.0 = normal.0.clone();
                commands
                    .entity(entity)
                    .remove::<bevy::pbr::wireframe::Wireframe>();
            }
        }
    }
}

/// Resolves the selected contact candidate into render-only part offsets.
/// This is kept separate from [`apply_contact_review`] so face-centroid
/// calculations run only when the candidate, model, or review settings
/// change rather than on every frame.
pub(crate) fn update_contact_review_pose(
    model: Option<Res<FemModel>>,
    candidates: Res<ContactCandidateState>,
    settings: Res<ContactReviewSettings>,
    mut pose: ResMut<ContactReviewPose>,
) {
    let model_changed = model.as_ref().is_some_and(|model| model.is_changed());

    if !model_changed && !candidates.is_changed() && !settings.is_changed() {
        return;
    }

    let next = model
        .as_deref()
        .zip(candidates.selected_candidate())
        .filter(|_| settings.active)
        .map(|(model, candidate)| {
            let (offset_a, offset_b) =
                contact_review_offsets(model, candidate, settings.separation_percent);

            ContactReviewPose {
                active: true,
                ghost_others: settings.ghost_others,
                mesh_a: candidate.mesh_a,
                mesh_b: candidate.mesh_b,
                offset_a,
                offset_b,
            }
        })
        .unwrap_or_default();

    if *pose != next {
        *pose = next;
    }
}

/// Applies contact-review ghosting and exploded offsets to model visuals.
///
/// When review is inactive this system only runs once on the active→inactive
/// transition, restoring the ordinary render state. That is important because
/// the assembly editor uses the same render transforms for its drag preview.
pub(crate) fn apply_contact_review(
    pose: Res<ContactReviewPose>,
    settings: Res<VisualizationSettings>,
    mut visuals: Query<(
        &FemPartVisual,
        &VisualLayer,
        &mut Transform,
        &mut Visibility,
        Option<&mut MeshMaterial3d<StandardMaterial>>,
        Option<&TransparentMaterial>,
    )>,
) {
    if !pose.active && !pose.is_changed() {
        return;
    }

    for (part, layer, mut transform, mut visibility, material, transparent) in &mut visuals {
        let relevant =
            pose.active && (part.mesh_index == pose.mesh_a || part.mesh_index == pose.mesh_b);

        transform.translation = if !pose.active {
            Vec3::ZERO
        } else if part.mesh_index == pose.mesh_a {
            pose.offset_a
        } else if part.mesh_index == pose.mesh_b {
            pose.offset_b
        } else {
            Vec3::ZERO
        };
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;

        *visibility = if layer.visible_in(settings.mode) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

        if pose.active && pose.ghost_others && !relevant {
            match (*layer, material, transparent) {
                (VisualLayer::Shaded, Some(mut material), Some(transparent)) => {
                    material.0 = transparent.0.clone();
                    *visibility = Visibility::Visible;
                }
                _ => {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}

fn contact_review_offsets(
    model: &FemModel,
    candidate: &ContactCandidate,
    separation_percent: f32,
) -> (Vec3, Vec3) {
    if candidate.is_self_contact() {
        return (Vec3::ZERO, Vec3::ZERO);
    }

    let Some(mesh_a) = model.meshes.get(candidate.mesh_a) else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    let Some(mesh_b) = model.meshes.get(candidate.mesh_b) else {
        return (Vec3::ZERO, Vec3::ZERO);
    };

    let contact_a = face_group_centroid(mesh_a, &candidate.faces_a);
    let contact_b = face_group_centroid(mesh_b, &candidate.faces_b);
    let part_a = mesh_a.bounds().map(|(min, max)| (min + max) * 0.5);
    let part_b = mesh_b.bounds().map(|(min, max)| (min + max) * 0.5);

    let direction = contact_a
        .zip(contact_b)
        .and_then(|(a, b)| (b - a).try_normalize())
        .or_else(|| {
            part_a
                .zip(part_b)
                .and_then(|(a, b)| (b - a).try_normalize())
        })
        .unwrap_or(Vec3::X);

    let diagonal = model
        .bounds()
        .map(|(min, max)| (max - min).length())
        .unwrap_or(0.0);
    let half_separation = diagonal * separation_percent.clamp(0.0, 30.0) * 0.005;

    (-direction * half_separation, direction * half_separation)
}

fn face_group_centroid(mesh: &FemMesh, face_ids: &[FaceId]) -> Option<Vec3> {
    let ids: BTreeSet<FaceId> = face_ids.iter().copied().collect();
    let mut total = Vec3::ZERO;
    let mut count = 0usize;

    for face in mesh
        .cached_boundary_faces()
        .iter()
        .filter(|face| ids.contains(&face.id))
    {
        if let Some(geometry) = mesh.face_geometry(face) {
            total += geometry.centroid;
            count += 1;
        }
    }

    (count > 0).then(|| total / count as f32)
}

/// Despawns and respawns all [`FemMeshVisual`] entities whenever
/// [`FemModelVersion`] changes (e.g. after a mesh file is loaded).
///
/// Selection and hover state reference entities and topology ids that no
/// longer exist once the model is replaced, so both are cleared here as
/// well to avoid stale highlights or panics on lookup. Any pending contact
/// candidates are cleared too, since their [`fem_core::FaceId`]s and mesh
/// indices are only meaningful for the model they were computed from.
pub(crate) fn respawn_visuals_on_reload(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    version: Res<fem_core::FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    visual_query: Query<Entity, With<FemMeshVisual>>,
    hovered_query: Query<Entity, With<Hovered>>,
    selected_query: Query<Entity, With<Selected>>,
    mut hover: ResMut<HoverResult>,
    mut selection: ResMut<SelectionState>,
    mut contact_candidates: ResMut<ContactCandidateState>,
    analysis_setup: Res<fem_core::AnalysisSetup>,
) {
    let current = version.value;

    if *last_version == Some(current) {
        return;
    }

    let first_run = last_version.is_none();
    *last_version = Some(current);

    if first_run {
        // The initial spawn is handled by `spawn_demo_mesh` at Startup.
        return;
    }

    let Some(model) = model else {
        return;
    };

    for entity in &visual_query {
        commands.entity(entity).despawn();
    }

    for entity in &hovered_query {
        commands.entity(entity).remove::<Hovered>();
    }

    for entity in &selected_query {
        commands.entity(entity).remove::<Selected>();
    }

    hover.clear();
    selection.clear();
    contact_candidates.candidates.clear();
    contact_candidates.selected = None;

    spawn_model_visuals(
        &mut commands,
        &mut meshes,
        &mut materials,
        &model,
        Some(&analysis_setup),
    );
}

/// Rebuilds element visuals when section assignments change outside of a
/// mesh reload, so shell/beam elements switch to their shape-specific
/// rendering once thickness/profile data becomes available.
///
/// Boundary conditions, loads, materials, and solver settings do not change
/// element geometry. Ignoring those changes avoids rebuilding a large
/// aggregate surface after every `.cnt` load.
pub(crate) fn respawn_elements_on_setup_change(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    setup: Res<fem_core::AnalysisSetup>,
    version: Res<fem_core::FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    mut last_sections: Local<Option<Vec<fem_core::Section>>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    visual_query: Query<Entity, With<FemMeshVisual>>,
) {
    let version_changed = *last_version != Some(version.value);
    *last_version = Some(version.value);

    if !setup.is_changed() {
        return;
    }

    let sections_changed = last_sections
        .as_deref()
        .is_some_and(|previous| previous != setup.sections.as_slice());
    *last_sections = Some(setup.sections.clone());

    if setup.is_added() || version_changed || !sections_changed {
        return;
    }

    let Some(model) = model else {
        return;
    };

    for entity in &visual_query {
        commands.entity(entity).despawn();
    }

    spawn_model_visuals(
        &mut commands,
        &mut meshes,
        &mut materials,
        &model,
        Some(&setup),
    );
}

/// Rebuilds the aggregate surface mesh with rainbow vertex colours whenever
/// [`FemResultSet`] or [`VisualizationSettings::contour`] changes.
///
/// When `settings.contour` is `None` the surface reverts to the plain
/// shaded material built by [`spawn_aggregate_surface_visual`].
pub(crate) fn update_contour_surface(
    model: Option<Res<FemModel>>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surface_query: Query<
        (&mut Mesh3d, &mut MeshMaterial3d<StandardMaterial>),
        (With<FemMeshVisual>, With<VisualLayer>),
    >,
) {
    if !results.is_changed() && !settings.is_changed() {
        return;
    }

    let Some(contour) = &settings.contour else {
        return;
    };

    let Some(model) = model.as_deref() else {
        return;
    };

    let Some(fem_mesh) = model.meshes.get(contour.mesh_index) else {
        return;
    };

    let Some(step) = results
        .by_mesh
        .get(contour.mesh_index)
        .and_then(|steps| steps.get(contour.step_index))
    else {
        return;
    };

    let Some(new_mesh) = build_contour_surface_mesh(fem_mesh, step, contour) else {
        return;
    };

    let unlit_material = materials.add(StandardMaterial {
        unlit: true,
        cull_mode: None,
        ..default()
    });

    for (mut mesh, mut material) in &mut surface_query {
        mesh.0 = meshes.add(new_mesh.clone());
        material.0 = unlit_material.clone();
    }
}

fn hide_topology_highlights(
    query: &mut Query<
        (
            &TopologyHighlight,
            &mut Mesh3d,
            &mut Transform,
            &mut Visibility,
        ),
        Without<VisualLayer>,
    >,
) {
    for (_, _, _, mut visibility) in query.iter_mut() {
        *visibility = Visibility::Hidden;
    }
}

/// Rebuilds the master/slave overlays for either the selected automatic
/// candidate or the selected contact pair already defined in the model.
/// Candidate review takes precedence. Defined NODE-SURF pairs render slave
/// nodes as orange markers; SURF-SURF pairs render both sides as surfaces.
pub(crate) fn update_contact_candidate_highlights(
    model: Option<Res<FemModel>>,
    state: Res<ContactCandidateState>,
    pose: Res<ContactReviewPose>,
    defined: Res<DefinedContactPreview>,
    draft: Res<ContactDraftPreview>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut query: Query<(
        &ContactCandidateHighlight,
        &mut Mesh3d,
        &mut Transform,
        &mut Visibility,
        &mut ContactHighlightAvailability,
    )>,
) {
    let rebuild = state.is_changed()
        || defined.is_changed()
        || draft.is_changed()
        || model.as_ref().is_some_and(|model| model.is_changed());
    let candidate = pose.active.then(|| state.selected_candidate()).flatten();
    let draft_active = candidate.is_none() && draft.active;
    let defined_contact = if candidate.is_none() && !draft_active && defined.active {
        model
            .as_deref()
            .and_then(|model| defined.selected.and_then(|index| model.contacts.get(index)))
    } else {
        None
    };
    let source_active = candidate.is_some() || draft_active || defined_contact.is_some();

    for (highlight, mut mesh, mut transform, mut visibility, mut availability) in &mut query {
        if rebuild {
            let built = if let Some(candidate) = candidate {
                model.as_deref().and_then(|model| {
                    let (mesh_index, face_ids) = match highlight {
                        ContactCandidateHighlight::Master => (candidate.mesh_a, &candidate.faces_a),
                        ContactCandidateHighlight::Slave => (candidate.mesh_b, &candidate.faces_b),
                    };
                    let fem_mesh = model.meshes.get(mesh_index)?;
                    build_highlight_faces_mesh(fem_mesh, face_ids)
                })
            } else if draft_active {
                model
                    .as_deref()
                    .and_then(|model| build_draft_contact_highlight(model, &draft, *highlight))
            } else {
                model.as_deref().and_then(|model| {
                    build_defined_contact_highlight(model, defined_contact?, *highlight)
                })
            };

            let Some(built) = built else {
                availability.0 = false;
                *visibility = Visibility::Hidden;
                continue;
            };

            mesh.0 = meshes.add(built);
            availability.0 = true;
        }

        transform.translation = match (candidate.is_some(), highlight) {
            (true, ContactCandidateHighlight::Master) => pose.offset_a,
            (true, ContactCandidateHighlight::Slave) => pose.offset_b,
            (false, _) => Vec3::ZERO,
        };
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
        *visibility = if source_active && availability.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn build_draft_contact_highlight(
    model: &FemModel,
    draft: &ContactDraftPreview,
    highlight: ContactCandidateHighlight,
) -> Option<Mesh> {
    match highlight {
        ContactCandidateHighlight::Master => {
            build_draft_surface_highlight_mesh(model, draft.master.as_ref()?)
        }
        ContactCandidateHighlight::Slave => match draft.slave.as_ref()? {
            ContactDraftSlave::Nodes { mesh_index, nodes } => build_highlight_nodes_mesh(
                model.meshes.get(*mesh_index)?,
                nodes,
                model_visual_scale(model) * 0.006,
            ),
            ContactDraftSlave::Surface(surface) => {
                build_draft_surface_highlight_mesh(model, surface)
            }
        },
    }
}

fn build_draft_surface_highlight_mesh(
    model: &FemModel,
    surface: &ContactDraftSurface,
) -> Option<Mesh> {
    let fem_mesh = model.meshes.get(surface.mesh_index)?;
    let element_faces: BTreeSet<_> = surface.surfaces.iter().copied().collect();
    let face_ids: Vec<_> = fem_mesh
        .cached_boundary_faces()
        .iter()
        .filter(|face| {
            face.element_face_ref()
                .is_some_and(|reference| element_faces.contains(&reference))
        })
        .map(|face| face.id)
        .collect();

    build_highlight_faces_mesh(fem_mesh, &face_ids)
}

fn build_defined_contact_highlight(
    model: &FemModel,
    contact: &ContactPair,
    highlight: ContactCandidateHighlight,
) -> Option<Mesh> {
    match highlight {
        ContactCandidateHighlight::Master => {
            build_surface_set_highlight_mesh(model, contact.master)
        }
        ContactCandidateHighlight::Slave => match contact.slave {
            ContactSlaveRef::Surface(reference) => {
                build_surface_set_highlight_mesh(model, reference)
            }
            ContactSlaveRef::Nodes(reference) => {
                let fem_mesh = model.meshes.get(reference.mesh_index)?;
                let node_set = fem_mesh.node_sets.get(reference.node_set_index)?;
                build_highlight_nodes_mesh(
                    fem_mesh,
                    &node_set.nodes,
                    model_visual_scale(model) * 0.006,
                )
            }
        },
    }
}

fn build_surface_set_highlight_mesh(model: &FemModel, reference: SurfaceSetRef) -> Option<Mesh> {
    let fem_mesh = model.meshes.get(reference.mesh_index)?;
    let surface_set = fem_mesh.surface_sets.get(reference.surface_set_index)?;
    let element_faces: BTreeSet<_> = surface_set.surfaces.iter().copied().collect();
    let face_ids: Vec<_> = fem_mesh
        .cached_boundary_faces()
        .iter()
        .filter(|face| {
            face.element_face_ref()
                .is_some_and(|reference| element_faces.contains(&reference))
        })
        .map(|face| face.id)
        .collect();

    build_highlight_faces_mesh(fem_mesh, &face_ids)
}

fn build_highlight_nodes_mesh(
    fem_mesh: &FemMesh,
    node_ids: &[NodeId],
    radius: f32,
) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let stride = node_ids
        .len()
        .div_ceil(MAX_DEFINED_CONTACT_NODE_MARKERS)
        .max(1);

    for node_id in node_ids.iter().step_by(stride) {
        let Some(center) = fem_mesh.node_position(*node_id) else {
            continue;
        };
        append_octahedron(&mut positions, &mut normals, center, radius);
    }

    (!positions.is_empty()).then(|| {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    })
}

fn append_octahedron(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    center: Vec3,
    radius: f32,
) {
    let top = center + Vec3::Y * radius;
    let bottom = center - Vec3::Y * radius;
    let ring = [
        center + Vec3::X * radius,
        center + Vec3::Z * radius,
        center - Vec3::X * radius,
        center - Vec3::Z * radius,
    ];

    for index in 0..ring.len() {
        let next = (index + 1) % ring.len();
        append_face_triangles(positions, normals, &[top, ring[index], ring[next]]);
        append_face_triangles(positions, normals, &[bottom, ring[next], ring[index]]);
    }
}

/// Renders `targets` as one highlight overlay.
///
/// Edge and Face/Element groups are merged into one overlay so multi-click
/// growth shows the complete group while preserving its topology kind.
/// Node selection still highlights only the most recent target.
fn apply_topology_highlight(
    model: &FemModel,
    targets: &[FemEntityRef],
    meshes: &mut Assets<Mesh>,
    mesh: &mut Mesh3d,
    transform: &mut Transform,
    visibility: &mut Visibility,
) -> Option<()> {
    let scale = model_visual_scale(model);

    let last = *targets.last()?;
    let fem_mesh = model.meshes.get(last.mesh_index)?;

    match last.entity {
        FemEntityId::Node(id) => {
            let position = fem_mesh.node_position(id)?;
            mesh.0 = meshes.add(Cuboid::new(scale * 0.012, scale * 0.012, scale * 0.012));
            *transform = Transform::from_translation(position);
        }
        FemEntityId::Edge(_) => {
            let edge_targets = targets
                .iter()
                .copied()
                .filter(|target| matches!(target.entity, FemEntityId::Edge(_)));
            mesh.0 = meshes.add(build_multi_edge_highlight_mesh(model, edge_targets, scale)?);
            *transform = Transform::default();
        }
        FemEntityId::Face(_) | FemEntityId::Element(_) => {
            let face_targets = targets
                .iter()
                .copied()
                .filter(|t| matches!(t.entity, FemEntityId::Face(_) | FemEntityId::Element(_)));

            mesh.0 = meshes.add(build_multi_face_highlight_mesh(model, face_targets)?);
            *transform = Transform::default();
        }
    }

    *visibility = Visibility::Visible;

    Some(())
}

fn build_multi_edge_highlight_mesh(
    model: &FemModel,
    targets: impl Iterator<Item = FemEntityRef>,
    model_scale: f32,
) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();

    for target in targets {
        let FemEntityId::Edge(edge_id) = target.entity else {
            continue;
        };
        let Some(fem_mesh) = model.meshes.get(target.mesh_index) else {
            continue;
        };
        let Some(edge) = fem_mesh
            .cached_boundary_edges()
            .iter()
            .find(|edge| edge.id == edge_id)
        else {
            continue;
        };
        let (Some(start), Some(end)) = (
            fem_mesh.node_position(edge.nodes[0]),
            fem_mesh.node_position(edge.nodes[1]),
        ) else {
            continue;
        };
        let length = start.distance(end);
        if length <= f32::EPSILON {
            continue;
        }

        // Keep short mesh edges legible without letting their marker become
        // wider than the surrounding finite elements.
        let thickness = (length * 0.08).min(model_scale * 0.010);
        append_edge_prism(&mut positions, &mut normals, start, end, thickness * 0.5);
    }

    (!positions.is_empty()).then(|| {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    })
}

fn append_edge_prism(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    start: Vec3,
    end: Vec3,
    half_width: f32,
) {
    let direction = (end - start).normalize();
    let helper = if direction.dot(Vec3::Y).abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let side = direction.cross(helper).normalize() * half_width;
    let up = direction.cross(side).normalize() * half_width;

    let s00 = start - side - up;
    let s10 = start + side - up;
    let s11 = start + side + up;
    let s01 = start - side + up;
    let e00 = end - side - up;
    let e10 = end + side - up;
    let e11 = end + side + up;
    let e01 = end - side + up;

    for face in [
        [s00, s01, s11, s10],
        [e00, e10, e11, e01],
        [s00, s10, e10, e00],
        [s10, s11, e11, e10],
        [s11, s01, e01, e11],
        [s01, s00, e00, e01],
    ] {
        append_face_triangles(positions, normals, &face);
    }
}

/// Builds one merged highlight mesh from `targets`. A `Face` target
/// contributes only that boundary face; an
/// `Element` target contributes every geometric face of the FEM element,
/// making surface selection and whole-element selection visually distinct.
/// The mesh is rendered exactly coincident with the true geometry — no
/// vertex offset.
///
/// An earlier version of this nudged each triangle outward along its own
/// normal to avoid z-fighting with the base mesh. That works fine for a
/// single small, roughly front-facing face, but once a whole coplanar
/// group is merged into one mesh (which is the point of this function),
/// that group typically wraps around a curved surface far enough to
/// include faces seen nearly edge-on — a cylindrical bore's silhouette
/// rim, say. There, even a tiny offset along the *local* normal shifts
/// the *screen-space* position by several pixels (the more grazing the
/// angle, the more a small out-of-plane nudge reads as a large in-plane
/// one), so the highlight's outline visibly saw-tooths away from the
/// model's actual silhouette. `depth_bias` on the material (see
/// `spawn_topology_highlights`) solves the z-fighting this offset existed
/// for without moving any vertices, so there's no longer a reason to pay
/// that cost.
fn build_multi_face_highlight_mesh(
    model: &FemModel,
    targets: impl Iterator<Item = FemEntityRef>,
) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();

    for target in targets {
        append_target_highlight_triangles(model, target, &mut positions, &mut normals);
    }

    (!positions.is_empty()).then(|| {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    })
}

fn append_target_highlight_triangles(
    model: &FemModel,
    target: FemEntityRef,
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
) {
    let Some(fem_mesh) = model.meshes.get(target.mesh_index) else {
        return;
    };

    match target.entity {
        FemEntityId::Face(id) => {
            let Some(face) = fem_mesh
                .cached_boundary_faces()
                .iter()
                .find(|face| face.id == id)
            else {
                return;
            };
            let Some(points) = fem_mesh.node_positions(&face.nodes) else {
                return;
            };

            append_face_triangles(positions, normals, &points);
        }
        FemEntityId::Element(id) => {
            let Some(element) = fem_mesh.elements.iter().find(|element| element.id == id) else {
                return;
            };

            for face_nodes in element.face_node_ids() {
                let Some(points) = fem_mesh.node_positions(&face_nodes) else {
                    continue;
                };
                append_face_triangles(positions, normals, &points);
            }
        }
        FemEntityId::Node(_) | FemEntityId::Edge(_) => {}
    }
}

/// Builds a single mesh covering every boundary face in `face_ids`,
/// rendered exactly coincident with the true surface — no vertex offset;
/// see [`build_multi_face_highlight_mesh`]'s doc comment for why (the
/// contact master/slave surfaces this is used for can be just as curved as
/// a coplanar-selected surface, so the same silhouette-drift problem would
/// apply).
///
/// Used to preview a [`fem_core::ContactCandidate`]'s master/slave surface
/// as one overlay — the contact-candidate analogue of
/// [`build_multi_face_highlight_mesh`] (which covers the topology hover/
/// selected overlays instead, and resolves faces from [`FemEntityId`]
/// targets rather than a flat [`FaceId`] list already scoped to one mesh).
/// Faces that no longer exist in `fem_mesh` (e.g. a stale candidate after a
/// reload) are silently skipped.
fn build_highlight_faces_mesh(fem_mesh: &FemMesh, face_ids: &[FaceId]) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();

    for face_id in face_ids {
        let Some(face) = fem_mesh
            .cached_boundary_faces()
            .iter()
            .find(|face| face.id == *face_id)
        else {
            continue;
        };

        let Some(points) = fem_mesh.node_positions(&face.nodes) else {
            continue;
        };

        append_face_triangles(&mut positions, &mut normals, &points);
    }

    (!positions.is_empty()).then(|| {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    })
}

pub(crate) fn model_visual_scale(model: &FemModel) -> f32 {
    model
        .bounds()
        .map(|(min, max)| (max - min).length().max(1.0))
        .unwrap_or(1.0)
}

/// Dispatches to a shape-appropriate spawn function based on
/// `element.element_type`: a thin extruded plate for plane/shell elements,
/// cylinders along the geometric segments of line/beam elements, and the
/// actual corner-face geometry for solids and interface elements. Unknown
/// element types retain the bounding-box fallback.
///
/// `section` is the [`fem_core::Section`] resolved for this specific
/// element (via [`fem_core::AnalysisSetup::build_element_section_map`]),
/// providing shell thickness / beam cross-section area when available. A
/// `None` section falls back to a size derived from the element's own
/// bounding box, so shells/beams still render reasonably (just without the
/// solver's exact thickness/area) on a mesh with no `.cnt` loaded yet.
fn spawn_element_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_index: usize,
    fem_mesh: &FemMesh,
    element: &FemElement,
    materials: &MaterialSet,
    section: Option<&fem_core::Section>,
    model_scale: f32,
) {
    if element.element_type.is_shell() {
        spawn_shell_element_visual(
            commands,
            meshes,
            mesh_index,
            fem_mesh,
            element,
            materials,
            section,
            model_scale,
        );
    } else if element.element_type.is_beam() {
        spawn_beam_element_visual(
            commands,
            meshes,
            mesh_index,
            fem_mesh,
            element,
            materials,
            section,
            model_scale,
        );
    } else {
        spawn_solid_element_visual(commands, meshes, mesh_index, fem_mesh, element, materials);
    }
}

/// Renders a 3-D solid or interface element from its actual corner faces.
/// A bounding-box cuboid is retained only as a fallback for unknown or
/// malformed element types with no usable face topology.
fn spawn_solid_element_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_index: usize,
    fem_mesh: &FemMesh,
    element: &FemElement,
    materials: &MaterialSet,
) {
    let (mesh, transform) = match build_element_surface_mesh(fem_mesh, element) {
        Some(mesh) => (meshes.add(mesh), Transform::default()),
        None => {
            let Some(points) = fem_mesh.node_positions(&element.nodes) else {
                return;
            };
            let Some((min, max)) = bounds(&points) else {
                return;
            };

            let center = (min + max) * 0.5;
            let size = visual_size(max - min);
            (
                meshes.add(Cuboid::new(size.x, size.y, size.z)),
                Transform::from_translation(center),
            )
        }
    };

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(materials.normal.clone()),
        transform,
        VisualLayer::Shaded,
        Visibility::Visible,
        Selectable::element(mesh_index, element.id),
        ElementEntity::new(element.id),
        NormalMaterial(materials.normal.clone()),
        FlatMaterial(materials.flat.clone()),
        TransparentMaterial(materials.transparent.clone()),
        HoverMaterial(materials.hover.clone()),
        SelectedMaterial(materials.selected.clone()),
        FemPartVisual { mesh_index },
        FemMeshVisual,
        Name::new(format!("Element {}", element.id.0)),
    ));
}

fn build_element_surface_mesh(fem_mesh: &FemMesh, element: &FemElement) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();

    for node_ids in element.face_node_ids() {
        let Some(points) = fem_mesh.node_positions(&node_ids) else {
            continue;
        };

        append_face_triangles(&mut positions, &mut normals, &points);
    }

    if positions.is_empty() {
        return None;
    }

    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals),
    )
}

/// Number of *corner* nodes for a shell element type, ignoring mid-side
/// nodes on quadratic elements (Tri6's nodes 4-6, Quad8's nodes 5-8) which
/// follow this corners-then-midsides ordering convention in every format
/// this platform parses (Gmsh, HECMW, Abaqus/CalculiX `.inp`).
fn shell_corner_count(element_type: &fem_core::ElementType) -> usize {
    element_type.surface_corner_count().unwrap_or(0)
}

/// Renders a shell element (Tri3/Tri6/Quad4/Quad8) as a thin plate: the
/// element's corner polygon extruded by `±thickness/2` along its face
/// normal, so the element's actual flat-panel shape is visible instead of
/// being hidden inside a cuboid bounding box.
///
/// Falls back to 2% of the element's own planar size when no [`Section`]
/// (or no `Shell` section) is available, so the element is still
/// recognizably thin rather than defaulting to solid-cuboid proportions.
fn spawn_shell_element_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_index: usize,
    fem_mesh: &FemMesh,
    element: &FemElement,
    materials: &MaterialSet,
    section: Option<&fem_core::Section>,
    model_scale: f32,
) {
    let corner_count = shell_corner_count(&element.element_type);
    let Some(all_points) = fem_mesh.node_positions(&element.nodes) else {
        return;
    };

    if all_points.len() < corner_count || corner_count < 3 {
        return;
    }

    let corners = &all_points[..corner_count];
    let Some(normal) = face_normal(corners) else {
        return;
    };

    let thickness = match section.map(|s| &s.kind) {
        Some(fem_core::SectionKind::Shell { thickness }) => *thickness,
        _ => {
            let Some((min, max)) = bounds(corners) else {
                return;
            };
            let element_size = (max - min).length();

            // Clamp against the *model's* scale, not just the element's
            // own bounding box: a degenerate or unexpectedly large element
            // (corrupt data, a unit mismatch, etc.) would otherwise produce
            // an equally-oversized "thin" plate that's anything but thin.
            // The element-relative term keeps normal meshes looking right;
            // the model-relative ceiling only ever kicks in for outliers.
            (element_size * 0.02).min(model_scale * 0.01)
        }
    }
    .max(1.0e-4);

    let half = thickness * 0.5;
    let top: Vec<Vec3> = corners.iter().map(|&p| p + normal * half).collect();
    let bottom: Vec<Vec3> = corners.iter().map(|&p| p - normal * half).collect();

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();

    // Top and bottom faces.
    append_face_triangles(&mut positions, &mut normals, &top);
    let mut bottom_rev = bottom.clone();
    bottom_rev.reverse();
    append_face_triangles(&mut positions, &mut normals, &bottom_rev);

    // Side walls: one quad (as two triangles) per edge of the polygon.
    // Winding order [bottom[i], bottom[j], top[j], top[i]] gives an
    // outward-facing normal for a corner polygon wound CCW as seen from
    // `normal`'s direction (verified by hand for a unit-square case before
    // committing to it — the seemingly-equivalent [top[i], top[j],
    // bottom[j], bottom[i]] order actually produces inward-facing normals).
    for i in 0..corner_count {
        let j = (i + 1) % corner_count;
        let quad = [bottom[i], bottom[j], top[j], top[i]];

        append_face_triangles(&mut positions, &mut normals, &quad);
    }

    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals);

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.normal.clone()),
        Transform::default(),
        VisualLayer::Shaded,
        Visibility::Visible,
        Selectable::element(mesh_index, element.id),
        ElementEntity::new(element.id),
        NormalMaterial(materials.normal.clone()),
        FlatMaterial(materials.flat.clone()),
        TransparentMaterial(materials.transparent.clone()),
        HoverMaterial(materials.hover.clone()),
        SelectedMaterial(materials.selected.clone()),
        FemPartVisual { mesh_index },
        FemMeshVisual,
        Name::new(format!("Shell element {}", element.id.0)),
    ));
}

/// Renders a line/beam/truss/connector element as cylinders along its
/// geometric segments. Quadratic line elements therefore include their
/// mid-side node, while mixed-DOF elements ignore rotation-only nodes.
///
/// radius derived from the assigned [`Section`]'s cross-sectional area
/// (`r = sqrt(area / π)`, i.e. an equivalent-area circular cross-section —
/// detailed beam profile shapes are out of scope, see
/// [`fem_core::SectionKind::Beam`]'s doc comment).
///
/// Falls back to 1.5% of the element's own length when no [`Section`] (or
/// no `Beam` section) is available, so the element still reads as "thin
/// and long" rather than the cuboid default.
fn spawn_beam_element_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_index: usize,
    fem_mesh: &FemMesh,
    element: &FemElement,
    materials: &MaterialSet,
    section: Option<&fem_core::Section>,
    model_scale: f32,
) {
    let segments: Vec<(Vec3, Vec3)> = element
        .edge_node_ids()
        .into_iter()
        .filter_map(|nodes| {
            Some((
                fem_mesh.node_position(nodes[0])?,
                fem_mesh.node_position(nodes[1])?,
            ))
        })
        .filter(|(start, end)| start.distance_squared(*end) > f32::EPSILON * f32::EPSILON)
        .collect();

    if segments.is_empty() {
        return;
    }

    let reference_length = segments
        .iter()
        .map(|(start, end)| start.distance(*end))
        .sum::<f32>();

    let radius = match section.map(|s| &s.kind) {
        Some(fem_core::SectionKind::Beam { area }) => (area / std::f32::consts::PI).sqrt(),
        // Same model-scale safety ceiling as the shell thickness fallback
        // above — see its comment for why a purely element-relative value
        // is risky for outlier/degenerate elements.
        _ => (reference_length * 0.015).min(model_scale * 0.01),
    }
    .max(1.0e-4);

    for (segment_index, (start, end)) in segments.into_iter().enumerate() {
        let delta = end - start;
        let length = delta.length();
        let center = (start + end) * 0.5;
        let rotation = Quat::from_rotation_arc(Vec3::Y, delta / length);

        commands.spawn((
            Mesh3d(meshes.add(Cylinder {
                radius,
                half_height: length * 0.5,
            })),
            MeshMaterial3d(materials.normal.clone()),
            Transform {
                translation: center,
                rotation,
                ..default()
            },
            VisualLayer::Shaded,
            Visibility::Visible,
            Selectable::element(mesh_index, element.id),
            ElementEntity::new(element.id),
            NormalMaterial(materials.normal.clone()),
            FlatMaterial(materials.flat.clone()),
            TransparentMaterial(materials.transparent.clone()),
            HoverMaterial(materials.hover.clone()),
            SelectedMaterial(materials.selected.clone()),
            FemPartVisual { mesh_index },
            FemMeshVisual,
            Name::new(format!(
                "Line element {} segment {}",
                element.id.0,
                segment_index + 1
            )),
        ));
    }
}

fn spawn_face_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_index: usize,
    fem_mesh: &FemMesh,
    face: &FemFace,
    materials: &MaterialSet,
) {
    let Some(points) = fem_mesh.node_positions(&face.nodes) else {
        return;
    };
    let Some(mesh) = build_extruded_polygon_mesh(&points, FACE_THICKNESS) else {
        return;
    };

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.normal.clone()),
        Transform::default(),
        VisualLayer::Shaded,
        Visibility::Visible,
        Selectable::face(mesh_index, face.id),
        FaceEntity::new(face.id),
        NormalMaterial(materials.normal.clone()),
        FlatMaterial(materials.flat.clone()),
        TransparentMaterial(materials.transparent.clone()),
        HoverMaterial(materials.hover.clone()),
        SelectedMaterial(materials.selected.clone()),
        FemPartVisual { mesh_index },
        FemMeshVisual,
        Name::new(format!("Face {}", face.id.0)),
    ));
}

/// Builds a thin prism that follows the exact face polygon. The previous
/// face visual used an oriented bounding cuboid, so triangular faces showed
/// the unused corners of that rectangle outside the element.
fn build_extruded_polygon_mesh(points: &[Vec3], thickness: f32) -> Option<Mesh> {
    if points.len() < 3 {
        return None;
    }

    let normal = face_normal(points)?;
    let half = thickness.max(f32::EPSILON) * 0.5;
    let top: Vec<Vec3> = points.iter().map(|point| *point + normal * half).collect();
    let bottom: Vec<Vec3> = points.iter().map(|point| *point - normal * half).collect();
    let mut positions = Vec::new();
    let mut normals = Vec::new();

    append_face_triangles(&mut positions, &mut normals, &top);

    let mut bottom_reversed = bottom.clone();
    bottom_reversed.reverse();
    append_face_triangles(&mut positions, &mut normals, &bottom_reversed);

    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        let wall = [bottom[index], bottom[next], top[next], top[index]];
        append_face_triangles(&mut positions, &mut normals, &wall);
    }

    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals),
    )
}

fn spawn_edge_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_index: usize,
    fem_mesh: &FemMesh,
    edge: &FemEdge,
    materials: &MaterialSet,
) {
    let Some(start) = fem_mesh.node_position(edge.nodes[0]) else {
        return;
    };
    let Some(end) = fem_mesh.node_position(edge.nodes[1]) else {
        return;
    };

    let delta = end - start;
    let length = delta.length();

    if length <= f32::EPSILON {
        return;
    }

    let direction = delta / length;
    let center = (start + end) * 0.5;
    let rotation = Quat::from_rotation_arc(Vec3::X, direction);

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(length, EDGE_THICKNESS, EDGE_THICKNESS))),
        MeshMaterial3d(materials.normal.clone()),
        Transform {
            translation: center,
            rotation,
            ..default()
        },
        VisualLayer::Edge,
        Visibility::Visible,
        Selectable::edge(mesh_index, edge.id),
        EdgeEntity::new(edge.id),
        NormalMaterial(materials.normal.clone()),
        FlatMaterial(materials.flat.clone()),
        TransparentMaterial(materials.transparent.clone()),
        HoverMaterial(materials.hover.clone()),
        SelectedMaterial(materials.selected.clone()),
        FemPartVisual { mesh_index },
        FemMeshVisual,
        Name::new(format!("Edge {}", edge.id.0)),
    ));
}

fn spawn_node_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh_index: usize,
    node: &FemNode,
    materials: &MaterialSet,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(NODE_SIZE, NODE_SIZE, NODE_SIZE))),
        MeshMaterial3d(materials.normal.clone()),
        Transform::from_translation(node.position),
        VisualLayer::Node,
        Visibility::Visible,
        Selectable::node(mesh_index, node.id),
        NodeEntity::new(node.id),
        NormalMaterial(materials.normal.clone()),
        FlatMaterial(materials.flat.clone()),
        TransparentMaterial(materials.transparent.clone()),
        HoverMaterial(materials.hover.clone()),
        SelectedMaterial(materials.selected.clone()),
        FemPartVisual { mesh_index },
        FemMeshVisual,
        Name::new(format!("Node {}", node.id.0)),
    ));
}

/// Builds one merged triangle mesh for a part's exterior surface. Assembly
/// tools reuse this geometry for whole-part hover and selected overlays.
pub fn build_part_surface_mesh(fem_mesh: &FemMesh) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();

    for face in fem_mesh.cached_boundary_faces() {
        let Some(points) = fem_mesh.node_positions(&face.nodes) else {
            continue;
        };

        append_face_triangles(&mut positions, &mut normals, &points);
    }

    if positions.is_empty() {
        return None;
    }

    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals),
    )
}

/// Like [`build_part_surface_mesh`] but colours each vertex according to
/// the supplied `contour` field (rainbow palette) and optionally offsets
/// node positions by a displacement field.
///
/// When deformation is enabled, each vertex is moved by
/// `displacement[node_index] × deformation_scale` before triangulation so
/// the deformed shape is rendered without a separate mesh.
pub(crate) fn build_contour_surface_mesh(
    fem_mesh: &FemMesh,
    step: &fem_core::StepResult,
    settings: &ContourSettings,
) -> Option<Mesh> {
    let contour_field = step.field_by_name(&settings.field_name)?;

    let disp_field = if settings.show_deformation {
        step.field_by_name(&settings.displacement_field)
    } else {
        None
    };

    let node_index_map: std::collections::HashMap<fem_core::NodeId, usize> = fem_mesh
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();

    for face in fem_mesh.cached_boundary_faces() {
        let Some(node_indices_in_mesh): Option<Vec<usize>> = face
            .nodes
            .iter()
            .map(|id| node_index_map.get(id).copied())
            .collect()
        else {
            continue;
        };

        let points: Vec<Vec3> = node_indices_in_mesh
            .iter()
            .filter_map(|&idx| fem_mesh.nodes.get(idx))
            .map(|node| {
                if let (Some(disp), true) = (&disp_field, settings.show_deformation) {
                    if let fem_core::ResultField::NodeVector { values, .. } = disp {
                        if let Some(&disp_vec) =
                            values.get(*node_index_map.get(&node.id).unwrap_or(&usize::MAX))
                        {
                            return node.position + disp_vec * settings.deformation_scale;
                        }
                    }
                }
                node.position
            })
            .collect();

        if points.len() < 3 {
            continue;
        }

        let Some(normal) = face_normal(&points) else {
            continue;
        };

        let vert_colors: Vec<[f32; 4]> = node_indices_in_mesh
            .iter()
            .map(|&mesh_idx| {
                let t = match contour_field {
                    fem_core::ResultField::NodeScalar { .. } => {
                        contour_field.normalize_node_scalar(mesh_idx)
                    }
                    fem_core::ResultField::NodeVector { .. } => {
                        contour_field.normalize_node_vector_mag(mesh_idx)
                    }
                    _ => 0.5,
                };

                let c = rainbow_color(t);
                [c.red, c.green, c.blue, c.alpha]
            })
            .collect();

        // Fan-triangulate.
        for idx in 1..(points.len() - 1) {
            let tri = [points[0], points[idx], points[idx + 1]];
            let col = [vert_colors[0], vert_colors[idx], vert_colors[idx + 1]];

            for (p, c) in tri.iter().zip(col.iter()) {
                positions.push(p.to_array());
                normals.push(normal.to_array());
                colors.push(*c);
            }
        }
    }

    if positions.is_empty() {
        return None;
    }

    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors),
    )
}

/// Builds a merged line mesh for a part's boundary and beam edges.
pub fn build_part_edge_mesh(fem_mesh: &FemMesh) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut seen = BTreeSet::new();

    for edge in fem_mesh.cached_boundary_edges() {
        seen.insert(ordered_node_pair(edge.nodes));

        let Some(start) = fem_mesh.node_position(edge.nodes[0]) else {
            continue;
        };
        let Some(end) = fem_mesh.node_position(edge.nodes[1]) else {
            continue;
        };

        positions.push(start.to_array());
        positions.push(end.to_array());
        normals.push(Vec3::Y.to_array());
        normals.push(Vec3::Y.to_array());
    }

    // Line-like elements do not contribute faces, so they never appear in
    // `cached_boundary_edges`. Add them explicitly, including in meshes
    // that also contain solids/shells.
    for element in &fem_mesh.elements {
        if !element.element_type.is_beam() {
            continue;
        }

        for nodes in element.edge_node_ids() {
            if !seen.insert(ordered_node_pair(nodes)) {
                continue;
            }

            let Some(start) = fem_mesh.node_position(nodes[0]) else {
                continue;
            };
            let Some(end) = fem_mesh.node_position(nodes[1]) else {
                continue;
            };

            positions.push(start.to_array());
            positions.push(end.to_array());
            normals.push(Vec3::Y.to_array());
            normals.push(Vec3::Y.to_array());
        }
    }

    if positions.is_empty() {
        return None;
    }

    Some(
        Mesh::new(
            PrimitiveTopology::LineList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals),
    )
}

fn ordered_node_pair(nodes: [fem_core::NodeId; 2]) -> (fem_core::NodeId, fem_core::NodeId) {
    if nodes[0] <= nodes[1] {
        (nodes[0], nodes[1])
    } else {
        (nodes[1], nodes[0])
    }
}

fn append_face_triangles(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    points: &[Vec3],
) {
    if points.len() < 3 {
        return;
    }

    let Some(normal) = face_normal(points) else {
        return;
    };

    for index in 1..(points.len() - 1) {
        push_triangle(
            positions,
            normals,
            [points[0], points[index], points[index + 1]],
            normal,
        );
    }
}

fn push_triangle(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    triangle: [Vec3; 3],
    normal: Vec3,
) {
    let normal = normal.to_array();

    for point in triangle {
        positions.push(point.to_array());
        normals.push(normal);
    }
}

fn material_set(
    materials: &mut Assets<StandardMaterial>,
    normal: Color,
    hover: Color,
    selected: Color,
    flat: Color,
    blend: bool,
) -> MaterialSet {
    let mut flat_material = standard_material(flat, blend);
    flat_material.unlit = true;

    // Transparent material: same hue as `normal` but always alpha-blended
    // with a low, fixed opacity, regardless of whether the base material
    // already blends (element/face fills already blend at a much higher
    // alpha for normal viewing, so we can't just reuse `normal` as-is).
    // `with_alpha` works across every `Color` variant, unlike matching on
    // `Color::Srgba` directly.
    let mut transparent_material = standard_material(normal.with_alpha(0.18), true);
    transparent_material.cull_mode = None;
    transparent_material.double_sided = true;

    MaterialSet {
        normal: materials.add(standard_material(normal, blend)),
        hover: materials.add(standard_material(hover, blend)),
        // Selection is always opaque, even when the resting face/element
        // material is blended. This keeps selected geometry in the depth-
        // writing render pass and prevents rear geometry showing through.
        selected: materials.add(selection_material(selected)),
        flat: materials.add(flat_material),
        transparent: materials.add(transparent_material),
    }
}

fn selection_material(color: Color) -> StandardMaterial {
    standard_material(color.with_alpha(1.0), false)
}

fn standard_material(color: Color, blend: bool) -> StandardMaterial {
    let mut material = StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.78,
        ..default()
    };

    if blend {
        material.alpha_mode = AlphaMode::Blend;
    }

    material
}

fn bounds(points: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let mut iter = points.iter();
    let first = *iter.next()?;
    let mut min = first;
    let mut max = first;

    for point in iter {
        min = min.min(*point);
        max = max.max(*point);
    }

    Some((min, max))
}

fn visual_size(size: Vec3) -> Vec3 {
    Vec3::new(
        size.x.max(MIN_VISUAL_SIZE),
        size.y.max(MIN_VISUAL_SIZE),
        size.z.max(MIN_VISUAL_SIZE),
    )
}

fn face_normal(points: &[Vec3]) -> Option<Vec3> {
    let origin = points[0];

    for i in 1..points.len() {
        let edge_a = points[i] - origin;

        for j in (i + 1)..points.len() {
            if let Some(normal) = edge_a.cross(points[j] - origin).try_normalize() {
                return Some(normal);
            }
        }
    }

    None
}
#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;
    use fem_core::{
        ElementId, ElementType, FemElement, FemMesh, FemNode, FemSurfaceSet, NodeId, SurfaceSetRef,
    };

    use super::*;

    #[test]
    fn selection_material_is_opaque_and_writes_depth() {
        let material = selection_material(Color::srgba(0.10, 1.0, 0.45, 0.25));

        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
        assert_eq!(material.base_color.to_srgba().alpha, 1.0);
    }

    #[test]
    fn contact_review_separates_parts_symmetrically_without_editing_nodes() {
        let mut model = FemModel::demo_hex8();
        let original_positions: Vec<Vec3> = model.meshes[0]
            .nodes
            .iter()
            .map(|node| node.position)
            .collect();
        let mut second = FemMesh::demo_hex8();
        for node in &mut second.nodes {
            node.position += Vec3::X * 3.0;
        }
        model.add_mesh("Second", second);

        let candidate = ContactCandidate {
            mesh_a: 0,
            mesh_b: 1,
            faces_a: Vec::new(),
            faces_b: Vec::new(),
            pair_count: 1,
            average_gap: 0.0,
        };
        let (offset_a, offset_b) = contact_review_offsets(&model, &candidate, 10.0);

        assert!(offset_a.x < 0.0);
        assert!(offset_b.x > 0.0);
        assert!((offset_a + offset_b).length() < 1.0e-6);
        assert_eq!(
            model.meshes[0]
                .nodes
                .iter()
                .map(|node| node.position)
                .collect::<Vec<_>>(),
            original_positions
        );
    }

    #[test]
    fn self_contact_review_does_not_explode_one_part() {
        let model = FemModel::demo_hex8();
        let candidate = ContactCandidate {
            mesh_a: 0,
            mesh_b: 0,
            faces_a: Vec::new(),
            faces_b: Vec::new(),
            pair_count: 1,
            average_gap: 0.0,
        };

        assert_eq!(
            contact_review_offsets(&model, &candidate, 30.0),
            (Vec3::ZERO, Vec3::ZERO)
        );
    }

    #[test]
    fn element_highlight_contains_the_whole_element_not_one_boundary_face() {
        let model = FemModel::demo_hex8();
        let face_id = model.meshes[0].cached_boundary_faces()[0].id;

        let face =
            build_multi_face_highlight_mesh(&model, [FemEntityRef::face(0, face_id)].into_iter())
                .unwrap();
        let element = build_multi_face_highlight_mesh(
            &model,
            [FemEntityRef::element(0, ElementId(0))].into_iter(),
        )
        .unwrap();

        assert_eq!(face.count_vertices(), 6);
        assert_eq!(element.count_vertices(), 36);
    }

    #[test]
    fn multi_edge_highlight_contains_only_the_requested_edges() {
        let model = FemModel::demo_hex8();
        let edges = model.meshes[0].cached_boundary_edges();
        let rendered = build_multi_edge_highlight_mesh(
            &model,
            [
                FemEntityRef::edge(0, edges[0].id),
                FemEntityRef::edge(0, edges[1].id),
            ]
            .into_iter(),
            model_visual_scale(&model),
        )
        .unwrap();

        assert_eq!(
            rendered.primitive_topology(),
            PrimitiveTopology::TriangleList
        );
        assert_eq!(rendered.count_vertices(), 72);
    }

    #[test]
    fn defined_surface_contact_highlight_uses_only_the_surface_set_faces() {
        let mut model = FemModel::demo_hex8();
        let surface = model.meshes[0].cached_boundary_faces()[0]
            .element_face_ref()
            .unwrap();
        model.meshes[0].surface_sets.push(FemSurfaceSet {
            name: "MASTER".to_string(),
            surfaces: vec![surface],
        });

        let rendered = build_surface_set_highlight_mesh(&model, SurfaceSetRef::new(0, 0)).unwrap();

        assert_eq!(rendered.count_vertices(), 6);
    }

    #[test]
    fn node_surface_slave_highlight_draws_one_marker_per_node() {
        let model = FemModel::demo_hex8();
        let rendered =
            build_highlight_nodes_mesh(&model.meshes[0], &[NodeId(0), NodeId(1)], 0.01).unwrap();

        assert_eq!(rendered.count_vertices(), 48);
    }

    #[test]
    fn builds_actual_tetrahedron_surface_instead_of_a_bounding_box() {
        let mesh = FemMesh::new(
            vec![
                FemNode::from_xyz(NodeId(1), 0.0, 0.0, 0.0),
                FemNode::from_xyz(NodeId(2), 1.0, 0.0, 0.0),
                FemNode::from_xyz(NodeId(3), 0.0, 1.0, 0.0),
                FemNode::from_xyz(NodeId(4), 0.0, 0.0, 1.0),
            ],
            vec![FemElement::new(
                ElementId(1),
                ElementType::Tet4,
                vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4)],
            )],
        );

        let rendered = build_element_surface_mesh(&mesh, &mesh.elements[0]).unwrap();

        assert_eq!(rendered.count_vertices(), 12);
    }

    #[test]
    fn aggregate_edge_mesh_keeps_line_only_models_visible() {
        let mesh = FemMesh::new(
            vec![
                FemNode::from_xyz(NodeId(1), 0.0, 0.0, 0.0),
                FemNode::from_xyz(NodeId(2), 2.0, 0.0, 0.0),
                FemNode::from_xyz(NodeId(3), 1.0, 1.0, 0.0),
            ],
            vec![FemElement::new(
                ElementId(1),
                ElementType::Rod3,
                vec![NodeId(1), NodeId(2), NodeId(3)],
            )],
        );

        assert!(mesh.cached_boundary_edges().is_empty());

        let rendered = build_part_edge_mesh(&mesh).unwrap();

        assert_eq!(rendered.count_vertices(), 4);
    }

    #[test]
    fn triangular_face_visual_does_not_include_bounding_rectangle_corners() {
        let rendered = build_extruded_polygon_mesh(
            &[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            0.01,
        )
        .unwrap();
        let Some(VertexAttributeValues::Float32x3(positions)) =
            rendered.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("face mesh is missing Float32x3 positions");
        };

        assert!(positions.iter().all(|position| {
            position[0] >= 0.0 && position[1] >= 0.0 && position[0] + position[1] <= 1.0
        }));
    }
}
