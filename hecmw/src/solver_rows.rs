//! The two standard numeric rows on a FrontISTR SOLVER card.
use fem_core::SolverSettings;
use std::str::FromStr;

pub(super) fn read(rows: &[&str], solver: &mut SolverSettings) -> Result<(), &'static str> {
    let [integer_row, real_row] = rows else {
        return Err("Expected the two standard SOLVER data rows");
    };
    let mut integers = integer_row.split(',');
    let max_iterations = required_integer(integers.next(), "NITER must be a positive integer")?;
    let preconditioner_iterations =
        required_integer(integers.next(), "iterPREmax must be a positive integer")?;
    let gmres_subspaces = optional(integers.next(), "Invalid NREST")?;
    let ordering_colors = optional(integers.next(), "Invalid NCOLOR_IN")?;
    let preconditioner_reuse = optional(integers.next(), "Invalid RECYCLEPRE")?;

    let mut reals = real_row.split(',');
    let convergence_tol: f32 = optional(reals.next(), "RESID must be finite and positive")?
        .filter(|value: &f32| value.is_finite() && *value > 0.0)
        .ok_or("RESID must be finite and positive")?;
    let diagonal_scale = optional(reals.next(), "Invalid SIGMA_DIAG")?;
    let ssor_relaxation = optional(reals.next(), "Invalid OMEGA")?;
    if integers.next().is_some() || reals.next().is_some() {
        return Err("Additional SOLVER data fields cannot be preserved");
    }

    let mut candidate = solver.clone();
    candidate.max_iterations = max_iterations;
    candidate.preconditioner_iterations = preconditioner_iterations;
    candidate.gmres_subspaces = gmres_subspaces;
    candidate.ordering_colors = ordering_colors;
    candidate.preconditioner_reuse = preconditioner_reuse;
    candidate.convergence_tol = convergence_tol;
    candidate.diagonal_scale = diagonal_scale;
    candidate.ssor_relaxation = ssor_relaxation;
    candidate.validate_linear_options()?;
    *solver = candidate;
    Ok(())
}

fn required_integer(field: Option<&str>, error: &'static str) -> Result<u32, &'static str> {
    optional(field, error)?
        .filter(|value| (1..=i32::MAX as u32).contains(value))
        .ok_or(error)
}

fn optional<T: FromStr>(field: Option<&str>, error: &'static str) -> Result<Option<T>, &'static str> {
    field
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.parse().map_err(|_| error))
        .transpose()
}
