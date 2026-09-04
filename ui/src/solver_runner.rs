//! Asynchronous FrontISTR process execution and its Solve-page controls.
//!
//! The process is deliberately kept outside Bevy's main thread.  A successful
//! export establishes the working directory and project stem; Run rewrites the
//! current model/setup to that target before starting `fistr1` there.

use crate::layout::{SidebarPage, SidebarPageContent};
use bevy::prelude::*;
use fem_core::{AnalysisSetup, FemModel};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
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

#[derive(Debug, Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
enum SolverEvent {
    Output(OutputStream, String),
    SpawnFailed(String),
    Finished(Option<i32>),
    Stopped,
}

#[derive(Resource)]
pub(crate) struct FrontistrRunState {
    executable: PathBuf,
    project: Option<FrontistrProjectTarget>,
    phase: SolverRunPhase,
    message: String,
    log_lines: VecDeque<String>,
    started_at: Option<Instant>,
    elapsed: Duration,
    events: Option<Mutex<Receiver<SolverEvent>>>,
    stop_sender: Option<Sender<()>>,
    stop_requested: bool,
}

impl Default for FrontistrRunState {
    fn default() -> Self {
        let executable = std::env::var_os("FRONTISTR_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("fistr1"));
        Self {
            executable,
            project: None,
            phase: SolverRunPhase::Idle,
            message: "Export a project to establish the run folder.".to_string(),
            log_lines: VecDeque::new(),
            started_at: None,
            elapsed: Duration::ZERO,
            events: None,
            stop_sender: None,
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
        let executable = self.executable.clone();
        let directory = target.directory.clone();
        self.start_command(executable, Vec::new(), directory)
    }

    fn start_command(
        &mut self,
        executable: PathBuf,
        arguments: Vec<String>,
        directory: PathBuf,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("FrontISTR is already running.".to_string());
        }

        let (event_sender, event_receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = mpsc::channel();
        thread::Builder::new()
            .name("frontistr-runner".to_string())
            .spawn(move || {
                run_process(
                    &executable,
                    &arguments,
                    &directory,
                    event_sender,
                    stop_receiver,
                );
            })
            .map_err(|error| format!("Could not start solver worker: {error}"))?;

        self.phase = SolverRunPhase::Running;
        self.message = "FrontISTR is running in the exported project folder.".to_string();
        self.log_lines.clear();
        self.started_at = Some(Instant::now());
        self.elapsed = Duration::ZERO;
        self.events = Some(Mutex::new(event_receiver));
        self.stop_sender = Some(stop_sender);
        self.stop_requested = false;
        Ok(())
    }

    fn request_stop(&mut self) {
        if !self.is_running() || self.stop_requested {
            return;
        }
        if let Some(sender) = &self.stop_sender {
            let _ = sender.send(());
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
            .events
            .as_ref()
            .map(|receiver| {
                let receiver = receiver.lock().unwrap_or_else(|poison| poison.into_inner());
                receiver.try_iter().collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut terminal = false;
        for event in pending {
            match event {
                SolverEvent::Output(stream, line) => self.append_log(stream, line),
                SolverEvent::SpawnFailed(error) => {
                    self.phase = SolverRunPhase::Failed;
                    self.message = error;
                    terminal = true;
                }
                SolverEvent::Finished(code) => {
                    if code == Some(0) {
                        self.phase = SolverRunPhase::Succeeded;
                        self.message = "FrontISTR completed successfully.".to_string();
                    } else {
                        self.phase = SolverRunPhase::Failed;
                        self.message = match code {
                            Some(code) => format!("FrontISTR exited with code {code}."),
                            None => "FrontISTR ended without an exit code.".to_string(),
                        };
                    }
                    terminal = true;
                }
                SolverEvent::Stopped => {
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
            self.events = None;
            self.stop_sender = None;
            self.stop_requested = false;
        }
    }

    fn append_log(&mut self, stream: OutputStream, line: String) {
        let mut shortened = line.chars().take(MAX_LOG_CHARS).collect::<String>();
        if line.chars().count() > MAX_LOG_CHARS {
            shortened.push('…');
        }
        if matches!(stream, OutputStream::Stderr) {
            shortened.insert_str(0, "ERR  ");
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
                Text::new("Executable: fistr1 (PATH)"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                FrontistrExecutableText,
            ));
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
                Text::new("Run refreshes the exported files, then starts fistr1 in that folder."),
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
            let dialog = rfd::FileDialog::new()
                .set_title("Choose FrontISTR executable")
                .add_filter("FrontISTR executable", &["exe"])
                .add_filter("All files", &["*"]);
            if let Some(path) = dialog.pick_file() {
                state.set_executable(path);
            }
        }
        *background = BackgroundColor(ordinary_button_color(*interaction, enabled));
        *border = BorderColor::all(PANEL_BORDER);
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
        **text = format!("Executable: {}", state.executable_label());
    }
    if let Ok(mut text) = project_text.single_mut() {
        **text = format!("Project: {}", state.project_label());
    }
    if let Ok(mut text) = status_text.single_mut() {
        **text = state.status_label();
    }
    if let Ok(mut text) = log_text.single_mut() {
        **text = state.log_label();
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

fn run_process(
    executable: &Path,
    arguments: &[String],
    directory: &Path,
    event_sender: Sender<SolverEvent>,
    stop_receiver: Receiver<()>,
) {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_command(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = event_sender.send(SolverEvent::SpawnFailed(format!(
                "Could not start {}: {error}",
                executable.display()
            )));
            return;
        }
    };

    let stdout_reader = child
        .stdout
        .take()
        .map(|stdout| spawn_output_reader(stdout, OutputStream::Stdout, event_sender.clone()));
    let stderr_reader = child
        .stderr
        .take()
        .map(|stderr| spawn_output_reader(stderr, OutputStream::Stderr, event_sender.clone()));

    let (stopped, exit_code) = loop {
        match stop_receiver.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => {
                let _ = child.kill();
                let _ = child.wait();
                break (true, None);
            }
            Err(TryRecvError::Empty) => {}
        }

        match child.try_wait() {
            Ok(Some(status)) => break (false, status.code()),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = event_sender.send(SolverEvent::SpawnFailed(format!(
                    "Could not monitor FrontISTR: {error}"
                )));
                return;
            }
        }
    };

    if let Some(reader) = stdout_reader {
        let _ = reader.join();
    }
    if let Some(reader) = stderr_reader {
        let _ = reader.join();
    }

    let terminal_event = if stopped {
        SolverEvent::Stopped
    } else {
        SolverEvent::Finished(exit_code)
    };
    let _ = event_sender.send(terminal_event);
}

fn spawn_output_reader<R: Read + Send + 'static>(
    reader: R,
    stream: OutputStream,
    sender: Sender<SolverEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => {
                    if sender.send(SolverEvent::Output(stream, line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(SolverEvent::Output(
                        OutputStream::Stderr,
                        format!("Could not read solver output: {error}"),
                    ));
                    break;
                }
            }
        }
    })
}

#[cfg(target_os = "windows")]
fn configure_command(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn configure_command(_command: &mut Command) {}

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
            state.append_log(OutputStream::Stdout, format!("line {index}"));
        }

        assert_eq!(state.log_lines.len(), MAX_LOG_LINES);
        assert_eq!(state.log_lines.front().unwrap(), "line 5");
        assert_eq!(
            state.log_lines.back().unwrap(),
            &format!("line {}", MAX_LOG_LINES + 4)
        );
    }

    #[test]
    fn asynchronous_process_collects_output_and_completion() {
        let mut state = FrontistrRunState::default();
        let directory = std::env::current_dir().unwrap();

        #[cfg(target_os = "windows")]
        let (executable, arguments) = (
            PathBuf::from("cmd"),
            vec!["/C".to_string(), "echo solver-ok".to_string()],
        );
        #[cfg(not(target_os = "windows"))]
        let (executable, arguments) = (
            PathBuf::from("sh"),
            vec!["-c".to_string(), "echo solver-ok".to_string()],
        );

        state
            .start_command(executable, arguments, directory)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while state.is_running() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
            state.poll();
        }

        assert_eq!(state.phase, SolverRunPhase::Succeeded);
        assert!(
            state
                .log_lines
                .iter()
                .any(|line| line.contains("solver-ok"))
        );
    }
}
