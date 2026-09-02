//! MPC equation editing, pair constraints, and rigid-spider review UI.

use crate::contact_ui::{ContactParameter, ContactParameterButton};
use crate::layout::SidebarPage;
use crate::measurement::MeasurementBoxState;
use crate::slider::{SliderConfig, SliderId, SliderState, SliderTrack, spawn_slider};
use bevy::prelude::*;
use fem_core::{
    ContactCandidateState, FemEntityId, FemModel, RigidSpiderCandidateState, RigidSpiderMode,
    SelectionFilter, SelectionLevel, ViewportTool,
};
use selection::{Selected, SelectionState};
use std::collections::{BTreeMap, BTreeSet};
use visualization::{
    ContactDraftPreview, DefinedContactPreview, DefinedMpcPreview, MpcPairDraftPreview,
};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const ACTIVE_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);

#[derive(Component)]
pub(crate) struct DetectRigidSpidersButton;

#[derive(Component)]
pub(crate) struct AcceptRigidSpiderButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RigidSpiderAction {
    Previous,
    Next,
    Reject,
}

#[derive(Component)]
pub(crate) struct RigidSpiderActionButton(pub RigidSpiderAction);

#[derive(Component)]
pub(crate) struct RigidSpiderCandidateText;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinedMpcAction {
    Show,
    Previous,
    Next,
    EditConstant,
    PreviousTerm,
    NextTerm,
    EditCoefficient,
    Remove,
}

#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct MpcEquationEditorState {
    equation: Option<usize>,
    term: usize,
}

#[derive(Component)]
pub(crate) struct DefinedMpcActionButton(pub DefinedMpcAction);

#[derive(Component)]
pub(crate) struct DefinedMpcText;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MpcPairSide {
    Positive,
    Negative,
}

#[derive(Component)]
pub(crate) struct CaptureMpcPairNodeButton(pub MpcPairSide);

#[derive(Component)]
pub(crate) struct MpcPairDofButton(pub u8);

#[derive(Component)]
pub(crate) struct CreateMpcPairButton;

#[derive(Component)]
pub(crate) struct ClearMpcPairButton;

#[derive(Component)]
pub(crate) struct MpcPairDraftText;

#[derive(Resource, Debug, Clone)]
pub(crate) struct MpcPairDraftState {
    dof: u8,
    message: String,
}

impl Default for MpcPairDraftState {
    fn default() -> Self {
        Self {
            dof: 0,
            message: "Select one node, then capture the reference (+) side".to_string(),
        }
    }
}

pub(crate) fn spawn_mpc_ui(parent: &mut ChildSpawnerCommands) {
    mpc_subheading(parent, "PAIR MPC FROM VIEWPORT");
    mpc_hint(
        parent,
        "Select exactly one Node, then capture each side; reference = magenta (+), coupled = cyan (-)",
    );
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            mpc_action_button(
                row,
                "1  Reference (+)",
                CaptureMpcPairNodeButton(MpcPairSide::Positive),
                "CaptureMpcPairPositiveButton",
            );
            mpc_action_button(
                row,
                "2  Coupled (-)",
                CaptureMpcPairNodeButton(MpcPairSide::Negative),
                "CaptureMpcPairNegativeButton",
            );
        });
    parent.spawn((
        Text::new("Reference: not set   Coupled: not set"),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
        MpcPairDraftText,
    ));
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(4.0),
            ..default()
        },))
        .with_children(|row| {
            for (dof, label, name) in [
                (0, "XYZ", "MpcPairDofXyzButton"),
                (1, "Ux", "MpcPairDofXButton"),
                (2, "Uy", "MpcPairDofYButton"),
                (3, "Uz", "MpcPairDofZButton"),
            ] {
                mpc_action_button(row, label, MpcPairDofButton(dof), name);
            }
        });
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            mpc_action_button(row, "Clear", ClearMpcPairButton, "ClearMpcPairButton");
            mpc_action_button(
                row,
                "3  Create !EQUATION",
                CreateMpcPairButton,
                "CreateMpcPairButton",
            );
        });
    mpc_hint(
        parent,
        "XYZ creates three grouped equations; exact constants and coefficients remain editable below",
    );
    mpc_divider(parent);

    mpc_subheading(parent, "AUTOMATIC RIGID SPIDER");
    spawn_slider(
        parent,
        SliderConfig {
            width: 272.0,
            min: 0.0,
            max: 20.0,
            value: 1.0,
            label: "Search radius (model units)",
            id: SliderId::RigidSpiderRadius,
        },
    );
    mpc_action_button(
        parent,
        "Edit radius exactly",
        ContactParameterButton(ContactParameter::SpiderRadius),
        "EditRigidSpiderRadiusButton",
    );
    mpc_action_button(
        parent,
        "Detect MPC Spiders",
        DetectRigidSpidersButton,
        "DetectRigidSpidersButton",
    );
    parent.spawn((
        Text::new("No MPC candidates — run Detect MPC Spiders"),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        RigidSpiderCandidateText,
    ));
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            mpc_action_button(
                row,
                "Previous",
                RigidSpiderActionButton(RigidSpiderAction::Previous),
                "PreviousRigidSpiderButton",
            );
            mpc_action_button(
                row,
                "Next",
                RigidSpiderActionButton(RigidSpiderAction::Next),
                "NextRigidSpiderButton",
            );
        });
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            mpc_action_button(
                row,
                "Reject",
                RigidSpiderActionButton(RigidSpiderAction::Reject),
                "RejectRigidSpiderButton",
            );
            mpc_action_button(
                row,
                "Create !EQUATION",
                AcceptRigidSpiderButton,
                "AcceptRigidSpiderButton",
            );
        });
    mpc_hint(
        parent,
        "Center = magenta, solid boundary nodes = cyan; isolated centers transfer translations only",
    );
    mpc_divider(parent);

    mpc_subheading(parent, "DEFINED MPC REVIEW");
    parent.spawn((
        Text::new("No MPC equations defined"),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        DefinedMpcText,
    ));
    mpc_action_button(
        parent,
        "Show selected in viewport",
        DefinedMpcActionButton(DefinedMpcAction::Show),
        "ShowDefinedMpcButton",
    );
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            mpc_action_button(
                row,
                "Previous equation",
                DefinedMpcActionButton(DefinedMpcAction::Previous),
                "PreviousDefinedMpcButton",
            );
            mpc_action_button(
                row,
                "Next equation",
                DefinedMpcActionButton(DefinedMpcAction::Next),
                "NextDefinedMpcButton",
            );
        });
    mpc_action_button(
        parent,
        "Edit constant exactly",
        DefinedMpcActionButton(DefinedMpcAction::EditConstant),
        "EditDefinedMpcConstantButton",
    );
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            mpc_action_button(
                row,
                "Previous term",
                DefinedMpcActionButton(DefinedMpcAction::PreviousTerm),
                "PreviousDefinedMpcTermButton",
            );
            mpc_action_button(
                row,
                "Next term",
                DefinedMpcActionButton(DefinedMpcAction::NextTerm),
                "NextDefinedMpcTermButton",
            );
        });
    mpc_action_button(
        parent,
        "Edit selected coefficient exactly",
        DefinedMpcActionButton(DefinedMpcAction::EditCoefficient),
        "EditDefinedMpcCoefficientButton",
    );
    mpc_hint(
        parent,
        "Exact edit uses the lower-right value box; Enter applies, Esc cancels",
    );
    mpc_action_button(
        parent,
        "Remove selected equation / group",
        DefinedMpcActionButton(DefinedMpcAction::Remove),
        "RemoveDefinedMpcButton",
    );
    mpc_hint(
        parent,
        "Expanded spiders are removed as one group; Ctrl+Z restores the change",
    );
    mpc_hint(
        parent,
        "Selected equation: positive coefficients = magenta, negative = cyan",
    );
}

fn mpc_subheading(parent: &mut ChildSpawnerCommands, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
    ));
}

fn mpc_action_button(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    marker: impl Bundle,
    name: &'static str,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(28.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            marker,
            Name::new(name),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(11.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

fn mpc_hint(parent: &mut ChildSpawnerCommands, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
    ));
}

fn mpc_divider(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: percent(100.0),
            height: px(1.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.28, 0.34, 0.38, 0.60)),
    ));
}

pub(crate) fn sync_rigid_spider_search_params(
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut state: ResMut<RigidSpiderCandidateState>,
) {
    let radius = slider_value(&sliders, SliderId::RigidSpiderRadius, 1.0).max(0.0);
    if (state.params.radius - radius).abs() <= f32::EPSILON {
        return;
    }
    state.params.radius = radius;
    state.candidates.clear();
    state.selected = None;
}

pub(crate) fn detect_rigid_spiders_button_system(
    model: Option<Res<FemModel>>,
    mut state: ResMut<RigidSpiderCandidateState>,
    mut contacts: ResMut<ContactCandidateState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<DetectRigidSpidersButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref() {
                contacts.candidates.clear();
                contacts.selected = None;
                state.refresh(model);
            }
        }
        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn rigid_spider_action_button_system(
    mut state: ResMut<RigidSpiderCandidateState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &RigidSpiderActionButton,
        ),
        With<RigidSpiderActionButton>,
    >,
) {
    for (interaction, mut background, mut border, action) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            match action.0 {
                RigidSpiderAction::Previous => state.select_previous(),
                RigidSpiderAction::Next => state.select_next(),
                RigidSpiderAction::Reject => state.remove_selected(),
            }
        }
        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn accept_rigid_spider_button_system(
    model: Option<Res<FemModel>>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut state: ResMut<RigidSpiderCandidateState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AcceptRigidSpiderButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let candidate = state.selected_candidate().cloned();
            if let (Some(model), Some(candidate)) = (model.as_deref(), candidate) {
                let name = format!("SPIDER_{}", setup.mpc_equations.len() + 1);
                if let Some(equations) = model.rigid_spider_equations(&candidate, &name) {
                    if !equations.is_empty() {
                        setup.mpc_equations.extend(equations);
                        state.remove_selected();
                    }
                }
            }
        }
        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn update_rigid_spider_candidate_text(
    state: Res<RigidSpiderCandidateState>,
    model: Option<Res<FemModel>>,
    setup: Res<fem_core::AnalysisSetup>,
    mut query: Query<&mut Text, With<RigidSpiderCandidateText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };
    let Some(candidate) = state.selected_candidate() else {
        **text = format!(
            "No MPC candidates — run Detect MPC Spiders\nDefined equations: {}",
            setup.mpc_equations.len()
        );
        return;
    };
    let index = state.selected.unwrap_or(0) + 1;
    let mode = match candidate.mode {
        RigidSpiderMode::TranslationOnly => "translation only",
        RigidSpiderMode::RigidBody => "rigid body (6 DOF center)",
    };
    **text = format!(
        "MPC candidate {index}/{}\nCenter: {} / node {}\nSolid: {} / {} boundary nodes\nMode: {mode}",
        state.candidates.len(),
        mesh_label(model.as_deref(), candidate.master_mesh),
        candidate.master_node.0,
        mesh_label(model.as_deref(), candidate.slave_mesh),
        candidate.slave_nodes.len(),
    );
}

fn single_selected_node(selection: &SelectionState) -> Result<(usize, fem_core::NodeId), String> {
    let nodes: Vec<_> = selected_nodes_by_mesh(selection)
        .into_iter()
        .flat_map(|(mesh_index, nodes)| nodes.into_iter().map(move |node| (mesh_index, node)))
        .collect();
    match nodes.as_slice() {
        [node] => Ok(*node),
        [] => Err("Select exactly one Node in the viewport first".to_string()),
        _ => Err(format!(
            "{} nodes selected; select exactly one Node",
            nodes.len()
        )),
    }
}

pub(crate) fn capture_mpc_pair_node_button_system(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    mut selection: ResMut<SelectionState>,
    mut filter: ResMut<SelectionFilter>,
    mut draft: ResMut<MpcPairDraftPreview>,
    mut contact_draft: ResMut<ContactDraftPreview>,
    mut state: ResMut<MpcPairDraftState>,
    mut defined_mpc: ResMut<DefinedMpcPreview>,
    mut defined_contact: ResMut<DefinedContactPreview>,
    mut contact_candidates: ResMut<ContactCandidateState>,
    mut spider_candidates: ResMut<RigidSpiderCandidateState>,
    selected_query: Query<Entity, With<Selected>>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &CaptureMpcPairNodeButton,
        ),
        With<CaptureMpcPairNodeButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            filter.level = SelectionLevel::Node;
            let result = single_selected_node(&selection).and_then(|(mesh_index, node)| {
                let exists = model
                    .as_deref()
                    .and_then(|model| model.meshes.get(mesh_index))
                    .and_then(|mesh| mesh.node_position(node))
                    .is_some();
                if !exists {
                    return Err("The selected node no longer exists in the model".to_string());
                }

                let opposite = match button.0 {
                    MpcPairSide::Positive => draft.negative,
                    MpcPairSide::Negative => draft.positive,
                };
                if opposite == Some((mesh_index, node)) {
                    return Err("Reference and coupled nodes must be different".to_string());
                }

                match button.0 {
                    MpcPairSide::Positive => draft.positive = Some((mesh_index, node)),
                    MpcPairSide::Negative => draft.negative = Some((mesh_index, node)),
                }
                draft.active = true;
                let side = match button.0 {
                    MpcPairSide::Positive => "Reference (+)",
                    MpcPairSide::Negative => "Coupled (-)",
                };
                Ok(format!(
                    "{side} captured: part {} / node {}; select the other side",
                    mesh_index + 1,
                    node.0
                ))
            });

            match result {
                Ok(message) => {
                    state.message = message;
                    selection.clear();
                    for entity in &selected_query {
                        commands.entity(entity).remove::<Selected>();
                    }
                    defined_mpc.active = false;
                    defined_contact.active = false;
                    contact_draft.clear();
                    contact_candidates.selected = None;
                    spider_candidates.selected = None;
                }
                Err(message) => state.message = message,
            }
        }

        let captured = match button.0 {
            MpcPairSide::Positive => draft.positive.is_some(),
            MpcPairSide::Negative => draft.negative.is_some(),
        };
        *background = BackgroundColor(match (*interaction, captured) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
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

pub(crate) fn mpc_pair_dof_button_system(
    mut state: ResMut<MpcPairDraftState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &MpcPairDofButton,
        ),
        With<MpcPairDofButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            state.dof = button.0;
        }
        let active = state.dof == button.0;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn next_mpc_pair_name(setup: &fem_core::AnalysisSetup) -> String {
    for serial in 1.. {
        let name = format!("MPC_PAIR_{serial}");
        let available = setup.mpc_equations.iter().all(|equation| {
            equation.group.as_deref() != Some(name.as_str())
                && equation.name != name
                && !equation.name.starts_with(&format!("{name}_"))
        });
        if available {
            return name;
        }
    }
    unreachable!("the MPC pair serial is unbounded")
}

pub(crate) fn pair_mpc_equations(
    group: &str,
    positive: (usize, fem_core::NodeId),
    negative: (usize, fem_core::NodeId),
    dof: u8,
) -> Result<Vec<fem_core::MpcEquation>, String> {
    if positive == negative {
        return Err("Reference and coupled nodes must be different".to_string());
    }
    let dofs: &[u8] = match dof {
        0 => &[1, 2, 3],
        1 => &[1],
        2 => &[2],
        3 => &[3],
        _ => return Err("Pair MPC supports only Ux, Uy, Uz, or XYZ".to_string()),
    };

    Ok(dofs
        .iter()
        .map(|&dof| {
            fem_core::MpcEquation::new(
                format!("{group}_{}", mpc_dof_label(dof).to_ascii_uppercase()),
                0.0,
                vec![
                    fem_core::MpcTerm::new(positive.0, positive.1, dof, 1.0),
                    fem_core::MpcTerm::new(negative.0, negative.1, dof, -1.0),
                ],
            )
            .with_group(group)
        })
        .collect())
}

pub(crate) fn create_mpc_pair_button_system(
    model: Option<Res<FemModel>>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut draft: ResMut<MpcPairDraftPreview>,
    mut state: ResMut<MpcPairDraftState>,
    mut defined: ResMut<DefinedMpcPreview>,
    mut editor: ResMut<MpcEquationEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<CreateMpcPairButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let result = (|| {
                let positive = draft
                    .positive
                    .ok_or_else(|| "Capture the reference (+) node first".to_string())?;
                let negative = draft
                    .negative
                    .ok_or_else(|| "Capture the coupled (-) node first".to_string())?;
                let model = model
                    .as_deref()
                    .ok_or_else(|| "No model is loaded".to_string())?;
                for (label, (mesh_index, node)) in [("Reference", positive), ("Coupled", negative)]
                {
                    let exists = model
                        .meshes
                        .get(mesh_index)
                        .and_then(|mesh| mesh.node_position(node))
                        .is_some();
                    if !exists {
                        return Err(format!("{label} node no longer exists in the model"));
                    }
                }

                let group = next_mpc_pair_name(&setup);
                let equations = pair_mpc_equations(&group, positive, negative, state.dof)?;
                let first = setup.mpc_equations.len();
                let count = equations.len();
                setup.mpc_equations.extend(equations);
                Ok((group, first, count))
            })();

            match result {
                Ok((group, first, count)) => {
                    draft.clear();
                    defined.selected = Some(first);
                    defined.active = true;
                    editor.equation = Some(first);
                    editor.term = 0;
                    measurement.clear();
                    state.message = format!(
                        "Created {group}: {count} equation{}; shown in viewport",
                        if count == 1 { "" } else { "s" }
                    );
                }
                Err(message) => state.message = message,
            }
        }

        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn clear_mpc_pair_button_system(
    mut draft: ResMut<MpcPairDraftPreview>,
    mut state: ResMut<MpcPairDraftState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ClearMpcPairButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            draft.clear();
            state.message = "Pair MPC draft cleared".to_string();
        }
        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn update_mpc_pair_draft_text(
    model: Option<Res<FemModel>>,
    draft: Res<MpcPairDraftPreview>,
    state: Res<MpcPairDraftState>,
    mut texts: Query<&mut Text, With<MpcPairDraftText>>,
) {
    let format_node = |node: Option<(usize, fem_core::NodeId)>| match node {
        Some((mesh_index, node)) => format!(
            "{} / node {}",
            mesh_label(model.as_deref(), mesh_index),
            node.0
        ),
        None => "not set".to_string(),
    };
    let dof = if state.dof == 0 {
        "XYZ"
    } else {
        mpc_dof_label(state.dof)
    };
    for mut text in &mut texts {
        **text = format!(
            "Reference (+): {}\nCoupled (-): {}   DOF: {dof}\n{}",
            format_node(draft.positive),
            format_node(draft.negative),
            state.message
        );
    }
}

pub(crate) fn sync_mpc_pair_draft_preview(
    page: Res<SidebarPage>,
    contact_candidates: Res<ContactCandidateState>,
    spider_candidates: Res<RigidSpiderCandidateState>,
    contact_draft: Res<ContactDraftPreview>,
    mut draft: ResMut<MpcPairDraftPreview>,
) {
    let active = *page == SidebarPage::Contact
        && (draft.positive.is_some() || draft.negative.is_some())
        && contact_candidates.selected_candidate().is_none()
        && spider_candidates.selected_candidate().is_none()
        && contact_draft.master.is_none()
        && contact_draft.slave.is_none();
    if draft.active != active {
        draft.active = active;
    }
}

pub(crate) fn defined_mpc_action_button_system(
    mut preview: ResMut<DefinedMpcPreview>,
    mut pair_draft: ResMut<MpcPairDraftPreview>,
    mut editor: ResMut<MpcEquationEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut tool: ResMut<ViewportTool>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut contact_preview: ResMut<DefinedContactPreview>,
    mut contact_draft: ResMut<ContactDraftPreview>,
    mut contact_candidates: ResMut<ContactCandidateState>,
    mut spider_candidates: ResMut<RigidSpiderCandidateState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &DefinedMpcActionButton,
        ),
        With<DefinedMpcActionButton>,
    >,
) {
    for (interaction, mut background, mut border, action) in &mut buttons {
        let count = setup.mpc_equations.len();
        if *interaction == Interaction::Pressed && interaction.is_changed() && count > 0 {
            let current = preview.selected.filter(|index| *index < count).unwrap_or(0);
            if editor.equation != Some(current) {
                editor.equation = Some(current);
                editor.term = 0;
            }
            match action.0 {
                DefinedMpcAction::Show => {
                    preview.selected = Some(current);
                    preview.active = true;
                }
                DefinedMpcAction::Previous => {
                    let selected = (current + count - 1) % count;
                    preview.selected = Some(selected);
                    editor.equation = Some(selected);
                    editor.term = 0;
                    measurement.clear();
                    preview.active = true;
                }
                DefinedMpcAction::Next => {
                    let selected = (current + 1) % count;
                    preview.selected = Some(selected);
                    editor.equation = Some(selected);
                    editor.term = 0;
                    measurement.clear();
                    preview.active = true;
                }
                DefinedMpcAction::EditConstant => {
                    *tool = ViewportTool::Selection;
                    measurement.begin_mpc_constant(current, setup.mpc_equations[current].constant);
                    preview.active = true;
                }
                DefinedMpcAction::PreviousTerm | DefinedMpcAction::NextTerm => {
                    let term_count = setup.mpc_equations[current].terms.len();
                    if term_count > 0 {
                        editor.term = match action.0 {
                            DefinedMpcAction::PreviousTerm => {
                                (editor.term + term_count - 1) % term_count
                            }
                            DefinedMpcAction::NextTerm => (editor.term + 1) % term_count,
                            _ => unreachable!(),
                        };
                        measurement.clear();
                        preview.active = true;
                    }
                }
                DefinedMpcAction::EditCoefficient => {
                    if let Some(term) = setup.mpc_equations[current].terms.get(editor.term) {
                        *tool = ViewportTool::Selection;
                        measurement.begin_mpc_coefficient(current, editor.term, term.coefficient);
                        preview.active = true;
                    }
                }
                DefinedMpcAction::Remove => {
                    remove_mpc_equation_or_group(&mut setup.mpc_equations, current);
                    let remaining = setup.mpc_equations.len();
                    preview.selected = (remaining > 0).then(|| current.min(remaining - 1));
                    editor.equation = preview.selected;
                    editor.term = 0;
                    measurement.clear();
                    preview.active = false;
                }
            }
            contact_preview.active = false;
            contact_draft.clear();
            pair_draft.clear();
            contact_candidates.selected = None;
            spider_candidates.selected = None;
        }

        let destructive = action.0 == DefinedMpcAction::Remove;
        *background = BackgroundColor(match (*interaction, destructive) {
            (Interaction::Pressed | Interaction::Hovered, true) => {
                Color::srgba(0.75, 0.22, 0.22, 0.95)
            }
            (Interaction::None, true) => Color::srgba(0.55, 0.18, 0.18, 0.85),
            (Interaction::Pressed, false) => BUTTON_PRESSED,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if destructive {
            Color::srgb(0.72, 0.30, 0.30)
        } else {
            PANEL_BORDER
        });
    }
}

pub(crate) fn remove_mpc_equation_or_group(
    equations: &mut Vec<fem_core::MpcEquation>,
    selected: usize,
) -> usize {
    let Some(equation) = equations.get(selected) else {
        return 0;
    };
    let group = equation.group.clone();
    let before = equations.len();
    if let Some(group) = group {
        equations.retain(|equation| equation.group.as_deref() != Some(group.as_str()));
    } else {
        equations.remove(selected);
    }
    before - equations.len()
}

pub(crate) fn update_defined_mpc_text(
    setup: Res<fem_core::AnalysisSetup>,
    preview: Res<DefinedMpcPreview>,
    editor: Res<MpcEquationEditorState>,
    model: Option<Res<FemModel>>,
    mut query: Query<&mut Text, With<DefinedMpcText>>,
) {
    if !setup.is_changed()
        && !preview.is_changed()
        && !editor.is_changed()
        && !model.as_ref().is_some_and(|m| m.is_changed())
    {
        return;
    }
    let Ok(mut text) = query.single_mut() else {
        return;
    };
    let count = setup.mpc_equations.len();
    let Some(index) = preview.selected.filter(|index| *index < count) else {
        **text = "No MPC equations defined".to_string();
        return;
    };
    let equation = &setup.mpc_equations[index];
    let mut lines = vec![
        format!("Equation {}/{}: {}", index + 1, count, equation.name),
        format!(
            "constant={:+.6e} | {} terms{}",
            equation.constant,
            equation.terms.len(),
            if preview.active {
                " | viewport review ON"
            } else {
                ""
            }
        ),
    ];
    if let Some(group) = equation.group.as_deref() {
        let group_count = setup
            .mpc_equations
            .iter()
            .filter(|candidate| candidate.group.as_deref() == Some(group))
            .count();
        lines.insert(1, format!("Group: {group} | {group_count} equations"));
    }
    if !equation.terms.is_empty() {
        let term_index = editor.term.min(equation.terms.len() - 1);
        let term = &equation.terms[term_index];
        lines.push(format!(
            "Selected term {}/{}",
            term_index + 1,
            equation.terms.len(),
        ));
        lines.push(format!(
            "{} / node {} / {}: {:+.6e}",
            mesh_label(model.as_deref(), term.mesh_index),
            term.node.0,
            mpc_dof_label(term.dof),
            term.coefficient,
        ));
    }
    **text = lines.join("\n");
}

fn mpc_dof_label(dof: u8) -> &'static str {
    match dof {
        1 => "Ux",
        2 => "Uy",
        3 => "Uz",
        4 => "Rx",
        5 => "Ry",
        6 => "Rz",
        _ => "invalid DOF",
    }
}

pub(crate) fn sync_defined_mpc_preview(
    page: Res<SidebarPage>,
    setup: Res<fem_core::AnalysisSetup>,
    contact_candidates: Res<ContactCandidateState>,
    spider_candidates: Res<RigidSpiderCandidateState>,
    pair_draft: Res<MpcPairDraftPreview>,
    mut preview: ResMut<DefinedMpcPreview>,
    mut editor: ResMut<MpcEquationEditorState>,
) {
    let count = setup.mpc_equations.len();
    let selected = match preview.selected {
        Some(index) if index < count => Some(index),
        _ if count > 0 => Some(0),
        _ => None,
    };
    if preview.selected != selected {
        preview.selected = selected;
    }
    if editor.equation != selected {
        editor.equation = selected;
        editor.term = 0;
    } else if let Some(index) = selected {
        let term_count = setup.mpc_equations[index].terms.len();
        if term_count == 0 {
            editor.term = 0;
        } else if editor.term >= term_count {
            editor.term = term_count - 1;
        }
    }

    let can_review = *page == SidebarPage::Contact
        && selected.is_some()
        && contact_candidates.selected_candidate().is_none()
        && spider_candidates.selected_candidate().is_none()
        && !pair_draft.active;
    if preview.active && !can_review {
        preview.active = false;
    }
}

pub(crate) fn sync_rigid_spider_review(
    page: Res<SidebarPage>,
    state: Res<RigidSpiderCandidateState>,
    mut review: ResMut<visualization::RigidSpiderReviewSettings>,
    mut defined: ResMut<DefinedMpcPreview>,
) {
    let active = *page == SidebarPage::Contact && state.selected_candidate().is_some();
    if review.active != active {
        review.active = active;
    }
    if active && defined.active {
        defined.active = false;
    }
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
