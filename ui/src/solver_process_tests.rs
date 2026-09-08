use super::*;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

struct TempProject(PathBuf);
impl TempProject {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bevyistr runner {} {}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> &'static Path {
    static FIXTURE: OnceLock<PathBuf> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let dir = TempProject::new();
        let output = dir.0.join(executable_name("fixture"));
        let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
        rustc
            .args(["--edition=2024", "--crate-name=solver_process_fixture"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/solver_process_fixture.rs"))
            .arg("-o")
            .arg(&output);
        configure_command(&mut rustc);
        let result = rustc.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        // Retain the one compiled fixture for the test-process lifetime.
        std::mem::forget(dir);
        output
    })
}

fn fake_project(mode: SolverLaunchMode) -> (TempProject, SolverProcessConfig) {
    let dir = TempProject::new();
    for name in [
        "fistr1",
        "hecmw_part1",
        "mpiexec",
        "tree-parent",
        "tree-child",
    ] {
        std::fs::copy(fixture(), dir.0.join(executable_name(name))).unwrap();
    }
    hecmw::write_hecmw_ctrl(
        &dir.0,
        &hecmw::HecmwCtrlParams {
            mesh_name: "model",
            cnt_name: "model",
            result_name: "model",
        },
    )
    .unwrap();
    let config = SolverProcessConfig {
        executable: dir.0.join(executable_name("fistr1")),
        project_stem: "model".into(),
        partitioner: None,
        working_directory: dir.0.clone(),
        environment: RuntimeEnvironment::Inherited,
        launch_mode: mode,
        mpi_ranks: 2,
        mpi_launcher: Some(dir.0.join(executable_name("mpiexec"))),
    };
    (dir, config)
}

fn wait_for(handle: &SolverProcessHandle, seconds: u64) -> Vec<SolverProcessEvent> {
    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        events.extend(handle.poll());
        if events.iter().any(|e| {
            matches!(
                e,
                SolverProcessEvent::Finished(_)
                    | SolverProcessEvent::Stopped
                    | SolverProcessEvent::SpawnFailed(_)
            )
        }) {
            return events;
        }
        if Instant::now() >= deadline {
            handle.request_stop();
            panic!("runner timed out; events: {events:?}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn direct_solve_skips_partitioning() {
    let (dir, config) = fake_project(SolverLaunchMode::Direct);
    let handle = spawn_solver_process(config).unwrap();
    let events = wait_for(&handle, 10);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SolverProcessEvent::Finished(Some(0)))),
        "{events:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.0.join("order.log"))
            .unwrap()
            .trim(),
        "fistr1"
    );
    assert!(
        std::fs::read_to_string(dir.0.join("hecmw_ctrl.dat"))
            .unwrap()
            .contains("TYPE=HECMW-ENTIRE")
    );
    let transcript = std::fs::read_to_string(handle.log_path()).unwrap();
    assert!(transcript.contains("solver-ok"));
    assert!(transcript.contains("FrontISTR exit code: 0"));
    assert!(crate::solver_log::completion_path(handle.log_path()).is_file());
}

#[test]
fn mpi_run_partitions_before_launching_matching_rank_count() {
    let (dir, config) = fake_project(SolverLaunchMode::Mpi);
    let handle = spawn_solver_process(config).unwrap();
    let events = wait_for(&handle, 10);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SolverProcessEvent::Finished(Some(0)))),
        "{events:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.0.join("order.log"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["hecmw_part1", "mpiexec", "fistr1"]
    );
    let reopened = hecmw::load_hecmw_ctrl(dir.0.join("hecmw_ctrl.dat")).unwrap();
    assert_eq!(reopened.mesh_path.as_deref(), Some("model.msh"));
    assert_eq!(reopened.cnt_path.as_deref(), Some("model.cnt"));
    assert!(
        events.iter().any(
            |e| matches!(e, SolverProcessEvent::Output(_, text) if text.contains("solver-ok"))
        )
    );
}

#[test]
fn partition_failure_or_missing_outputs_prevents_solver_launch() {
    for marker in ["fail-partition", "missing-partition"] {
        let (dir, config) = fake_project(SolverLaunchMode::Mpi);
        std::fs::write(dir.0.join(marker), "").unwrap();
        let handle = spawn_solver_process(config).unwrap();
        let events = wait_for(&handle, 10);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SolverProcessEvent::SpawnFailed(_))),
            "{events:?}"
        );
        assert!(!dir.0.join("solver-called").exists());
        assert_eq!(
            std::fs::read_to_string(dir.0.join("order.log"))
                .unwrap()
                .trim(),
            "hecmw_part1"
        );
        let transcript = std::fs::read_to_string(handle.log_path()).unwrap();
        assert!(transcript.contains("Run failed:"));
        assert!(crate::solver_log::completion_path(handle.log_path()).is_file());
    }
}

#[test]
fn stopping_partition_does_not_start_solver() {
    let (dir, config) = fake_project(SolverLaunchMode::Mpi);
    std::fs::write(dir.0.join("sleep-partition"), "").unwrap();
    let handle = spawn_solver_process(config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !dir.0.join("partition-started").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        dir.0.join("partition-started").exists(),
        "partitioner did not start"
    );
    handle.request_stop();
    let events = wait_for(&handle, 5);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SolverProcessEvent::Stopped)),
        "{events:?}"
    );
    assert!(!dir.0.join("solver-called").exists());
    assert!(
        std::fs::read_to_string(handle.log_path())
            .unwrap()
            .contains("Run stopped by user")
    );
    assert!(crate::solver_log::completion_path(handle.log_path()).is_file());
}

#[test]
fn live_log_is_readable_before_exit_and_closing_reader_does_not_stop_solver() {
    let (dir, config) = fake_project(SolverLaunchMode::Direct);
    std::fs::write(dir.0.join("live-output"), "").unwrap();
    let handle = spawn_solver_process(config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !std::fs::read_to_string(handle.log_path())
        .unwrap()
        .contains("running-before-completion")
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!crate::solver_log::completion_path(handle.log_path()).exists());
    drop(std::fs::File::open(handle.log_path()).unwrap());
    std::fs::write(dir.0.join("allow-finish"), "").unwrap();
    let events = wait_for(&handle, 10);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SolverProcessEvent::Finished(Some(0))))
    );
    let transcript = std::fs::read_to_string(handle.log_path()).unwrap();
    assert!(transcript.contains("solver-ok"));
    assert!(transcript.contains("[stderr] diagnostic-at-completion"));
}

#[test]
fn ui_backlog_and_line_limit_do_not_truncate_or_block_the_transcript() {
    let dir = TempProject::new();
    let log = RunLog::create(&dir.0).unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let events = ProcessEvents::new(sender, log.clone());
    let long_line = "結果".repeat(1500);
    events
        .send(SolverProcessEvent::Output(
            ProcessOutputStream::Stdout,
            long_line.clone(),
        ))
        .unwrap();
    events
        .send(SolverProcessEvent::Output(
            ProcessOutputStream::Stdout,
            "still recording".into(),
        ))
        .unwrap();
    let SolverProcessEvent::Output(_, ui_line) = receiver.recv().unwrap() else {
        panic!()
    };
    assert!(ui_line.chars().count() <= 1025);
    let transcript = std::fs::read_to_string(log.path()).unwrap();
    assert!(transcript.contains(&long_line));
    assert!(transcript.contains("still recording"));
    let another = RunLog::create(&dir.0).unwrap();
    assert_ne!(log.path(), another.path());
}

#[test]
fn stop_terminates_local_launcher_descendants_holding_output_pipes() {
    let (dir, mut config) = fake_project(SolverLaunchMode::Direct);
    config.executable = dir.0.join(executable_name("tree-parent"));
    let handle = spawn_solver_process(config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !dir.0.join("child-pid").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(dir.0.join("child-pid").exists(), "descendant did not start");
    handle.request_stop();
    let events = wait_for(&handle, 5);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SolverProcessEvent::Stopped)),
        "{events:?}"
    );
}

#[test]
fn effective_path_resolves_only_its_own_launcher() {
    let (dir, _) = fake_project(SolverLaunchMode::Direct);
    let environment = vec![("PATH".into(), dir.0.as_os_str().to_owned())];
    assert_eq!(
        resolve_program(Path::new("mpiexec"), Some(&environment)),
        Some(dir.0.join(executable_name("mpiexec")))
    );
    assert!(resolve_program(Path::new("mpiexec"), Some(&[])).is_none());
}

#[cfg(windows)]
#[test]
fn environment_dump_preserves_equals_and_ignores_drive_variables() {
    let parsed =
        parse_environment_dump("PATH=C:\\tools\r\nTOKEN=a=b=c\r\n=C:=ignored\r\n").unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[1].1, OsString::from("a=b=c"));
}

#[test]
fn non_utf8_diagnostics_do_not_stop_output_drainage() {
    let (sender, receiver) = mpsc::sync_channel(4);
    let dir = TempProject::new();
    let sender = ProcessEvents::new(sender, RunLog::create(&dir.0).unwrap());
    spawn_output_reader(
        std::io::Cursor::new(b"bad:\xff\r\nnext line\n"),
        ProcessOutputStream::Stderr,
        sender,
    )
    .join()
    .unwrap();
    let lines = receiver
        .try_iter()
        .filter_map(|event| match event {
            SolverProcessEvent::Output(_, line) => Some(line),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lines, ["bad:\u{fffd}", "next line"]);
}

#[test]
fn rerunning_with_missing_partitions_cannot_reuse_previous_output() {
    let (dir, config) = fake_project(SolverLaunchMode::Mpi);
    let first = spawn_solver_process(config.clone()).unwrap();
    assert!(
        wait_for(&first, 10)
            .iter()
            .any(|event| matches!(event, SolverProcessEvent::Finished(Some(0))))
    );
    std::fs::write(dir.0.join("missing-partition"), "").unwrap();
    let second = spawn_solver_process(config).unwrap();
    assert!(
        wait_for(&second, 10)
            .iter()
            .any(|event| matches!(event, SolverProcessEvent::SpawnFailed(_)))
    );
    let order = std::fs::read_to_string(dir.0.join("order.log")).unwrap();
    assert_eq!(order.lines().filter(|line| *line == "fistr1").count(), 1);
}

#[test]
#[ignore = "Requires an installed MPI FrontISTR and BEVYISTR_FRONTISTR_SMOKE_INPUT (official tutorial folder)"]
fn installed_frontistr_parallel_smoke() {
    let input = PathBuf::from(
        std::env::var_os("BEVYISTR_FRONTISTR_SMOKE_INPUT").expect("set tutorial path"),
    );
    let dir = TempProject::new();
    for file in ["hinge.msh", "hinge.cnt"] {
        std::fs::copy(input.join(file), dir.0.join(file)).unwrap();
    }
    let config = SolverProcessConfig {
        executable: std::env::var_os("FRONTISTR_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| "fistr1".into()),
        project_stem: "hinge".into(),
        partitioner: None,
        working_directory: dir.0.clone(),
        environment: RuntimeEnvironment::detect(),
        launch_mode: SolverLaunchMode::Mpi,
        mpi_ranks: 2,
        mpi_launcher: std::env::var_os("FRONTISTR_MPI_LAUNCHER").map(PathBuf::from),
    };
    let mut result_source = crate::run_results::RunResultSource::capture(&dir.0, "hinge", 2, 0).unwrap();
    let handle = spawn_solver_process(config).unwrap();
    result_source.partition_prefix = handle.partition_prefix.clone();
    let events = wait_for(&handle, 120);
    for event in events.iter().take(15).chain(events.iter().rev().take(15).rev()) {
        println!("{event:?}");
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SolverProcessEvent::Finished(Some(0)))),
        "{events:?}"
    );
    assert!(dir.0.join("hinge.res.0.1").is_file(), "no rank-0 result");
    assert!(dir.0.join("hinge.res.1.1").is_file(), "no rank-1 result");
    let transcript = std::fs::read_to_string(handle.log_path()).unwrap();
    assert!(transcript.contains("=== Partitioning (hecmw_part1) ==="));
    assert!(transcript.contains("=== Solving ==="));
    assert!(transcript.contains("FrontISTR exit code: 0"));
    assert!(crate::solver_log::completion_path(handle.log_path()).is_file());
    let mesh = hecmw::load_mesh_file(dir.0.join("hinge.msh")).unwrap();
    result_source.finish().unwrap();
    let ids = mesh.nodes.iter().map(|node| node.id).collect();
    let loaded = result_source.load(&[ids]).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].iter().map(|step| step.step).collect::<Vec<_>>(), [0,1]);
    let Some(fem_core::ResultField::NodeVector { values, max_mag, .. }) = loaded[0].last().unwrap().field_by_name("Displacement") else { panic!("Missing displacement result") };
    assert_eq!(values.len(), mesh.nodes.len());
    assert!(*max_mag > 0.0);
}
