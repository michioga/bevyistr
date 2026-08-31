//! Contact definition, automatic candidate detection, and review UI.

use crate::layout::SidebarPage;
use crate::measurement::{MeasurementBoxState, MeasurementTarget};
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::prelude::*;
use fem_core::{
    ContactCandidate, ContactCandidateState, ContactPair, ContactSlaveRef, ContactType,
    FemEntityId, FemModel, FemModelVersion, FemNodeSet, FemSurfaceSet, RigidSpiderCandidateState,
    SelectionFilter, SelectionLevel, ViewportTool,
};
use selection::{Selected, SelectionState};
use std::collections::{BTreeMap, BTreeSet};
use visualization::{
    ContactDraftPreview, ContactDraftSlave, ContactDraftSurface, ContactReviewSettings,
    DefinedContactPreview, DefinedMpcPreview, MpcPairDraftPreview,
};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const ACTIVE_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);
const CONTACT_MASTER_BUTTON: Color = Color::srgb(0.16, 0.34, 0.60);
const CONTACT_SLAVE_BUTTON: Color = Color::srgb(0.62, 0.31, 0.08);

#[derive(Component)]
pub(crate) struct CreateSurfaceButton;

#[derive(Component)]
pub(crate) struct CreateContactButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContactPairKind {
    #[default]
    NodeSurface,

    SurfaceSurface,
}

impl ContactPairKind {
    pub(crate) const ALL: [Self; 2] = [Self::NodeSurface, Self::SurfaceSurface];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::NodeSurface => "NODE-SURF",
            Self::SurfaceSurface => "SURF-SURF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContactParameter {
    #[default]
    Friction,

    PenaltyFactor,

    SearchGap,

    SearchAngle,

    SpiderRadius,
}

#[derive(Resource, Debug, Clone)]
pub(crate) struct ContactDefinitionSettings {
    pub pair_kind: ContactPairKind,

    pub contact_type: ContactType,

    pub use_penalty_factor: bool,

    pub active_parameter: ContactParameter,

    pub message: String,
}

impl Default for ContactDefinitionSettings {
    fn default() -> Self {
        Self {
            pair_kind: ContactPairKind::NodeSurface,
            contact_type: ContactType::SmallSliding,
            use_penalty_factor: false,
            active_parameter: ContactParameter::Friction,
            message: "Select slave nodes, then capture Slave".to_string(),
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ContactPairKindButton(pub ContactPairKind);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ContactBehaviorButton(pub ContactType);

#[derive(Component)]
pub(crate) struct ContactSlidingParameterControls;

#[derive(Component)]
pub(crate) struct ContactPenaltyControls;

#[derive(Component)]
pub(crate) struct ContactPenaltyToggleButton;

#[derive(Component)]
pub(crate) struct ContactPenaltyToggleLabel;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ContactParameterButton(pub ContactParameter);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactCaptureSide {
    Slave,

    Master,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct CaptureContactSideButton(pub ContactCaptureSide);

#[derive(Component)]
pub(crate) struct FinalizeContactButton;

#[derive(Component)]
pub(crate) struct ContactDraftStatusText;

#[derive(Component)]
pub(crate) struct DetectContactsButton;

#[derive(Component)]
pub(crate) struct AcceptContactButton;

#[derive(Component)]
pub(crate) struct AcceptContactLabel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactCandidateAction {
    Previous,

    Next,

    Reject,
}

#[derive(Component)]
pub(crate) struct ContactCandidateActionButton(pub ContactCandidateAction);

#[derive(Component)]
pub(crate) struct ContactGhostToggleButton;

#[derive(Component)]
pub(crate) struct ContactGhostToggleLabel;

#[derive(Component)]
pub(crate) struct ContactCandidateText;

#[derive(Component)]
pub(crate) struct ContactDefinitionsText;

#[derive(Component)]
pub(crate) struct ContactDefinitionsListContainer;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DefinedContactButton(pub usize);

#[derive(Component)]
pub(crate) struct ContactReviewControls;

pub(crate) fn create_surface_button_system(
    mut model: Option<ResMut<FemModel>>,
    selection: Res<SelectionState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<CreateSurfaceButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref_mut() {
                let surface_set_count: usize = model
                    .meshes
                    .iter()
                    .map(|mesh| mesh.surface_sets.len())
                    .sum();
                let name = format!("SURFACE_{}", surface_set_count + 1);

                model.create_surface_set_from_targets(name, &selection.targets);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn create_contact_button_system(
    mut model: Option<ResMut<FemModel>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<CreateContactButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref_mut() {
                let name = format!("CONTACT_{}", model.contacts.len() + 1);

                model.create_contact_pair_from_recent_surface_sets(name, ContactType::Tied);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn contact_pair_kind_button_system(
    mut commands: Commands,
    mut settings: ResMut<ContactDefinitionSettings>,
    mut draft: ResMut<ContactDraftPreview>,
    mut filter: ResMut<SelectionFilter>,
    mut selection: ResMut<SelectionState>,
    mut candidates: ResMut<ContactCandidateState>,
    selected_query: Query<Entity, With<Selected>>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ContactPairKindButton,
        ),
        With<ContactPairKindButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed
            && interaction.is_changed()
            && settings.pair_kind != button.0
        {
            settings.pair_kind = button.0;
            draft.slave = None;
            settings.message = match button.0 {
                ContactPairKind::NodeSurface => {
                    filter.level = SelectionLevel::Node;
                    "Select slave nodes, then capture Slave".to_string()
                }
                ContactPairKind::SurfaceSurface => {
                    filter.level = SelectionLevel::Face;
                    "Select the slave surface, then capture Slave".to_string()
                }
            };
            candidates.candidates.clear();
            candidates.selected = None;
            selection.clear();
            for entity in &selected_query {
                commands.entity(entity).remove::<Selected>();
            }
        }

        let active = settings.pair_kind == button.0;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn contact_behavior_button_system(
    mut settings: ResMut<ContactDefinitionSettings>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ContactBehaviorButton,
        ),
        With<ContactBehaviorButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.contact_type = button.0;
        }

        let active = settings.contact_type == button.0;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn contact_penalty_toggle_button_system(
    mut settings: ResMut<ContactDefinitionSettings>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ContactPenaltyToggleButton>,
    >,
    mut labels: Query<&mut Text, With<ContactPenaltyToggleLabel>>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.use_penalty_factor = !settings.use_penalty_factor;
            settings.active_parameter = if settings.use_penalty_factor {
                ContactParameter::PenaltyFactor
            } else {
                ContactParameter::Friction
            };
            let (slider_id, label, units, value) = match settings.active_parameter {
                ContactParameter::Friction => (
                    SliderId::ContactFriction,
                    "Friction coefficient",
                    "dimensionless",
                    slider_value(&sliders, SliderId::ContactFriction, 0.0),
                ),
                ContactParameter::PenaltyFactor => (
                    SliderId::ContactPenaltyFactor,
                    "Contact penalty factor",
                    "FrontISTR input value",
                    slider_value(&sliders, SliderId::ContactPenaltyFactor, 1.0e5),
                ),
                ContactParameter::SearchGap => (
                    SliderId::ContactSearchGap,
                    "Contact search gap",
                    "model units",
                    slider_value(&sliders, SliderId::ContactSearchGap, 0.05),
                ),
                ContactParameter::SearchAngle => (
                    SliderId::ContactSearchAngle,
                    "Contact normal tolerance",
                    "degrees",
                    slider_value(&sliders, SliderId::ContactSearchAngle, 20.0),
                ),
                ContactParameter::SpiderRadius => (
                    SliderId::RigidSpiderRadius,
                    "MPC spider search radius",
                    "model units",
                    slider_value(&sliders, SliderId::RigidSpiderRadius, 1.0),
                ),
            };
            measurement.begin_slider_value(slider_id, label, units, value);
        }

        let active = settings.use_penalty_factor;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }

    for mut label in &mut labels {
        **label = format!(
            "Penalty factor: {}",
            if settings.use_penalty_factor {
                "CUSTOM"
            } else {
                "AUTO"
            }
        );
    }
}

pub(crate) fn contact_parameter_button_system(
    mut settings: ResMut<ContactDefinitionSettings>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &ContactParameterButton,
        ),
        With<ContactParameterButton>,
    >,
) {
    for (interaction, mut background, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.active_parameter = button.0;
            let (slider_id, label, units, fallback) = match button.0 {
                ContactParameter::Friction => (
                    SliderId::ContactFriction,
                    "Friction coefficient",
                    "dimensionless",
                    0.0,
                ),
                ContactParameter::PenaltyFactor => (
                    SliderId::ContactPenaltyFactor,
                    "Contact penalty factor",
                    "FrontISTR input value",
                    1.0e5,
                ),
                ContactParameter::SearchGap => (
                    SliderId::ContactSearchGap,
                    "Contact search gap",
                    "model units",
                    0.05,
                ),
                ContactParameter::SearchAngle => (
                    SliderId::ContactSearchAngle,
                    "Contact normal tolerance",
                    "degrees",
                    20.0,
                ),
                ContactParameter::SpiderRadius => (
                    SliderId::RigidSpiderRadius,
                    "MPC spider search radius",
                    "model units",
                    1.0,
                ),
            };
            measurement.begin_slider_value(
                slider_id,
                label,
                units,
                slider_value(&sliders, slider_id, fallback),
            );
        }

        let active = settings.active_parameter == button.0;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
    }
}

pub(crate) fn update_contact_parameter_controls(
    settings: Res<ContactDefinitionSettings>,
    mut sliding: Query<
        &mut Node,
        (
            With<ContactSlidingParameterControls>,
            Without<ContactPenaltyControls>,
        ),
    >,
    mut penalty: Query<
        &mut Node,
        (
            With<ContactPenaltyControls>,
            Without<ContactSlidingParameterControls>,
        ),
    >,
) {
    if !settings.is_changed() {
        return;
    }
    let sliding_visible = settings.contact_type != ContactType::Tied;
    for mut node in &mut sliding {
        node.display = if sliding_visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut penalty {
        node.display = if sliding_visible && settings.use_penalty_factor {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub(crate) fn sync_contact_measurement_box(
    page: Res<SidebarPage>,
    tool: Res<ViewportTool>,
    settings: Res<ContactDefinitionSettings>,
    sliders: Query<Ref<SliderState>, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
) {
    if *page != SidebarPage::Contact || *tool != ViewportTool::Selection {
        return;
    }
    if matches!(
        measurement.target,
        Some(MeasurementTarget::MpcConstant { .. } | MeasurementTarget::MpcCoefficient { .. })
    ) {
        return;
    }
    if settings.contact_type == ContactType::Tied
        && matches!(
            settings.active_parameter,
            ContactParameter::Friction | ContactParameter::PenaltyFactor
        )
    {
        if matches!(
            measurement.target,
            Some(MeasurementTarget::SliderValue {
                slider_id: SliderId::ContactFriction | SliderId::ContactPenaltyFactor,
                ..
            })
        ) {
            measurement.clear();
        }
        return;
    }

    let parameter = if settings.active_parameter == ContactParameter::PenaltyFactor
        && !settings.use_penalty_factor
    {
        ContactParameter::Friction
    } else {
        settings.active_parameter
    };
    let (slider_id, label, units, fallback) = match parameter {
        ContactParameter::Friction => (
            SliderId::ContactFriction,
            "Friction coefficient",
            "dimensionless",
            0.0,
        ),
        ContactParameter::PenaltyFactor => (
            SliderId::ContactPenaltyFactor,
            "Contact penalty factor",
            "FrontISTR input value",
            1.0e5,
        ),
        ContactParameter::SearchGap => (
            SliderId::ContactSearchGap,
            "Contact search gap",
            "model units",
            0.05,
        ),
        ContactParameter::SearchAngle => (
            SliderId::ContactSearchAngle,
            "Contact normal tolerance",
            "degrees",
            20.0,
        ),
        ContactParameter::SpiderRadius => (
            SliderId::RigidSpiderRadius,
            "MPC spider search radius",
            "model units",
            1.0,
        ),
    };
    let slider = sliders.iter().find(|slider| slider.id == slider_id);
    let value = slider
        .as_ref()
        .map(|slider| slider.value)
        .unwrap_or(fallback);
    let target_matches = matches!(
        measurement.target,
        Some(MeasurementTarget::SliderValue {
            slider_id: target,
            ..
        }) if target == slider_id
    );
    if !target_matches {
        measurement.begin_slider_value(slider_id, label, units, value);
    } else if slider.is_some_and(|slider| slider.is_changed()) {
        measurement.update_slider_value(slider_id, value);
    }
}

fn contact_nodes_from_selection(
    selection: &SelectionState,
) -> Result<(usize, Vec<fem_core::NodeId>), String> {
    let mut groups = selected_nodes_by_mesh(selection).into_iter();
    let Some((mesh_index, nodes)) = groups.next() else {
        return Err("Slave requires selected nodes".to_string());
    };
    if groups.next().is_some() {
        return Err("Select slave nodes from one mesh only".to_string());
    }

    Ok((mesh_index, nodes))
}

fn contact_surface_from_selection(
    model: &FemModel,
    selection: &SelectionState,
) -> Result<ContactDraftSurface, String> {
    let mut by_mesh = BTreeMap::<usize, Vec<FemEntityId>>::new();
    for target in &selection.targets {
        if matches!(
            target.entity,
            FemEntityId::Face(_) | FemEntityId::Element(_)
        ) {
            by_mesh
                .entry(target.mesh_index)
                .or_default()
                .push(target.entity);
        }
    }

    let mut surfaces = by_mesh.into_iter().filter_map(|(mesh_index, targets)| {
        let surfaces = model
            .meshes
            .get(mesh_index)?
            .surface_refs_from_targets(&targets);
        (!surfaces.is_empty()).then_some(ContactDraftSurface {
            mesh_index,
            surfaces,
        })
    });
    let Some(surface) = surfaces.next() else {
        return Err("Select boundary faces or surface elements first".to_string());
    };
    if surfaces.next().is_some() {
        return Err("Capture one mesh surface at a time".to_string());
    }

    Ok(surface)
}

pub(crate) fn capture_contact_side_button_system(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    mut settings: ResMut<ContactDefinitionSettings>,
    mut draft: ResMut<ContactDraftPreview>,
    mut mpc_pair_draft: ResMut<MpcPairDraftPreview>,
    mut filter: ResMut<SelectionFilter>,
    mut selection: ResMut<SelectionState>,
    mut candidates: ResMut<ContactCandidateState>,
    selected_query: Query<Entity, With<Selected>>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &CaptureContactSideButton,
        ),
        With<CaptureContactSideButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let result = match (button.0, settings.pair_kind, model.as_deref()) {
                (ContactCaptureSide::Slave, ContactPairKind::NodeSurface, _) => {
                    contact_nodes_from_selection(&selection).map(|(mesh_index, nodes)| {
                        draft.slave = Some(ContactDraftSlave::Nodes { mesh_index, nodes });
                        filter.level = SelectionLevel::Face;
                        "Slave nodes captured; select the master surface".to_string()
                    })
                }
                (ContactCaptureSide::Slave, ContactPairKind::SurfaceSurface, Some(model)) => {
                    contact_surface_from_selection(model, &selection).map(|surface| {
                        draft.slave = Some(ContactDraftSlave::Surface(surface));
                        filter.level = SelectionLevel::Face;
                        "Slave surface captured; select the master surface".to_string()
                    })
                }
                (ContactCaptureSide::Master, _, Some(model)) => {
                    contact_surface_from_selection(model, &selection).map(|surface| {
                        draft.master = Some(surface);
                        "Master surface captured; create the contact pair".to_string()
                    })
                }
                _ => Err("No model is loaded".to_string()),
            };

            match result {
                Ok(message) => {
                    settings.message = message;
                    mpc_pair_draft.clear();
                    candidates.candidates.clear();
                    candidates.selected = None;
                    selection.clear();
                    for entity in &selected_query {
                        commands.entity(entity).remove::<Selected>();
                    }
                }
                Err(message) => settings.message = message,
            }
        }

        let captured = match button.0 {
            ContactCaptureSide::Slave => draft.slave.is_some(),
            ContactCaptureSide::Master => draft.master.is_some(),
        };
        let captured_color = match button.0 {
            ContactCaptureSide::Slave => CONTACT_SLAVE_BUTTON,
            ContactCaptureSide::Master => CONTACT_MASTER_BUTTON,
        };
        *background = BackgroundColor(match (*interaction, captured) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => captured_color,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if captured {
            ACTIVE_BORDER
        } else {
            PANEL_BORDER
        });
    }
}

pub(crate) fn create_contact_from_draft(
    model: &mut FemModel,
    draft: &ContactDraftPreview,
    pair_kind: ContactPairKind,
    contact_type: ContactType,
    friction_coefficient: f32,
    penalty_factor: Option<f32>,
) -> Result<usize, String> {
    let master = draft
        .master
        .as_ref()
        .ok_or_else(|| "Capture the master surface first".to_string())?;
    let slave = draft
        .slave
        .as_ref()
        .ok_or_else(|| "Capture the slave side first".to_string())?;
    let slave_matches_kind = matches!(
        (pair_kind, slave),
        (
            ContactPairKind::NodeSurface,
            ContactDraftSlave::Nodes { .. }
        ) | (
            ContactPairKind::SurfaceSurface,
            ContactDraftSlave::Surface(_)
        )
    );
    if !slave_matches_kind {
        return Err("Captured slave does not match the selected topology".to_string());
    }
    let (friction_coefficient, penalty_factor) =
        validated_contact_parameters(contact_type, friction_coefficient, penalty_factor)?;
    let pair_name = next_contact_name(model);

    let master_ref = {
        let mesh = model
            .meshes
            .get_mut(master.mesh_index)
            .ok_or_else(|| "Master mesh no longer exists".to_string())?;
        let index = mesh.surface_sets.len();
        mesh.surface_sets.push(FemSurfaceSet {
            name: format!("{pair_name}_MASTER"),
            surfaces: master.surfaces.clone(),
        });
        fem_core::SurfaceSetRef::new(master.mesh_index, index)
    };

    let contact = match (pair_kind, slave) {
        (ContactPairKind::NodeSurface, ContactDraftSlave::Nodes { mesh_index, nodes }) => {
            let mesh = model
                .meshes
                .get_mut(*mesh_index)
                .ok_or_else(|| "Slave mesh no longer exists".to_string())?;
            let index = mesh.node_sets.len();
            mesh.node_sets.push(FemNodeSet {
                name: format!("{pair_name}_SLAVE"),
                nodes: nodes.clone(),
            });
            ContactPair::new_node_surface(
                pair_name.clone(),
                master_ref,
                fem_core::NodeSetRef::new(*mesh_index, index),
                contact_type,
            )
        }
        (ContactPairKind::SurfaceSurface, ContactDraftSlave::Surface(surface)) => {
            let mesh = model
                .meshes
                .get_mut(surface.mesh_index)
                .ok_or_else(|| "Slave mesh no longer exists".to_string())?;
            let index = mesh.surface_sets.len();
            mesh.surface_sets.push(FemSurfaceSet {
                name: format!("{pair_name}_SLAVE"),
                surfaces: surface.surfaces.clone(),
            });
            ContactPair::new(
                pair_name.clone(),
                master_ref,
                fem_core::SurfaceSetRef::new(surface.mesh_index, index),
                contact_type,
            )
        }
        _ => return Err("Captured slave does not match the selected topology".to_string()),
    };

    model
        .contacts
        .push(contact.with_contact_parameters(friction_coefficient, penalty_factor));
    Ok(model.contacts.len() - 1)
}

pub(crate) fn create_contact_from_candidate(
    model: &mut FemModel,
    candidate: &ContactCandidate,
    contact_type: ContactType,
    friction_coefficient: f32,
    penalty_factor: Option<f32>,
) -> Result<usize, String> {
    let (friction_coefficient, penalty_factor) =
        validated_contact_parameters(contact_type, friction_coefficient, penalty_factor)?;
    if model.contact_candidate_is_defined(candidate) {
        return Err("This contact interface is already defined".to_string());
    }

    let name = next_contact_name(model);
    let index = model
        .accept_contact_candidate(candidate, name, contact_type)
        .ok_or_else(|| "The detected contact faces are no longer available".to_string())?;
    let contact = model
        .contacts
        .get_mut(index)
        .ok_or_else(|| "The accepted contact could not be updated".to_string())?;
    contact.friction_coefficient = friction_coefficient;
    contact.penalty_factor = penalty_factor;

    Ok(index)
}

fn validated_contact_parameters(
    contact_type: ContactType,
    friction_coefficient: f32,
    penalty_factor: Option<f32>,
) -> Result<(f32, Option<f32>), String> {
    match contact_type {
        ContactType::Tied => Ok((0.0, None)),
        ContactType::SmallSliding | ContactType::FiniteSliding => {
            if !friction_coefficient.is_finite() || friction_coefficient < 0.0 {
                return Err("Friction coefficient must be zero or greater".to_string());
            }
            if penalty_factor.is_some_and(|factor| !factor.is_finite() || factor <= 0.0) {
                return Err("Custom penalty factor must be greater than zero".to_string());
            }

            Ok((friction_coefficient, penalty_factor))
        }
    }
}

fn next_contact_name(model: &FemModel) -> String {
    let mut number = model.contacts.len() + 1;

    loop {
        let name = format!("CONTACT_{number}");

        if !model
            .contacts
            .iter()
            .any(|contact| contact.name.eq_ignore_ascii_case(&name))
        {
            return name;
        }

        number += 1;
    }
}

pub(crate) fn finalize_contact_button_system(
    mut model: Option<ResMut<FemModel>>,
    mut settings: ResMut<ContactDefinitionSettings>,
    mut draft: ResMut<ContactDraftPreview>,
    mut defined: ResMut<DefinedContactPreview>,
    mut candidates: ResMut<ContactCandidateState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<FinalizeContactButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let friction_coefficient = if settings.contact_type == ContactType::Tied {
                0.0
            } else {
                slider_value(&sliders, SliderId::ContactFriction, 0.0)
            };
            let penalty_factor = (settings.contact_type != ContactType::Tied
                && settings.use_penalty_factor)
                .then(|| slider_value(&sliders, SliderId::ContactPenaltyFactor, 1.0e5));
            let result = model
                .as_deref_mut()
                .ok_or_else(|| "No model is loaded".to_string())
                .and_then(|model| {
                    create_contact_from_draft(
                        model,
                        &draft,
                        settings.pair_kind,
                        settings.contact_type,
                        friction_coefficient,
                        penalty_factor,
                    )
                });
            match result {
                Ok(index) => {
                    let name = model
                        .as_deref()
                        .and_then(|model| model.contacts.get(index))
                        .map(|contact| contact.name.clone())
                        .unwrap_or_else(|| "contact".to_string());
                    draft.clear();
                    defined.selected = Some(index);
                    candidates.candidates.clear();
                    candidates.selected = None;
                    settings.message = format!("Created {name}");
                }
                Err(message) => settings.message = message,
            }
        }

        let ready = draft.master.is_some() && draft.slave.is_some();
        *background = BackgroundColor(match (*interaction, ready) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if ready { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn update_contact_draft_status(
    settings: Res<ContactDefinitionSettings>,
    draft: Res<ContactDraftPreview>,
    mut query: Query<&mut Text, With<ContactDraftStatusText>>,
) {
    if !settings.is_changed() && !draft.is_changed() {
        return;
    }
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let slave = match draft.slave.as_ref() {
        Some(ContactDraftSlave::Nodes { mesh_index, nodes }) => {
            format!("Slave: {} nodes (part {})", nodes.len(), mesh_index + 1)
        }
        Some(ContactDraftSlave::Surface(surface)) => format!(
            "Slave: {} faces (part {})",
            surface.surfaces.len(),
            surface.mesh_index + 1
        ),
        None => "Slave: not set".to_string(),
    };
    let master = draft.master.as_ref().map_or_else(
        || "Master: not set".to_string(),
        |surface| {
            format!(
                "Master: {} faces (part {})",
                surface.surfaces.len(),
                surface.mesh_index + 1
            )
        },
    );
    **text = format!("{slave}\n{master}\n{}", settings.message);
}

/// Runs [`FemModel::find_contact_candidates`] against the current model and
/// stores the results (and selects the first one) in
/// [`ContactCandidateState`].
///
/// This is the "近接surfaceを自動検出 → 接触候補を提案" half of the
/// topology-aware contact workflow; [`accept_contact_button_system`]
/// implements the "ユーザーが数クリックで承認" half.
pub(crate) fn detect_contacts_button_system(
    model: Option<Res<FemModel>>,
    mut state: ResMut<ContactCandidateState>,
    mut spiders: ResMut<RigidSpiderCandidateState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<DetectContactsButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref() {
                spiders.candidates.clear();
                spiders.selected = None;
                state.refresh(model);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Copies the visible detection controls into the reusable contact-search
/// resource. Changing a criterion invalidates old candidates so the review
/// panel never presents results produced with stale tolerances.
pub(crate) fn sync_contact_search_params(
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut state: ResMut<ContactCandidateState>,
) {
    let max_gap = slider_value(&sliders, SliderId::ContactSearchGap, 0.05).max(0.0);
    let normal_tolerance_deg =
        slider_value(&sliders, SliderId::ContactSearchAngle, 20.0).clamp(0.0, 90.0);

    if (state.params.max_gap - max_gap).abs() <= f32::EPSILON
        && (state.params.normal_tolerance_deg - normal_tolerance_deg).abs() <= f32::EPSILON
    {
        return;
    }

    state.params.max_gap = max_gap;
    state.params.normal_tolerance_deg = normal_tolerance_deg;
    state.candidates.clear();
    state.selected = None;
}

pub(crate) fn accept_contact_button_system(
    mut model: Option<ResMut<FemModel>>,
    mut state: ResMut<ContactCandidateState>,
    mut settings: ResMut<ContactDefinitionSettings>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AcceptContactButton>,
    >,
    mut labels: Query<&mut Text, With<AcceptContactLabel>>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let candidate = state.selected_candidate().cloned();
            let friction_coefficient = if settings.contact_type == ContactType::Tied {
                0.0
            } else {
                slider_value(&sliders, SliderId::ContactFriction, 0.0)
            };
            let penalty_factor = (settings.contact_type != ContactType::Tied
                && settings.use_penalty_factor)
                .then(|| slider_value(&sliders, SliderId::ContactPenaltyFactor, 1.0e5));
            let result = model
                .as_deref_mut()
                .ok_or_else(|| "No model is loaded".to_string())
                .and_then(|model| {
                    let candidate = candidate
                        .as_ref()
                        .ok_or_else(|| "No contact candidate is selected".to_string())?;
                    create_contact_from_candidate(
                        model,
                        candidate,
                        settings.contact_type,
                        friction_coefficient,
                        penalty_factor,
                    )
                });

            match result {
                Ok(index) => {
                    let name = model
                        .as_deref()
                        .and_then(|model| model.contacts.get(index))
                        .map(|contact| contact.name.clone())
                        .unwrap_or_else(|| "contact".to_string());
                    state.remove_selected();
                    settings.message = format!(
                        "Accepted {name} as {} surface-to-surface contact",
                        settings.contact_type.label()
                    );
                }
                Err(message) => settings.message = message,
            }
        }

        let ready = state.selected_candidate().is_some();
        let color = match (*interaction, ready) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(if ready { ACTIVE_BORDER } else { PANEL_BORDER });
    }

    for mut label in &mut labels {
        **label = format!("Accept as {}", settings.contact_type.label());
    }
}

/// Moves through detected contact candidates or rejects the current one
/// without changing the analysis model.
pub(crate) fn contact_candidate_action_button_system(
    mut state: ResMut<ContactCandidateState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ContactCandidateActionButton,
        ),
        With<ContactCandidateActionButton>,
    >,
) {
    for (interaction, mut background, mut border, action) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            match action.0 {
                ContactCandidateAction::Previous => state.select_previous(),
                ContactCandidateAction::Next => state.select_next(),
                ContactCandidateAction::Reject => state.remove_selected(),
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Toggles whether parts outside the reviewed pair are rendered as a quiet,
/// transparent context shell.
pub(crate) fn contact_ghost_toggle_button_system(
    mut review: ResMut<ContactReviewSettings>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ContactGhostToggleButton>,
    >,
    mut labels: Query<&mut Text, With<ContactGhostToggleLabel>>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            review.ghost_others = !review.ghost_others;
        }

        let active = review.ghost_others;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }

    for mut label in &mut labels {
        **label = format!(
            "Ghost others: {}",
            if review.ghost_others { "ON" } else { "OFF" }
        );
    }
}

/// Keeps visualization review state synchronized with the Contact page and
/// its view-only separation slider.
pub(crate) fn update_contact_review_settings(
    page: Res<SidebarPage>,
    candidates: Res<ContactCandidateState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut review: ResMut<ContactReviewSettings>,
) {
    let active = *page == SidebarPage::Contact && candidates.selected_candidate().is_some();
    let separation_percent = sliders
        .iter()
        .find(|slider| slider.id == SliderId::ContactReviewSeparation)
        .map(|slider| slider.value)
        .unwrap_or(8.0);

    if review.active != active {
        review.active = active;
    }
    if (review.separation_percent - separation_percent).abs() > f32::EPSILON {
        review.separation_percent = separation_percent;
    }
}

/// Rebuilds the dynamic list of set buttons inside [`SetsListContainer`]
/// whenever [`FemModelVersion`] changes (new mesh loaded / imported /
/// reloaded).
///
/// Every node/element/surface set in every mesh of the model gets one
/// button, labelled with its name, kind, and member count.

pub(crate) fn update_contact_candidate_text(
    state: Res<ContactCandidateState>,
    model: Option<Res<FemModel>>,
    mut query: Query<&mut Text, With<ContactCandidateText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    **text = contact_candidate_summary(&state, model.as_deref());
}

pub(crate) fn rebuild_contact_definitions_list(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    container_query: Query<Entity, With<ContactDefinitionsListContainer>>,
    children_query: Query<&Children>,
    mut summary_query: Query<&mut Text, With<ContactDefinitionsText>>,
    mut preview: ResMut<DefinedContactPreview>,
) {
    let current = version.value;
    let model_changed = model.as_ref().is_some_and(|model| model.is_changed());
    if *last_version == Some(current) && !model_changed {
        return;
    }
    *last_version = Some(current);

    let Ok(container) = container_query.single() else {
        return;
    };

    if let Ok(children) = children_query.get(container) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    let contact_count = model.as_deref().map_or(0, |model| model.contacts.len());
    if let Ok(mut summary) = summary_query.single_mut() {
        **summary = format!("Defined contacts: {contact_count}");
    }

    let next_selected = match preview.selected {
        Some(index) if index < contact_count => Some(index),
        _ if contact_count > 0 => Some(0),
        _ => None,
    };
    if preview.selected != next_selected {
        preview.selected = next_selected;
    }
    if contact_count == 0 && preview.active {
        preview.active = false;
    }

    let Some(model) = model.as_deref() else {
        return;
    };
    commands.entity(container).with_children(|list| {
        for (index, contact) in model.contacts.iter().enumerate() {
            let slave = model.contact_slave_name(contact.slave).unwrap_or("?");
            let master = model.surface_set_name(contact.master).unwrap_or("?");
            let pair_kind = match contact.slave {
                ContactSlaveRef::Nodes(_) => "NODE-SURF",
                ContactSlaveRef::Surface(_) => "SURF-SURF",
            };
            let parameters = match contact.contact_type {
                ContactType::Tied => String::new(),
                ContactType::SmallSliding | ContactType::FiniteSliding => {
                    let penalty = contact
                        .penalty_factor
                        .map(|factor| format!(" | penalty={factor:.3e}"))
                        .unwrap_or_default();
                    format!(" | mu={:.4}{penalty}", contact.friction_coefficient)
                }
            };
            contact_definition_button(
                list,
                index,
                &format!(
                    "[{}] {} | {}{}\n{} -> {}",
                    contact.name,
                    contact.contact_type.label(),
                    pair_kind,
                    parameters,
                    slave,
                    master,
                ),
            );
        }
    });
}

fn contact_definition_button(parent: &mut ChildSpawnerCommands, index: usize, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                width: percent(100.0),
                min_height: px(42.0),
                padding: UiRect::axes(px(8.0), px(5.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            DefinedContactButton(index),
            Name::new(format!("DefinedContact_{index}")),
        ))
        .with_child((
            Text::new(label.to_string()),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

pub(crate) fn defined_contact_button_system(
    mut preview: ResMut<DefinedContactPreview>,
    mut mpc_preview: ResMut<DefinedMpcPreview>,
    mut mpc_pair_draft: ResMut<MpcPairDraftPreview>,
    mut draft: ResMut<ContactDraftPreview>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &DefinedContactButton,
        ),
        With<DefinedContactButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            preview.selected = Some(button.0);
            mpc_preview.active = false;
            mpc_pair_draft.clear();
            draft.clear();
        }

        let selected = preview.selected == Some(button.0);
        let color = match (*interaction, selected) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *background = BackgroundColor(color);
        *border = BorderColor::all(if selected {
            ACTIVE_BORDER
        } else {
            PANEL_BORDER
        });
    }
}

/// Enables contact overlays only on the Contact page. Candidate review has
/// first priority, an in-progress draft second, and a defined pair third.
pub(crate) fn sync_defined_contact_preview(
    page: Res<SidebarPage>,
    model: Option<Res<FemModel>>,
    candidates: Res<ContactCandidateState>,
    mpc_preview: Res<DefinedMpcPreview>,
    mpc_pair_draft: Res<MpcPairDraftPreview>,
    mut preview: ResMut<DefinedContactPreview>,
    mut draft: ResMut<ContactDraftPreview>,
) {
    let contact_count = model.as_deref().map_or(0, |model| model.contacts.len());
    let selected = match preview.selected {
        Some(index) if index < contact_count => Some(index),
        _ if contact_count > 0 => Some(0),
        _ => None,
    };
    let draft_has_geometry = draft.master.is_some() || draft.slave.is_some();
    let draft_active = *page == SidebarPage::Contact
        && candidates.selected_candidate().is_none()
        && !mpc_pair_draft.active
        && draft_has_geometry;
    let active = *page == SidebarPage::Contact
        && candidates.selected_candidate().is_none()
        && !draft_has_geometry
        && !mpc_preview.active
        && !mpc_pair_draft.active
        && selected.is_some();

    if preview.selected != selected {
        preview.selected = selected;
    }
    if preview.active != active {
        preview.active = active;
    }
    if draft.active != draft_active {
        draft.active = draft_active;
    }
}

pub(crate) fn update_contact_review_controls(
    state: Res<ContactCandidateState>,
    mut controls: Query<&mut Node, With<ContactReviewControls>>,
) {
    if !state.is_changed() {
        return;
    }

    let display = if state.selected_candidate().is_some() {
        Display::Flex
    } else {
        Display::None
    };

    for mut node in &mut controls {
        node.display = display;
    }
}

fn contact_candidate_summary(state: &ContactCandidateState, model: Option<&FemModel>) -> String {
    let total = state.candidates.len();

    let Some(candidate) = state.selected_candidate() else {
        return if total == 0 {
            "No candidates — run Detect Contact Candidates".to_string()
        } else {
            format!("Contacts: {total} candidates")
        };
    };

    let selected_index = state.selected.unwrap_or(0);
    let mesh_a = mesh_label(model, candidate.mesh_a);
    let mesh_b = mesh_label(model, candidate.mesh_b);
    let kind = if candidate.is_self_contact() {
        "self"
    } else {
        "cross-part"
    };

    format!(
        "Contact candidate {}/{total} ({kind})\nMaster: {mesh_a}\nSlave: {mesh_b}\nFaces M/S: {} / {}  Pairs: {}  Avg gap: {:.4}",
        selected_index + 1,
        candidate.faces_a.len(),
        candidate.faces_b.len(),
        candidate.pair_count,
        candidate.average_gap,
    )
}

fn mesh_label(model: Option<&FemModel>, mesh_index: usize) -> String {
    model
        .and_then(|model| {
            model
                .parts
                .iter()
                .find(|part| part.mesh_index == mesh_index)
        })
        .map(|part| part.name.clone())
        .unwrap_or_else(|| format!("Mesh {mesh_index}"))
}

fn slider_value(
    query: &Query<&SliderState, With<SliderTrack>>,
    id: SliderId,
    fallback: f32,
) -> f32 {
    query
        .iter()
        .find(|state| state.id == id)
        .map(|state| state.value)
        .unwrap_or(fallback)
}

fn selected_nodes_by_mesh(selection: &SelectionState) -> BTreeMap<usize, Vec<fem_core::NodeId>> {
    let mut grouped = BTreeMap::<usize, BTreeSet<fem_core::NodeId>>::new();
    for target in &selection.targets {
        if let FemEntityId::Node(id) = target.entity {
            grouped.entry(target.mesh_index).or_default().insert(id);
        }
    }
    grouped
        .into_iter()
        .map(|(mesh_index, nodes)| (mesh_index, nodes.into_iter().collect()))
        .collect()
}
