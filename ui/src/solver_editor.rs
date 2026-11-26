//! Exact numeric editor for solver controls shown on the Solve page.
//!
//! Values are committed only by Enter. This keeps partially typed text out
//! of `AnalysisSetup`, while Esc reliably restores the last solver value.

use crate::layout::UiInputCapture;
use crate::measurement::editable_value;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, TextCursorStyle};
use bevy::ui_widgets::SelectAllOnFocus;
use fem_core::{
    AnalysisType, DynamicStep, LinearSolverMethod, SolverSettings, StepGroup, UiKeyboardState,
};

const INPUT_BG: Color = Color::srgba(0.075, 0.09, 0.10, 0.98);
const INPUT_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const INPUT_FOCUS_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);
const INPUT_ERROR_BORDER: Color = Color::srgb(0.98, 0.36, 0.30);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);

#[cfg(test)]
#[path = "eigen_editor_tests.rs"]
mod eigen_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolverField {
    Substeps,
    MaxIterations,
    ConvergenceTolerance,
    PreconditionerIterations,
    GmresSubspaces,
    OrderingColors,
    PreconditionerReuse,
    DiagonalScale,
    SsorRelaxation,
    NonlinearMaxIterations,
    NonlinearConvergenceTolerance,
    EigenModes,
    EigenTolerance,
    EigenMaxIterations,
    EigenShift,
    DynamicSubsteps,
    DynamicIncrement,
    NewmarkGamma,
    NewmarkBeta,
}

impl SolverField {
    fn label(self) -> &'static str {
        match self {
            Self::Substeps => "Substeps",
            Self::MaxIterations => "Linear max iterations",
            Self::ConvergenceTolerance => "Linear tolerance",
            Self::PreconditionerIterations => "Preconditioner iterations",
            Self::GmresSubspaces => "GMRES subspaces",
            Self::OrderingColors => "Ordering colors",
            Self::PreconditionerReuse => "Preconditioner reuse",
            Self::DiagonalScale => "Diagonal scaling factor",
            Self::SsorRelaxation => "SSOR relaxation",
            Self::NonlinearMaxIterations => "Nonlinear max iterations",
            Self::NonlinearConvergenceTolerance => "Nonlinear residual tolerance",
            Self::EigenModes => "Requested modes",
            Self::EigenTolerance => "Eigen tolerance",
            Self::EigenMaxIterations => "Eigen maximum iterations",
            Self::EigenShift => "Eigen shift",
            Self::DynamicSubsteps => "Fallback substeps",
            Self::DynamicIncrement => "Time increment (t_delta)",
            Self::NewmarkGamma => "Newmark gamma",
            Self::NewmarkBeta => "Newmark beta",
        }
    }

    fn active(self, settings: &SolverSettings) -> bool {
        let iterative = settings.solver_method.is_iterative();
        match self {
            Self::MaxIterations
            | Self::ConvergenceTolerance
            | Self::PreconditionerIterations
            | Self::OrderingColors => iterative,
            Self::GmresSubspaces => settings.solver_method == LinearSolverMethod::Gmres,
            Self::PreconditionerReuse => {
                iterative && settings.analysis_type == AnalysisType::NlStatic
            }
            Self::DiagonalScale => iterative && settings.preconditioner == 3,
            Self::SsorRelaxation => iterative && matches!(settings.preconditioner, 1 | 2),
            Self::EigenModes
            | Self::EigenTolerance
            | Self::EigenMaxIterations
            | Self::EigenShift => settings.analysis_type == AnalysisType::Eigen,
            Self::DynamicSubsteps | Self::DynamicIncrement | Self::NewmarkGamma | Self::NewmarkBeta => {
                settings.analysis_type == AnalysisType::Dynamic && settings.dynamic.is_some()
            }
            Self::Substeps | Self::NonlinearMaxIterations | Self::NonlinearConvergenceTolerance => {
                settings.analysis_type == AnalysisType::NlStatic
            }
        }
    }

    fn optional(self) -> bool {
        matches!(
            self,
            Self::MaxIterations | Self::ConvergenceTolerance | Self::PreconditionerIterations
                | Self::GmresSubspaces | Self::OrderingColors | Self::PreconditionerReuse
                | Self::DiagonalScale | Self::SsorRelaxation
        )
    }

    fn integer(self) -> bool {
        matches!(
            self,
            Self::Substeps | Self::MaxIterations | Self::PreconditionerIterations
                | Self::GmresSubspaces | Self::OrderingColors | Self::PreconditionerReuse
                | Self::NonlinearMaxIterations | Self::EigenModes | Self::EigenMaxIterations
                | Self::DynamicSubsteps
        )
    }
}

#[derive(Component)]
pub(crate) enum SolverControlGroup {
    Linear,
    Field(SolverField),
    Nonlinear,
    DirectHint,
    Eigen,
    Dynamic,
    DynamicStep,
}

#[derive(Component)]
pub(crate) struct SolverValueInput(SolverField);

#[derive(Component)]
pub(crate) struct SolverInputStatus;

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SolverEditorState {
    error: Option<(SolverField, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ParsedSolverValue {
    Integer(u32),
    OptionalInteger(Option<u32>),
    Float(f32),
    Double(f64),
    OptionalDouble(Option<f64>),
}

pub(crate) fn spawn_solver_exact_editor(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
            SolverControlGroup::Linear,
        ))
        .with_children(|group| {
            solver_hint(group, "LINEAR ITERATION | !SOLVER");
            for (label, field, name) in [
                ("Max iterations", SolverField::MaxIterations, "SolverMaxIterationsInput"),
                (
                    "Relative residual",
                    SolverField::ConvergenceTolerance,
                    "SolverConvergenceToleranceInput",
                ),
                (
                    "Precond. iterations",
                    SolverField::PreconditionerIterations,
                    "SolverPreconditionerIterationsInput",
                ),
                ("GMRES subspaces", SolverField::GmresSubspaces, "SolverGmresSubspacesInput"),
                ("Ordering colors", SolverField::OrderingColors, "SolverOrderingColorsInput"),
                ("Precond. reuse", SolverField::PreconditionerReuse, "SolverPreconditionerReuseInput"),
                ("Diagonal scale", SolverField::DiagonalScale, "SolverDiagonalScaleInput"),
                ("SSOR relaxation", SolverField::SsorRelaxation, "SolverSsorRelaxationInput"),
            ] {
                solver_value_row(group, label, field, name);
            }
            solver_hint(group, "Blank optional fields use FrontISTR defaults.");
        });
    parent.spawn((Node::default(), SolverControlGroup::DirectHint)).with_children(|group| {
        solver_hint(group, "Direct method: linear iteration controls are not used. Availability depends on the FrontISTR build.");
    });
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
            SolverControlGroup::Nonlinear,
        ))
        .with_children(|group| {
            solver_hint(group, "NONLINEAR STEP | !STEP (fixed increment)");
            solver_value_row(
                group,
                "Substeps",
                SolverField::Substeps,
                "SolverSubstepsInput",
            );
            solver_value_row(
                group,
                "Newton max iter.",
                SolverField::NonlinearMaxIterations,
                "SolverNonlinearIterationsInput",
            );
            solver_value_row(
                group,
                "Residual tol.",
                SolverField::NonlinearConvergenceTolerance,
                "SolverNonlinearToleranceInput",
            );
            solver_hint(
                group,
                "Independent of linear tolerance. Other nonlinear criteria use FrontISTR defaults.",
            );
        });
    parent.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(4.0), ..default() }, SolverControlGroup::Eigen))
        .with_children(|group| {
            solver_hint(group, "EIGENVALUE | !EIGEN (Lanczos)");
            for (label, field, name) in [
                ("Requested modes", SolverField::EigenModes, "EigenModesInput"),
                ("Tolerance", SolverField::EigenTolerance, "EigenToleranceInput"),
                ("Max iterations", SolverField::EigenMaxIterations, "EigenIterationsInput"),
                ("Shift (sigma)", SolverField::EigenShift, "EigenShiftInput"),
            ] { solver_value_row(group, label, field, name); }
            solver_hint(group, "Max iterations must exceed modes; allow ample margin. Shift is a solver parameter, not Hz. Independent of linear iterations.");
        });
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
            SolverControlGroup::Dynamic,
        ))
        .with_children(|group| {
            solver_hint(group, "TIME INTEGRATION | !DYNAMIC (Newmark)");
            for (label, field, name) in [
                (
                    "Fallback substeps",
                    SolverField::DynamicSubsteps,
                    "DynamicSubstepsInput",
                ),
                (
                    "Increment (t_delta)",
                    SolverField::DynamicIncrement,
                    "DynamicIncrementInput",
                ),
                (
                    "Newmark gamma",
                    SolverField::NewmarkGamma,
                    "DynamicGammaInput",
                ),
                ("Newmark beta", SolverField::NewmarkBeta, "DynamicBetaInput"),
            ] {
                solver_value_row(group, label, field, name);
            }
        });
    parent.spawn((
        Text::default(),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        SolverControlGroup::DynamicStep,
    ));
    parent.spawn((
        Text::new("Enter applies an exact value; Esc restores"),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
        SolverInputStatus,
    ));
}

fn solver_hint(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
    ));
}

fn solver_value_row(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    field: SolverField,
    name: &'static str,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                height: px(25.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(5.0),
                ..default()
            },
            SolverControlGroup::Field(field),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                Node {
                    width: px(112.0),
                    ..default()
                },
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    min_width: px(70.0),
                    height: px(25.0),
                    padding: UiRect::axes(px(5.0), px(2.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(3.0)),
                    ..default()
                },
                bevy::ui_widgets::TextInput,
                EditableText {
                    visible_width: Some(12.0),
                    max_characters: Some(32),
                    allow_newlines: false,
                    ..EditableText::new("0")
                },
                EditableTextFilter::new(move |character| {
                    character.is_ascii_digit()
                        || (!field.integer() && matches!(character, '+' | '-' | '.' | 'e' | 'E'))
                }),
                TextLayout {
                    justify: Justify::End,
                    ..TextLayout::no_wrap()
                },
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                TextCursorStyle {
                    color: INPUT_FOCUS_BORDER,
                    selected_text_color: Some(Color::srgb(0.02, 0.04, 0.05)),
                    ..default()
                },
                SelectAllOnFocus,
                BackgroundColor(INPUT_BG),
                BorderColor::all(INPUT_BORDER),
                UiInputCapture,
                SolverValueInput(field),
                Name::new(name),
            ));
        });
}

pub(crate) fn solver_numeric_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut input_focus: ResMut<InputFocus>,
    mut keyboard_state: ResMut<UiKeyboardState>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut state: ResMut<SolverEditorState>,
    mut inputs: Query<(
        Entity,
        &SolverValueInput,
        &mut EditableText,
        &mut BorderColor,
    )>,
    mut statuses: Query<&mut Text, (With<SolverInputStatus>, Without<SolverControlGroup>)>,
    mut groups: Query<(&SolverControlGroup, &mut Node, Option<&mut Text>)>,
) {
    for (group, mut node, summary) in &mut groups {
        let visible = match group {
            SolverControlGroup::Linear => setup.solver.solver_method.is_iterative(),
            SolverControlGroup::Field(field) => field.active(&setup.solver),
            SolverControlGroup::Nonlinear => {
                setup.solver.analysis_type == AnalysisType::NlStatic
            }
            SolverControlGroup::DirectHint => !setup.solver.solver_method.is_iterative(),
            SolverControlGroup::Eigen => {
                setup.solver.analysis_type == AnalysisType::Eigen
            }
            SolverControlGroup::Dynamic => SolverField::DynamicSubsteps.active(&setup.solver),
            SolverControlGroup::DynamicStep => {
                if let Some(step) = setup.solver.dynamic_step.as_ref()
                    && setup.solver.analysis_type == AnalysisType::Dynamic
                {
                    if let Some(mut text) = summary
                        && (setup.is_changed() || text.0.is_empty())
                    {
                        let value = dynamic_step_summary(step);
                        if text.0 != value {
                            text.0 = value;
                        }
                    }
                    true
                } else {
                    false
                }
            }
        };
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let focused_entity = input_focus.get();
    if !focused_entity.is_some_and(|entity| inputs.contains(entity)) {
        state.error = None;
    }

    for (entity, field, mut input, mut border) in &mut inputs {
        let active = field.0.active(&setup.solver);
        if !active && focused_entity == Some(entity) {
            input_focus.clear();
            state.error = None;
        }
        let focused = active && focused_entity == Some(entity);
        let current = editable_value(&input);

        if focused {
            if keys.just_pressed(KeyCode::Escape) {
                let restored = solver_value_text(&setup.solver, field.0);
                input.editor_mut().set_text(&restored);
                input.queue_edit(bevy::text::TextEdit::TextEnd(false));
                state.error = None;
                input_focus.clear();
                keyboard_state.text_editing = true;
            } else {
                match parse_solver_value(field.0, &current) {
                    Ok(value) => {
                        state.error = None;
                        if keys.just_pressed(KeyCode::Enter) && !input.is_composing() {
                            apply_solver_value(&mut setup.solver, field.0, value);
                            input_focus.clear();
                            keyboard_state.text_editing = true;
                        }
                    }
                    Err(message) => state.error = Some((field.0, message)),
                }
            }
        } else {
            let desired = solver_value_text(&setup.solver, field.0);
            if current != desired {
                input.editor_mut().set_text(&desired);
                input.queue_edit(bevy::text::TextEdit::TextEnd(false));
            }
        }

        let field_has_error = state
            .error
            .as_ref()
            .is_some_and(|(error_field, _)| *error_field == field.0);
        *border = BorderColor::all(if field_has_error {
            INPUT_ERROR_BORDER
        } else if focused {
            INPUT_FOCUS_BORDER
        } else {
            INPUT_BORDER
        });
    }

    if let Ok(mut status) = statuses.single_mut() {
        **status = state
            .error
            .as_ref()
            .map(|(field, message)| format!("{}: {message}; Esc restores", field.label()))
            .unwrap_or_else(|| "Enter applies an exact value; Esc restores".to_string());
    }
}

fn solver_value_text(settings: &SolverSettings, field: SolverField) -> String {
    match field {
        SolverField::Substeps => settings.substeps.to_string(),
        SolverField::MaxIterations => format_optional_count(settings.max_iterations),
        SolverField::ConvergenceTolerance => format_optional_real(settings.convergence_tol),
        SolverField::PreconditionerIterations => format_optional_count(settings.preconditioner_iterations),
        SolverField::GmresSubspaces => format_optional_count(settings.gmres_subspaces),
        SolverField::OrderingColors => format_optional_count(settings.ordering_colors),
        SolverField::PreconditionerReuse => format_optional_count(settings.preconditioner_reuse),
        SolverField::DiagonalScale => format_optional_real(settings.diagonal_scale),
        SolverField::SsorRelaxation => format_optional_real(settings.ssor_relaxation),
        SolverField::NonlinearMaxIterations => settings.nonlinear_max_iterations.to_string(),
        SolverField::NonlinearConvergenceTolerance => {
            format!("{:.8e}", settings.nonlinear_convergence_tol)
        }
        SolverField::EigenModes => settings.eigen.mode_count.to_string(),
        SolverField::EigenMaxIterations => settings.eigen.max_iterations.to_string(),
        SolverField::EigenTolerance => format!("{:.16e}", settings.eigen.tolerance),
        SolverField::EigenShift => format!("{:.16e}", settings.eigen.shift),
        SolverField::DynamicSubsteps => format_optional_count(settings.dynamic.as_ref().map(|d| d.fallback_substeps)),
        SolverField::DynamicIncrement => format_optional_real(settings.dynamic.as_ref().map(|d| d.fallback_increment)),
        SolverField::NewmarkGamma => format_optional_real(settings.dynamic.as_ref().map(|d| d.gamma)),
        SolverField::NewmarkBeta => format_optional_real(settings.dynamic.as_ref().map(|d| d.beta)),
    }
}

fn dynamic_step_summary(step: &DynamicStep) -> String {
    let substeps = format_optional_count(step.substeps);
    let iterations = format_optional_count(step.max_iterations);
    let convergence = format_optional_real(step.convergence);
    let default = |value: String| {
        if value.is_empty() {
            "solver default".to_owned()
        } else {
            value
        }
    };
    let boundary = step
        .groups
        .iter()
        .filter_map(|group| match group {
            StepGroup::Boundary(id) => Some(id.to_string()),
            StepGroup::Load(_) => None,
        })
        .collect::<Vec<_>>();
    let load = step
        .groups
        .iter()
        .filter_map(|group| match group {
            StepGroup::Boundary(_) => None,
            StepGroup::Load(id) => Some(id.to_string()),
        })
        .collect::<Vec<_>>();
    let group_list = |ids: Vec<String>| {
        if ids.is_empty() {
            "none listed".to_owned()
        } else {
            ids.join(", ")
        }
    };
    format!(
        "IMPORTED !STEP | fixed increment\nDTIME {:e} | ETIME {:e}\nSUBSTEPS {} | MAXITER {}\nCONVERG {}\nBOUNDARY groups: {}\nLOAD groups: {}",
        step.increment,
        step.duration,
        default(substeps),
        default(iterations),
        default(convergence),
        group_list(boundary),
        group_list(load),
    )
}

fn format_optional_count(value: Option<u32>) -> String {
    value.map(|count| count.to_string()).unwrap_or_default()
}

fn format_optional_real(value: Option<f64>) -> String {
    value.map(|number| format!("{number:e}")).unwrap_or_default()
}

fn parse_solver_value(field: SolverField, text: &str) -> Result<ParsedSolverValue, String> {
    let text = text.trim();
    let optional = field.optional();
    if field.integer() {
        if optional && text.is_empty() {
            return Ok(ParsedSolverValue::OptionalInteger(None));
        }
        let minimum = u32::from(field != SolverField::PreconditionerReuse);
        let value = text.parse::<u32>().ok()
            .filter(|value| (minimum..=i32::MAX as u32).contains(value))
            .ok_or_else(|| format!("enter a whole number from {minimum} to {}", i32::MAX))?;
        return Ok(if optional {
            ParsedSolverValue::OptionalInteger(Some(value))
        } else {
            ParsedSolverValue::Integer(value)
        });
    }

    if field == SolverField::NonlinearConvergenceTolerance {
        let value = text
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or("enter a finite positive number")?;
        return Ok(ParsedSolverValue::Float(value));
    }
    if optional && text.is_empty() {
        return Ok(ParsedSolverValue::OptionalDouble(None));
    }
    let value = text.parse::<f64>().ok().filter(|value| value.is_finite())
        .ok_or("enter a finite number")?;
    if matches!(field, SolverField::EigenTolerance | SolverField::ConvergenceTolerance)
        && value <= 0.0
    {
        return Err("tolerance must be finite and positive".into());
    }
    if matches!(field, SolverField::DynamicIncrement | SolverField::NewmarkBeta) && value <= 0.0 {
        return Err("enter a finite positive number".into());
    }
    Ok(if optional {
        ParsedSolverValue::OptionalDouble(Some(value))
    } else {
        ParsedSolverValue::Double(value)
    })
}

fn apply_solver_value(
    settings: &mut SolverSettings,
    field: SolverField,
    value: ParsedSolverValue,
) {
    match (field, value) {
        (SolverField::Substeps, ParsedSolverValue::Integer(value)) => settings.substeps = value,
        (SolverField::MaxIterations, ParsedSolverValue::OptionalInteger(value)) => {
            settings.max_iterations = value;
        }
        (SolverField::ConvergenceTolerance, ParsedSolverValue::OptionalDouble(value)) => {
            settings.convergence_tol = value;
        }
        (SolverField::PreconditionerIterations, ParsedSolverValue::OptionalInteger(value)) => {
            settings.preconditioner_iterations = value;
        }
        (SolverField::GmresSubspaces, ParsedSolverValue::OptionalInteger(value)) => {
            settings.gmres_subspaces = value;
        }
        (SolverField::OrderingColors, ParsedSolverValue::OptionalInteger(value)) => {
            settings.ordering_colors = value;
        }
        (SolverField::PreconditionerReuse, ParsedSolverValue::OptionalInteger(value)) => {
            settings.preconditioner_reuse = value;
        }
        (SolverField::DiagonalScale, ParsedSolverValue::OptionalDouble(value)) => {
            settings.diagonal_scale = value;
        }
        (SolverField::SsorRelaxation, ParsedSolverValue::OptionalDouble(value)) => {
            settings.ssor_relaxation = value;
        }
        (SolverField::NonlinearMaxIterations, ParsedSolverValue::Integer(value)) => {
            settings.nonlinear_max_iterations = value
        }
        (SolverField::NonlinearConvergenceTolerance, ParsedSolverValue::Float(value)) => {
            settings.nonlinear_convergence_tol = value
        }
        (SolverField::EigenModes, ParsedSolverValue::Integer(value)) => {
            settings.eigen.mode_count = value
        }
        (SolverField::EigenMaxIterations, ParsedSolverValue::Integer(value)) => {
            settings.eigen.max_iterations = value
        }
        (SolverField::EigenTolerance, ParsedSolverValue::Double(value)) => {
            settings.eigen.tolerance = value
        }
        (SolverField::EigenShift, ParsedSolverValue::Double(value)) => settings.eigen.shift = value,
        (SolverField::DynamicSubsteps, ParsedSolverValue::Integer(value)) => {
            settings.dynamic.as_mut().unwrap().fallback_substeps = value;
        }
        (SolverField::DynamicIncrement, ParsedSolverValue::Double(value)) => {
            settings.dynamic.as_mut().unwrap().fallback_increment = value;
        }
        (SolverField::NewmarkGamma, ParsedSolverValue::Double(value)) => {
            settings.dynamic.as_mut().unwrap().gamma = value;
        }
        (SolverField::NewmarkBeta, ParsedSolverValue::Double(value)) => {
            settings.dynamic.as_mut().unwrap().beta = value;
        }
        _ => unreachable!("parsed solver value must match its field"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver_value_parser_enforces_integer_and_positive_domains() {
        assert_eq!(
            parse_solver_value(SolverField::Substeps, "12"),
            Ok(ParsedSolverValue::Integer(12))
        );
        assert!(parse_solver_value(SolverField::Substeps, "1.5").is_err());
        assert!(parse_solver_value(SolverField::MaxIterations, "0").is_err());
        assert_eq!(
            parse_solver_value(SolverField::ConvergenceTolerance, "1e-8"),
            Ok(ParsedSolverValue::OptionalDouble(Some(1.0e-8)))
        );
        assert!(parse_solver_value(SolverField::ConvergenceTolerance, "-1e-8").is_err());
    }

    #[test]
    fn gmres_field_edits_the_optional_subspace_count() {
        use fem_core::LinearSolverMethod::{Cg, Gmres};

        let field = SolverField::GmresSubspaces;
        let mut settings = fem_core::SolverSettings::default();
        settings.ordering_colors = Some(100);
        assert!(!field.active(&settings));
        settings.solver_method = Gmres;
        assert!(field.active(&settings));
        assert!(parse_solver_value(field, "0").is_err());
        apply_solver_value(&mut settings, field, parse_solver_value(field, "40").unwrap());
        settings.solver_method = Cg;
        assert!(!field.active(&settings));
        settings.solver_method = Gmres;
        assert_eq!(solver_value_text(&settings, field), "40");
        apply_solver_value(&mut settings, field, parse_solver_value(field, "").unwrap());
        assert_eq!(solver_value_text(&settings, field), "");
        assert_eq!(settings.ordering_colors, Some(100));
    }

    #[test]
    fn solve_preconditioner_fields_commit_and_round_trip_through_cnt() {
        let mut setup = fem_core::AnalysisSetup::default();
        setup.solver.analysis_type = AnalysisType::NlStatic;
        setup.solver.solver_method = LinearSolverMethod::Gmres;
        let mut app = App::new();
        app.insert_resource(setup)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<UiKeyboardState>()
            .init_resource::<SolverEditorState>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn_solver_exact_editor);
            })
            .add_systems(Update, solver_numeric_input_system);
        app.update();

        let diagonal_scale = 1.2345678901234567_f64;
        for (field, typed) in [
            (SolverField::MaxIterations, ""),
            (SolverField::ConvergenceTolerance, "1.2345678901234567e-25"),
            (SolverField::PreconditionerIterations, "2"),
            (SolverField::OrderingColors, "24"),
            (SolverField::PreconditionerReuse, "0"),
            (SolverField::SsorRelaxation, "1.125"),
            (SolverField::DiagonalScale, "1.2345678901234567"),
        ] {
            if field == SolverField::DiagonalScale {
                app.world_mut().resource_mut::<fem_core::AnalysisSetup>().solver.preconditioner = 3;
            }
            let input = app.world_mut().query::<(Entity, &SolverValueInput)>().iter(app.world())
                .find(|(_, input)| input.0 == field).unwrap().0;
            app.world_mut().get_mut::<EditableText>(input).unwrap().editor_mut().set_text(typed);
            app.world_mut().resource_mut::<InputFocus>()
                .set(input, bevy::input_focus::FocusCause::Navigated);
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::Enter);
            app.update();
            assert!(app.world().resource::<InputFocus>().get().is_none(), "{field:?}");
        }

        let setup = app.world().resource::<fem_core::AnalysisSetup>();
        assert_eq!(setup.solver.max_iterations, None);
        assert_eq!(setup.solver.convergence_tol, Some(1.2345678901234567e-25));
        assert_eq!(setup.solver.preconditioner_iterations, Some(2));
        assert_eq!(setup.solver.ordering_colors, Some(24));
        assert_eq!(setup.solver.preconditioner_reuse, Some(0));
        assert_eq!(setup.solver.diagonal_scale, Some(diagonal_scale));
        assert_eq!(setup.solver.ssor_relaxation, Some(1.125));
        assert_eq!(setup.solver.gmres_subspaces, None);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("solver.cnt");
        hecmw::cnt_writer::write_cnt_file(&path, setup).unwrap();
        let data = hecmw::load_cnt_file(&path, &fem_core::FemMesh::demo_hex8(), 0).unwrap();
        assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
        assert_eq!(data.solver.as_ref(), Some(&setup.solver));
    }

    #[test]
    fn parsed_values_update_only_the_requested_solver_setting() {
        let mut settings = fem_core::SolverSettings::default();
        apply_solver_value(
            &mut settings,
            SolverField::MaxIterations,
            ParsedSolverValue::OptionalInteger(Some(250)),
        );

        assert_eq!(settings.substeps, 1);
        assert_eq!(settings.max_iterations, Some(250));
        assert_eq!(settings.convergence_tol, Some(1.0e-6));
        assert_eq!(settings.nonlinear_max_iterations, 50);
        apply_solver_value(
            &mut settings,
            SolverField::NonlinearMaxIterations,
            ParsedSolverValue::Integer(37),
        );
        assert_eq!(settings.max_iterations, Some(250));
        assert_eq!(settings.nonlinear_max_iterations, 37);
    }

    #[test]
    fn tolerances_round_trip_without_display_rounding() {
        let mut settings = fem_core::SolverSettings::default();
        for (field, value) in [
            (
                SolverField::ConvergenceTolerance,
                ParsedSolverValue::OptionalDouble(Some(1.2345678901234567e-125)),
            ),
            (
                SolverField::ConvergenceTolerance,
                ParsedSolverValue::OptionalDouble(None),
            ),
            (
                SolverField::NonlinearConvergenceTolerance,
                ParsedSolverValue::Float(f32::MIN_POSITIVE),
            ),
        ] {
            apply_solver_value(&mut settings, field, value);
            assert_eq!(parse_solver_value(field, &solver_value_text(&settings, field)), Ok(value));
            for invalid in ["NaN", "inf", "0", "-1"] {
                assert!(parse_solver_value(field, invalid).is_err());
            }
        }
        assert!(parse_solver_value(SolverField::NonlinearMaxIterations, "4294967295").is_err());
    }

    #[test]
    fn nonlinear_editor_commits_on_enter_and_restores_on_escape() {
        let mut app = App::new();
        let mut setup = fem_core::AnalysisSetup::default();
        setup.solver.analysis_type = fem_core::AnalysisType::NlStatic;
        setup.solver.solver_method = fem_core::LinearSolverMethod::Mumps;
        app.insert_resource(setup)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<UiKeyboardState>()
            .init_resource::<SolverEditorState>()
            .add_systems(Update, solver_numeric_input_system);
        let input = app
            .world_mut()
            .spawn((
                SolverValueInput(SolverField::NonlinearMaxIterations),
                EditableText::new("37"),
                BorderColor::all(INPUT_BORDER),
            ))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, bevy::input_focus::FocusCause::Navigated);
        app.update();
        assert_eq!(
            app.world()
                .resource::<fem_core::AnalysisSetup>()
                .solver
                .nonlinear_max_iterations,
            50
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            app.world()
                .resource::<fem_core::AnalysisSetup>()
                .solver
                .nonlinear_max_iterations,
            37
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text("99");
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, bevy::input_focus::FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(
            editable_value(app.world().get::<EditableText>(input).unwrap()),
            "37"
        );
        assert_eq!(
            app.world()
                .resource::<fem_core::AnalysisSetup>()
                .solver
                .max_iterations,
            Some(100)
        );
        assert_eq!(
            app.world()
                .resource::<fem_core::AnalysisSetup>()
                .solver
                .nonlinear_max_iterations,
            37
        );
    }

    #[test]
    fn hidden_controls_discard_pending_enter_and_restore_stored_value() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<UiKeyboardState>()
            .init_resource::<fem_core::AnalysisSetup>()
            .init_resource::<SolverEditorState>()
            .add_systems(Update, solver_numeric_input_system);
        let linear = app
            .world_mut()
            .spawn((Node::default(), SolverControlGroup::Linear))
            .id();
        let nonlinear = app
            .world_mut()
            .spawn((Node::default(), SolverControlGroup::Nonlinear))
            .id();
        let direct = app
            .world_mut()
            .spawn((Node::default(), SolverControlGroup::DirectHint))
            .id();
        let input = app
            .world_mut()
            .spawn((
                SolverValueInput(SolverField::MaxIterations),
                EditableText::new("999"),
                BorderColor::all(INPUT_BORDER),
            ))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, bevy::input_focus::FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.world_mut()
            .resource_mut::<fem_core::AnalysisSetup>()
            .solver
            .solver_method = fem_core::LinearSolverMethod::Mumps;
        app.update();
        assert_eq!(
            app.world()
                .resource::<fem_core::AnalysisSetup>()
                .solver
                .max_iterations,
            Some(100)
        );
        assert!(app.world().resource::<InputFocus>().get().is_none());
        assert_eq!(
            editable_value(app.world().get::<EditableText>(input).unwrap()),
            "100"
        );
        assert_eq!(
            app.world().get::<Node>(linear).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(nonlinear).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(direct).unwrap().display,
            Display::Flex
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        {
            let mut setup = app.world_mut().resource_mut::<fem_core::AnalysisSetup>();
            setup.solver.analysis_type = fem_core::AnalysisType::NlStatic;
            setup.solver.solver_method = fem_core::LinearSolverMethod::Cg;
        }
        app.update();
        assert_eq!(
            app.world().get::<Node>(linear).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(nonlinear).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(direct).unwrap().display,
            Display::None
        );
    }
}
