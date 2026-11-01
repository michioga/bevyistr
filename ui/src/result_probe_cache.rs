//! Geometry-query lifetime, independent of tooltip layout and display modes.
use bevy::prelude::*;
use fem_core::{FemModel, FemResultSet};
use visualization::{ContourSettings, result_probe::ProbeSurface};

#[cfg(test)]
#[path = "result_probe_cache_tests.rs"]
mod tests;

#[derive(Resource, Default)]
pub(crate) struct ProbeCache {
    contour: Option<ContourSettings>,
    surfaces: Option<Vec<(usize, ProbeSurface)>>,
    #[cfg(test)]
    pub builds: usize,
}

impl ProbeCache {
    /// Run even when playback or a popup prevents hovering. Discard stale
    /// geometry immediately, but defer the expensive build until a query.
    pub fn sync(&mut self, contour: Option<&ContourSettings>, data_changed: bool) {
        if data_changed || self.contour.as_ref() != contour {
            self.surfaces = None;
            self.contour = contour.cloned();
        }
    }

    pub fn surfaces(
        &mut self,
        model: &FemModel,
        results: &FemResultSet,
        contour: &ContourSettings,
    ) -> &[(usize, ProbeSurface)] {
        if self.surfaces.is_none() {
            self.surfaces = Some(
                model
                    .meshes
                    .iter()
                    .enumerate()
                    .filter_map(|(i, mesh)| {
                        let step = results.by_mesh.get(i)?.get(contour.step_index)?;
                        Some((i, ProbeSurface::build(mesh, step, contour)))
                    })
                    .collect(),
            );
            #[cfg(test)]
            {
                self.builds += 1;
            }
        }
        self.surfaces.as_deref().unwrap()
    }
}
