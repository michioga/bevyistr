//! User preferences, not project data. Never restore an active export target.
use crate::solver_process::SolverLaunchMode;
use crate::solver_runner::FrontistrRunState;
use bevy::prelude::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SolverPreferences {
    pub(crate) executable: PathBuf,
    pub(crate) partitioner: Option<PathBuf>,
    pub(crate) mpi_launcher: Option<PathBuf>,
    pub(crate) launch_mode: SolverLaunchMode,
    pub(crate) mpi_ranks: u16,
    pub(crate) openmp_threads: u16,
    pub(crate) last_output_directory: Option<PathBuf>,
}

impl Default for SolverPreferences {
    fn default() -> Self {
        Self {
            executable: "fistr1".into(),
            partitioner: None,
            mpi_launcher: None,
            launch_mode: SolverLaunchMode::Direct,
            mpi_ranks: 4,
            openmp_threads: 1,
            last_output_directory: None,
        }
    }
}

impl SolverPreferences {
    fn read(document: &DocumentMut) -> Result<Self, String> {
        let mut settings = Self::default();
        let Some(table) = document.get("frontistr") else {
            return Ok(settings);
        };
        if !table.is_table() {
            return Err("[frontistr] must be a TOML table".into());
        }
        let string = |key: &str| -> Result<Option<&str>, String> {
            table
                .get(key)
                .map(|item| {
                    item.as_str()
                        .ok_or_else(|| format!("frontistr.{key} must be a string"))
                })
                .transpose()
        };
        let path = |key: &str| -> Result<Option<PathBuf>, String> {
            Ok(string(key)?
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from))
        };
        settings.executable = path("executable")?.unwrap_or_else(|| "fistr1".into());
        settings.partitioner = path("partitioner")?;
        settings.mpi_launcher = path("mpi_launcher")?;
        settings.last_output_directory = path("last_output_directory")?;
        if let Some(mode) = string("launch_mode")? {
            settings.launch_mode = match mode {
                "direct" => SolverLaunchMode::Direct,
                "mpi" => SolverLaunchMode::Mpi,
                _ => return Err("frontistr.launch_mode must be 'direct' or 'mpi'".into()),
            };
        }
        if let Some(ranks) = table.get("mpi_ranks") {
            settings.mpi_ranks = ranks
                .as_integer()
                .filter(|n| (1..=4096).contains(n))
                .ok_or("frontistr.mpi_ranks must be an integer from 1 to 4096")?
                as u16;
        }
        if let Some(threads) = table.get("openmp_threads") {
            settings.openmp_threads = threads
                .as_integer()
                .filter(|n| (1..=4096).contains(n))
                .ok_or("frontistr.openmp_threads must be an integer from 1 to 4096")?
                as u16;
        }
        Ok(settings)
    }

    fn apply_environment(&mut self) {
        for (name, target) in [
            ("FRONTISTR_PARTITIONER", &mut self.partitioner),
            ("FRONTISTR_MPI_LAUNCHER", &mut self.mpi_launcher),
        ] {
            if let Some(path) = std::env::var_os(name).filter(|s| !s.is_empty()) {
                *target = Some(path.into());
            }
        }
        if let Some(path) = std::env::var_os("FRONTISTR_EXECUTABLE").filter(|s| !s.is_empty()) {
            self.executable = path.into();
        }
        if let Ok(mode) = std::env::var("FRONTISTR_LAUNCH_MODE") {
            match mode.to_ascii_lowercase().as_str() {
                "mpi" => self.launch_mode = SolverLaunchMode::Mpi,
                "direct" => self.launch_mode = SolverLaunchMode::Direct,
                _ => {}
            }
        }
        if let Some(ranks) = std::env::var("FRONTISTR_MPI_RANKS")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|n| (1..=4096).contains(n))
        {
            self.mpi_ranks = ranks;
        }
        if let Some(threads) = std::env::var("FRONTISTR_OPENMP_THREADS")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|n| (1..=4096).contains(n))
        {
            self.openmp_threads = threads;
        }
    }
}

#[derive(Resource)]
pub(crate) struct AppSettings {
    path: Option<PathBuf>,
    pub(crate) solver: SolverPreferences,
    warning: Option<String>,
}

impl AppSettings {
    pub(crate) fn load_default() -> Self {
        let mut settings = match default_path() {
            Ok(path) => Self::load(path),
            Err(error) => Self {
                path: None,
                solver: Default::default(),
                warning: Some(error),
            },
        };
        settings.solver.apply_environment();
        settings
    }

    fn load(path: PathBuf) -> Self {
        match read_document(&path).and_then(|doc| SolverPreferences::read(&doc)) {
            Ok(solver) => Self {
                path: Some(path),
                solver,
                warning: None,
            },
            Err(error) => Self {
                path: Some(path),
                solver: Default::default(),
                warning: Some(error),
            },
        }
    }

    fn save_changed(&mut self, solver: SolverPreferences) {
        if solver == self.solver {
            return;
        }
        self.solver = solver;
        // Retry on the next edit, not every frame if the disk is read-only.
        self.warning = match &self.path {
            Some(path) => save_preferences(path, &self.solver).err(),
            None => Some("No user configuration directory available".into()),
        };
    }

    fn label(&self) -> String {
        let path = self
            .path
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "unavailable".into());
        match &self.warning {
            Some(warning) => format!("Settings: {path}\nNot saved/loaded: {warning}"),
            None => format!("Settings: {path}\nExecution preferences are saved automatically."),
        }
    }
}

fn default_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("BEVYISTR_CONFIG").filter(|s| !s.is_empty()) {
        return std::path::absolute(PathBuf::from(path)).map_err(|e| e.to_string());
    }
    config_path(cfg!(windows), |key| {
        std::env::var_os(key).map(PathBuf::from)
    })
    .ok_or_else(|| "Set BEVYISTR_CONFIG to a writable bevyistr.toml path".into())
}

fn config_path(windows: bool, get: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let absolute = |key| get(key).filter(|p| p.is_absolute());
    let base = if windows {
        absolute("APPDATA").or_else(|| absolute("USERPROFILE").map(|p| p.join("AppData/Roaming")))
    } else {
        absolute("XDG_CONFIG_HOME").or_else(|| absolute("HOME").map(|p| p.join(".config")))
    }?;
    Some(base.join("bevyistr/bevyistr.toml"))
}

fn read_document(path: &Path) -> Result<DocumentMut, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DocumentMut::new()),
        Err(e) => return Err(e.to_string()),
    };
    text.parse::<DocumentMut>()
        .map_err(|e| format!("Invalid bevyistr.toml: {e}"))
}

fn save_preferences(path: &Path, settings: &SolverPreferences) -> Result<(), String> {
    // Re-read before saving: preserve unrelated tables/keys, and never replace
    // a malformed file with defaults after a manual editing mistake.
    let mut doc = read_document(path)?;
    SolverPreferences::read(&doc)?;
    if doc.get("frontistr").is_none() {
        doc["frontistr"] = Item::Table(Default::default());
    }
    for (key, path) in [
        ("executable", Some(settings.executable.as_path())),
        ("partitioner", settings.partitioner.as_deref()),
        ("mpi_launcher", settings.mpi_launcher.as_deref()),
        (
            "last_output_directory",
            settings.last_output_directory.as_deref(),
        ),
    ] {
        let path = path
            .map(|p| p.to_str().ok_or("Path cannot be represented in UTF-8 TOML"))
            .transpose()?
            .unwrap_or("");
        doc["frontistr"][key] = value(path);
    }
    doc["frontistr"]["launch_mode"] = value(match settings.launch_mode {
        SolverLaunchMode::Direct => "direct",
        SolverLaunchMode::Mpi => "mpi",
    });
    doc["frontistr"]["mpi_ranks"] = value(i64::from(settings.mpi_ranks));
    doc["frontistr"]["openmp_threads"] = value(i64::from(settings.openmp_threads));
    let parent = path
        .parent()
        .ok_or("Settings path has no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // Same-directory temporary file + replacement, including on Windows.
    // A failed write leaves the previous settings intact.
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(doc.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Component)]
pub(crate) struct AppSettingsText;

pub(crate) fn save_settings_system(
    state: Res<FrontistrRunState>,
    mut settings: ResMut<AppSettings>,
) {
    if state.is_changed() {
        settings.save_changed(state.preferences());
    }
}

pub(crate) fn update_settings_text_system(
    settings: Res<AppSettings>,
    mut labels: Query<&mut Text, With<AppSettingsText>>,
) {
    for mut label in &mut labels {
        label.set_if_neq(Text::new(settings.label()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_reload_paths_modes_and_output_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/bevyistr.toml");
        let mut store = AppSettings::load(path.clone());
        let prefs = SolverPreferences {
            executable: PathBuf::from("C:\\Program Files\\解析's tools\\fistr1.exe"),
            partitioner: Some("/opt/frontistr/hecmw_part1".into()),
            mpi_launcher: Some("/opt/mpi/mpiexec".into()),
            launch_mode: SolverLaunchMode::Mpi,
            mpi_ranks: 8,
            openmp_threads: 4,
            last_output_directory: Some(dir.path().into()),
        };
        store.save_changed(prefs.clone());
        assert!(store.warning.is_none(), "{:?}", store.warning);
        assert_eq!(AppSettings::load(path.clone()).solver, prefs);
        let mut next = prefs.clone();
        next.mpi_ranks = 2;
        store.save_changed(next.clone());
        assert!(store.warning.is_none());
        assert_eq!(AppSettings::load(path).solver, next);
        let state = FrontistrRunState::from_preferences(&prefs);
        assert!(
            state.project().is_none(),
            "old output hint must not become a run target"
        );
        assert_eq!(state.preferences(), prefs);
    }

    #[test]
    fn malformed_or_invalid_settings_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bevyistr.toml");
        for text in [
            "[bad",
            "[frontistr]\nmpi_ranks = 0",
            "[frontistr]\nexecutable = 42",
            "[frontistr]\nlaunch_mode = 'unknown'",
        ] {
            std::fs::write(&path, text).unwrap();
            let mut store = AppSettings::load(path.clone());
            assert!(store.warning.is_some());
            let mut prefs = SolverPreferences::default();
            prefs.mpi_ranks = 3;
            store.save_changed(prefs);
            assert!(store.warning.is_some());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn save_preserves_unrelated_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bevyistr.toml");
        std::fs::write(
            &path,
            "# user comment\n[view]\ncolor = 'material'\n[frontistr]\ncustom = 'keep'\n",
        )
        .unwrap();
        save_preferences(&path, &SolverPreferences::default()).unwrap();
        let doc = read_document(&path).unwrap();
        assert_eq!(doc["view"]["color"].as_str(), Some("material"));
        assert_eq!(doc["frontistr"]["custom"].as_str(), Some("keep"));
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains("# user comment")
        );
    }

    #[test]
    fn config_location_is_independent_of_working_directory() {
        let root = std::env::temp_dir();
        let expected = root.join("bevyistr/bevyistr.toml");
        assert_eq!(
            config_path(true, |k| (k == "APPDATA").then(|| root.clone())),
            Some(expected.clone())
        );
        assert_eq!(
            config_path(false, |k| (k == "XDG_CONFIG_HOME").then(|| root.clone())),
            Some(expected)
        );
        assert_eq!(
            config_path(false, |k| match k {
                "XDG_CONFIG_HOME" => Some("relative".into()),
                "HOME" => Some(root.clone()),
                _ => None,
            }),
            Some(root.join(".config/bevyistr/bevyistr.toml"))
        );
    }

    #[test]
    fn unwritable_settings_report_failure_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bevyistr.toml");
        std::fs::create_dir(&path).unwrap();
        let mut store = AppSettings::load(path.clone());
        let mut prefs = SolverPreferences::default();
        prefs.mpi_ranks = 3;
        store.save_changed(prefs);
        assert!(store.warning.is_some());
        assert!(path.is_dir());
    }

    #[test]
    fn preferences_system_saves_changes_but_not_startup_or_transient_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bevyistr.toml");
        let store = AppSettings::load(path.clone());
        let mut prefs = store.solver.clone();
        let mut app = App::new();
        app.insert_resource(FrontistrRunState::from_preferences(&prefs));
        app.insert_resource(store);
        app.add_systems(Update, save_settings_system);
        app.update();
        assert!(!path.exists());
        prefs.executable = "C:/FrontISTR/bin/fistr1.exe".into();
        prefs.launch_mode = SolverLaunchMode::Mpi;
        prefs.mpi_ranks = 2;
        *app.world_mut().resource_mut::<FrontistrRunState>() =
            FrontistrRunState::from_preferences(&prefs);
        app.update();
        assert_eq!(AppSettings::load(path.clone()).solver, prefs);
        let before = std::fs::read(&path).unwrap();
        app.world_mut()
            .resource_mut::<FrontistrRunState>()
            .clear_export_target();
        app.update();
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
