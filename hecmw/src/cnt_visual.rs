use super::{KeywordHeader, strip_comment};
use fem_core::{PsrVisualControl, VisualOutputFormat};

/// Retains one PSR surface block with the known rendering settings and output formats.
/// Stop at the first unrelated or unknown card, so the main CNT audit still sees it.
pub(super) fn read_block(
    lines: &[&str],
    start: usize,
    header: &KeywordHeader,
) -> (usize, Result<Option<PsrVisualControl>, String>) {
    let mut end = start + 1;
    let mut options = Vec::new();
    let mut unexpected_data = false;
    for line in &lines[end..] {
        let text = strip_comment(line);
        if text.is_empty() {
            end += 1;
            continue;
        }
        let Some((key, value)) = subcard(text) else {
            if text.starts_with('!') {
                break;
            }
            end += 1;
            unexpected_data = true;
            continue;
        };
        options.push((key, value));
        end += 1;
    }
    let result = (|| {
        if header.params.len() != 1
            || !header
                .params
                .get("METHOD")
                .is_some_and(|v| v.eq_ignore_ascii_case("PSR"))
            || strip_comment(lines[start])
                .split(',')
                .filter(|entry| !entry.trim().is_empty())
                .count()
                != 2
        {
            return Err("Only one METHOD=PSR visualization block is preserved".into());
        }
        if unexpected_data
            || !options.starts_with(&[("SURFACE_NUM".into(), "1"), ("SURFACE".into(), "1")])
        {
            return Err("PSR requires one surface (!surface_num=1 followed by !surface 1)".into());
        }

        let surface_options = &options[2..];
        let mut names = std::collections::HashSet::new();
        let mut format = None;
        for (name, value) in surface_options {
            if value.is_empty() || !names.insert(name) {
                return Err(format!("Missing or repeated PSR setting: {name}"));
            }
            match name.as_str() {
                "SURFACE_NUM" | "SURFACE" => {
                    return Err("Multiple PSR surfaces are not preserved".into());
                }
                "GROUP_NAME" if !value.eq_ignore_ascii_case("boundary") => {
                    return Err(
                        "Only the whole boundary is preserved; named PSR groups require remapping"
                            .into(),
                    );
                }
                "SURFACE_STYLE" if *value != "1" => {
                    return Err("Only PSR boundary surfaces (surface_style=1) are preserved".into());
                }
                "OUTPUT_TYPE" => {
                    format = Some(match value.to_ascii_uppercase().as_str() {
                        "VTK" => VisualOutputFormat::Vtk,
                        "COMPLETE_MICROAVS" => VisualOutputFormat::CompleteMicroAvs,
                        _ => return Err(format!("Unsupported PSR output_type: {value}")),
                    })
                }
                _ => {}
            }
        }
        let format = format.ok_or_else(|| "PSR output_type must be explicit".to_string())?;
        if surface_options.len() == 1 && format == VisualOutputFormat::Vtk {
            return Ok(None); // The writer's default is the same minimal PSR block.
        }
        Ok(Some(PsrVisualControl {
            lines: lines[start..end]
                .iter()
                .map(|line| (*line).to_string())
                .collect(),
            format,
        }))
    })();
    (end, result)
}

fn subcard(line: &str) -> Option<(String, &str)> {
    let text = line.strip_prefix('!')?.trim();
    let index = text
        .find(|ch: char| ch == '=' || ch.is_ascii_whitespace())
        .unwrap_or(text.len());
    let (name, value) = text.split_at(index);
    let name = name.to_ascii_uppercase();
    if !matches!(
        name.as_str(),
        "SURFACE_NUM"
            | "SURFACE"
            | "SURFACE_STYLE"
            | "GROUP_NAME"
            | "DISPLAY_METHOD"
            | "COLOR_COMP_NAME"
            | "COLOR_SUBCOMP"
            | "INITIAL_STYLE"
            | "DEFORM_STYLE"
            | "OUTPUT_TYPE"
            | "X_RESOLUTION"
            | "Y_RESOLUTION"
            | "NUM_OF_LIGHTS"
            | "POSITION_OF_LIGHTS"
            | "VIEWPOINT"
            | "UP_DIRECTION"
            | "AMBIENT_COEF"
            | "DIFFUSE_COEF"
            | "SPECULAR_COEF"
            | "COLOR_MAPPING_STYLE"
            | "COLOR_MAPPING_BAR_ON"
            | "SCALE_MARKING_ON"
            | "NUM_OF_SCALE"
            | "FONT_SIZE"
            | "FONT_COLOR"
            | "DEFORM_DISPLAY_ON"
            | "DEFORM_NUM_OF_FRAMES"
            | "INTERVAL_MAPPING"
            | "FIXED_RANGE_ON"
            | "RANGE_VALUE"
    ) {
        return None;
    }
    let value = value.trim_start();
    Some((name, value.strip_prefix('=').unwrap_or(value).trim()))
}
