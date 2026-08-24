use bevy::prelude::*;
use fem_core::{EdgeId, ElementId, FaceId, FemEntityId, FemEntityRef, NodeId, SelectionLevel};

#[derive(Component)]
pub struct Hovered;

#[derive(Component)]
pub struct Selected;

#[derive(Component)]
pub struct Selectable {
    pub target: FemEntityRef,
}

impl Selectable {
    pub const fn new(mesh_index: usize, target: FemEntityId) -> Self {
        Self {
            target: FemEntityRef::new(mesh_index, target),
        }
    }

    pub const fn node(mesh_index: usize, id: NodeId) -> Self {
        Self::new(mesh_index, FemEntityId::Node(id))
    }

    pub const fn edge(mesh_index: usize, id: EdgeId) -> Self {
        Self::new(mesh_index, FemEntityId::Edge(id))
    }

    pub const fn face(mesh_index: usize, id: FaceId) -> Self {
        Self::new(mesh_index, FemEntityId::Face(id))
    }

    pub const fn element(mesh_index: usize, id: ElementId) -> Self {
        Self::new(mesh_index, FemEntityId::Element(id))
    }

    pub const fn level(&self) -> SelectionLevel {
        self.target.level()
    }
}

impl Default for Selectable {
    fn default() -> Self {
        Self::element(0, ElementId(0))
    }
}

#[derive(Component)]
pub struct NodeEntity {
    pub id: NodeId,
}

impl NodeEntity {
    pub const fn new(id: NodeId) -> Self {
        Self { id }
    }
}

#[derive(Component)]
pub struct EdgeEntity {
    pub id: EdgeId,
}

impl EdgeEntity {
    pub const fn new(id: EdgeId) -> Self {
        Self { id }
    }
}

#[derive(Component)]
pub struct FaceEntity {
    pub id: FaceId,
}

impl FaceEntity {
    pub const fn new(id: FaceId) -> Self {
        Self { id }
    }
}

#[derive(Component)]
pub struct ElementEntity {
    pub id: ElementId,
}

impl ElementEntity {
    pub const fn new(id: ElementId) -> Self {
        Self { id }
    }
}
