//! Strict parsing of the single EIGEN data row. No partial/default substitution
//! for malformed values; optional empty fields use FrontISTR defaults.
use fem_core::EigenSettings;

#[cfg(test)]
#[path = "eigen_settings_tests.rs"]
mod tests;

pub(super) fn parse_rows(rows: &[&str]) -> Result<EigenSettings, &'static str> {
    if rows.len() != 1 {
        return Err("EIGEN requires exactly one data row: NGET, LCZTOL, LCZMAX, SIGMA");
    }
    let fields: Vec<_> = rows[0].split(',').map(str::trim).collect();
    if fields.len() > 4 {
        return Err("Extra EIGEN columns cannot be preserved");
    }
    let mut settings = EigenSettings {
        mode_count: fields[0]
            .parse()
            .map_err(|_| "EIGEN NGET must be a positive integer")?,
        ..Default::default()
    };
    let optional = |index| fields.get(index).copied().filter(|s: &&str| !s.is_empty());
    let real = |s: &str| {
        s.replace(['d', 'D'], "e")
            .parse::<f64>()
            .map_err(|_| "Invalid EIGEN real number")
    };
    if let Some(value) = optional(1) {
        settings.tolerance = real(value)?;
    }
    if let Some(value) = optional(2) {
        settings.max_iterations = value
            .parse()
            .map_err(|_| "EIGEN LCZMAX must be a positive integer")?;
    }
    if let Some(value) = optional(3) {
        settings.shift = real(value)?;
    }
    settings.validate()?;
    Ok(settings)
}
