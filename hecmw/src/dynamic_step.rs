//! One explicit fixed-increment STEP for time-history input.
use super::{KeywordHeader, positive_group_id};
use fem_core::{DynamicStep, StepGroup};
use std::collections::HashSet;

pub(super) fn parse(header: &KeywordHeader, rows: &[&str]) -> Result<DynamicStep, &'static str> {
    let incompatible = |key: &str, supported: &str| {
        header
            .params
            .get(key)
            .is_some_and(|value| !value.eq_ignore_ascii_case(supported))
    };
    if incompatible("TYPE", "STATIC") || incompatible("INC_TYPE", "FIXED") {
        return Err("Dynamic STEP requires a single fixed increment without viscoelastic controls");
    }
    let (increment, duration) = rows
        .first()
        .and_then(|row| two_columns(row))
        .and_then(|(increment, duration)| {
            Some((positive_real(increment)?, positive_real(duration)?))
        })
        .ok_or("Dynamic STEP requires a DTIME, ETIME row with two finite positive values")?;

    let count = |key| {
        header
            .params
            .get(key)
            .map(|value| {
                positive_group_id(value).ok_or("Dynamic STEP counts must be positive integers")
            })
            .transpose()
    };
    let convergence = header
        .params
        .get("CONVERG")
        .map(|value| {
            positive_real(value).ok_or("Dynamic STEP convergence must be finite and positive")
        })
        .transpose()?;

    let mut groups = Vec::new();
    let mut seen = HashSet::new();
    for row in &rows[1..] {
        let (keyword, id) =
            two_columns(row).ok_or("Dynamic STEP activation requires one group ID per row")?;
        let id = positive_group_id(id).ok_or("Invalid dynamic STEP group ID")?;
        let group = if keyword.eq_ignore_ascii_case("BOUNDARY") {
            StepGroup::Boundary(id)
        } else if keyword.eq_ignore_ascii_case("LOAD") {
            StepGroup::Load(id)
        } else {
            return Err("Only BOUNDARY and LOAD activation is supported in dynamic STEP");
        };
        if !seen.insert(group) {
            return Err("Repeated group activation in dynamic STEP");
        }
        groups.push(group);
    }
    Ok(DynamicStep {
        increment,
        duration,
        substeps: count("SUBSTEPS")?,
        max_iterations: count("MAXITER")?,
        convergence,
        groups,
    })
}

fn two_columns(row: &str) -> Option<(&str, &str)> {
    let (first, second) = row.split_once(',')?;
    (!second.contains(',')).then_some((first.trim(), second.trim()))
}

fn positive_real(text: &str) -> Option<f64> {
    text.replace(['d', 'D'], "e")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}
