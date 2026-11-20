//! Pointer-path regression tests must not bypass widget activation with Activate/MenuEvent.
use bevy::{
    camera::NormalizedRenderTarget,
    picking::{
        backend::HitData,
        events::{Pointer, PointerClick, PointerPress, PointerRelease},
        pointer::{Location, PointerButton, PointerId},
    },
    prelude::*,
};

pub(crate) fn enable_menu_widgets(app: &mut App) {
    // Tab navigation checks inherited visibility before focusing menu items.
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_plugins((
            bevy::camera::visibility::VisibilityPlugin,
            bevy::ui_widgets::ButtonPlugin,
            bevy::ui_widgets::MenuPlugin,
        ));
}

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
    // Pointer traversal queries both components, even in tests without a window.
    world.register_component::<ChildOf>();
    world.register_component::<Window>();
    world.trigger(PointerPress {
        entity,
        pointer: Pointer::new(PointerId::Mouse, location()),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        count: 1,
    });
    world.flush();
}

pub(crate) fn click(world: &mut World, entity: Entity) {
    press(world, entity);
    // Bevy emits Click before Release (both the widget and menu test Pressed).
    world.trigger(PointerClick {
        entity,
        pointer: Pointer::new(PointerId::Mouse, location()),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: std::time::Duration::from_millis(50),
        count: 1,
    });
    world.flush();
    if world.get_entity(entity).is_ok() {
        world.trigger(PointerRelease {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location()),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        });
        world.flush();
    }
}
