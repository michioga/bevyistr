//! Common input fields of explicit fixed-increment steps.
use super::{KeywordHeader, positive_group_id};
use fem_core::{DynamicStep, NonlinearStep, NonlinearStepTime, StepGroup};
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

    let groups = parse_groups(&rows[1..], false)?;
    Ok(DynamicStep {
        increment,
        duration,
        substeps: count("SUBSTEPS")?,
        max_iterations: count("MAXITER")?,
        convergence,
        groups,
    })
}

pub(super) fn parse_nonlinear(
    header: &KeywordHeader,
    rows: &[&str],
) -> Result<NonlinearStep, &'static str> {
    for (key, allowed) in [("TYPE", "STATIC"), ("INC_TYPE", "FIXED")] {
        if header
            .params
            .get(key)
            .is_some_and(|v| !v.eq_ignore_ascii_case(allowed))
        {
            return Err("Nonlinear static STEP requires TYPE=STATIC and INC_TYPE=FIXED");
        }
    }
    let count = |key| {
        header
            .params
            .get(key)
            .map(|value| positive_group_id(value).ok_or("STEP counts must be positive integers"))
            .transpose()
    };
    let convergence = header
        .params
        .get("CONVERG")
        .map(|value| positive_real(value).ok_or("STEP convergence must be finite and positive"))
        .transpose()?;
    let mut remaining = rows;
    let mut time = None;
    if let Some((row, rest)) = rows.split_first()
        && row
            .trim_start()
            .starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
    {
        let (increment, duration) = two_columns(row)
            .and_then(|(a, b)| Some((positive_real(a)?, positive_real(b)?)))
            .ok_or("STEP DTIME and ETIME must be finite positive values")?;
        time = Some(NonlinearStepTime {
            increment,
            duration,
        });
        remaining = rest;
    }
    let groups = parse_groups(remaining, true)?;
    Ok(NonlinearStep {
        time,
        substeps: count("SUBSTEPS")?,
        max_iterations: count("MAXITER")?,
        convergence,
        groups,
    })
}

fn parse_groups(rows: &[&str], allow_contact: bool) -> Result<Vec<StepGroup>, &'static str> {
    let mut groups = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let (keyword, id) =
            two_columns(row).ok_or("STEP activation requires one group ID per row")?;
        let id = positive_group_id(id).ok_or("Invalid STEP group ID")?;
        let group = if keyword.eq_ignore_ascii_case("BOUNDARY") {
            StepGroup::Boundary(id)
        } else if keyword.eq_ignore_ascii_case("LOAD") {
            StepGroup::Load(id)
        } else if allow_contact && keyword.eq_ignore_ascii_case("CONTACT") {
            StepGroup::Contact(id)
        } else {
            return Err("Unsupported STEP activation keyword");
        };
        if !seen.insert(group) {
            return Err("Repeated group activation in STEP");
        }
        groups.push(group);
    }
    Ok(groups)
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
