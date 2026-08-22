use bevy::prelude::*;
use fem_core::{EdgeId, ElementId, FaceId, FemEntityId, NodeId, SelectionLevel};

#[derive(Component)]
pub struct Hovered;

#[derive(Component)]
pub struct Selected;

#[derive(Component)]
pub struct Selectable {
    pub target: FemEntityId,
}

impl Selectable {
    pub const fn new(target: FemEntityId) -> Self {
        Self { target }
    }

    pub const fn node(id: NodeId) -> Self {
        Self::new(FemEntityId::Node(id))
    }

    pub const fn edge(id: EdgeId) -> Self {
        Self::new(FemEntityId::Edge(id))
    }

    pub const fn face(id: FaceId) -> Self {
        Self::new(FemEntityId::Face(id))
    }

    pub const fn element(id: ElementId) -> Self {
        Self::new(FemEntityId::Element(id))
    }

    pub const fn level(&self) -> SelectionLevel {
        self.target.level()
    }
}

impl Default for Selectable {
    fn default() -> Self {
        Self::element(ElementId(0))
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
