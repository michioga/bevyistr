//! Strict subset of !DYNAMIC for implicit Newmark time-history input.
use super::KeywordHeader;
use fem_core::DynamicSettings;

pub(super) fn parse(
    header: &KeywordHeader,
    rows: &[&str],
) -> Result<DynamicSettings, &'static str> {
    let nonlinear = match header
        .params
        .get("TYPE")
        .map(|kind| kind.to_ascii_uppercase())
        .as_deref()
    {
        Some("LINEAR") => false,
        Some("NONLINEAR") => true,
        _ => return Err("DYNAMIC requires an explicit LINEAR or NONLINEAR type"),
    };
    let [method, time, newmark, mass, monitor, flags] = rows else {
        return Err("DYNAMIC requires six complete data rows");
    };
    let [integrator, response] = columns(method)?;
    if count(integrator)? != 1 || count(response)? != 1 {
        return Err("Only implicit Newmark time-history DYNAMIC (1, 1) is supported");
    }
    let [start, end, substeps, increment] = columns(time)?;
    let [gamma, beta] = columns(newmark)?;
    let [mass_matrix, damping, rayleigh_mass, rayleigh_stiffness] = columns(mass)?;
    let [legacy_output, node, monitor_interval] = columns(monitor)?;
    let mass_matrix = match count(mass_matrix)? {
        1 => 1,
        2 => 2,
        _ => return Err("DYNAMIC mass matrix must be lumped (1) or consistent (2)"),
    };
    if count(damping)? != 1 {
        return Err("Only Rayleigh damping (1) is supported for DYNAMIC");
    }
    if node.is_empty() || node.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("DYNAMIC monitor requires a node ID or a group name");
    }
    let mut output_flags = [0; 6];
    for (index, field) in columns::<6>(flags)?.into_iter().enumerate() {
        let flag = count(field)?;
        if flag > if index < 4 { 1 } else { 3 } {
            return Err("Unsupported DYNAMIC output flag");
        }
        output_flags[index] = flag as u8;
    }
    let settings = DynamicSettings {
        nonlinear,
        start_time: real(start)?,
        end_time: real(end)?,
        fallback_substeps: count(substeps)?,
        fallback_increment: real(increment)?,
        gamma: real(gamma)?,
        beta: real(beta)?,
        mass_matrix,
        rayleigh_mass: real(rayleigh_mass)?,
        rayleigh_stiffness: real(rayleigh_stiffness)?,
        legacy_output_interval: count(legacy_output)?,
        monitor_node_or_group: node.to_owned(),
        monitor_interval: count(monitor_interval)?,
        output_flags,
    };
    if settings.fallback_substeps == 0 || settings.monitor_interval == 0 {
        return Err("DYNAMIC substep count and monitoring interval must be positive");
    }
    if settings.fallback_increment <= 0.0 || settings.beta <= 0.0 {
        return Err("DYNAMIC time increment and Newmark beta must be positive");
    }
    Ok(settings)
}

fn columns<const N: usize>(row: &str) -> Result<[&str; N], &'static str> {
    row.split(',')
        .map(str::trim)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "Unexpected number of columns in DYNAMIC data row")
}

fn count(text: &str) -> Result<u32, &'static str> {
    text.parse::<u32>()
        .ok()
        .filter(|&value| value <= i32::MAX as u32)
        .ok_or("Invalid DYNAMIC integer")
}

fn real(text: &str) -> Result<f64, &'static str> {
    text.replace(['d', 'D'], "e")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or("Invalid DYNAMIC real number")
}
