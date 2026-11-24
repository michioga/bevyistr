//! The two standard numeric rows on a FrontISTR SOLVER card.
use fem_core::SolverSettings;
use std::str::FromStr;

pub(super) fn read(rows: &[&str], solver: &mut SolverSettings) -> Result<(), &'static str> {
    let (integer_row, real_row) = match rows {
        [] => ("", ""),
        [integer_row] => (*integer_row, ""),
        [integer_row, real_row] => (*integer_row, *real_row),
        _ => return Err("Additional SOLVER data rows cannot be preserved"),
    };
    let mut integers = integer_row.split(',');
    let max_iterations = optional(integers.next(), "Invalid NITER")?;
    let preconditioner_iterations = optional(integers.next(), "Invalid iterPREmax")?;
    let gmres_subspaces = optional(integers.next(), "Invalid NREST")?;
    let ordering_colors = optional(integers.next(), "Invalid NCOLOR_IN")?;
    let preconditioner_reuse = optional(integers.next(), "Invalid RECYCLEPRE")?;

    let mut reals = real_row.split(',');
    let convergence_tol = optional(reals.next(), "Invalid RESID")?;
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

fn optional<T: FromStr>(field: Option<&str>, error: &'static str) -> Result<Option<T>, &'static str> {
    field
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.parse().map_err(|_| error))
        .transpose()
}
