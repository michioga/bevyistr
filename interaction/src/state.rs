use bevy::prelude::*;
use fem_core::{FemEntityId, SelectionLevel};

#[derive(Resource, Default)]
pub struct HoverResult {
    pub entity: Option<Entity>,

    pub target: Option<FemEntityId>,

    pub level: Option<SelectionLevel>,

    pub world_position: Option<Vec3>,
}

impl HoverResult {
    pub fn clear(&mut self) {
        self.entity = None;
        self.target = None;
        self.level = None;
        self.world_position = None;
    }

    pub fn set_entity(&mut self, entity: Entity, target: FemEntityId, world_position: Vec3) {
        self.entity = Some(entity);
        self.target = Some(target);
        self.level = Some(target.level());
        self.world_position = Some(world_position);
    }

    pub fn set_target(&mut self, target: FemEntityId, world_position: Vec3) {
        self.entity = None;
        self.target = Some(target);
        self.level = Some(target.level());
        self.world_position = Some(world_position);
    }
}
