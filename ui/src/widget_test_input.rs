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

pub(crate) fn enable_keyboard(app: &mut App) {
    app.add_message::<bevy::input::keyboard::KeyboardInput>()
        .add_systems(
            PreUpdate,
            bevy::input_focus::dispatch_focused_input::<bevy::input::keyboard::KeyboardInput>,
        );
    app.world_mut().spawn(bevy::window::PrimaryWindow);
}

pub(crate) fn key(app: &mut App, key_code: KeyCode) {
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    };
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    let logical_key = match key_code {
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter => Key::Enter,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        _ => panic!("unsupported test key"),
    };
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}

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
