use bevy::prelude::*;
use fem_core::FemEntityRef;

#[derive(Resource, Default)]
pub struct SelectionState {
    pub entities: Vec<Entity>,

    pub targets: Vec<FemEntityRef>,
}

impl SelectionState {
    pub fn clear(&mut self) {
        self.entities.clear();
        self.targets.clear();
    }

    pub fn len(&self) -> usize {
        if self.targets.is_empty() {
            self.entities.len()
        } else {
            self.targets.len()
        }
    }
}
