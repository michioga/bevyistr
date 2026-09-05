//! Asynchronous FrontISTR process execution and its Solve-page controls.
//!
//! The process is deliberately kept outside Bevy's main thread.  A successful
//! export establishes the working directory and project stem; Run rewrites the
//! current model/setup to that target before starting `fistr1` there.

use crate::layout::{SidebarPage, SidebarPageContent};
use crate::solver_process::{
    ProcessOutputStream, RuntimeEnvironment, SolverLaunchMode, SolverProcessConfig,
    SolverProcessEvent, SolverProcessHandle, spawn_solver_process,
};
use bevy::prelude::*;
use fem_core::{AnalysisSetup, FemModel};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_DISABLED: Color = Color::srgba(0.08, 0.09, 0.10, 0.72);
const RUN_NORMAL: Color = Color::srgb(0.10, 0.32, 0.18);
const RUN_HOVERED: Color = Color::srgb(0.14, 0.48, 0.24);
const STOP_NORMAL: Color = Color::srgb(0.38, 0.12, 0.12);
const STOP_HOVERED: Color = Color::srgb(0.52, 0.16, 0.16);
const MAX_LOG_LINES: usize = 14;
const MAX_LOG_CHARS: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrontistrProjectTarget {
    pub(crate) directory: PathBuf,
    pub(crate) stem: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolverRunPhase {
    Idle,
    Ready,
    Running,
    Succeeded,
    Failed,
    Stopped,
}

impl SolverRunPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::Succeeded => "Completed",
            Self::Failed => "Failed",
            Self::Stopped => "Stopped",
        }
    }
}

#[derive(Resource)]
pub(crate) struct FrontistrRunState {
    executable: PathBuf,
    runtime_environment: RuntimeEnvironment,
    launch_mode: SolverLaunchMode,
    mpi_ranks: u16,
    mpi_launcher: Option<PathBuf>,
    project: Option<FrontistrProjectTarget>,
    phase: SolverRunPhase,
    message: String,
    log_lines: VecDeque<String>,
    started_at: Option<Instant>,
    elapsed: Duration,
    process: Option<SolverProcessHandle>,
    stop_requested: bool,
}

impl Default for FrontistrRunState {
    fn default() -> Self {
        let executable = std::env::var_os("FRONTISTR_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("fistr1"));
        let launch_mode = match std::env::var("FRONTISTR_LAUNCH_MODE") {
            Ok(value) if value.eq_ignore_ascii_case("mpi") => SolverLaunchMode::Mpi,
            _ => SolverLaunchMode::Direct,
        };
        let mpi_ranks = std::env::var("FRONTISTR_MPI_RANKS")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|ranks| (1..=4096).contains(ranks))
            .unwrap_or(4);
        Self {
            executable,
            runtime_environment: RuntimeEnvironment::detect(),
            launch_mode,
            mpi_ranks,
            mpi_launcher: std::env::var_os("FRONTISTR_MPI_LAUNCHER").map(PathBuf::from),
            project: None,
            phase: SolverRunPhase::Idle,
            message: "Export a project to establish the run folder.".to_string(),
            log_lines: VecDeque::new(),
            started_at: None,
            elapsed: Duration::ZERO,
            process: None,
            stop_requested: false,
        }
    }
}

impl FrontistrRunState {
    pub(crate) fn is_running(&self) -> bool {
        self.phase == SolverRunPhase::Running
    }

    pub(crate) fn register_export(&mut self, directory: PathBuf, stem: String) {
        self.project = Some(FrontistrProjectTarget { directory, stem });
        if !self.is_running() {
            self.phase = SolverRunPhase::Ready;
            self.message = "Export is current; ready to run FrontISTR.".to_string();
        }
    }

    pub(crate) fn clear_export_target(&mut self) {
        self.project = None;
        if self.is_running() {
            return;
        }
        self.phase = SolverRunPhase::Idle;
        self.message = "Export the current project before running FrontISTR.".to_string();
        self.log_lines.clear();
        self.elapsed = Duration::ZERO;
    }

    fn project(&self) -> Option<&FrontistrProjectTarget> {
        self.project.as_ref()
    }

    fn set_executable(&mut self, executable: PathBuf) {
        if !self.is_running() {
            self.executable = executable;
            self.message = "FrontISTR executable selected.".to_string();
        }
    }

    fn set_launch_mode(&mut self, mode: SolverLaunchMode) {
        if self.is_running() {
            return;
        }
        self.launch_mode = mode;
        self.message = match mode {
            SolverLaunchMode::Direct => "Direct launch selected (one process).".to_string(),
            SolverLaunchMode::Mpi => {
                format!("MPI launch selected ({} ranks).", self.mpi_ranks)
            }
        };
    }

    fn adjust_mpi_ranks(&mut self, delta: i16) {
        if self.is_running() {
            return;
        }
        self.mpi_ranks = (i32::from(self.mpi_ranks) + i32::from(delta)).clamp(1, 4096) as u16;
        self.message = format!("MPI process count set to {} ranks.", self.mpi_ranks);
    }

    fn launch_label(&self) -> String {
        match self.launch_mode {
            SolverLaunchMode::Direct => "Direct (1 process)".to_string(),
            SolverLaunchMode::Mpi => {
                let launcher = self
                    .mpi_launcher
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "auto: mpiexec / mpirun".to_string());
                format!("MPI ({} ranks, {launcher})", self.mpi_ranks)
            }
        }
    }

    fn report_preflight_error(&mut self, message: impl Into<String>) {
        self.phase = SolverRunPhase::Failed;
        self.message = message.into();
        self.started_at = None;
        self.elapsed = Duration::ZERO;
    }

    fn start(&mut self) -> Result<(), String> {
        let target = self
            .project
            .as_ref()
            .ok_or_else(|| "Export the project before running FrontISTR.".to_string())?;
        if self.is_running() {
            return Err("FrontISTR is already running.".to_string());
        }
        let process = spawn_solver_process(SolverProcessConfig {
            executable: self.executable.clone(),
            project_stem: target.stem.clone(),
            partitioner: std::env::var_os("FRONTISTR_PARTITIONER").map(PathBuf::from),
            working_directory: target.directory.clone(),
            environment: self.runtime_environment.clone(),
            launch_mode: self.launch_mode,
            mpi_ranks: self.mpi_ranks,
            mpi_launcher: self.mpi_launcher.clone(),
        })?;

        self.phase = SolverRunPhase::Running;
        self.message = "Preparing FrontISTR run...".to_string();
        self.log_lines.clear();
        self.started_at = Some(Instant::now());
        self.elapsed = Duration::ZERO;
        self.process = Some(process);
        self.stop_requested = false;
        Ok(())
    }

    fn request_stop(&mut self) {
        if !self.is_running() || self.stop_requested {
            return;
        }
        if let Some(process) = &self.process {
            process.request_stop();
        }
        self.stop_requested = true;
        self.message = "Stopping FrontISTR...".to_string();
    }

    fn poll(&mut self) {
        if self.is_running() {
            if let Some(started_at) = self.started_at {
                self.elapsed = started_at.elapsed();
            }
        }

        let pending = self
            .process
            .as_ref()
            .map(SolverProcessHandle::poll)
            .unwrap_or_default();

        let mut terminal = false;
        for event in pending {
            match event {
                SolverProcessEvent::Stage(stage) => {
                    if !self.stop_requested {
                        self.message = stage;
                    }
                }
                SolverProcessEvent::Output(stream, line) => self.append_log(stream, line),
                SolverProcessEvent::SpawnFailed(error) => {
                    self.phase = SolverRunPhase::Failed;
                    self.message = error;
                    terminal = true;
                }
                SolverProcessEvent::Finished(code) => {
                    if code == Some(0) {
                        self.phase = SolverRunPhase::Succeeded;
                        self.message =
                            "FrontISTR exited with code 0. Review solver output and results."
                                .to_string();
                    } else {
                        self.phase = SolverRunPhase::Failed;
                        self.message = match code {
                            Some(code) => format!("FrontISTR exited with code {code}."),
                            None => "FrontISTR ended without an exit code.".to_string(),
                        };
                    }
                    terminal = true;
                }
                SolverProcessEvent::Stopped => {
                    self.phase = SolverRunPhase::Stopped;
                    self.message = "FrontISTR was stopped by the user.".to_string();
                    terminal = true;
                }
            }
        }

        if terminal {
            if let Some(started_at) = self.started_at.take() {
                self.elapsed = started_at.elapsed();
            }
            self.process = None;
            self.stop_requested = false;
        }
    }

    fn append_log(&mut self, stream: ProcessOutputStream, line: String) {
        let mut shortened = line.chars().take(MAX_LOG_CHARS).collect::<String>();
        if line.chars().count() > MAX_LOG_CHARS {
            shortened.push('…');
        }
        if matches!(stream, ProcessOutputStream::Stderr) {
            // HEC-MW also reports routine partition progress on stderr.
            shortened.insert_str(0, "stderr  ");
        }
        self.log_lines.push_back(shortened);
        while self.log_lines.len() > MAX_LOG_LINES {
            self.log_lines.pop_front();
        }
    }

    fn executable_label(&self) -> String {
        if self.executable.components().count() == 1 {
            format!("{} (PATH)", self.executable.display())
        } else {
            self.executable.display().to_string()
        }
    }

    fn environment_label(&self) -> String {
        self.runtime_environment.label()
    }

    fn project_label(&self) -> String {
        self.project
            .as_ref()
            .map(|target| {
                target
                    .directory
                    .join(format!("{}.*", target.stem))
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| "Export a project first".to_string())
    }

    fn status_label(&self) -> String {
        let elapsed = if matches!(
            self.phase,
            SolverRunPhase::Running
                | SolverRunPhase::Succeeded
                | SolverRunPhase::Failed
                | SolverRunPhase::Stopped
        ) && self.elapsed > Duration::ZERO
        {
            format!("  {:.1}s", self.elapsed.as_secs_f32())
        } else {
            String::new()
        };
        format!("Status: {}{elapsed}\n{}", self.phase.label(), self.message)
    }

    fn log_label(&self) -> String {
        if self.log_lines.is_empty() {
            "Solver output appears here.".to_string()
        } else {
            self.log_lines
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

#[derive(Component)]
pub(crate) struct SelectFrontistrExecutableButton;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SolverLaunchModeButton(pub(crate) SolverLaunchMode);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MpiRankAdjustButton(pub(crate) i16);

#[derive(Component)]
pub(crate) struct MpiRankText;

#[derive(Component)]
pub(crate) struct MpiRankControls;

#[derive(Component)]
pub(crate) struct RunFrontistrButton;

#[derive(Component)]
pub(crate) struct StopFrontistrButton;

#[derive(Component)]
pub(crate) struct FrontistrExecutableText;

#[derive(Component)]
pub(crate) struct FrontistrProjectText;

#[derive(Component)]
pub(crate) struct FrontistrStatusText;

#[derive(Component)]
pub(crate) struct FrontistrLogText;

pub(crate) fn spawn_solver_execution_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                margin: UiRect::top(px(6.0)),
                padding: UiRect::all(px(6.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.20, 0.48, 0.34, 0.62)),
            SidebarPageContent::page(SidebarPage::Solve),
            Name::new("FrontistrExecutionPanel"),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("FRONTISTR EXECUTION"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(Color::srgba(0.55, 0.88, 0.68, 0.94)),
            ));

            panel
                .spawn((
                    Button,
                    Node {
                        height: px(26.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    SelectFrontistrExecutableButton,
                    Name::new("SelectFrontistrExecutableButton"),
                ))
                .with_child((
                    Text::new("Choose fistr1 executable..."),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));

            panel.spawn((
                Text::new(
                    "Executable: fistr1 (PATH)\nRuntime: detecting environment...\nLaunch: Direct (1 process)",
                ),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                FrontistrExecutableText,
            ));

            panel
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(5.0),
                    ..default()
                },))
                .with_children(|row| {
                    for mode in [SolverLaunchMode::Direct, SolverLaunchMode::Mpi] {
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(24.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            SolverLaunchModeButton(mode),
                            Name::new(format!("SolverLaunchMode_{}", mode.label())),
                        ))
                        .with_child((
                            Text::new(mode.label()),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });

            panel
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(5.0),
                        ..default()
                    },
                    MpiRankControls,
                    Name::new("MpiRankControls"),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new("MPI ranks"),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(TEXT_MUTED),
                    ));
                    for (delta, label) in [(-1, "−"), (1, "+")] {
                        if delta > 0 {
                            row.spawn((
                                Text::new("4"),
                                Node {
                                    width: px(48.0),
                                    height: px(24.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(px(4.0)),
                                    ..default()
                                },
                                BorderColor::all(PANEL_BORDER),
                                TextFont {
                                    font_size: FontSize::Px(10.5),
                                    ..default()
                                },
                                TextColor(TEXT_MAIN),
                                MpiRankText,
                            ));
                        }
                        row.spawn((
                            Button,
                            Node {
                                width: px(34.0),
                                height: px(24.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            MpiRankAdjustButton(delta),
                            Name::new(if delta < 0 {
                                "DecreaseMpiRanks"
                            } else {
                                "IncreaseMpiRanks"
                            }),
                        ))
                        .with_child((
                            Text::new(label),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });

            panel.spawn((
                Text::new("Project: export a project first"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                FrontistrProjectText,
            ));

            panel
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },))
                .with_children(|row| {
                    row.spawn((
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
                        BackgroundColor(BUTTON_DISABLED),
                        BorderColor::all(Color::srgb(0.15, 0.50, 0.28)),
                        RunFrontistrButton,
                        Name::new("RunFrontistrButton"),
                    ))
                    .with_child((
                        Text::new("Run FrontISTR"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.75, 0.97, 0.80)),
                    ));

                    row.spawn((
                        Button,
                        Node {
                            width: px(72.0),
                            height: px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_DISABLED),
                        BorderColor::all(Color::srgb(0.48, 0.18, 0.18)),
                        StopFrontistrButton,
                        Name::new("StopFrontistrButton"),
                    ))
                    .with_child((
                        Text::new("Stop"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.98, 0.76, 0.76)),
                    ));
                });

            panel.spawn((
                Text::new("Status: Idle\nExport a project to establish the run folder."),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                FrontistrStatusText,
            ));
            panel.spawn((
                Text::new(
                    "MPI: write partition controls, run hecmw_part1, then solve with N ranks. Direct: solve the entire mesh. Run refreshes exported inputs.",
                ),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            panel.spawn((
                Text::new("Solver output appears here."),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(Color::srgba(0.58, 0.72, 0.62, 0.88)),
                FrontistrLogText,
            ));
        });
}

pub(crate) fn select_frontistr_executable_system(
    mut state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<SelectFrontistrExecutableButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        let enabled = !state.is_running();
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
            let dialog = rfd::FileDialog::new().set_title("Choose FrontISTR executable");
            #[cfg(windows)]
            let dialog = dialog.add_filter("FrontISTR executable", &["exe"]);
            let dialog = dialog.add_filter("All files", &["*"]);
            if let Some(path) = dialog.pick_file() {
                state.set_executable(path);
            }
        }
        *background = BackgroundColor(ordinary_button_color(*interaction, enabled));
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn solver_launch_mode_button_system(
    mut state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &SolverLaunchModeButton,
        ),
        With<SolverLaunchModeButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        let enabled = !state.is_running();
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
            state.set_launch_mode(button.0);
        }
        let active = state.launch_mode == button.0;
        *background = BackgroundColor(if !enabled {
            BUTTON_DISABLED
        } else if active {
            match *interaction {
                Interaction::Pressed => BUTTON_PRESSED,
                Interaction::Hovered | Interaction::None => BUTTON_ACTIVE,
            }
        } else {
            ordinary_button_color(*interaction, true)
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn mpi_rank_adjust_button_system(
    mut state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &MpiRankAdjustButton,
        ),
        With<MpiRankAdjustButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        let enabled = state.launch_mode == SolverLaunchMode::Mpi && !state.is_running();
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
            state.adjust_mpi_ranks(button.0);
        }
        *background = BackgroundColor(ordinary_button_color(*interaction, enabled));
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn update_mpi_rank_controls_system(
    state: Res<FrontistrRunState>,
    mut controls: Query<&mut Node, With<MpiRankControls>>,
    mut labels: Query<&mut Text, With<MpiRankText>>,
) {
    if let Ok(mut node) = controls.single_mut() {
        let display = if state.launch_mode == SolverLaunchMode::Mpi {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    if let Ok(mut text) = labels.single_mut() {
        text.set_if_neq(Text::new(state.mpi_ranks.to_string()));
    }
}

pub(crate) fn run_frontistr_button_system(
    model: Option<Res<FemModel>>,
    setup: Res<AnalysisSetup>,
    mut state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<RunFrontistrButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        let enabled = state.project().is_some() && !state.is_running();
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(model) = model.as_deref() else {
                state.report_preflight_error("No mesh is loaded.");
                continue;
            };
            let Some(target) = state.project().cloned() else {
                continue;
            };

            let validation = hecmw::validate_frontistr_project(model, &setup);
            if validation.has_errors() {
                state.report_preflight_error(validation.summary(5));
                continue;
            }
            if let Err(error) =
                hecmw::write_frontistr_project(&target.directory, &target.stem, model, &setup)
            {
                state.report_preflight_error(format!("Could not refresh solver input: {error}"));
                continue;
            }
            if let Err(error) = state.start() {
                state.report_preflight_error(error);
            }
        }

        *background = BackgroundColor(if !enabled {
            BUTTON_DISABLED
        } else {
            match *interaction {
                Interaction::Pressed => BUTTON_PRESSED,
                Interaction::Hovered => RUN_HOVERED,
                Interaction::None => RUN_NORMAL,
            }
        });
        *border = BorderColor::all(Color::srgb(0.15, 0.50, 0.28));
    }
}

pub(crate) fn stop_frontistr_button_system(
    mut state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<StopFrontistrButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        let enabled = state.is_running() && !state.stop_requested;
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
            state.request_stop();
        }
        *background = BackgroundColor(if !enabled {
            BUTTON_DISABLED
        } else {
            match *interaction {
                Interaction::Pressed => BUTTON_PRESSED,
                Interaction::Hovered => STOP_HOVERED,
                Interaction::None => STOP_NORMAL,
            }
        });
        *border = BorderColor::all(Color::srgb(0.48, 0.18, 0.18));
    }
}

pub(crate) fn poll_frontistr_process_system(mut state: ResMut<FrontistrRunState>) {
    state.poll();
}

pub(crate) fn update_frontistr_run_ui_system(
    state: Res<FrontistrRunState>,
    mut executable_text: Query<
        &mut Text,
        (
            With<FrontistrExecutableText>,
            Without<FrontistrProjectText>,
            Without<FrontistrStatusText>,
            Without<FrontistrLogText>,
        ),
    >,
    mut project_text: Query<
        &mut Text,
        (
            With<FrontistrProjectText>,
            Without<FrontistrExecutableText>,
            Without<FrontistrStatusText>,
            Without<FrontistrLogText>,
        ),
    >,
    mut status_text: Query<
        &mut Text,
        (
            With<FrontistrStatusText>,
            Without<FrontistrExecutableText>,
            Without<FrontistrProjectText>,
            Without<FrontistrLogText>,
        ),
    >,
    mut log_text: Query<
        &mut Text,
        (
            With<FrontistrLogText>,
            Without<FrontistrExecutableText>,
            Without<FrontistrProjectText>,
            Without<FrontistrStatusText>,
        ),
    >,
) {
    if let Ok(mut text) = executable_text.single_mut() {
        text.set_if_neq(Text::new(format!(
            "Executable: {}\nRuntime: {}\nLaunch: {}",
            state.executable_label(),
            state.environment_label(),
            state.launch_label()
        )));
    }
    if let Ok(mut text) = project_text.single_mut() {
        text.set_if_neq(Text::new(format!("Project: {}", state.project_label())));
    }
    if let Ok(mut text) = status_text.single_mut() {
        text.set_if_neq(Text::new(state.status_label()));
    }
    if let Ok(mut text) = log_text.single_mut() {
        text.set_if_neq(Text::new(state.log_label()));
    }
}

fn ordinary_button_color(interaction: Interaction, enabled: bool) -> Color {
    if !enabled {
        return BUTTON_DISABLED;
    }
    match interaction {
        Interaction::Pressed => BUTTON_PRESSED,
        Interaction::Hovered => BUTTON_HOVERED,
        Interaction::None => BUTTON_NORMAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_target_controls_ready_state_and_can_be_cleared() {
        let mut state = FrontistrRunState::default();
        assert_eq!(state.phase, SolverRunPhase::Idle);
        assert!(state.project().is_none());

        state.register_export(PathBuf::from("project"), "model".to_string());
        assert_eq!(state.phase, SolverRunPhase::Ready);
        assert_eq!(state.project().unwrap().stem, "model");

        state.clear_export_target();
        assert_eq!(state.phase, SolverRunPhase::Idle);
        assert!(state.project().is_none());
    }

    #[test]
    fn solver_log_keeps_a_bounded_tail() {
        let mut state = FrontistrRunState::default();
        for index in 0..(MAX_LOG_LINES + 5) {
            state.append_log(ProcessOutputStream::Stdout, format!("line {index}"));
        }

        assert_eq!(state.log_lines.len(), MAX_LOG_LINES);
        assert_eq!(state.log_lines.front().unwrap(), "line 5");
        assert_eq!(
            state.log_lines.back().unwrap(),
            &format!("line {}", MAX_LOG_LINES + 4)
        );
    }

    #[test]
    fn mpi_rank_adjustment_is_exact_and_bounded() {
        let mut state = FrontistrRunState::default();
        state.set_launch_mode(SolverLaunchMode::Mpi);
        state.mpi_ranks = 1;

        state.adjust_mpi_ranks(-1);
        assert_eq!(state.mpi_ranks, 1);
        state.adjust_mpi_ranks(7);
        assert_eq!(state.mpi_ranks, 8);
        state.mpi_ranks = 4096;
        state.adjust_mpi_ranks(1);
        assert_eq!(state.mpi_ranks, 4096);
    }

    #[test]
    fn execution_controls_switch_modes_and_freeze_during_a_run() {
        fn spawn_panel(mut commands: Commands) {
            commands
                .spawn(Node::default())
                .with_children(spawn_solver_execution_ui);
        }
        let mut app = App::new();
        app.insert_resource(FrontistrRunState {
            launch_mode: SolverLaunchMode::Direct,
            mpi_ranks: 2,
            ..default()
        });
        app.add_systems(Startup, spawn_panel);
        app.add_systems(
            Update,
            (
                solver_launch_mode_button_system,
                mpi_rank_adjust_button_system,
                update_mpi_rank_controls_system,
                update_frontistr_run_ui_system,
            )
                .chain(),
        );
        app.update();
        let controls = app
            .world_mut()
            .query_filtered::<Entity, With<MpiRankControls>>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            app.world().get::<Node>(controls).unwrap().display,
            Display::None
        );
        let mpi_button = app
            .world_mut()
            .query::<(Entity, &SolverLaunchModeButton)>()
            .iter(app.world())
            .find(|(_, mode)| mode.0 == SolverLaunchMode::Mpi)
            .unwrap()
            .0;
        *app.world_mut().get_mut::<Interaction>(mpi_button).unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(
            app.world().get::<Node>(controls).unwrap().display,
            Display::Flex
        );
        let increment = app
            .world_mut()
            .query::<(Entity, &MpiRankAdjustButton)>()
            .iter(app.world())
            .find(|(_, delta)| delta.0 == 1)
            .unwrap()
            .0;
        *app.world_mut().get_mut::<Interaction>(increment).unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(app.world().resource::<FrontistrRunState>().mpi_ranks, 3);

        app.world_mut().resource_mut::<FrontistrRunState>().phase = SolverRunPhase::Running;
        let direct = app
            .world_mut()
            .query::<(Entity, &SolverLaunchModeButton)>()
            .iter(app.world())
            .find(|(_, mode)| mode.0 == SolverLaunchMode::Direct)
            .unwrap()
            .0;
        *app.world_mut().get_mut::<Interaction>(direct).unwrap() = Interaction::Pressed;
        *app.world_mut().get_mut::<Interaction>(increment).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(increment).unwrap() = Interaction::Pressed;
        app.update();
        let state = app.world().resource::<FrontistrRunState>();
        assert_eq!(state.launch_mode, SolverLaunchMode::Mpi);
        assert_eq!(state.mpi_ranks, 3);
    }
}
