//! Shared Run/Export destination selection. A remembered directory is only
//! a dialog hint; choosing a destination is explicit for each loaded model.
use std::path::{Path, PathBuf};

pub(crate) fn project_stem(source: Option<&Path>) -> String {
    source
        .and_then(Path::file_stem)
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("mesh")
        .to_string()
}

fn existing_inputs(directory: &Path, stem: &str) -> Vec<PathBuf> {
    [
        "hecmw_ctrl.dat".to_string(),
        "hecmw_part_ctrl.dat".to_string(),
        format!("{stem}.msh"),
        format!("{stem}.cnt"),
    ]
    .into_iter()
    .map(|name| directory.join(name))
    .filter(|path| path.exists())
    .collect()
}

pub(crate) fn choose_output_directory(stem: &str, previous: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Choose FrontISTR output folder (inputs will be written here)");
    if let Some(previous) = previous.filter(|p| p.is_dir()) {
        dialog = dialog.set_directory(previous);
    }
    let directory = dialog.pick_folder()?;
    let existing = existing_inputs(&directory, stem);
    if !existing.is_empty() {
        let names = existing
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let choice = rfd::MessageDialog::new()
            .set_title("Replace existing FrontISTR input files?")
            .set_description(format!("The current model will replace these files:\n{names}\n\nChoose Cancel to keep them and use another folder."))
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show();
        if choice != rfd::MessageDialogResult::Ok {
            return None;
        }
    }
    Some(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_input_targets_require_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        assert!(existing_inputs(dir.path(), "hinge").is_empty());
        for name in ["other.msh", "hinge.res", "hinge.cnt", "hecmw_ctrl.dat"] {
            std::fs::write(dir.path().join(name), "original").unwrap();
        }
        let found = existing_inputs(dir.path(), "hinge");
        assert_eq!(found.len(), 2);
        assert!(found.contains(&dir.path().join("hinge.cnt")));
        assert!(found.contains(&dir.path().join("hecmw_ctrl.dat")));
        assert_eq!(project_stem(Some(Path::new("models/hinge.msh"))), "hinge");
        assert_eq!(project_stem(None), "mesh");
    }
}
