//! Embedded icon: installed executables do not depend on the working directory.
use bevy::winit::WINIT_WINDOWS;
use bevy::{ecs::system::NonSendMarker, prelude::*, window::PrimaryWindow};
use winit::window::Icon;

const PNG: &[u8] = include_bytes!("../assets/bevyistr.png");

#[derive(Component)]
pub(crate) struct IconInstalled;

fn decode_icon() -> Result<Icon, String> {
    let pixels = image::load_from_memory_with_format(PNG, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .resize(256, 256, image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    let (width, height) = pixels.dimensions();
    Icon::from_rgba(pixels.into_raw(), width, height).map_err(|e| e.to_string())
}

pub(crate) fn set_window_icon(
    mut commands: Commands,
    windows: Query<Entity, (With<PrimaryWindow>, Without<IconInstalled>)>,
    mut cached: Local<Option<Result<Icon, String>>>,
    _main_thread: NonSendMarker,
) {
    if windows.is_empty() {
        return;
    }
    let icon = cached.get_or_insert_with(decode_icon);
    WINIT_WINDOWS.with_borrow(|backend| {
        for entity in &windows {
            let Some(window) = backend.get_window(entity) else {
                continue; // Winit may not have created the window yet.
            };
            match icon {
                Ok(icon) => {
                    window.set_window_icon(Some(icon.clone()));
                    #[cfg(target_os = "windows")]
                    {
                        use winit::platform::windows::WindowExtWindows;
                        window.set_taskbar_icon(Some(icon.clone()));
                    }
                }
                Err(error) => warn!("Cannot decode application icon: {error}"),
            }
            commands.entity(entity).insert(IconInstalled);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn packaged_icon_decodes_without_external_files() {
        assert!(super::decode_icon().is_ok());
    }
}
