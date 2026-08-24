use bevy::prelude::*;
use fem_core::{FemEntityRef, SelectionHit, SelectionLevel};

#[derive(Resource, Default)]
pub struct HoverResult {
    pub entity: Option<Entity>,

    pub hit: Option<SelectionHit>,
}

impl HoverResult {
    pub fn clear(&mut self) {
        self.entity = None;
        self.hit = None;
    }

    pub fn set_entity(&mut self, entity: Entity, hit: SelectionHit) {
        self.entity = Some(entity);
        self.hit = Some(hit);
    }

    pub fn set_hit(&mut self, hit: SelectionHit) {
        self.entity = None;
        self.hit = Some(hit);
    }

    pub fn target(&self) -> Option<FemEntityRef> {
        self.hit.map(|hit| hit.target)
    }

    pub fn level(&self) -> Option<SelectionLevel> {
        self.hit.map(SelectionHit::level)
    }
}
