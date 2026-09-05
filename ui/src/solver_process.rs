//! Platform-neutral FrontISTR child-process backend.
//!
//! UI state chooses Direct or MPI.  This module prepares an optional runtime
//! environment, resolves a launcher from that environment's PATH, and owns the
//! child process off Bevy's main thread.  Platform adapters stay behind `cfg`.

#[path = "solver_process_tree.rs"]
mod process_tree;
use process_tree::ProcessTree;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolverLaunchMode {
    Direct,
    Mpi,
}

impl SolverLaunchMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Direct => "Direct",
            Self::Mpi => "MPI",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum RuntimeEnvironment {
    Inherited,
    #[cfg(target_os = "windows")]
    WindowsBatch(PathBuf),
}

impl RuntimeEnvironment {
    pub(crate) fn detect() -> Self {
        if std::env::var("FRONTISTR_RUNTIME").is_ok_and(|v| v.eq_ignore_ascii_case("inherit")) {
            return Self::Inherited;
        }
        #[cfg(target_os = "windows")]
        {
            if std::env::var_os("I_MPI_ROOT").is_none() {
                if let Some(path) = detect_oneapi_setvars() {
                    return Self::WindowsBatch(path);
                }
            }
        }
        Self::Inherited
    }

    pub(crate) fn label(&self) -> String {
        match self {
            Self::Inherited => "inherited process environment".to_string(),
            #[cfg(target_os = "windows")]
            Self::WindowsBatch(path) => {
                format!("Intel oneAPI auto ({})", path.display())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SolverProcessConfig {
    pub(crate) executable: PathBuf,
    pub(crate) project_stem: String,
    pub(crate) partitioner: Option<PathBuf>,
    pub(crate) working_directory: PathBuf,
    pub(crate) environment: RuntimeEnvironment,
    pub(crate) launch_mode: SolverLaunchMode,
    pub(crate) mpi_ranks: u16,
    pub(crate) mpi_launcher: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProcessOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub(crate) enum SolverProcessEvent {
    Output(ProcessOutputStream, String),
    SpawnFailed(String),
    Stage(String),
    Finished(Option<i32>),
    Stopped,
}

pub(crate) struct SolverProcessHandle {
    events: Mutex<Receiver<SolverProcessEvent>>,
    stop_sender: Sender<()>,
}

impl SolverProcessHandle {
    pub(crate) fn poll(&self) -> Vec<SolverProcessEvent> {
        let receiver = self
            .events
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        receiver.try_iter().take(256).collect()
    }

    pub(crate) fn request_stop(&self) {
        let _ = self.stop_sender.send(());
    }
}

pub(crate) fn spawn_solver_process(
    config: SolverProcessConfig,
) -> Result<SolverProcessHandle, String> {
    // Backpressure keeps verbose parallel output from growing an unbounded
    // queue while the UI consumes a bounded batch each frame.
    let (event_sender, event_receiver) = mpsc::sync_channel(1024);
    let (stop_sender, stop_receiver) = mpsc::channel();
    thread::Builder::new()
        .name("frontistr-runner".to_string())
        .spawn(move || run_process(config, event_sender, stop_receiver))
        .map_err(|error| format!("Could not start solver worker: {error}"))?;

    Ok(SolverProcessHandle {
        events: Mutex::new(event_receiver),
        stop_sender,
    })
}

#[derive(Debug, Clone)]
struct ProcessStep {
    label: &'static str,
    program: PathBuf,
    arguments: Vec<OsString>,
}

enum StepResult {
    Exited(Option<i32>),
    Stopped,
}

fn cancelled(receiver: &Receiver<()>) -> bool {
    matches!(
        receiver.try_recv(),
        Ok(()) | Err(TryRecvError::Disconnected)
    )
}

fn run_process(
    config: SolverProcessConfig,
    event_sender: SyncSender<SolverProcessEvent>,
    stop_receiver: Receiver<()>,
) {
    let result = run_pipeline(&config, &event_sender, &stop_receiver);
    let event = match result {
        Ok(StepResult::Stopped) => SolverProcessEvent::Stopped,
        Ok(StepResult::Exited(code)) => SolverProcessEvent::Finished(code),
        Err(error) => SolverProcessEvent::SpawnFailed(error),
    };
    let _ = event_sender.send(event);
}

fn run_pipeline(
    config: &SolverProcessConfig,
    events: &SyncSender<SolverProcessEvent>,
    stop: &Receiver<()>,
) -> Result<StepResult, String> {
    if cancelled(stop) {
        return Ok(StepResult::Stopped);
    }
    let _ = events.send(SolverProcessEvent::Stage(
        "Preparing runtime environment".into(),
    ));
    let environment = prepare_environment(&config.environment, events)?;
    if cancelled(stop) {
        return Ok(StepResult::Stopped);
    }
    // Resolve every executable before changing the control files.
    let (program, arguments, _) = launch_command(config, environment.as_deref())?;
    let solve = ProcessStep {
        label: "Solving",
        program,
        arguments,
    };
    if config.launch_mode == SolverLaunchMode::Mpi {
        let fistr =
            resolve_program(&config.executable, environment.as_deref()).ok_or_else(|| {
                format!(
                    "FrontISTR executable not found: {}",
                    config.executable.display()
                )
            })?;
        let partitioner = match &config.partitioner {
            Some(path) => resolve_program(path, environment.as_deref()),
            None => fistr.parent()
                .and_then(|dir| resolve_program(&dir.join(executable_name("hecmw_part1")), environment.as_deref()))
                .or_else(|| resolve_program(Path::new("hecmw_part1"), environment.as_deref())),
        }.ok_or_else(|| "hecmw_part1 was not found beside fistr1 or on PATH. Set FRONTISTR_PARTITIONER if needed.".to_string())?;
        // A fresh prefix prevents a previous successful run from masking
        // incomplete/missing output from this partitioner invocation.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let distributed = format!(
            "bevyistr_part_{}_{}_{}",
            config.mpi_ranks,
            std::process::id(),
            stamp
        );
        hecmw::write_parallel_hecmw_ctrl(
            &config.working_directory,
            &hecmw::HecmwCtrlParams {
                mesh_name: &config.project_stem,
                cnt_name: &config.project_stem,
                result_name: &config.project_stem,
            },
            &distributed,
            config.mpi_ranks,
        )
        .map_err(|e| format!("Could not write partition controls: {e}"))?;
        let partition = ProcessStep {
            label: "Partitioning (hecmw_part1)",
            program: partitioner,
            arguments: Vec::new(),
        };
        match execute_step(
            &partition,
            &config.working_directory,
            environment.as_deref(),
            events,
            stop,
        )? {
            StepResult::Stopped => return Ok(StepResult::Stopped),
            StepResult::Exited(Some(0)) => {}
            StepResult::Exited(code) => {
                return Err(format!(
                    "hecmw_part1 failed (exit {code:?}); solver was not started."
                ));
            }
        }
        verify_partition_files(&config.working_directory, &distributed, config.mpi_ranks)?;
    }
    if cancelled(stop) {
        return Ok(StepResult::Stopped);
    }
    execute_step(
        &solve,
        &config.working_directory,
        environment.as_deref(),
        events,
        stop,
    )
}

fn verify_partition_files(directory: &Path, prefix: &str, ranks: u16) -> Result<(), String> {
    for rank in 0..ranks {
        let path = directory.join(format!("{prefix}.{rank}"));
        if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() > 0) {
            return Err(format!(
                "Partition output missing or empty: {}; solver was not started.",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Used by both the partitioner and solver, and by subprocess regression tests.
fn execute_step(
    step: &ProcessStep,
    directory: &Path,
    environment: Option<&[(OsString, OsString)]>,
    events: &SyncSender<SolverProcessEvent>,
    stop: &Receiver<()>,
) -> Result<StepResult, String> {
    if cancelled(stop) {
        return Ok(StepResult::Stopped);
    }
    let _ = events.send(SolverProcessEvent::Stage(step.label.into()));
    let _ = events.send(SolverProcessEvent::Output(
        ProcessOutputStream::Stdout,
        format!("{}: {:?} {:?}", step.label, step.program, step.arguments),
    ));
    let mut command = Command::new(&step.program);
    command
        .args(&step.arguments)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(env) = environment {
        command.env_clear().envs(env.iter().cloned());
    }
    ProcessTree::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", step.program.display()))?;
    let tree = match ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Could not attach solver process control: {e}"));
        }
    };
    let stdout = child
        .stdout
        .take()
        .map(|r| spawn_output_reader(r, ProcessOutputStream::Stdout, events.clone()));
    let stderr = child
        .stderr
        .take()
        .map(|r| spawn_output_reader(r, ProcessOutputStream::Stderr, events.clone()));
    let result = loop {
        if cancelled(stop) {
            let outcome = tree
                .terminate()
                .map_err(|e| format!("Could not stop solver processes: {e}"));
            let _ = child.kill();
            let _ = child.wait();
            break outcome.map(|_| StepResult::Stopped);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(StepResult::Exited(status.code())),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = tree.terminate();
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("Could not monitor {}: {e}", step.label));
            }
        }
    };
    // Release any local launcher descendants before waiting for EOF. Without
    // this, an MPI rank holding a pipe open can leave Stop stuck indefinitely.
    drop(tree);
    if let Some(reader) = stdout {
        let _ = reader.join();
    }
    if let Some(reader) = stderr {
        let _ = reader.join();
    }
    result
}

fn executable_name(name: &str) -> OsString {
    #[cfg(windows)]
    {
        OsString::from(format!("{name}.exe"))
    }
    #[cfg(not(windows))]
    {
        OsString::from(name)
    }
}

fn prepare_environment(
    environment: &RuntimeEnvironment,
    _event_sender: &SyncSender<SolverProcessEvent>,
) -> Result<Option<Vec<(OsString, OsString)>>, String> {
    match environment {
        RuntimeEnvironment::Inherited => Ok(None),
        #[cfg(target_os = "windows")]
        RuntimeEnvironment::WindowsBatch(script) => {
            let _ = _event_sender.send(SolverProcessEvent::Output(
                ProcessOutputStream::Stdout,
                format!("Loading runtime environment: {}", script.display()),
            ));
            capture_batch_environment(script).map(Some)
        }
    }
}

fn launch_command(
    config: &SolverProcessConfig,
    environment: Option<&[(OsString, OsString)]>,
) -> Result<(PathBuf, Vec<OsString>, String), String> {
    if config.launch_mode == SolverLaunchMode::Direct {
        let executable = resolve_program(&config.executable, environment).ok_or_else(|| {
            format!(
                "FrontISTR executable not found: {}",
                config.executable.display()
            )
        })?;
        let description = executable.display().to_string();
        return Ok((executable, Vec::new(), description));
    }

    let launcher = if let Some(explicit) = &config.mpi_launcher {
        resolve_program(explicit, environment)
            .ok_or_else(|| format!("MPI launcher was not found: {}", explicit.display()))?
    } else {
        ["mpiexec", "mpirun"]
            .into_iter()
            .find_map(|name| resolve_program(Path::new(name), environment))
            .ok_or_else(|| {
                "No MPI launcher found on PATH (tried mpiexec and mpirun).".to_string()
            })?
    };
    let executable = resolve_program(&config.executable, environment).ok_or_else(|| {
        format!(
            "FrontISTR executable not found: {}",
            config.executable.display()
        )
    })?;
    if config.mpi_ranks == 0 {
        return Err("MPI ranks must be at least 1.".into());
    }
    let ranks = config.mpi_ranks.to_string();
    let arguments = vec![
        OsString::from("-n"),
        OsString::from(&ranks),
        executable.as_os_str().to_owned(),
    ];
    let description = format!(
        "{} -n {} {}",
        launcher.display(),
        ranks,
        executable.display()
    );
    Ok((launcher, arguments, description))
}

fn resolve_program(
    program: &Path,
    environment: Option<&[(OsString, OsString)]>,
) -> Option<PathBuf> {
    if program.components().count() > 1 {
        return is_executable(program)
            .then(|| std::path::absolute(program).ok())
            .flatten();
    }

    let path_value = environment
        .and_then(|entries| environment_value(entries, "PATH"))
        .or_else(|| {
            environment
                .is_none()
                .then(|| std::env::var_os("PATH"))
                .flatten()
        });
    let path_value = path_value?;
    for directory in std::env::split_paths(&path_value) {
        let candidate = directory.join(program);
        if is_executable(&candidate) {
            return std::path::absolute(candidate).ok();
        }
        #[cfg(target_os = "windows")]
        if program.extension().is_none() {
            let executable = candidate.with_extension("exe");
            if executable.is_file() {
                return std::path::absolute(executable).ok();
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn environment_value(environment: &[(OsString, OsString)], requested: &str) -> Option<OsString> {
    environment
        .iter()
        .find(|(name, _)| {
            #[cfg(windows)]
            {
                name.to_string_lossy().eq_ignore_ascii_case(requested)
            }
            #[cfg(not(windows))]
            {
                name == requested
            }
        })
        .map(|(_, value)| value.clone())
}

fn spawn_output_reader<R: Read + Send + 'static>(
    reader: R,
    stream: ProcessOutputStream,
    sender: SyncSender<SolverProcessEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).split(b'\n') {
            match line {
                Ok(line) => {
                    // Non-UTF8 diagnostics must not stop pipe drainage and
                    // deadlock a native solver. Only the UI tail is retained.
                    let line = String::from_utf8_lossy(&line)
                        .trim_end_matches('\r')
                        .chars()
                        .take(1024)
                        .collect();
                    if sender
                        .send(SolverProcessEvent::Output(stream, line))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(SolverProcessEvent::Output(
                        ProcessOutputStream::Stderr,
                        format!("Could not read solver output: {error}"),
                    ));
                    break;
                }
            }
        }
    })
}

#[cfg(target_os = "windows")]
fn detect_oneapi_setvars() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(root) = std::env::var_os("ONEAPI_ROOT") {
        candidates.push(PathBuf::from(root).join("setvars.bat"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(program_files_x86)
                .join("Intel")
                .join("oneAPI")
                .join("setvars.bat"),
        );
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("Intel")
                .join("oneAPI")
                .join("setvars.bat"),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn capture_batch_environment(script: &Path) -> Result<Vec<(OsString, OsString)>, String> {
    use std::os::windows::process::CommandExt;

    let command_line = format!("/D /U /S /C \"call \"{}\" >nul && set\"", script.display());
    let command_processor = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut command = Command::new(command_processor);
    command
        .raw_arg(command_line)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_command(&mut command);
    let output = command.output().map_err(|error| {
        format!(
            "Could not load runtime environment from {}: {error}",
            script.display()
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "Runtime environment setup failed with exit code {:?}: {}",
            output.status.code(),
            decode_utf16_output(&output.stderr).trim()
        ));
    }
    parse_environment_dump(&decode_utf16_output(&output.stdout))
}

#[cfg(target_os = "windows")]
fn decode_utf16_output(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

#[cfg(target_os = "windows")]
fn parse_environment_dump(dump: &str) -> Result<Vec<(OsString, OsString)>, String> {
    let environment = dump
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            if name.is_empty() || name.starts_with('=') {
                return None;
            }
            Some((OsString::from(name), OsString::from(value)))
        })
        .collect::<Vec<_>>();
    if environment.is_empty() {
        Err("Runtime environment setup returned no variables.".to_string())
    } else {
        Ok(environment)
    }
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
#[path = "solver_process_tests.rs"]
mod tests;
