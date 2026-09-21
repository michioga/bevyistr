//! Shared value extraction for the history plot and CSV. No display scaling.
use crate::result_probe_pin::Target;
use fem_core::{ResultField, StepResult};

#[derive(Clone, Debug)]
pub(crate) struct Sample {
    pub step: u32,
    pub time: Option<f32>,
    pub value: Option<f32>,
    pub quantity: &'static str,
}

pub(crate) fn samples(steps: &[StepResult], target: Target, field: &str) -> Vec<Sample> {
    steps
        .iter()
        .map(|step| {
            let field = step.field_by_name(field);
            Sample {
                step: step.step,
                // The reader uses zero for missing time. Preserve that zero,
                // without claiming that it is a measured time in seconds.
                time: step.time.is_finite().then_some(step.time),
                value: target.value(field),
                quantity: match field {
                    Some(ResultField::NodeVector { .. }) => "magnitude",
                    Some(_) => "scalar_or_component",
                    None => "unavailable",
                },
            }
        })
        .collect()
}
