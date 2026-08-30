//! Boundary conditions, loads, materials, and section properties.
//!
//! This is the data side of CLAUDE.md's "人間とsolverの間" (between human
//! and solver) mission applied to analysis setup: a prepost tool isn't
//! useful for real models until it can show what constraints and loads are
//! already defined on a mesh (loaded from a FrontISTR `.cnt` file or
//! similar), even before it can author them from scratch.
//!
//! These types intentionally mirror the FrontISTR/Abaqus `.cnt`/`.inp`
//! keyword vocabulary (`!BOUNDARY`, `!CLOAD`, `!MATERIAL`, `!SECTION`)
//! closely enough that a parser can build them with minimal translation,
//! while staying solver-neutral in the same spirit as the rest of
//! `fem_core`.

use bevy::prelude::*;

use crate::{ElementFaceRef, ElementId, NodeId};

/// One term of a FrontISTR `!EQUATION` multi-point constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct MpcTerm {
    /// Mesh/part owning `node`; node IDs are scoped to a part until export.
    pub mesh_index: usize,

    pub node: NodeId,

    /// Structural degree of freedom (`1..=6`).
    pub dof: u8,

    pub coefficient: f32,
}

impl MpcTerm {
    pub fn new(mesh_index: usize, node: NodeId, dof: u8, coefficient: f32) -> Self {
        Self {
            mesh_index,
            node,
            dof,
            coefficient,
        }
    }
}

/// A linear multi-point constraint exported through the official HEC-MW
/// mesh keyword `!EQUATION`.
#[derive(Debug, Clone, PartialEq)]
pub struct MpcEquation {
    /// Human-readable identifier retained by bevyistr. HEC-MW equations do
    /// not have names, so this is emitted only as a comment when possible.
    pub name: String,

    pub constant: f32,

    pub terms: Vec<MpcTerm>,

    /// Optional operation-level group. One reviewed rigid spider or one
    /// compact source equation can expand into many explicit HEC-MW
    /// equations; retaining that relationship lets the UI manage the whole
    /// constraint without accidentally deleting only one DOF.
    pub group: Option<String>,
}

impl MpcEquation {
    pub fn new(name: impl Into<String>, constant: f32, terms: Vec<MpcTerm>) -> Self {
        Self {
            name: name.into(),
            constant,
            terms,
            group: None,
        }
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn is_valid(&self) -> bool {
        self.terms.len() >= 2
            && self
                .terms
                .iter()
                .all(|term| (1..=6).contains(&term.dof) && term.coefficient.is_finite())
            && self.constant.is_finite()
    }
}

/// FrontISTR `ROT_CENTER` reference used by `!BOUNDARY` and `!CLOAD`.
///
/// The center is independent from the constrained/loaded target nodes.  A
/// source file may name either one node directly or a node group; keeping the
/// optional group name preserves compact round-trip output, while `node`
/// provides a resolved point for viewport feedback when one is available.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RotationCenter {
    pub mesh_index: usize,
    pub node: Option<NodeId>,
    pub ngrp_name: Option<String>,
}

impl RotationCenter {
    pub fn from_node(mesh_index: usize, node: NodeId) -> Self {
        Self {
            mesh_index,
            node: Some(node),
            ngrp_name: None,
        }
    }

    pub fn from_group(
        mesh_index: usize,
        name: impl Into<String>,
        resolved_node: Option<NodeId>,
    ) -> Self {
        Self {
            mesh_index,
            node: resolved_node,
            ngrp_name: Some(name.into()),
        }
    }
}

// ─── boundary conditions (displacement / rotation constraints) ──────────────

/// A displacement (or rotation) constraint applied to a degree-of-freedom
/// range on a set of nodes — the data behind FrontISTR's `!BOUNDARY` /
/// Abaqus's `*BOUNDARY` keyword.
///
/// `dof_start`/`dof_end` use the standard FEM numbering: `1=Ux, 2=Uy,
/// 3=Uz, 4=Rx, 5=Ry, 6=Rz`. A single-DOF constraint has `dof_start ==
/// dof_end`.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryCondition {
    pub name: String,

    /// Index into [`crate::FemModel::meshes`] the constrained nodes belong to.
    pub mesh_index: usize,

    pub nodes: Vec<NodeId>,

    /// When the source `.cnt` file referenced a node-group name (NGRP) rather
    /// than individual node IDs, this preserves the original name so that
    /// round-trip export can write `GRPNAME, dof_start, dof_end, value`
    /// instead of enumerating every node ID individually.  `None` when the
    /// boundary condition was created directly from a node selection in the UI.
    pub ngrp_name: Option<String>,

    /// Center used by FrontISTR's `ROT_CENTER` rotational-displacement form.
    /// In that form data-line DOFs 1..3 are rotations about global X/Y/Z;
    /// this is distinct from direct shell/beam rotational DOFs 4..6.
    pub rotation_center: Option<RotationCenter>,

    pub dof_start: u8,

    pub dof_end: u8,

    /// Prescribed value (commonly `0.0` for a fixed constraint).
    pub value: f32,
}

impl BoundaryCondition {
    /// `true` if this constrains a translational DOF (1, 2, or 3) within
    /// its `[dof_start, dof_end]` range.
    pub fn constrains_translation(&self) -> bool {
        self.rotation_center.is_none() && self.dof_start <= 3 && self.dof_end >= 1
    }

    /// `true` if this constrains a rotational DOF (4, 5, or 6) within its
    /// `[dof_start, dof_end]` range — only meaningful for shell/beam nodes.
    pub fn constrains_rotation(&self) -> bool {
        self.rotation_center.is_some() || (self.dof_start <= 6 && self.dof_end >= 4)
    }

    /// Short human-readable summary of the constrained DOF range, e.g.
    /// `"Ux-Uz"`, `"Uy"`, `"Rx-Rz"`.
    pub fn dof_label(&self) -> String {
        const DIRECT_NAMES: [&str; 6] = ["Ux", "Uy", "Uz", "Rx", "Ry", "Rz"];
        const CENTER_NAMES: [&str; 3] = ["Rx", "Ry", "Rz"];
        let names: &[&str] = if self.rotation_center.is_some() {
            &CENTER_NAMES
        } else {
            &DIRECT_NAMES
        };

        let start_label = names
            .get(self.dof_start.saturating_sub(1) as usize)
            .copied()
            .unwrap_or("?");

        if self.dof_start == self.dof_end {
            start_label.to_string()
        } else {
            let end_label = names
                .get(self.dof_end.saturating_sub(1) as usize)
                .copied()
                .unwrap_or("?");

            format!("{start_label}-{end_label}")
        }
    }
}

// ─── loads ───────────────────────────────────────────────────────────────────

/// A concentrated nodal load — FrontISTR's `!CLOAD` / Abaqus's `*CLOAD`.
#[derive(Debug, Clone, PartialEq)]
pub struct NodalLoad {
    pub name: String,

    pub mesh_index: usize,

    pub node: NodeId,

    /// When the source `.cnt` used a NGRP name for the load application
    /// point, this preserves it for round-trip export (same rationale as
    /// [`BoundaryCondition::ngrp_name`]).
    pub ngrp_name: Option<String>,

    /// Center used by FrontISTR's `ROT_CENTER` torque form. When present,
    /// data-line DOFs 1..3 describe torque components about global X/Y/Z;
    /// without it DOFs 1..3 are ordinary concentrated forces.
    pub rotation_center: Option<RotationCenter>,

    /// DOF the load acts on: `1=Fx, 2=Fy, 3=Fz` (4-6 for moments).
    pub dof: u8,

    pub value: f32,
}

impl NodalLoad {
    pub fn dof_label(&self) -> &'static str {
        if self.rotation_center.is_some() {
            match self.dof {
                1 => "Mx",
                2 => "My",
                3 => "Mz",
                _ => "?",
            }
        } else {
            match self.dof {
                1 => "Fx",
                2 => "Fy",
                3 => "Fz",
                4 => "Mx",
                5 => "My",
                6 => "Mz",
                _ => "?",
            }
        }
    }
}

/// A distributed load on element faces — FrontISTR's `!DLOAD` / Abaqus's
/// `*DLOAD`, most commonly surface pressure.
#[derive(Debug, Clone, PartialEq)]
pub struct DistributedLoad {
    pub name: String,

    pub mesh_index: usize,

    /// What the load acts on. Pressure loads need a specific element face
    /// (FrontISTR's `P1`.._`P6`_ local-face codes — different elements in a
    /// picked surface can have the picked face at a different local index,
    /// e.g. face 3 on one hex and face 5 on its neighbour); gravity/body
    /// force loads act on whole elements and have no face to speak of. See
    /// [`DistributedLoadTarget`].
    pub target: DistributedLoadTarget,

    pub kind: DistributedLoadKind,

    /// Pressure magnitude (positive = acting inward along the face normal,
    /// matching the FrontISTR/Abaqus `P` convention) or traction
    /// magnitude, depending on `kind`.
    pub value: f32,

    /// Direction of a gravity load as a direction cosine. `None` for
    /// pressure loads. Older inputs that omit the four required `GRAV`
    /// parameters are interpreted as acting in the global -Y direction.
    pub direction: Option<Vec3>,
}

/// What a [`DistributedLoad`] is applied to.
///
/// FrontISTR's `!DLOAD` keyword uses two different reference styles
/// depending on the load type: a surface pressure (`P1`..`P6`) names one
/// specific local face per element, while a body force (`GRAV`, `BX`/`BY`/
/// `BZ`, `CENT`) just names the element (or element group) it acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistributedLoadTarget {
    /// Whole elements — used for body-force loads ([`DistributedLoadKind::Gravity`]).
    Elements(Vec<ElementId>),

    /// Specific element faces — used for surface pressure
    /// ([`DistributedLoadKind::Pressure`]), so the exported `.cnt` can emit
    /// the correct `P<n>` code per element rather than guessing.
    Faces(Vec<ElementFaceRef>),
}

impl DistributedLoadTarget {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Elements(v) => v.is_empty(),
            Self::Faces(v) => v.is_empty(),
        }
    }

    /// Number of elements (for `Elements`) or faces (for `Faces`) targeted.
    /// Used for UI summaries where "how many things does this load touch"
    /// matters more than which variant it is.
    pub fn len(&self) -> usize {
        match self {
            Self::Elements(v) => v.len(),
            Self::Faces(v) => v.len(),
        }
    }

    /// The distinct elements referenced, regardless of variant — every
    /// face in `Faces` belongs to some element, so this always has a
    /// sensible answer. Used for remapping IDs into assembly-wide numbering
    /// and for element-set based rendering.
    pub fn element_ids(&self) -> Vec<ElementId> {
        match self {
            Self::Elements(v) => v.clone(),
            Self::Faces(v) => v.iter().map(|f| f.element).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributedLoadKind {
    /// Uniform pressure normal to the face.
    Pressure,

    /// Gravity / body force along a fixed direction (`value` is
    /// acceleration magnitude and [`DistributedLoad::direction`] stores
    /// the direction cosine).
    Gravity,
}

// ─── materials ───────────────────────────────────────────────────────────────

/// Linear-elastic material properties — FrontISTR's `!MATERIAL` +
/// `!ELASTIC` + `!DENSITY` block, flattened into one struct since prepost
/// only needs to display/validate these, not drive a solver.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FemMaterial {
    pub name: String,

    pub young_modulus: Option<f32>,

    pub poisson_ratio: Option<f32>,

    /// Mass density (solver units, typically t/mm³ or kg/m³ depending on
    /// the model's unit system — `fem_core` does not assume a system).
    pub density: Option<f32>,
}

impl FemMaterial {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }
}

// ─── sections (element properties: shell thickness, beam profile, etc.) ─────

/// Section/property assignment — FrontISTR's `!SECTION` keyword, which
/// binds a [`FemMaterial`] (and, for shells/beams, a thickness or profile) to
/// an element set.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub name: String,

    pub mesh_index: usize,

    pub material_name: String,

    /// Name of the [`crate::FemElementSet`] this section applies to, if
    /// the source file specified one (FrontISTR sections are usually
    /// scoped to an `EGRP`). `None` means "applies to the whole mesh".
    pub element_set_name: Option<String>,

    pub kind: SectionKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionKind {
    /// Solid elements — no extra geometric property beyond the mesh itself.
    Solid,

    /// Shell elements — uniform thickness.
    Shell { thickness: f32 },

    /// Beam elements — cross-sectional area (used for axial stiffness and,
    /// in [`visualization`](../../visualization), to scale the beam's
    /// rendered radius). Detailed section shape (I-beam, channel, etc.) is
    /// out of scope for now; `area` alone is enough for a representative
    /// circular-tube rendering.
    Beam { area: f32 },
}

// ─── collected resource ──────────────────────────────────────────────────────

/// All analysis-setup data (boundary conditions, loads, materials,
/// sections) loaded for the current model, independent of mesh geometry.
///
/// Kept as a separate [`Resource`] rather than fields on
/// [`crate::FemModel`] for the same reason as [`crate::FemResultSet`]: a
/// mesh can be loaded (and re-loaded) without this data, and replacing it
/// shouldn't require rebuilding topology caches.

/// Solver settings that control how FrontISTR runs the analysis.
/// Corresponds to the `!SOLUTION`, `!STEP`, `!STATIC`, `!DYNAMIC`, and
/// `!SOLVER` keywords in the `.cnt` file.
#[derive(Debug, Clone, PartialEq)]
pub struct SolverSettings {
    /// Analysis type: static, dynamic, or eigenvalue.
    pub analysis_type: AnalysisType,
    /// Number of substeps (for nonlinear or time-dependent analyses).
    pub substeps: u32,
    /// Maximum Newton–Raphson iterations per substep.
    pub max_iterations: u32,
    /// Convergence tolerance (relative residual norm).
    pub convergence_tol: f32,
    /// Linear solver method.
    pub solver_method: LinearSolverMethod,
}

impl Default for SolverSettings {
    fn default() -> Self {
        Self {
            analysis_type: AnalysisType::Static,
            substeps: 1,
            max_iterations: 100,
            convergence_tol: 1.0e-6,
            solver_method: LinearSolverMethod::Cg,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisType {
    Static,
    NlStatic,
    Dynamic,
    Eigen,
}

impl AnalysisType {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::NlStatic => "Nonlinear",
            Self::Dynamic => "Dynamic",
            Self::Eigen => "Eigenvalue",
        }
    }
    /// FrontISTR `!SOLUTION,TYPE=` keyword value.
    pub const fn frontistr_type(self) -> &'static str {
        match self {
            Self::Static => "STATIC",
            Self::NlStatic => "NLSTATIC",
            Self::Dynamic => "DYNAMIC",
            Self::Eigen => "EIGEN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearSolverMethod {
    Cg,
    Direct,
    Gmres,
}

impl LinearSolverMethod {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cg => "CG",
            Self::Direct => "Direct",
            Self::Gmres => "GMRES",
        }
    }
    pub const fn frontistr_method(self) -> &'static str {
        match self {
            Self::Cg => "CG",
            Self::Direct => "DIRECT",
            Self::Gmres => "GMRES",
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct AnalysisSetup {
    pub boundary_conditions: Vec<BoundaryCondition>,

    pub nodal_loads: Vec<NodalLoad>,

    pub distributed_loads: Vec<DistributedLoad>,

    pub materials: Vec<FemMaterial>,

    pub sections: Vec<Section>,

    /// General multi-point constraints stored in mesh-local coordinates and
    /// exported as HEC-MW `!EQUATION` entries.
    pub mpc_equations: Vec<MpcEquation>,

    /// Solver settings for this analysis. Written as `!SOLUTION`, `!STEP`,
    /// `!SOLVER` etc. in the exported `.cnt` file.
    pub solver: SolverSettings,
}

impl Default for AnalysisSetup {
    fn default() -> Self {
        Self {
            boundary_conditions: Vec::new(),
            nodal_loads: Vec::new(),
            distributed_loads: Vec::new(),
            materials: Vec::new(),
            sections: Vec::new(),
            mpc_equations: Vec::new(),
            solver: SolverSettings::default(),
        }
    }
}

impl AnalysisSetup {
    pub fn is_empty(&self) -> bool {
        self.boundary_conditions.is_empty()
            && self.nodal_loads.is_empty()
            && self.distributed_loads.is_empty()
            && self.materials.is_empty()
            && self.sections.is_empty()
            && self.mpc_equations.is_empty()
    }

    pub fn clear(&mut self) {
        self.boundary_conditions.clear();
        self.nodal_loads.clear();
        self.distributed_loads.clear();
        self.materials.clear();
        self.sections.clear();
        self.mpc_equations.clear();
    }

    pub fn material_by_name(&self, name: &str) -> Option<&FemMaterial> {
        self.materials.iter().find(|m| m.name == name)
    }

    /// Adds a new [`BoundaryCondition`] constraining `nodes` over
    /// `[dof_start, dof_end]` to `value`, auto-naming it `BC1`, `BC2`, …
    /// (skipping any name already in use, so re-loading a `.cnt` with its
    /// own `BC1` doesn't collide).
    ///
    /// This is the data-model half of the "select nodes, pick DOFs, click
    /// Apply" authoring workflow — see `ui`'s constraint-creation panel for
    /// the UI half. Returns the index of the new entry in
    /// [`AnalysisSetup::boundary_conditions`].
    pub fn add_constraint(
        &mut self,
        mesh_index: usize,
        nodes: Vec<NodeId>,
        dof_start: u8,
        dof_end: u8,
        value: f32,
    ) -> usize {
        let name = self.next_auto_name("BC");

        self.boundary_conditions.push(BoundaryCondition {
            name,
            mesh_index,
            nodes,
            ngrp_name: None,
            rotation_center: None,
            dof_start,
            dof_end,
            value,
        });

        self.boundary_conditions.len() - 1
    }

    /// Adds a new [`NodalLoad`] for each node in `nodes`, all sharing one
    /// auto-generated name (`LOAD1`, `LOAD2`, …) and acting on `dof` with
    /// magnitude `value`.
    ///
    /// FrontISTR's `!CLOAD` block is inherently one-load-per-node (unlike
    /// `!BOUNDARY`, which accepts a node group directly), so a multi-node
    /// selection becomes multiple [`NodalLoad`] entries here — they share a
    /// name so the UI can still treat "the load just created" as one unit
    /// (e.g. for undoing or summarizing it).
    pub fn add_load(&mut self, mesh_index: usize, nodes: &[NodeId], dof: u8, value: f32) -> usize {
        let name = self.next_auto_name("LOAD");
        let start_index = self.nodal_loads.len();

        for &node in nodes {
            self.nodal_loads.push(NodalLoad {
                name: name.clone(),
                mesh_index,
                node,
                ngrp_name: None,
                rotation_center: None,
                dof,
                value,
            });
        }

        start_index
    }

    /// Adds a new [`FemMaterial`], auto-naming it `MAT1`, `MAT2`, … if
    /// `name` is empty. Returns the index of the new entry.
    pub fn add_material(
        &mut self,
        name: impl Into<String>,
        young_modulus: Option<f32>,
        poisson_ratio: Option<f32>,
        density: Option<f32>,
    ) -> usize {
        let name = name.into();
        let name = if name.is_empty() {
            self.next_auto_name("MAT")
        } else {
            name
        };

        self.materials.push(FemMaterial {
            name,
            young_modulus,
            poisson_ratio,
            density,
        });

        self.materials.len() - 1
    }

    /// Adds a new [`Section`], auto-naming it `SEC1`, `SEC2`, … . Returns
    /// the index of the new entry.
    pub fn add_section(
        &mut self,
        mesh_index: usize,
        material_name: impl Into<String>,
        element_set_name: Option<String>,
        kind: SectionKind,
    ) -> usize {
        let name = self.next_auto_name("SEC");

        self.sections.push(Section {
            name,
            mesh_index,
            material_name: material_name.into(),
            element_set_name,
            kind,
        });

        self.sections.len() - 1
    }

    /// Removes the boundary condition at `index`, if present. The UI uses
    /// this to let a person undo a constraint they created by mistake —
    /// `.cnt` import doesn't need it (a fresh load just calls
    /// [`AnalysisSetup::clear`] first).
    pub fn remove_boundary_condition(&mut self, index: usize) {
        if index < self.boundary_conditions.len() {
            self.boundary_conditions.remove(index);
        }
    }

    /// Removes every [`NodalLoad`] sharing the name of the entry at
    /// `index` — since [`AnalysisSetup::add_load`] splits one multi-node
    /// load into several same-named [`NodalLoad`] entries, removing "the
    /// load" means removing all of them together, not just the one at
    /// `index`.
    pub fn remove_load_group(&mut self, index: usize) {
        let Some(name) = self.nodal_loads.get(index).map(|l| l.name.clone()) else {
            return;
        };

        self.nodal_loads.retain(|l| l.name != name);
    }

    /// Removes the distributed load at `index`, if present.
    pub fn remove_distributed_load(&mut self, index: usize) {
        if index < self.distributed_loads.len() {
            self.distributed_loads.remove(index);
        }
    }

    pub fn remove_material(&mut self, index: usize) {
        if index < self.materials.len() {
            self.materials.remove(index);
        }
    }

    pub fn remove_section(&mut self, index: usize) {
        if index < self.sections.len() {
            self.sections.remove(index);
        }
    }

    /// Generates `"{prefix}{n}"` for the smallest `n >= 1` not already used
    /// as a name among boundary conditions, loads, or materials (whichever
    /// is relevant for `prefix`) — kept deliberately simple (a linear scan)
    /// since these lists are small (tens of entries, not thousands) even
    /// for a fairly involved hand-built analysis setup.
    pub fn next_auto_name_pub(&self, prefix: &str) -> String {
        self.next_auto_name(prefix)
    }

    fn next_auto_name(&self, prefix: &str) -> String {
        let mut n = 1u32;

        loop {
            let candidate = format!("{prefix}{n}");
            let in_use = self.boundary_conditions.iter().any(|b| b.name == candidate)
                || self.nodal_loads.iter().any(|l| l.name == candidate)
                || self.distributed_loads.iter().any(|d| d.name == candidate)
                || self.materials.iter().any(|m| m.name == candidate)
                || self.sections.iter().any(|s| s.name == candidate);

            if !in_use {
                return candidate;
            }

            n += 1;
        }
    }

    /// Sections that apply to `mesh_index`, in load order.
    pub fn sections_for_mesh(&self, mesh_index: usize) -> impl Iterator<Item = &Section> {
        self.sections
            .iter()
            .filter(move |s| s.mesh_index == mesh_index)
    }

    /// Resolves every [`Section`] for `mesh_index` against `mesh`'s
    /// `element_sets`, producing a per-element lookup.
    ///
    /// A section with `element_set_name: Some(name)` covers exactly the
    /// elements in that named [`crate::FemElementSet`]; a section with
    /// `element_set_name: None` is a "whole mesh" default that fills in
    /// any element not already covered by a more specific section. This
    /// mirrors how FrontISTR `!SECTION,EGRP=...` assignments behave: named
    /// groups take precedence, and at most one un-scoped section acts as
    /// the fallback.
    ///
    /// Built once per mesh (not once per element) since it's an O(sections
    /// + elements) pass — call it from a visualization-rebuild path, not
    /// per-element inside a render loop.
    pub fn build_element_section_map(
        &self,
        mesh_index: usize,
        mesh: &crate::FemMesh,
    ) -> std::collections::HashMap<ElementId, &Section> {
        let mut map = std::collections::HashMap::new();
        let mut whole_mesh_section: Option<&Section> = None;

        for section in self.sections_for_mesh(mesh_index) {
            match &section.element_set_name {
                Some(set_name) => {
                    if let Some(set) = mesh.element_sets.iter().find(|s| &s.name == set_name) {
                        for &element_id in &set.elements {
                            map.insert(element_id, section);
                        }
                    }
                }
                None => {
                    whole_mesh_section = Some(section);
                }
            }
        }

        if let Some(section) = whole_mesh_section {
            for element in &mesh.elements {
                map.entry(element.id).or_insert(section);
            }
        }

        map
    }
}
