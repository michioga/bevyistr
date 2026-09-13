//! Pointer-path regression tests must not bypass widget activation with Activate/MenuEvent.
use bevy::{
    camera::NormalizedRenderTarget,
    picking::{
        backend::HitData,
        events::{Click, Pointer, Press, Release},
        pointer::{Location, PointerButton, PointerId},
    },
    prelude::*,
};

fn location() -> Location {
    Location {
        target: NormalizedRenderTarget::None {
            width: 1280,
            height: 720,
        },
        position: Vec2::new(100.0, 100.0),
    }
}

pub(crate) fn press(world: &mut World, entity: Entity) {
    world.trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        Press {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        },
        entity,
    ));
    world.flush();
}

pub(crate) fn click(world: &mut World, entity: Entity) {
    press(world, entity);
    // Bevy emits Click before Release (both the widget and menu test Pressed).
    world.trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        Click {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: std::time::Duration::from_millis(50),
            count: 1,
        },
        entity,
    ));
    world.flush();
    if world.get_entity(entity).is_ok() {
        world.trigger(Pointer::new(
            PointerId::Mouse,
            location(),
            Release {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        ));
        world.flush();
    }
}
