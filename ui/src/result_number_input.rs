use crate::slider::{SliderConfig, SliderId, SliderState, SliderTrack};
use bevy::{
    feathers::{
        FeathersCorePlugin,
        controls::{
            FeathersNumberInput, HardLimit, NumberInputPrecision, NumberInputRange,
            NumberInputStep, NumberInputValue,
        },
        dark_theme::create_dark_theme,
        theme::UiTheme,
    },
    prelude::*,
    ui_widgets::ValueChange,
};

#[derive(Component)]
struct ResultNumber(SliderId);

pub(crate) fn register(app: &mut App) {
    if !app.is_plugin_added::<FeathersCorePlugin>() {
        app.add_plugins(FeathersCorePlugin)
            .insert_resource(UiTheme(create_dark_theme()));
    }
    app.add_systems(PostUpdate, sync.after(crate::slider::sync_slider_visuals));
}

pub(crate) fn spawn(row: &mut ChildSpawnerCommands, config: &SliderConfig) {
    let parent = row.target_entity();
    let value = config.value;
    let range = config.min..=config.max;
    row.commands()
        .queue_spawn_scene(bsn! {
            @FeathersNumberInput
            NumberInputValue::F32(value)
            HardLimit(NumberInputRange::F32(range))
            NumberInputPrecision(2)
            NumberInputStep(0.1)
            Node {
                width: px(82),
                align_self: AlignSelf::Center,
            }
        })
        .insert((
            ChildOf(parent),
            ResultNumber(config.id),
            Name::new(config.label),
        ))
        .observe(change);
}

fn change(
    event: On<ValueChange<f32>>,
    inputs: Query<&ResultNumber>,
    tracks: Query<&mut SliderState, With<SliderTrack>>,
) {
    let Ok(input) = inputs.get(event.source) else {
        return;
    };
    if let Some(mut track) = tracks.into_iter().find(|track| track.id == input.0)
        && event.value.is_finite()
    {
        track.value = event.value.clamp(track.min, track.max);
    }
}

fn sync(
    tracks: Query<&SliderState, With<SliderTrack>>,
    inputs: Query<(Entity, &ResultNumber, &NumberInputValue)>,
    mut commands: Commands,
) {
    for (entity, input, value) in &inputs {
        if let Some(track) = tracks.iter().find(|track| track.id == input.0)
            && *value != NumberInputValue::F32(track.value)
        {
            commands
                .entity(entity)
                .insert(NumberInputValue::F32(track.value));
        }
    }
}
