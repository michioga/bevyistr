//! Result files belonging to one run. Snapshotting is read-only: old results
//! remain on disk, but are never silently mixed into this run's timeline.
use fem_core::{ElementId, NodeId, StepResult};
use hecmw::distributed_nodes::{read_partition_owners, read_partitioned_step};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: SystemTime,
}
type Snapshot = BTreeMap<(u32, u16), (PathBuf, Stamp)>;

#[derive(Clone)]
pub(crate) struct RunResultSource {
    pub(crate) id: u64,
    pub(crate) model_version: u64,
    directory: PathBuf,
    stem: String,
    ranks: u16,
    before: Snapshot,
    completed: Option<Snapshot>,
    pub(crate) partition_prefix: Option<String>,
}

fn stamp(path: &Path) -> Result<Stamp, String> {
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err(format!("Not a result file: {}", path.display()));
    }
    Ok(Stamp {
        bytes: metadata.len(),
        modified: metadata.modified().map_err(|e| e.to_string())?,
    })
}

fn snapshot(directory: &Path, stem: &str) -> Result<Snapshot, String> {
    let prefix = format!("{stem}.res.");
    let mut snapshot = BTreeMap::new();
    for entry in directory.read_dir().map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(suffix) = name.to_str().and_then(|n| n.strip_prefix(&prefix)) else {
            continue;
        };
        let mut fields = suffix.split('.');
        let (Some(rank), Some(step), None) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let (Ok(rank), Ok(step)) = (rank.parse::<u16>(), step.parse::<u32>()) else {
            continue;
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let meta = stamp(&path)?;
        if snapshot.insert((step, rank), (path, meta)).is_some() {
            return Err("Ambiguous result filenames".into());
        }
    }
    Ok(snapshot)
}

impl RunResultSource {
    pub(crate) fn capture(
        directory: &Path,
        stem: &str,
        ranks: u16,
        model_version: u64,
    ) -> Result<Self, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            model_version,
            before: snapshot(directory, stem)?,
            completed: None,
            directory: directory.into(),
            stem: stem.into(),
            ranks,
            partition_prefix: None,
        })
    }

    /// Freeze the completed run before a later external solver invocation
    /// can replace its values in the same directory.
    pub(crate) fn finish(&mut self) -> Result<(), String> {
        self.completed = Some(snapshot(&self.directory, &self.stem)?);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn load(&self, parts: &[Vec<NodeId>]) -> Result<Vec<Vec<StepResult>>, String> {
        self.load_with_elements(parts, &[])
    }
    #[cfg(test)]
    pub(crate) fn load_with_elements(
        &self,
        parts: &[Vec<NodeId>],
        elements: &[Vec<ElementId>],
    ) -> Result<Vec<Vec<StepResult>>, String> {
        self.load_with_mesh_ids(parts, elements, None)
    }

    pub(crate) fn load_with_mesh_ids(
        &self,
        parts: &[Vec<NodeId>],
        elements: &[Vec<ElementId>],
        used_nodes: Option<&[HashSet<NodeId>]>,
    ) -> Result<Vec<Vec<StepResult>>, String> {
        if parts.iter().all(Vec::is_empty) {
            return Err("No model nodes for these results".into());
        }
        let mut partition_stamps = Vec::new();
        let owners = if let Some(prefix) = &self.partition_prefix {
            let paths: Vec<_> = (0..self.ranks)
                .map(|rank| self.directory.join(format!("{prefix}.{rank}")))
                .collect();
            for path in &paths {
                partition_stamps.push((path.clone(), stamp(path)?));
            }
            Some(read_partition_owners(&paths)?)
        } else if self.ranks > 1 {
            return Err("MPI result loading requires this run's distributed node ownership".into());
        } else {
            None
        };
        let after = snapshot(&self.directory, &self.stem)?;
        if self
            .completed
            .as_ref()
            .is_some_and(|completed| completed != &after)
        {
            return Err("Result files changed after this run completed. Run again or open external results manually.".into());
        }
        let fresh: Snapshot = after
            .into_iter()
            .filter(|(key, value)| self.before.get(key) != Some(value))
            .collect();
        let steps: std::collections::BTreeSet<u32> = fresh
            .keys()
            .filter(|(_, rank)| *rank < self.ranks)
            .map(|(step, _)| *step)
            .collect();
        if steps.is_empty() {
            return Err("No new FrontISTR .res files from this run. Check result output settings and the solver log; older files were not loaded.".into());
        }
        let mut by_mesh: Vec<Vec<StepResult>> = parts.iter().map(|_| Vec::new()).collect();
        for step in steps {
            let paths = (0..self.ranks).map(|rank| {
                let (path, expected) = fresh.get(&(step, rank))
                    .ok_or_else(|| format!("Step {step}: missing/unchanged result for MPI rank {rank}. No partial result was loaded."))?;
                if expected.bytes == 0 {
                    return Err(format!("Empty result file: {}", path.display()));
                }
                Ok(path.as_path())
            }).collect::<Result<Vec<_>, String>>()?;
            let raw = read_partitioned_step(&paths, owners.as_deref())?;
            for (mi, (node_ids, results)) in parts.iter().zip(&mut by_mesh).enumerate() {
                let element_ids: &[ElementId] = elements.get(mi).map_or(&[], Vec::as_slice);
                let result = if let Some(used) = used_nodes.and_then(|sets| sets.get(mi)) {
                    raw.step_with_mesh_ids(node_ids, element_ids, used, step)
                } else {
                    raw.step(node_ids, element_ids, step)
                };
                results.push(result.map_err(|e| format!("Step {step}, part {}: {e}", mi + 1))?);
            }
        }
        // A second writer must not turn a completed scan into mixed data.
        for (path, expected) in fresh.values().chain(partition_stamps.iter()) {
            if &stamp(path)? != expected {
                return Err("Result files changed during loading; try again.".into());
            }
        }
        Ok(by_mesh)
    }
}

#[cfg(test)]
#[path = "run_results_benchmark.rs"]
mod benchmark;

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: u32, value: f32) -> String {
        format!("*fstrresult\n1 0\n1 0\n3\nDISPLACEMENT\n{id}\n{value} 0 0\n")
    }
    fn parallel_source(dir: &Path, ids: [u32; 2], version: u64) -> RunResultSource {
        let mut source = RunResultSource::capture(dir, "job", 2, version).unwrap();
        source.partition_prefix = Some("parts".into());
        for (rank, id) in ids.into_iter().enumerate() {
            std::fs::write(dir.join(format!("parts.{rank}")), format!(
                "!HECMW-DMD-ASCII version=5\n0\n0\n1\n1\n5\n0\nmesh\n0\n0\n0.0\n1 1 1 1\n1 {rank}\n{id}\n0.0 0.0 0.0\n3 1\n0 3\n3\n0 0 0\n"
            )).unwrap();
        }
        source
    }
    #[test]
    fn automatic_results_keep_unused_nodes_unavailable_but_reject_missing_used_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = RunResultSource::capture(dir.path(), "job", 1, 0).unwrap();
        std::fs::write(dir.path().join("job.res.0.1"), record(10, 2.0)).unwrap();
        source.finish().unwrap();
        let ids = [vec![NodeId(20), NodeId(10)]];
        let used = [HashSet::from([NodeId(10)])];
        let steps = source.load_with_mesh_ids(&ids, &[], Some(&used)).unwrap();
        let Some(fem_core::ResultField::NodeVector { values, .. }) =
            steps[0][0].field_by_name("Displacement")
        else { panic!("missing displayed displacement") };
        assert!(values[0].is_nan());
        assert_eq!(values[1].x, 2.0);

        let used = [HashSet::from([NodeId(10), NodeId(20)])];
        assert!(source.load_with_mesh_ids(&ids, &[], Some(&used))
            .unwrap_err().contains("Missing result for node 20"));
    }

    #[test]
    fn new_numeric_steps_merge_ranks_and_map_assembly_ids() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("job.res.0.99"), record(10, 99.0)).unwrap();
        let source = parallel_source(dir.path(), [10, 30], 4);
        for step in [10, 2] {
            std::fs::write(
                dir.path().join(format!("job.res.0.{step}")),
                record(10, 1.0),
            )
            .unwrap();
            std::fs::write(
                dir.path().join(format!("job.res.1.{step}")),
                record(30, 2.0),
            )
            .unwrap();
        }
        let results = source.load(&[vec![NodeId(30)], vec![NodeId(10)]]).unwrap();
        assert_eq!(
            results[0].iter().map(|s| s.step).collect::<Vec<_>>(),
            [2, 10]
        );
        let fem_core::ResultField::NodeVector {
            values,
            min_mag,
            max_mag,
            ..
        } = &results[0][0].fields[0]
        else {
            panic!()
        };
        assert_eq!(values[0].x, 2.0);
        assert_eq!((*min_mag, *max_mag), (1.0, 2.0));
    }
    #[test]
    fn missing_rank_stale_files_and_truncation_do_not_become_zero_results() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("job.res.1.1"), record(2, 2.0)).unwrap();
        let source = parallel_source(dir.path(), [1, 2], 0);
        assert!(
            source
                .load(&[vec![NodeId(1)]])
                .unwrap_err()
                .contains("No new")
        );
        std::fs::write(dir.path().join("job.res.0.1"), record(1, 1.0)).unwrap();
        assert!(
            source
                .load(&[vec![NodeId(1)]])
                .unwrap_err()
                .contains("rank 1")
        );
        std::fs::write(dir.path().join("job.res.1.1"), "*fstrresult\n").unwrap();
        assert!(source.load(&[vec![NodeId(1)]]).is_err());
    }

    #[test]
    fn later_external_results_are_not_relabelled_as_this_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = RunResultSource::capture(dir.path(), "job", 1, 0).unwrap();
        let path = dir.path().join("job.res.0.1");
        std::fs::write(&path, record(1, 1.0)).unwrap();
        source.finish().unwrap();
        assert!(source.load(&[vec![NodeId(1)]]).is_ok());
        std::fs::write(&path, record(1, 1000.0)).unwrap();
        assert!(
            source
                .load(&[vec![NodeId(1)]])
                .unwrap_err()
                .contains("after this run")
        );
    }
}
