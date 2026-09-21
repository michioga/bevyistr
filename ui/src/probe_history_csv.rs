//! Snapshot-based, asynchronous CSV export. Never changes result/setup state.
use crate::{
    layout::SidebarPage,
    probe_history_data::{Sample, samples},
    result_probe_pin::{ProbePin, Target},
};
use bevy::{
    prelude::*,
    ui_widgets::{Activate, Button as WidgetButton},
};
use fem_core::FemResultSet;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
};
use visualization::VisualizationSettings;

struct Snapshot {
    part: usize,
    kind: &'static str,
    id: String,
    field: String,
    samples: Vec<Sample>,
}

impl Snapshot {
    fn capture(results: &FemResultSet, part: usize, target: Target, field: &str) -> Option<Self> {
        let steps = results.by_mesh.get(part).filter(|s| !s.is_empty())?;
        let (kind, id) = match target {
            Target::Node { id, .. } => ("node", id.0.to_string()),
            Target::Element { id, .. } => ("element", id.0.to_string()),
        };
        Some(Self {
            part: part + 1, // Match the UI's one-based Part and Frame numbers.
            kind,
            id,
            field: field.into(),
            samples: samples(steps, target, field),
        })
    }

    fn description(&self) -> String {
        format!(
            "Part {} | {} {} | {} | {} frames",
            self.part,
            self.kind,
            self.id,
            self.field,
            self.samples.len()
        )
    }

    fn write(&self, out: &mut impl Write) -> std::io::Result<()> {
        out.write_all(b"part,target_kind,target_id,field,quantity,frame,step,time_or_load_factor,time_status,value,value_status,units\r\n")?;
        for (frame, sample) in self.samples.iter().enumerate() {
            let fields = [
                self.part.to_string(),
                self.kind.into(),
                self.id.clone(),
                self.field.clone(),
                sample.quantity.into(),
                (frame + 1).to_string(),
                sample.step.to_string(),
                sample.time.map_or_else(String::new, |v| v.to_string()),
                if sample.time.is_some() {
                    "recorded_or_default"
                } else {
                    "unavailable"
                }
                .into(),
                sample.value.map_or_else(String::new, |v| v.to_string()),
                if sample.value.is_some() {
                    "available"
                } else {
                    "unavailable"
                }
                .into(),
                "result/model units".into(),
            ];
            for (i, field) in fields.iter().enumerate() {
                if i != 0 {
                    out.write_all(b",")?;
                }
                // Quoting retains commas, quotes and newlines in solver labels.
                write!(out, "\"{}\"", field.replace('"', "\"\""))?;
            }
            out.write_all(b"\r\n")?;
        }
        Ok(())
    }
}

type Reply = Result<Option<PathBuf>, String>;

#[derive(Resource)]
pub(crate) struct ExportState {
    pending: Option<Mutex<mpsc::Receiver<Reply>>>,
    context: String,
    status: String,
    worker: fn(Snapshot) -> Reply,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            pending: None,
            context: String::new(),
            status: String::new(),
            worker: choose_and_save,
        }
    }
}

fn csv_path(mut path: PathBuf) -> Result<PathBuf, String> {
    match path.extension() {
        None => {
            path.set_extension("csv");
        }
        Some(ext) if ext.eq_ignore_ascii_case("csv") => {}
        Some(_) => return Err("Choose a .csv filename; no file was written.".into()),
    }
    Ok(path)
}

/// Write a sibling temporary file first, so errors do not truncate an existing
/// export. Never overwrite a newly appeared file without prior confirmation.
fn save(snapshot: &Snapshot, path: &Path, replace: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    snapshot
        .write(temp.as_file_mut())
        .map_err(|e| e.to_string())?;
    temp.as_file_mut().flush().map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    let result = if replace {
        temp.persist(path)
    } else {
        temp.persist_noclobber(path)
    };
    result.map(|_| ()).map_err(|e| e.error.to_string())
}

fn choose_and_save(snapshot: Snapshot) -> Reply {
    let Some(path) = rfd::FileDialog::new()
        .set_title(format!("Export history CSV: {}", snapshot.description()))
        .set_file_name(format!(
            "probe_part{}_{}_{}.csv",
            snapshot.part, snapshot.kind, snapshot.id
        ))
        .add_filter("CSV", &["csv"])
        .save_file()
    else {
        return Ok(None);
    };
    let path = csv_path(path)?;
    let replace = match std::fs::symlink_metadata(&path) {
        Ok(meta) => {
            if !meta.file_type().is_file() {
                return Err("The destination is not a regular file; no file was written.".into());
            }
            if rfd::MessageDialog::new()
                .set_title("Replace existing history CSV?")
                .set_description(path.display().to_string())
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::OkCancel)
                .show()
                != rfd::MessageDialogResult::Ok
            {
                return Ok(None);
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
    };
    save(&snapshot, &path, replace)?;
    Ok(Some(path))
}

fn activate(
    _: On<Activate>,
    page: Res<SidebarPage>,
    pin: Res<ProbePin>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    mut state: ResMut<ExportState>,
) {
    if *page != SidebarPage::Results || state.pending.is_some() {
        return;
    }
    let Some((_, part, target)) = pin.selection() else {
        return;
    };
    let Some(contour) = &settings.contour else {
        return;
    };
    let Some(snapshot) = Snapshot::capture(&results, part, target, &contour.field_name) else {
        return;
    };
    state.context = snapshot.description();
    let (tx, rx) = mpsc::channel();
    let worker = state.worker;
    match std::thread::Builder::new()
        .name("probe-csv-export".into())
        .spawn(move || {
            let _ = tx.send(worker(snapshot));
        }) {
        Ok(_) => {
            state.pending = Some(Mutex::new(rx));
            state.status = format!("Saving snapshot: {}", state.context);
        }
        Err(e) => state.status = format!("Cannot start CSV export: {e}"),
    }
}

#[derive(Component)]
pub(crate) struct ExportButton;
#[derive(Component)]
pub(crate) struct ExportStatus;

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Button,
            WidgetButton,
            ExportButton,
            bevy::input_focus::tab_navigation::TabIndex(0),
            Node {
                width: percent(100),
                min_height: px(26),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.14, 0.17)),
            BorderColor::all(Color::srgb(0.35, 0.55, 0.62)),
        ))
        .observe(activate)
        .with_child((
            Text::new("Export pinned history CSV..."),
            Pickable::IGNORE,
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
    parent.spawn((
        ExportStatus,
        Text::new("Pin a result to export its history."),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgb(0.8, 0.88, 0.92)),
    ));
}

pub(crate) fn update(
    mut commands: Commands,
    mut state: ResMut<ExportState>,
    page: Res<SidebarPage>,
    pin: Res<ProbePin>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    mut buttons: Query<
        (
            Entity,
            Option<&bevy::ui::InteractionDisabled>,
            &mut BackgroundColor,
            &Interaction,
        ),
        With<ExportButton>,
    >,
    mut labels: Query<&mut Text, With<ExportStatus>>,
) {
    let reply = state.pending.as_ref().and_then(|rx| match rx.lock() {
        Ok(rx) => match rx.try_recv() {
            Ok(reply) => Some(reply),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Export worker stopped.".into())),
        },
        Err(_) => Some(Err("Cannot read CSV export status.".into())),
    });
    if let Some(reply) = reply {
        state.pending = None;
        state.status = match reply {
            Ok(Some(path)) => format!("Saved: {}\n{}", path.display(), state.context),
            Ok(None) => format!("CSV export cancelled.\n{}", state.context),
            Err(e) => format!("CSV export failed: {e}\n{}", state.context),
        };
    }
    let available = *page == SidebarPage::Results
        && settings.contour.is_some()
        && pin
            .selection()
            .is_some_and(|(_, part, _)| results.by_mesh.get(part).is_some_and(|s| !s.is_empty()));
    for (entity, disabled, mut color, interaction) in &mut buttons {
        let inactive = !available || state.pending.is_some();
        if inactive && disabled.is_none() {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        } else if !inactive && disabled.is_some() {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        }
        color.set_if_neq(BackgroundColor(if inactive {
            Color::srgb(0.07, 0.08, 0.09)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.18, 0.22, 0.24)
        } else {
            Color::srgb(0.10, 0.14, 0.17)
        }));
    }
    let hint = if !available {
        "Pin a result to export its history."
    } else {
        "All frames | result/model units | missing values stay empty"
    };
    let message = if state.status.is_empty() {
        hint.into()
    } else {
        format!("{}\n{hint}", state.status)
    };
    for mut label in &mut labels {
        label.set_if_neq(Text::new(message.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{ElementId, NodeId, ResultField, StepResult};

    fn target() -> Target {
        Target::Node {
            index: 0,
            id: NodeId(71),
        }
    }
    fn step(step: u32, time: f32, field: &str, value: f32) -> StepResult {
        StepResult {
            step,
            time,
            fields: vec![ResultField::NodeScalar {
                name: field.into(),
                values: vec![value],
                min: value,
                max: value,
            }],
        }
    }
    fn results() -> FemResultSet {
        FemResultSet {
            by_mesh: vec![
                vec![step(0, 0., "P", 999.)],
                vec![
                    step(10, 0., "P", 1.2345678),
                    step(50, 0.25, "P", -4.),
                    step(90, 0.25, "P", f32::NAN),
                ],
            ],
            ..default()
        }
    }
    fn csv(snapshot: &Snapshot) -> String {
        let mut data = Vec::new();
        snapshot.write(&mut data).unwrap();
        String::from_utf8(data).unwrap()
    }

    #[test]
    fn snapshot_keeps_part_frame_step_and_missing_values_after_reload() {
        let mut results = results();
        let snapshot = Snapshot::capture(&results, 1, target(), "P").unwrap();
        results.by_mesh.clear(); // Export owns samples, not references into a live result.
        let text = csv(&snapshot);
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(
            lines[1],
            "\"2\",\"node\",\"71\",\"P\",\"scalar_or_component\",\"1\",\"10\",\"0\",\"recorded_or_default\",\"1.2345678\",\"available\",\"result/model units\""
        );
        assert!(lines[2].contains("\"2\",\"50\",\"0.25\""));
        assert!(
            lines[3].contains("\"3\",\"90\",\"0.25\",\"recorded_or_default\",\"\",\"unavailable\"")
        );
        assert!(!text.contains("999"));
        assert!(!text.contains("NaN"));
        assert_eq!(
            lines[1]
                .split(',')
                .nth(9)
                .unwrap()
                .trim_matches('"')
                .parse::<f32>()
                .unwrap(),
            1.2345678_f32
        );
    }

    #[test]
    fn quotes_unicode_newlines_and_backwards_times_are_preserved() {
        let name = "応力,\"XY\"\r\ncomponent";
        let results = FemResultSet {
            by_mesh: vec![vec![
                step(7, 2., name, 4.),
                step(1, -1., name, 4.),
                step(9, f32::INFINITY, name, f32::INFINITY),
            ]],
            ..default()
        };
        let text = csv(&Snapshot::capture(&results, 0, target(), name).unwrap());
        assert!(text.contains("\"応力,\"\"XY\"\"\r\ncomponent\""));
        assert!(text.contains("\"1\",\"7\",\"2\""));
        assert!(text.contains("\"2\",\"1\",\"-1\""));
        assert!(text.contains("\"3\",\"9\",\"\",\"unavailable\",\"\",\"unavailable\""));
    }

    #[test]
    fn element_and_vector_fields_share_the_plot_sampler_without_conversion() {
        let steps = vec![StepResult {
            fields: vec![
                ResultField::NodeVector {
                    name: "U".into(),
                    values: vec![Vec3::new(3., 4., 0.)],
                    min_mag: 5.,
                    max_mag: 5.,
                },
                ResultField::ElementScalar {
                    name: "S".into(),
                    values: vec![12.],
                    min: 12.,
                    max: 12.,
                },
            ],
            ..default()
        }];
        let results = FemResultSet {
            by_mesh: vec![steps.clone()],
            ..default()
        };
        let vector = Snapshot::capture(&results, 0, target(), "U").unwrap();
        assert_eq!(vector.samples[0].value, Some(5.));
        assert!(csv(&vector).contains("\"magnitude\""));
        let element = Target::Element {
            index: 0,
            id: ElementId(71),
        };
        let scalar = Snapshot::capture(&results, 0, element, "S").unwrap();
        assert_eq!(scalar.samples[0].value, Some(12.));
        assert!(csv(&scalar).contains("\"element\",\"71\""));
        assert_eq!(samples(&steps, element, "U")[0].value, None);
        assert_eq!(samples(&steps, target(), "S")[0].value, None);
        assert_eq!(samples(&steps, target(), "missing")[0].value, None);
        assert!(Snapshot::capture(&results, 5, target(), "U").is_none());
        assert!(Snapshot::capture(&FemResultSet::default(), 0, target(), "U").is_none());
    }

    #[test]
    fn atomic_save_requires_permission_to_replace_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.csv");
        let snapshot = Snapshot::capture(&results(), 1, target(), "P").unwrap();
        std::fs::write(&path, "original").unwrap();
        assert!(save(&snapshot, &path, false).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        save(&snapshot, &path, true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), csv(&snapshot));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(save(&snapshot, &dir.path().join("absent/history.csv"), false).is_err());
        let fresh = dir.path().join("fresh.csv");
        save(&snapshot, &fresh, false).unwrap();
        assert_eq!(std::fs::read_to_string(fresh).unwrap(), csv(&snapshot));
        assert_eq!(
            csv_path("history".into()).unwrap(),
            PathBuf::from("history.csv")
        );
        assert!(csv_path("history.CSV".into()).is_ok());
        assert!(csv_path("model.msh".into()).is_err());
    }

    #[test]
    fn ui_disables_without_target_and_reports_cancel_error_and_disconnect() {
        let mut app = App::new();
        app.insert_resource(SidebarPage::Results)
            .init_resource::<ProbePin>()
            .insert_resource(results())
            .insert_resource(VisualizationSettings {
                contour: Some(visualization::ContourSettings {
                    mesh_index: 0,
                    step_index: 0,
                    field_name: "P".into(),
                    show_deformation: true,
                    displacement_field: "U".into(),
                    deformation_scale: 100.,
                }),
                ..default()
            })
            .init_resource::<ExportState>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, update);
        app.update();
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<ExportButton>>()
            .single(app.world())
            .unwrap();
        assert!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(button)
                .is_some()
        );
        app.insert_resource(ProbePin::for_test(1, target()));
        app.update();
        assert!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(button)
                .is_none()
        );
        for (reply, expected) in [
            (Some(Ok(None)), "cancelled"),
            (Some(Err("disk full".into())), "disk full"),
            (None, "worker stopped"),
            (Some(Ok(Some("history.csv".into()))), "Saved:"),
        ] {
            let (tx, rx) = mpsc::channel();
            app.world_mut().resource_mut::<ExportState>().pending = Some(Mutex::new(rx));
            app.update();
            assert!(
                app.world()
                    .get::<bevy::ui::InteractionDisabled>(button)
                    .is_some()
            );
            if let Some(reply) = reply {
                tx.send(reply).unwrap();
            }
            drop(tx);
            app.update();
            assert!(app.world().resource::<ExportState>().pending.is_none());
            let text = app
                .world_mut()
                .query_filtered::<&Text, With<ExportStatus>>()
                .single(app.world())
                .unwrap();
            assert!(text.0.contains(expected), "{}", text.0);
        }
    }

    #[test]
    fn real_widget_click_starts_one_export_and_cancel_keeps_results() {
        use std::time::{Duration, Instant};
        let mut app = App::new();
        app.add_plugins(bevy::ui_widgets::ButtonPlugin)
            .insert_resource(SidebarPage::Results)
            .insert_resource(ProbePin::for_test(1, target()))
            .insert_resource(results())
            .insert_resource(VisualizationSettings {
                contour: Some(visualization::ContourSettings {
                    mesh_index: 0,
                    step_index: 1,
                    field_name: "P".into(),
                    show_deformation: true,
                    displacement_field: "U".into(),
                    deformation_scale: 500.,
                }),
                ..default()
            })
            .insert_resource(ExportState {
                worker: |snapshot| {
                    assert_eq!(snapshot.part, 2);
                    assert_eq!(snapshot.samples.len(), 3);
                    assert_eq!(snapshot.samples[0].value, Some(1.2345678));
                    Ok(None)
                },
                ..default()
            })
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, update);
        app.update();
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<ExportButton>>()
            .single(app.world())
            .unwrap();
        crate::widget_test_input::click(app.world_mut(), button);
        // A real pointer path must reach the observer, not just a test-only event.
        assert!(app.world().resource::<ExportState>().pending.is_some());
        assert!(
            app.world()
                .resource::<ExportState>()
                .context
                .contains("Part 2 | node 71 | P | 3 frames")
        );
        crate::widget_test_input::click(app.world_mut(), button); // Busy guard.
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<ExportState>().pending.is_some() {
            assert!(Instant::now() < deadline, "export worker did not finish");
            app.update();
            std::thread::yield_now();
        }
        assert!(
            app.world()
                .resource::<ExportState>()
                .status
                .contains("cancelled")
        );
        assert_eq!(app.world().resource::<FemResultSet>().by_mesh[1].len(), 3);
        assert_eq!(app.world().resource::<ProbePin>().selection().unwrap().1, 1);
        assert_eq!(
            app.world()
                .resource::<VisualizationSettings>()
                .contour
                .as_ref()
                .unwrap()
                .deformation_scale,
            500.
        );
        app.world_mut().resource_mut::<ProbePin>().clear();
        app.update();
        crate::widget_test_input::click(app.world_mut(), button);
        assert!(app.world().resource::<ExportState>().pending.is_none());
    }
}
