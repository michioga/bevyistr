//! Discrete solver choices: click to commit, native menu dismissal to cancel.
use crate::layout::{ScrollableList, SidebarPage, UiInputCapture};
use bevy::{
    input_focus::{
        FocusCause, InputFocus,
        tab_navigation::{NavAction, TabIndex},
    },
    picking::{Pickable, hover::Hovered},
    prelude::*,
    ui::ScrollPosition,
    ui_widgets::{
        Activate, Button as WidgetButton, MenuAction, MenuButton, MenuEvent, MenuFocusState,
        MenuItem, MenuPopup,
        popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    },
};
use fem_core::{AnalysisSetup, AnalysisType, LinearSolverMethod, SolverSettings};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Analysis,
    Method,
}
#[derive(Component)]
struct Label(Kind);
#[derive(Component)]
struct Selector;
#[derive(Component)]
struct Popup {
    original: SolverSettings,
}
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Analysis(AnalysisType),
    Method(LinearSolverMethod),
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Self::Analysis => "Analysis type",
            Self::Method => "Linear solver",
        }
    }
    fn current(self, settings: &SolverSettings) -> Choice {
        match self {
            Self::Analysis => Choice::Analysis(settings.analysis_type),
            Self::Method => Choice::Method(settings.solver_method),
        }
    }
    fn choices(self) -> Vec<Choice> {
        match self {
            Self::Analysis => [
                AnalysisType::Static,
                AnalysisType::NlStatic,
                AnalysisType::Dynamic,
                AnalysisType::Eigen,
            ]
            .map(Choice::Analysis)
            .to_vec(),
            Self::Method => [
                LinearSolverMethod::Mumps,
                LinearSolverMethod::Cg,
                LinearSolverMethod::Gmres,
                LinearSolverMethod::Direct,
            ]
            .map(Choice::Method)
            .to_vec(),
        }
    }
}
impl Choice {
    fn label(self) -> &'static str {
        match self {
            Self::Analysis(a) => a.label(),
            Self::Method(m) => m.label(),
        }
    }
    fn keyword(self) -> String {
        match self {
            Self::Analysis(AnalysisType::NlStatic) => "!SOLUTION, TYPE=STATIC, NONLINEAR".into(),
            Self::Analysis(a) => format!("!SOLUTION, TYPE={}", a.frontistr_type()),
            Self::Method(m) => format!("!SOLVER, METHOD={}", m.frontistr_method()),
        }
    }
    fn selected(self, settings: &SolverSettings) -> bool {
        match self {
            Self::Analysis(a) => settings.analysis_type == a,
            Self::Method(m) => settings.solver_method == m,
        }
    }
}
fn text(value: impl Into<String>) -> impl Bundle {
    (
        Text::new(value),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    for kind in [Kind::Analysis, Kind::Method] {
        parent
            .spawn((
                kind,
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ))
            .observe(menu_event)
            .with_children(|anchor| {
                anchor
                    .spawn((
                        Button,
                        WidgetButton,
                        MenuButton,
                        Selector,
                        TabIndex(0),
                        crate::popup_trigger::node(),
                        crate::popup_trigger::bundle(),
                        BackgroundColor(Color::srgb(0.14, 0.30, 0.37)),
                    ))
                    .with_children(|button| crate::popup_trigger::content(button,
                        (text(kind.title()), Label(kind)), "Click to choose a value. Enter opens; Esc closes."));
            });
    }
    parent.spawn(text(
        "Choose a value to apply. Ctrl+Z undoes. Launch Direct / MPI below is separate.",
    ));
}

fn menu_event(
    event: On<MenuEvent>,
    menus: Query<(Entity, &Kind, &Children)>,
    parents: Query<&ChildOf>,
    popups: Query<Entity, With<Popup>>,
    buttons: Query<(), With<Selector>>,
    setup: Res<AnalysisSetup>,
    page: Res<SidebarPage>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let Some((anchor, kind, children)) = std::iter::once(event.source)
        .chain(parents.iter_ancestors(event.source))
        .find_map(|e| menus.get(e).ok())
    else {
        return;
    };
    let existing = children.iter().find(|e| popups.contains(*e));
    match event.action {
        MenuAction::CloseAll => {
            if let Some(e) = existing {
                commands.entity(e).despawn();
            }
        }
        MenuAction::FocusRoot => {
            if *page == SidebarPage::Solve {
                if let Some(button) = children.iter().find(|e| buttons.contains(*e)) {
                    focus.set(button, FocusCause::Navigated);
                }
            }
        }
        MenuAction::Open(_) | MenuAction::Toggle => {
            if *page != SidebarPage::Solve {
                return;
            }
            if let Some(e) = existing {
                if matches!(event.action, MenuAction::Toggle) {
                    commands.entity(e).despawn();
                }
                return;
            }
            // Keep just one of our selectors open; other native menus also dismiss on focus loss.
            for e in &popups {
                commands.entity(e).despawn();
            }
            let nav = if let MenuAction::Open(nav) = event.action {
                nav
            } else {
                NavAction::First
            };
            commands
                .spawn((
                    ChildOf(anchor),
                    Popup {
                        original: setup.solver.clone(),
                    },
                    MenuPopup::default(),
                    MenuFocusState::Opening(nav),
                    GlobalZIndex(200),
                    OverrideClip,
                    UiInputCapture,
                    ScrollableList,
                    ScrollPosition::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(310),
                        max_width: Val::Vw(90.0),
                        max_height: Val::Vh(50.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::all(px(5)),
                        row_gap: px(3),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.07, 0.10, 0.12)),
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Top,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                        ],
                        window_margin: 10.0,
                    },
                ))
                .with_children(|popup| {
                    popup.spawn(text(format!(
                        "{} | click to apply; Esc cancels",
                        kind.title()
                    )));
                    for choice in kind.choices() {
                        popup
                            .spawn((
                                MenuItem,
                                choice,
                                Hovered::default(),
                                TabIndex(0),
                                Node {
                                    min_height: px(40),
                                    flex_shrink: 0.0,
                                    padding: UiRect::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.10, 0.14, 0.17)),
                            ))
                            .observe(choose)
                            .observe(crate::popup_keyboard::handle)
                            .with_child(text(format!(
                                "{} {}\n{}",
                                if choice.selected(&setup.solver) {
                                    "[x]"
                                } else {
                                    "[ ]"
                                },
                                choice.label(),
                                choice.keyword()
                            )));
                    }
                });
        }
    }
}

fn choose(
    event: On<Activate>,
    choices: Query<&Choice>,
    parents: Query<&ChildOf>,
    popups: Query<&Popup>,
    page: Res<SidebarPage>,
    mut setup: ResMut<AnalysisSetup>,
) {
    if *page != SidebarPage::Solve {
        return;
    }
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    let Some(popup) = parents
        .iter_ancestors(event.entity)
        .find_map(|e| popups.get(e).ok())
    else {
        return;
    };
    // Do not apply an old menu over a project load, undo or numeric edit.
    if setup.solver != popup.original || choice.selected(&setup.solver) {
        return;
    }
    match *choice {
        Choice::Analysis(value) => setup.solver.analysis_type = value,
        Choice::Method(value) => setup.solver.solver_method = value,
    }
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(
        Update,
        sync.after(crate::layout::sidebar_page_button_system)
            .after(crate::layout::undo_redo_system)
            .after(crate::solver_editor::solver_numeric_input_system).in_set(crate::popup_trigger::MenuSync),
    );
}
fn sync(
    page: Res<SidebarPage>,
    setup: Res<AnalysisSetup>,
    mut commands: Commands,
    popups: Query<(Entity, &Popup)>,
    mut labels: Query<(&Label, &mut Text)>,
    mut items: Query<(Entity, &Choice, &Hovered, &mut BackgroundColor)>,
    mut focus: ResMut<InputFocus>,
    menu_focus: Query<(), Or<(With<Selector>, With<Choice>)>>,
) {
    if *page != SidebarPage::Solve && focus.get().is_some_and(|e| menu_focus.contains(e)) {
        focus.clear();
    }
    for (entity, popup) in &popups {
        if *page != SidebarPage::Solve || setup.solver != popup.original {
            if *page == SidebarPage::Solve && focus.get().is_some_and(|e| menu_focus.contains(e)) {
                focus.clear();
            }
            commands.entity(entity).despawn();
        }
    }
    for (label, mut value) in &mut labels {
        value.set_if_neq(Text::new(format!(
            "{}: {}",
            label.0.title(),
            label.0.current(&setup.solver).label()
        )));
    }
    for (entity, choice, hovered, mut color) in &mut items {
        *color = BackgroundColor(if hovered.0 || focus.get() == Some(entity) {
            Color::srgb(0.22, 0.40, 0.47)
        } else if choice.selected(&setup.solver) {
            Color::srgb(0.18, 0.45, 0.55)
        } else {
            Color::srgb(0.10, 0.14, 0.17)
        });
    }
}

#[cfg(test)]
#[path = "solver_menu_tests.rs"]
mod tests;
