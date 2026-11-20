use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub(crate) struct StagedInputs<'a> {
    destination: &'a Path,
    temporary: tempfile::TempDir,
    generated: PathBuf,
    previous: PathBuf,
}

impl<'a> StagedInputs<'a> {
    pub(crate) fn new(destination: &'a Path) -> io::Result<Self> {
        let temporary = tempfile::Builder::new()
            .prefix(".bevyistr-input-")
            .tempdir_in(destination)?;
        let generated = temporary.path().join("generated");
        let previous = temporary.path().join("previous");
        fs::create_dir(&generated)?;
        fs::create_dir(&previous)?;
        Ok(Self {
            destination,
            temporary,
            generated,
            previous,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.generated
    }

    /// Publish in the supplied order, with the global control file last.
    pub(crate) fn publish(self, names: &[&str]) -> io::Result<()> {
        static PUBLISH: Mutex<()> = Mutex::new(());
        let _guard = PUBLISH.lock().unwrap_or_else(|error| error.into_inner());

        let mut existed = Vec::with_capacity(names.len());
        for name in names {
            let path = self.destination.join(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_file() => existed.push(true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => existed.push(false),
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{} is not a regular file", path.display()),
                    ));
                }
                Err(error) => return Err(error),
            }
        }

        let mut backups = Vec::new();
        let mut published = Vec::new();
        let result = (|| {
            for (name, existed) in names.iter().zip(existed) {
                let destination = self.destination.join(name);
                if existed {
                    fs::rename(&destination, self.previous.join(name))?;
                    backups.push(*name);
                }
                fs::rename(self.generated.join(name), &destination)?;
                published.push(*name);
            }
            Ok(())
        })();

        if let Err(error) = result {
            let mut failures = Vec::new();
            for name in published.into_iter().rev() {
                if let Err(restore) = fs::remove_file(self.destination.join(name)) {
                    failures.push(format!("could not remove {name}: {restore}"));
                }
            }
            for name in backups.into_iter().rev() {
                if let Err(restore) =
                    fs::rename(self.previous.join(name), self.destination.join(name))
                {
                    failures.push(format!("could not restore {name}: {restore}"));
                }
            }
            if !failures.is_empty() {
                let recovery = self.temporary.keep().join("previous");
                return Err(io::Error::other(format!(
                    "{error}; {}; remaining earlier files in {}",
                    failures.join("; "),
                    recovery.display(),
                )));
            }
            return Err(error);
        }
        Ok(())
    }
}

pub(crate) fn is_project_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\r', '\n', '/', '\\', ':', '\0'])
        && !matches!(name, "." | "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_overwritten_files_when_publish_fails_partway() {
        let destination = tempfile::tempdir().unwrap();
        let dir = destination.path();
        fs::write(dir.join("a.msh"), "old mesh").unwrap();
        fs::write(dir.join("hecmw_ctrl.dat"), "old control").unwrap();
        let inputs = StagedInputs::new(dir).unwrap();
        fs::write(inputs.path().join("a.msh"), "new mesh").unwrap();

        assert!(inputs.publish(&["a.msh", "hecmw_ctrl.dat"]).is_err());
        assert_eq!(fs::read_to_string(dir.join("a.msh")).unwrap(), "old mesh");
        assert_eq!(
            fs::read_to_string(dir.join("hecmw_ctrl.dat")).unwrap(),
            "old control"
        );
    }
}
