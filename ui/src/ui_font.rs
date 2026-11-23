use bevy::{asset::uuid_handle, picking::Pickable, prelude::*};

const BOLD: Handle<Font> = uuid_handle!("9d44aee2-54fc-4746-921d-3724bf4713ef");
const REGULAR_BYTES: &[u8] = include_bytes!("../assets/fonts/UDEVGothic35NF-Regular.ttf");
const BOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/UDEVGothic35NF-Bold.ttf");

pub(crate) fn load_fonts(mut fonts: Option<ResMut<Assets<Font>>>) {
    let Some(fonts) = fonts.as_deref_mut() else {
        return;
    };
    // Install before Bevy registers the default font with its text pipeline.
    fonts
        .insert(
            Handle::<Font>::default().id(),
            Font::from_bytes(REGULAR_BYTES.to_vec()),
        )
        .expect("default UDEV Gothic font asset");
    fonts
        .insert(BOLD.id(), Font::from_bytes(BOLD_BYTES.to_vec()))
        .expect("bold UDEV Gothic font asset");
}

pub(crate) fn heading_font(size: f32) -> TextFont {
    TextFont {
        font: BOLD.into(),
        font_size: FontSize::Px(size),
        weight: FontWeight::BOLD,
        ..default()
    }
}

pub(crate) enum ButtonIcon {
    Open,
    Add,
    Run,
    Stop,
    Save,
    Cube,
    Link,
    Anchor,
    Flask,
    Gears,
    Chart,
    First,
    Previous,
    Next,
    Last,
    Pause,
    Check,
    Cancel,
    Refresh,
    Trash,
    Edit,
    Crosshair,
    Search,
}

impl ButtonIcon {
    pub(crate) fn glyph(&self) -> &'static str {
        // Font Awesome glyphs in the bundled Nerd Fonts build.
        match self {
            Self::Open => "\u{f07c}",
            Self::Add => "\u{f067}",
            Self::Run => "\u{f04b}",
            Self::Stop => "\u{f04d}",
            Self::Save => "\u{f0c7}",
            Self::Cube => "\u{f1b2}",
            Self::Link => "\u{f0c1}",
            Self::Anchor => "\u{f13d}",
            Self::Flask => "\u{f0c3}",
            Self::Gears => "\u{f085}",
            Self::Chart => "\u{f080}",
            Self::First => "\u{f048}",
            Self::Previous => "\u{f053}",
            Self::Next => "\u{f054}",
            Self::Last => "\u{f051}",
            Self::Pause => "\u{f04c}",
            Self::Check => "\u{f00c}",
            Self::Cancel => "\u{f00d}",
            Self::Refresh => "\u{f021}",
            Self::Trash => "\u{f1f8}",
            Self::Edit => "\u{f044}",
            Self::Crosshair => "\u{f05b}",
            Self::Search => "\u{f002}",
        }
    }
}

pub(crate) fn button_label(
    button: &mut ChildSpawnerCommands,
    icon: ButtonIcon,
    label: &str,
    font_size: f32,
    color: Color,
) -> (Entity, Entity) {
    let icon = button
        .spawn((
            Text::new(icon.glyph()),
            TextFont {
                font_size: FontSize::Px(font_size),
                ..default()
            },
            TextLayout::justify(Justify::Center),
            TextColor(color),
            Pickable::IGNORE,
            Node {
                width: px(14),
                flex_shrink: 0.0,
                margin: UiRect::right(px(4)),
                ..default()
            },
        ))
        .id();
    let label = button
        .spawn((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(font_size),
                ..default()
            },
            TextColor(color),
            Pickable::IGNORE,
        ))
        .id();
    (icon, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_faces_use_default_text_and_the_bold_heading_handle() {
        let mut app = App::new();
        app.init_resource::<Assets<Font>>()
            .add_systems(PreStartup, load_fonts);
        app.update();
        let fonts = app.world().resource::<Assets<Font>>();
        let regular = fonts.get(&Handle::<Font>::default()).unwrap();
        let bold = fonts.get(&BOLD).unwrap();
        assert_eq!(regular.data.as_ref(), REGULAR_BYTES);
        assert_eq!(bold.data.as_ref(), BOLD_BYTES);
        assert_eq!(heading_font(10.0).font, FontSource::Handle(BOLD));
        assert_eq!(heading_font(10.0).weight, FontWeight::BOLD);
    }
}
