use bevy::prelude::*;
use fem_core::{FaceId, FemEntityId, SelectionLevel};

#[derive(Resource, Default)]
pub struct HoverResult {
    pub entity: Option<Entity>,

    pub target: Option<FemEntityId>,

    pub level: Option<SelectionLevel>,

    pub world_position: Option<Vec3>,

    /// Boundary face actually hit by the picking ray. This stays populated
    /// in Element mode so planar expansion starts from the face under the
    /// cursor rather than an arbitrary face of the owning element.
    pub surface_face: Option<FaceId>,
}

impl HoverResult {
    pub fn clear(&mut self) {
        self.entity = None;
        self.target = None;
        self.level = None;
        self.world_position = None;
        self.surface_face = None;
    }

    pub fn set_entity(&mut self, entity: Entity, target: FemEntityId, world_position: Vec3) {
        self.entity = Some(entity);
        self.target = Some(target);
        self.level = Some(target.level());
        self.world_position = Some(world_position);
        self.surface_face = None;
    }

    pub fn set_target(&mut self, target: FemEntityId, world_position: Vec3) {
        self.entity = None;
        self.target = Some(target);
        self.level = Some(target.level());
        self.world_position = Some(world_position);
        self.surface_face = None;
    }

    pub fn set_surface_target(
        &mut self,
        target: FemEntityId,
        surface_face: FaceId,
        world_position: Vec3,
    ) {
        self.entity = None;
        self.target = Some(target);
        self.level = Some(target.level());
        self.world_position = Some(world_position);
        self.surface_face = Some(surface_face);
    }
}
