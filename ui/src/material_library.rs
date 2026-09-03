//! Library selections are drafts. Only final confirmation changes AnalysisSetup.
use crate::layout::{ScrollableList, SidebarPage};
use crate::material_catalog::{Catalog, LibraryUnits, default_path};
use crate::materials_ui::{SelectedEgrp, SelectedMaterialForSection};
use bevy::{prelude::*, ui::ScrollPosition};
use fem_core::{AnalysisSetup, FemMaterial};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
};

type LoadReply = Option<(PathBuf, Result<Catalog, String>)>;

#[derive(Resource)]
pub(crate) struct MaterialLibraryState {
    pub(crate) catalog: Option<Catalog>,
    pub(crate) selected: Option<String>,
    pub(crate) units: Option<LibraryUnits>,
    pub(crate) path: PathBuf,
    revision: u64,
    pub(crate) status: String,
    pending: Option<Mutex<Receiver<LoadReply>>>,
}
impl Default for MaterialLibraryState {
    fn default() -> Self {
        Self::from_path(default_path())
    }
}
impl MaterialLibraryState {
    pub(crate) fn from_path(path: PathBuf) -> Self {
        let result = Catalog::read(&path);
        let mut state = Self {
            catalog: None,
            selected: None,
            units: None,
            path: path.clone(),
            revision: 0,
            status: String::new(),
            pending: None,
        };
        state.install(path, result);
        state
    }
    pub(crate) fn install(&mut self, path: PathBuf, result: Result<Catalog, String>) {
        self.path = path;
        self.selected = None;
        self.revision += 1;
        match result {
            Ok(catalog) => {
                self.catalog = Some(catalog);
                self.status.clear();
            }
            Err(error) => {
                self.catalog = None;
                self.status = format!("Cannot load library: {error}");
            }
        }
    }
    pub(crate) fn draft(&self) -> Option<FemMaterial> {
        if self.pending.is_some() {
            return None;
        }
        let entry = self
            .catalog
            .as_ref()?
            .materials
            .iter()
            .find(|m| Some(&m.name) == self.selected.as_ref())?;
        Some(self.units?.material(entry))
    }
    fn start_load(&mut self, open_dialog: bool) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let path = self.path.clone();
        self.pending = Some(Mutex::new(rx));
        self.selected = None;
        self.status = if open_dialog {
            "Choose a TOML file..."
        } else {
            "Reloading..."
        }
        .into();
        std::thread::spawn(move || {
            let path = if open_dialog {
                rfd::FileDialog::new()
                    .set_title("Open material library")
                    .add_filter("Material library (TOML)", &["toml"])
                    .pick_file()
            } else {
                Some(path)
            };
            let _ = tx.send(path.map(|path| {
                let result = Catalog::read(&path);
                (path, result)
            }));
        });
    }
}

/// Reuse equal values; never overwrite a customized project record.
pub(crate) fn use_material(setup: &mut AnalysisSetup, mut material: FemMaterial) -> String {
    material.name = resolved_material_name(setup, &material);
    let name = material.name.clone();
    if setup.material_by_name(&name).is_none() {
        setup.materials.push(material);
    }
    name
}

pub(crate) fn resolved_material_name(setup: &AnalysisSetup, material: &FemMaterial) -> String {
    let mut material = material.clone();
    let base = material.name.clone();
    let mut suffix = 2;
    while let Some(existing) = setup.material_by_name(&material.name) {
        if *existing == material
            && setup
                .materials
                .iter()
                .filter(|m| m.name == material.name)
                .count()
                == 1
        {
            return existing.name.clone();
        }
        material.name = format!("{base}_{suffix}");
        suffix += 1;
    }
    material.name
}

enum LibraryAction {
    Open,
    Reload,
    Select(String),
    Units(LibraryUnits),
}
#[derive(Component)]
pub(crate) struct LibraryButton(LibraryAction);
#[derive(Component)]
pub(crate) struct LibraryList;
#[derive(Component)]
pub(crate) struct LibraryDetails;
#[derive(Component)]
pub(crate) struct LibraryPath;

pub(crate) fn spawn_material_library(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Text::new("Or choose from a TOML library"),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgb(0.7, 0.8, 0.84)),
    ));
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(4.0),
            ..default()
        })
        .with_children(|row| {
            library_button(
                row,
                "Open TOML...",
                LibraryAction::Open,
                "OpenMaterialLibrary",
            );
            library_button(
                row,
                "Reload",
                LibraryAction::Reload,
                "ReloadMaterialLibrary",
            );
        });
    parent.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(Color::srgb(0.58, 0.66, 0.70)),
        LibraryPath,
    ));
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(4.0),
            ..default()
        })
        .with_children(|row| {
            for units in [LibraryUnits::Metres, LibraryUnits::Millimetres] {
                library_button(
                    row,
                    units.label(),
                    LibraryAction::Units(units),
                    &format!("LibraryUnits_{units:?}"),
                );
            }
        });
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            max_height: px(116.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollableList,
        ScrollPosition::default(),
        LibraryList,
    ));
    parent.spawn((
        Text::new("Select a material; confirm below"),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(Color::srgb(0.68, 0.74, 0.77)),
        LibraryDetails,
    ));
}

fn library_button(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    action: LibraryAction,
    name: &str,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                min_height: px(25.0),
                padding: UiRect::axes(px(6.0), px(3.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.12, 0.14)),
            BorderColor::all(Color::srgb(0.34, 0.40, 0.44)),
            LibraryButton(action),
            Name::new(name.to_owned()),
        ))
        .with_child((
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(Color::srgb(0.88, 0.92, 0.94)),
        ));
}

pub(crate) fn material_library_system(
    mut commands: Commands,
    page: Res<SidebarPage>,
    target: Res<SelectedEgrp>,
    mut state: ResMut<MaterialLibraryState>,
    mut selected: ResMut<SelectedMaterialForSection>,
    mut buttons: Query<(Ref<Interaction>, &LibraryButton, &mut BackgroundColor)>,
    lists: Query<Entity, With<LibraryList>>,
    children: Query<&Children>,
    mut revision: Local<Option<u64>>,
    mut details: Query<&mut Text, With<LibraryDetails>>,
    mut paths: Query<&mut Text, (With<LibraryPath>, Without<LibraryDetails>)>,
) {
    let reply = state
        .pending
        .as_ref()
        .map(|rx| rx.lock().unwrap().try_recv());
    match reply {
        Some(Ok(reply)) => {
            state.pending = None;
            if let Some((path, result)) = reply {
                state.install(path, result);
            } else {
                state.status = "File selection cancelled".into();
            }
        }
        Some(Err(TryRecvError::Disconnected)) => {
            state.pending = None;
            state.status = "Library load interrupted; try again".into();
        }
        _ => {}
    }
    for (interaction, button, _) in &buttons {
        if *page != SidebarPage::Materials
            || target.0.is_none()
            || *interaction != Interaction::Pressed
            || !interaction.is_changed()
            || state.pending.is_some()
        {
            continue;
        }
        match &button.0 {
            LibraryAction::Open => state.start_load(true),
            LibraryAction::Reload => state.start_load(false),
            LibraryAction::Select(name) => {
                if state
                    .catalog
                    .as_ref()
                    .is_some_and(|c| c.materials.iter().any(|m| &m.name == name))
                {
                    state.selected = Some(name.clone());
                    selected.0 = None;
                    state.status.clear();
                }
            }
            LibraryAction::Units(units) => state.units = Some(*units),
        }
    }
    if *revision != Some(state.revision) {
        *revision = Some(state.revision);
        for list in &lists {
            if let Ok(children) = children.get(list) {
                for &child in children {
                    commands.entity(child).despawn();
                }
            }
            commands.entity(list).with_children(|list| {
                if let Some(catalog) = &state.catalog {
                    for entry in &catalog.materials {
                        library_button(
                            list,
                            &entry.label,
                            LibraryAction::Select(entry.name.clone()),
                            &format!("Library_{}", entry.name),
                        );
                    }
                }
            });
        }
    }
    for (interaction, button, mut bg) in &mut buttons {
        let active = match &button.0 {
            LibraryAction::Select(name) => state.selected.as_ref() == Some(name),
            LibraryAction::Units(units) => state.units == Some(*units),
            _ => false,
        };
        bg.set_if_neq(BackgroundColor(if state.pending.is_some() {
            Color::srgb(0.06, 0.07, 0.08)
        } else if active {
            Color::srgb(0.18, 0.45, 0.55)
        } else if *interaction != Interaction::None {
            Color::srgb(0.18, 0.22, 0.24)
        } else {
            Color::srgb(0.10, 0.12, 0.14)
        }));
    }
    for mut path in &mut paths {
        path.set_if_neq(Text::new(format!("File: {}", state.path.display())));
    }
    let text = if !state.status.is_empty() {
        state.status.clone()
    } else if let Some(entry) = state.catalog.as_ref().and_then(|c| {
        c.materials
            .iter()
            .find(|m| Some(&m.name) == state.selected.as_ref())
    }) {
        let values = state
            .draft()
            .map(|mat| {
                format!(
                    "{}\nE={:.6e}  nu={}  rho={}",
                    state.units.unwrap().label(),
                    mat.young_modulus.unwrap(),
                    mat.poisson_ratio.unwrap(),
                    mat.density
                        .map(|v| format!("{v:.6e}"))
                        .unwrap_or_else(|| "unspecified".into())
                )
            })
            .unwrap_or_else(|| "Choose model units above before confirming".into());
        let source = if !entry.source.is_empty() {
            &entry.source
        } else if !entry.source_url.is_empty() {
            &entry.source_url
        } else {
            "User-defined (verify constants)"
        };
        format!(
            "{values}\nSource: {source}\n{}\nSelection only; Confirm assignment applies it",
            entry.note
        )
    } else {
        "Choose model units and a library material. No project values change until confirmation."
            .into()
    };
    for mut details in &mut details {
        details.set_if_neq(Text::new(&text));
    }
}
