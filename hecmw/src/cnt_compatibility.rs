//! Conservative import/export compatibility checks, independent of the lenient
//! reader. Recognizing a card is not enough: ignored parameters/data must also
//! be reported. Expand this allow-list only with round-trip regression tests.

use super::{CntData, KeywordHeader, parse_keyword_header, parse_material_real, strip_comment};
use fem_core::{
    AnalysisType, LinearSolverMethod, MatrixDumpFormat, PsrVisualControl, SetupImportIssue,
    SolverSettings, SolverTimeLog,
};
use std::collections::HashSet;

pub(super) fn audit(
    text: &str,
    data: &CntData,
) -> (Vec<SetupImportIssue>, Option<PsrVisualControl>) {
    let lines: Vec<_> = text.lines().collect();
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    let mut material = false;
    let mut fluid_seen = false;
    let mut contact_names = HashSet::new();
    let mut visual = None;
    let mut i = 0;
    while i < lines.len() {
        let raw = strip_comment(lines[i]);
        if !raw.starts_with('!') || raw.starts_with("!!") {
            i += 1;
            continue;
        }
        let line = i + 1;
        let header = parse_keyword_header(raw);
        if header.name == "VISUAL" {
            let (end, block) = super::visual::read_block(&lines, i, &header);
            i = end;
            material = false;
            let problem = if !seen.insert(header.name) {
                Some("Repeated VISUAL blocks cannot be preserved".to_string())
            } else {
                match block {
                    Ok(block) => {
                        visual = block;
                        None
                    }
                    Err(reason) => Some(reason),
                }
            };
            if let Some(message) = problem {
                issues.push(SetupImportIssue {
                    source: "CNT".into(),
                    line,
                    keyword: raw.into(),
                    message,
                });
            }
            continue;
        }
        i += 1;
        let mut body = Vec::new();
        while i < lines.len() {
            let row = strip_comment(lines[i]);
            if row.starts_with('!') && !row.starts_with("!!") {
                break;
            }
            if !row.is_empty() && !row.starts_with("!!") {
                body.push(row);
            }
            i += 1;
        }
        let name = header.name.as_str();
        let reason = if matches!(
            name,
            "WRITE" | "CONTACT" | "CONTACT_ALGO" | "STEP" | "SOLVER" | "FLUID" | "DYNAMIC"
        ) && raw
            .split(',')
            .skip(1)
            .filter(|p| !p.trim().is_empty())
            .count()
            != header.params.len()
        {
            Some("Repeated or empty parameters cannot be preserved unambiguously".to_string())
        } else if matches!(
            name,
            "SOLUTION" | "SOLVER" | "STATIC" | "EIGEN" | "VERSION" | "STEP" | "CONTACT_ALGO" | "DYNAMIC"
        ) && !seen.insert(header.name.clone())
        {
            Some("Repeated control cards cannot be preserved".to_string())
        } else if name == "FLUID" && material && fluid_seen {
            Some("Repeated FLUID cards within a material cannot be preserved".into())
        } else if name == "CONTACT"
            && body.iter().any(|row| {
                !contact_names.insert(
                    row.split(',')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_uppercase(),
                )
            })
        {
            Some("A contact pair must have exactly one interaction definition".to_string())
        } else {
            check_card(&header, &body, data, material)
        };
        if let Some(message) = reason {
            issues.push(SetupImportIssue {
                source: "CNT".to_string(),
                line,
                keyword: raw.to_string(),
                message,
            });
        }
        // The reader only consumes material subcards immediately after MATERIAL.
        material = name == "MATERIAL"
            || (material && (matches!(name, "ELASTIC" | "DENSITY" | "FLUID") || name.starts_with("ITEM")));
        if !material || name == "MATERIAL" {
            fluid_seen = false;
        } else if name == "FLUID" {
            fluid_seen = true;
        }
    }
    if data
        .solver
        .as_ref()
        .is_some_and(|s| s.analysis_type == AnalysisType::Eigen)
        && !seen.contains("EIGEN")
    {
        issues.push(SetupImportIssue {
            source: "CNT".into(),
            line: 1,
            keyword: "!EIGEN (missing)".into(),
            message:
                "Missing EIGEN card: implicit solver defaults are not regenerated; inspection only"
                    .into(),
        });
    }
    if !data.contact_settings.is_empty() && !seen.contains("STEP") {
        issues.push(SetupImportIssue {
            source: "CNT".into(),
            line: 1,
            keyword: "!STEP (missing)".into(),
            message: "Contact activation requires an explicit supported STEP".into(),
        });
    }
    (issues, visual)
}

fn check_card(h: &KeywordHeader, body: &[&str], data: &CntData, material: bool) -> Option<String> {
    let name = h.name.as_str();
    // These cards are stored verbatim by the reader, including extension items.
    if matches!(name, "OUTPUT_RES" | "OUTPUT_VIS") {
        return None;
    }
    let allowed: &[&str] = match name {
        "SOLUTION" => &["TYPE", "NONLINEAR"],
        "SOLVER" => &[
            "METHOD", "PRECOND", "ITERLOG", "TIMELOG", "SCALING", "DUMPTYPE", "DUMPEXIT",
        ],
        "BOUNDARY" | "CLOAD" => &["GRPID", "ROT_CENTER"],
        "DLOAD" => &["GRPID"],
        "CONTACT" => &["GRPID", "INTERACTION"],
        "CONTACT_ALGO" => &["TYPE"],
        "MATERIAL" => &["NAME"],
        "NGROUP" | "NGRP" => &["NGRP"],
        "WRITE" => &["RESULT", "VISUAL", "FREQUENCY"],
        "ELASTIC" => &["TYPE"],
        "FLUID" => &["TYPE", "DEPENDENCIES"],
        "VERSION" | "END" | "STATIC" | "EIGEN" | "DENSITY" | "ITEM=1" | "ITEM=2" => &[],
        "STEP" => &["TYPE", "INC_TYPE", "SUBSTEPS", "MAXITER", "CONVERG"],
        "DYNAMIC" => &["TYPE"],
        "SECTION" => {
            return Some(
                "CNT section formulation/orientation is not preserved by the editor".into(),
            );
        }
        _ => {
            return Some("Unsupported CNT keyword; its settings would be omitted on export".into());
        }
    };
    let mut unsupported: Vec<_> = h
        .params
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .collect();
    unsupported.sort();
    if !unsupported.is_empty() {
        return Some(format!(
            "Unsupported parameter(s): {}",
            unsupported
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let value = |key: &str| parameter_value(h, key);
    let is = |key: &str, expected: &str| parameter_is(h, key, expected);
    let reason = match name {
        "SOLUTION" => {
            if !matches!(
                value("TYPE").as_deref(),
                Some("STATIC" | "NLSTATIC" | "DYNAMIC" | "EIGEN" | "EIGENVALUE")
            ) {
                Some(
                    "Unsupported or missing analysis TYPE; displayed solver defaults are not the imported analysis",
                )
            } else if h.params.contains_key("NONLINEAR")
                && (!matches!(value("TYPE").as_deref(), Some("STATIC" | "NLSTATIC"))
                    || !is("NONLINEAR", ""))
            {
                Some("This NONLINEAR option cannot be represented")
            } else if !body.is_empty() {
                Some("Unexpected SOLUTION data")
            } else {
                None
            }
        }
        "DYNAMIC" => {
            if data.solver.as_ref().is_none_or(|solver| solver.analysis_type != AnalysisType::Dynamic) {
                Some("DYNAMIC controls require SOLUTION, TYPE=DYNAMIC")
            } else {
                super::dynamic_settings::parse(h, body).err()
            }
        }
        "SOLVER" => check_solver(h, body, data),
        "STEP" => check_step(h, body, data),
        "STATIC" if body.len() != 1 || !numbers(body[0], &[1.0, 1.0]) => {
            Some("Static increment/time settings are not preserved (writer uses 1, 1)")
        }
        "EIGEN" => {
            if !data
                .solver
                .as_ref()
                .is_some_and(|s| s.analysis_type == AnalysisType::Eigen)
            {
                Some("EIGEN controls require the supported EIGEN analysis type")
            } else {
                super::eigen_settings::parse_rows(body).err()
            }
        }
        "VERSION" if body.len() != 1 || !numbers(body[0], &[3.0]) => {
            Some("Only VERSION 3 is emitted")
        }
        "BOUNDARY" | "CLOAD" | "DLOAD" if value("GRPID").is_some_and(|group| group != "1") => Some(
            "Only GRPID=1 (including the FrontISTR default) is preserved for boundary/load groups",
        ),
        "BOUNDARY" | "CLOAD"
            if body.is_empty()
                || body.iter().any(|row| {
                    let fields: Vec<_> = row.split(',').map(str::trim).collect();
                    let expected = if name == "BOUNDARY" { 4 } else { 3 };
                    fields.len() != expected
                        || fields[0].is_empty()
                        || fields[1..expected - 1]
                            .iter()
                            .any(|s| s.parse::<u8>().ok().is_none_or(|d| !(1..=6).contains(&d)))
                        || fields[expected - 1]
                            .parse::<f32>()
                            .ok()
                            .is_none_or(|v| !v.is_finite())
                }) =>
        {
            Some(
                "Boundary/load row is unsupported or malformed; extra columns and invalid values cannot be dropped",
            )
        }
        "BOUNDARY" | "CLOAD"
            if body.iter().any(|row| {
                let target = row.split(',').next().unwrap().trim();
                if name == "BOUNDARY" {
                    !data
                        .boundary_conditions
                        .iter()
                        .any(|condition| condition.name == target)
                } else {
                    !data.nodal_loads.iter().any(|load| load.name == target)
                }
            }) =>
        {
            Some("Unresolved boundary/load target would be omitted on export")
        }
        "DLOAD"
            if body.iter().any(|row| {
                let kind = row
                    .split(',')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_ascii_uppercase();
                !matches!(
                    kind.as_str(),
                    "P1" | "P2" | "P3" | "P4" | "P5" | "P6" | "GRAV"
                )
            }) =>
        {
            Some("Unsupported distributed load type; it must not be converted to gravity")
        }
        "DLOAD"
            if body.is_empty()
                || body.iter().any(|row| {
                    let fields: Vec<_> = row.split(',').map(str::trim).collect();
                    let expected = if fields
                        .get(1)
                        .is_some_and(|s| s.eq_ignore_ascii_case("GRAV"))
                    {
                        6
                    } else {
                        3
                    };
                    fields.len() != expected
                        || fields[2..]
                            .iter()
                            .any(|s| s.parse::<f32>().ok().is_none_or(|v| !v.is_finite()))
                        || !data
                            .distributed_loads
                            .iter()
                            .any(|load| load.name == fields[0])
                }) =>
        {
            Some("Distributed load row is incomplete or its target cannot be resolved")
        }
        "CONTACT" => check_contact(h, body, data),
        "CONTACT_ALGO"
            if !is("TYPE", "ALAGRANGE") || !body.is_empty() || data.contact_settings.is_empty() =>
        {
            Some("Only the default ALAGRANGE contact algorithm without extra options is preserved")
        }
        "FLUID" => {
            if !material
                || h.params.get("TYPE").is_some_and(|_| !is("TYPE", "INCOMP_NEWTONIAN"))
                || h.params.get("DEPENDENCIES").is_some_and(|n| n.parse::<u32>() != Ok(0))
                || !matches!(body, [row] if parse_material_real(row).is_some_and(|mu| mu.is_finite() && mu > 0.0))
            {
                Some("FLUID requires one finite positive viscosity for an incompressible Newtonian material without dependencies")
            } else {
                None
            }
        }
        "ELASTIC" | "DENSITY" | "ITEM=1" | "ITEM=2" => {
            let elastic = matches!(name, "ELASTIC" | "ITEM=1");
            let values: Option<Vec<f32>> = body.first().and_then(|row| {
                row.split(',')
                    .map(|value| if name == "DENSITY" {
                        parse_material_real(value)
                    } else {
                        value.trim().parse::<f32>().ok()
                    })
                    .collect()
            });
            let count = if elastic { 2 } else { 1 };
            if !material
                || body.len() != 1
                || values.as_ref().is_none_or(|v| {
                    !(v.len() == count || (v.len() == count + 1 && v[count] == 0.0))
                        || v.iter().any(|x| !x.is_finite())
                })
                || (name == "ELASTIC" && h.params.contains_key("TYPE") && !is("TYPE", "ISOTROPIC"))
            {
                Some(
                    "Only one isotropic elastic/density row without temperature dependence is preserved",
                )
            } else {
                None
            }
        }
        "WRITE" => {
            if !body.is_empty() {
                Some("Unexpected WRITE data is not preserved")
            } else {
                super::write_settings::parse(h).err()
            }
        }
        "MATERIAL" | "END" if !body.is_empty() => Some("Unexpected data would be omitted"),
        _ => None,
    };
    reason.map(str::to_string)
}

fn check_solver(h: &KeywordHeader, body: &[&str], data: &CntData) -> Option<&'static str> {
    let defaults = SolverSettings::default();
    let solver = data.solver.as_ref().unwrap_or(&defaults);
    let Some(method) = h
        .params
        .get("METHOD")
        .and_then(|value| LinearSolverMethod::from_frontistr(value))
    else {
        return Some("Unsupported or missing solver METHOD; no automatic substitution is allowed");
    };
    if !matches!(parameter_value(h, "SCALING").as_deref(), None | Some("YES" | "NO")) {
        Some("SCALING must be YES or NO")
    } else if !method.is_iterative() && h.params.contains_key("SCALING") {
        Some("SCALING is supported only for iterative solver methods")
    } else if h
        .params
        .get("DUMPTYPE")
        .is_some_and(|value| MatrixDumpFormat::from_frontistr(value).is_none())
    {
        Some("DUMPTYPE must be NONE, MM, CSR or BSR")
    } else if !matches!(
        parameter_value(h, "DUMPEXIT").as_deref(),
        None | Some("YES" | "NO")
    ) {
        Some("DUMPEXIT must be YES or NO")
    } else if !matches!(parameter_value(h, "ITERLOG").as_deref(), None | Some("YES" | "NO")) {
        Some("ITERLOG must be YES or NO")
    } else if h.params.get("TIMELOG").is_some_and(|value| SolverTimeLog::from_frontistr(value).is_none()) {
        Some("TIMELOG must be YES, NO or VERBOSE")
    } else if !method.is_iterative() && body.is_empty() {
        h.params.contains_key("PRECOND")
            .then_some("PRECOND on a direct solver header without data rows cannot be preserved")
    } else if !matches!(
        parameter_value(h, "PRECOND").as_deref(),
        Some("1" | "2" | "3")
    ) {
        Some("Only explicit PRECOND=1, 2 or 3 is preserved")
    } else {
        let mut parameters = solver.clone();
        if let Err(reason) = super::solver_rows::read(body, &mut parameters) {
            return Some(reason);
        }
        // Older direct-solver files may include the two standard placeholder rows.
        if !method.is_iterative()
            && (body.len() != 2
                || body[0].split(',').count() != 2
                || parameters.max_iterations.is_none()
                || parameters.preconditioner_iterations.is_none()
                || parameters.convergence_tol.is_none()
                || parameters.diagonal_scale != Some(1.0)
                || parameters.ssor_relaxation != Some(0.0))
        {
            return Some("Non-default direct solver data rows cannot be preserved");
        }
        None
    }
}

fn check_step(h: &KeywordHeader, body: &[&str], data: &CntData) -> Option<&'static str> {
    let analysis = data.solver.as_ref().map(|solver| solver.analysis_type);
    let valid_integer = |key: &str| {
        h.params.get(key).is_none_or(|s| {
            s.parse::<u32>()
                .ok()
                .is_some_and(|v| v > 0 && v <= i32::MAX as u32)
        })
    };
    let valid_tol = h.params.get("CONVERG").is_none_or(|s| {
        s.parse::<f32>()
            .ok()
            .is_some_and(|v| v.is_finite() && v > 0.0)
    });
    let mut expected = Vec::new();
    if !data.boundary_conditions.is_empty() {
        expected.push("BOUNDARY,1".to_string());
    }
    if !data.nodal_loads.is_empty() || !data.distributed_loads.is_empty() {
        expected.push("LOAD,1".to_string());
    }
    let groups: HashSet<_> = data
        .contact_settings
        .iter()
        .map(|contact| contact.group_id)
        .collect();
    expected.extend(groups.into_iter().map(|group| format!("CONTACT,{group}")));
    let mut actual: Vec<_> = body
        .iter()
        .map(|row| {
            row.split(',')
                .map(|s| s.trim().to_ascii_uppercase())
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    expected.sort();
    actual.sort();
    if analysis != Some(AnalysisType::NlStatic)
        || (h.params.contains_key("TYPE") && !parameter_is(h, "TYPE", "STATIC"))
        || (h.params.contains_key("INC_TYPE") && !parameter_is(h, "INC_TYPE", "FIXED"))
        || expected != actual
    {
        Some(
            "Only one fixed-increment nonlinear static STEP activating all boundary, load and contact groups is supported",
        )
    } else if !valid_integer("SUBSTEPS") || !valid_integer("MAXITER") || !valid_tol {
        Some("STEP substeps/iterations/tolerance must be valid positive values")
    } else {
        None
    }
}

fn check_contact(h: &KeywordHeader, body: &[&str], data: &CntData) -> Option<&'static str> {
    let interaction = parameter_value(h, "INTERACTION");
    let tied = interaction.as_deref() == Some("TIED");
    let valid_group = h
        .params
        .get("GRPID")
        .is_none_or(|group| group.parse::<i32>().is_ok_and(|group| group > 0));
    let nonlinear = data
        .solver
        .as_ref()
        .is_some_and(|s| s.analysis_type == AnalysisType::NlStatic);
    if !nonlinear
        || !valid_group
        || !matches!(
            interaction.as_deref(),
            None | Some("SSLID" | "FSLID" | "TIED")
        )
    {
        Some(
            "Contact requires nonlinear static analysis, a positive GRPID and SSLID, FSLID or TIED",
        )
    } else if body.is_empty()
        || body.iter().any(|row| {
            let fields: Vec<_> = row.split(',').map(str::trim).collect();
            fields[0].is_empty()
                || if tied {
                    fields.len() != 1
                } else {
                    fields.len() > 3
                        || fields.get(1).is_some_and(|v| {
                            !v.parse::<f32>().is_ok_and(|v| v.is_finite() && v >= 0.0)
                        })
                        || fields.get(2).is_some_and(|v| {
                            !v.parse::<f32>().is_ok_and(|v| v.is_finite() && v > 0.0)
                        })
                }
        })
    {
        Some(
            "Unsupported contact data; use a pair name and optional friction coefficient and positive friction penalty for sliding",
        )
    } else {
        None
    }
}

fn parameter_value(h: &KeywordHeader, key: &str) -> Option<String> {
    h.params.get(key).map(|value| value.to_ascii_uppercase())
}

fn parameter_is(h: &KeywordHeader, key: &str, expected: &str) -> bool {
    h.params
        .get(key)
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn numbers(row: &str, expected: &[f32]) -> bool {
    let values: Option<Vec<f32>> = row
        .split(',')
        .map(|s| s.trim().parse::<f32>().ok())
        .collect();
    values.as_deref() == Some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{AnalysisSetup, AnalysisType, FemModel, LinearSolverMethod};

    fn read(text: &str) -> CntData {
        super::super::parse_cnt(text, &FemModel::demo_hex8().meshes[0], 0)
    }

    #[test]
    fn generated_basic_static_setup_remains_exportable() {
        for method in [
            LinearSolverMethod::Cg,
            LinearSolverMethod::BiCgStab,
            LinearSolverMethod::Mumps,
            LinearSolverMethod::DirectMkl,
            LinearSolverMethod::Gmres,
            LinearSolverMethod::Direct,
        ] {
            let mut setup = AnalysisSetup::default();
            setup.solver.solver_method = method;
            setup.add_material("STEEL", Some(210_000.0), Some(0.3), Some(7.85e-9));
            setup.add_constraint(0, vec![fem_core::NodeId(1)], 1, 3, 0.0);
            setup.add_load(0, &[fem_core::NodeId(2)], 1, 5e-8);
            let text = crate::cnt_writer::build_cnt(&setup, &[]);
            let parsed = read(&text);
            assert!(
                parsed.import_issues.is_empty(),
                "{:?}",
                parsed.import_issues
            );
            let mut reloaded = AnalysisSetup::default();
            parsed.merge_into(&mut reloaded);
            assert!(
                !crate::validate_frontistr_project(&FemModel::demo_hex8(), &reloaded).has_errors()
            );
        }
    }

    #[test]
    fn unsupported_settings_are_located_including_nested_cards() {
        for card in [
            "!SOLUTION, TYPE=HEAT",
            "!SOLUTION, TYPE=STATICEIGEN",
            "!SOLUTION, TYPE=TYPO",
            "!SOLUTION, TYPE=DYNAMIC, NONLINEAR",
            "!SOLVER, METHOD=DIRECTMKL, TIMELOG=INVALID",
            "!SOLVER, METHOD=DIRECTMKL, ITERLOG=VERBOSE",
            "!SOLVER, METHOD=UNKNOWN",
            "!SOLVER, METHOD=CG, PRECOND=4",
            "!STEP, SUBSTEPS=3, CONVERG=1e-8",
            "!EIGEN\n10, 1e-8, 100, 0.001",
            "!DYNAMIC\n1, 1",
            "!STATIC\n0.1, 1",
            "!BOUNDARY, GRPID=2\n1,1,3,0",
            "!CLOAD, GRPID=1, AMP=RAMP\n1,1,1",
            "!CLOAD, GRPID=1\nMISSING_GROUP,1,10",
            "!CLOAD, GRPID=1\n1,1,not-a-number",
            "!BOUNDARY, GRPID=1\n1,1,3,0,extra",
            "!DLOAD, GRPID=1\n1,GRAV,9.8",
            "!DLOAD, GRPID=1\n1,CENT,1",
            "!MATERIAL, NAME=A\n!PLASTIC\n10,0.1",
            "!MATERIAL, NAME=A\n!ELASTIC, TYPE=ORTHOTROPIC\n1,2,3",
            "!MATERIAL, NAME=A\n!ELASTIC\n100,0.3,20\n200,0.3,100",
            "!MATERIAL, NAME=A\n!ITEM=3\n1e-5",
            "!SECTION, SECNUM=1",
            "!CONTACT_ALGO, TYPE=ALAGRANGE",
            "!WRITE, RESULT, FREQUENCY=0",
            "!VISUAL, METHOD=PVR",
            "!RESTART, READ",
            "!UNKNOWN\n1,2,3",
        ] {
            let data = read(&format!("!! comment\n# comment\n{card}\n!END\n"));
            assert!(!data.import_issues.is_empty(), "unreported: {card}");
            assert!(data.import_issues.iter().all(|issue| issue.line >= 3));
            let mut setup = AnalysisSetup::default();
            data.merge_into(&mut setup);
            assert!(
                crate::validate_frontistr_project(&FemModel::demo_hex8(), &setup).has_errors(),
                "not blocked: {card}"
            );
        }
        let issue = read("!! comment\n\n!SOLUTION, TYPE=HEAT\n")
            .import_issues
            .remove(0);
        assert_eq!(issue.line, 3);
        assert_eq!(issue.keyword, "!SOLUTION, TYPE=HEAT");
    }

    #[test]
    fn unresolved_boundary_blocks_export_but_implicit_all_is_resolved() {
        let data = read("!BOUNDARY\nMISSING,1,3,0\nALL,3,3,0\n!END\n");
        assert!(
            data.import_issues
                .iter()
                .any(|issue| issue.message.contains("Unresolved"))
        );
        assert_eq!(data.boundary_conditions.len(), 1);
        let all = &data.boundary_conditions[0];
        assert_eq!(all.nodes.len(), FemModel::demo_hex8().meshes[0].nodes.len());
        assert!(all.ngrp_name.is_none());
        assert!(
            read("!BOUNDARY\nALL,3,3,0\n!END\n")
                .import_issues
                .is_empty()
        );
    }

    #[test]
    fn supported_contact_groups_round_trip_and_partial_activation_blocks() {
        use fem_core::{ContactPair, ContactType, SurfaceSetRef};
        let mut setup = AnalysisSetup::default();
        setup.solver.analysis_type = AnalysisType::NlStatic;
        let pairs = [
            ContactPair::new(
                "PAIR_A",
                SurfaceSetRef::new(0, 0),
                SurfaceSetRef::new(0, 1),
                ContactType::FiniteSliding,
            )
            .with_contact_parameters(0.12345678, Some(12_345.678)),
            ContactPair::new(
                "PAIR_B",
                SurfaceSetRef::new(0, 0),
                SurfaceSetRef::new(0, 1),
                ContactType::Tied,
            ),
        ];
        let text = crate::cnt_writer::build_cnt(&setup, &pairs);
        let mut loaded = read(&text);
        loaded.check_contact_references(&pairs, std::path::Path::new("input.cnt"), true);
        assert!(
            loaded.import_issues.is_empty(),
            "{:?}",
            loaded.import_issues
        );
        assert_eq!(
            loaded.contact_settings[0].friction_coefficient,
            pairs[0].friction_coefficient
        );
        assert_eq!(
            loaded.contact_settings[0].penalty_factor,
            pairs[0].penalty_factor
        );
        assert_eq!(loaded.contact_settings[1].contact_type, ContactType::Tied);

        let mut unmatched = read(&text);
        unmatched.check_contact_references(&pairs[..1], std::path::Path::new("input.cnt"), true);
        assert!(
            unmatched
                .import_issues
                .iter()
                .any(|issue| issue.message.contains("PAIR_B"))
        );
        for unsupported in [
            text.replace("CONTACT, 2\n", ""),
            text.replace("TYPE=ALAGRANGE", "TYPE=SLAGRANGE"),
            text.replace("INTERACTION=FSLID", "INTERACTION=FSLID, NPENALTY=100"),
            text.replace("INTERACTION=FSLID", "INTERACTION=UNKNOWN"),
            text.replace("PAIR_B\n", "PAIR_A\n"),
        ] {
            assert!(
                !read(&unsupported).import_issues.is_empty(),
                "{unsupported}"
            );
        }
    }

    #[test]
    fn conrod_solver_and_rendering_options_survive_output_field_edits() {
        for (format, label) in [("VTK", "VTK"), ("COMPLETE_MICROAVS", "MicroAVS")] {
            let visual = format!(
                "!VISUAL, method=PSR\n!surface_num = 1\n!surface 1\n!surface_style = 1\n\
                 !group_name = boundary\n!display_method  1\n!color_comp_name = STRESS\n\
                 !color_subcomp       7\n!initial_style = 1\n!deform_style = 3\n\
                 !output_type  = {format}\n!x_resolution = 500\n!y_resolution = 500\n\
                 !position_of_lights = 100 100 50\n!up_direction = 0.0 0.0 1.0\n\
                 !font_color = 1.0 1.0 1.0\n"
            );
            let input = format!(
                "!SOLUTION, TYPE=static\n!SOLVER,METHOD=CG,PRECOND=3,ITERLOG=YES,TIMELOG=YES\n\
                 50000, 1\n1.0e-8, 1.0, 0.0\n!WRITE,RESULT\n!WRITE,VISUAL,\n{visual}!END\n"
            );
            let data = read(&input);
            assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
            let mut setup = AnalysisSetup::default();
            data.merge_into(&mut setup);
            assert_eq!(setup.solver.preconditioner, 3);
            assert_eq!(setup.output.label(fem_core::OutputTarget::Vis), label);
            assert!(
                setup
                    .output
                    .vis
                    .set(fem_core::OutputTarget::Vis, "NSTRESS", Some(true))
            );
            let written = crate::cnt_writer::build_cnt(&setup, &[]);
            assert!(written.contains("PRECOND=3"));
            assert!(written.contains(&visual));
            let reloaded = read(&written);
            assert!(
                reloaded.import_issues.is_empty(),
                "{:?}",
                reloaded.import_issues
            );
            assert_eq!(reloaded.output, setup.output);
            assert_eq!(reloaded.solver.as_ref(), Some(&setup.solver));
        }
    }

    #[test]
    fn only_one_supported_psr_boundary_and_format_are_preserved() {
        let prefix = "!VISUAL, METHOD=PSR\n!surface_num=1\n!surface 1\n";
        for options in [
            "!group_name=SEAT\n!output_type=VTK",
            "!surface_style=2\n!output_type=VTK",
            "!output_type=AVS",
            "!output_type=VTK\n!output_type=COMPLETE_MICROAVS",
            "!output_type=VTK\n!RESTART, READ",
            "!output_type=VTK\n!UNKNOWN_VISUAL=1",
        ] {
            assert!(
                !read(&format!("{prefix}{options}\n!END\n"))
                    .import_issues
                    .is_empty(),
                "{options}"
            );
        }
        assert!(!read("!surface_num=1\n!END\n").import_issues.is_empty());
    }

    #[test]
    fn repeated_cards_are_reported_but_linear_iteration_settings_are_preserved() {
        assert!(
            !read("!SOLUTION, TYPE=STATIC\n!SOLUTION, TYPE=EIGEN")
                .import_issues
                .is_empty()
        );
        let text = "!SOLVER, METHOD=CG, PRECOND=1, ITERLOG=YES, TIMELOG=YES\n200,1\n1e-8,1,0\n";
        let parsed = read(text);
        assert!(parsed.import_issues.is_empty());
        let solver = parsed.solver.unwrap();
        assert_eq!(solver.max_iterations, Some(200));
        assert_eq!(solver.convergence_tol, Some(1e-8));
    }

    #[test]
    fn preserved_output_extensions_and_comments_are_not_blockers() {
        let text = "!! comment\n!OUTPUT_RES, GROUP=BOUNDARY, ACTION=ON\nUNKNOWN_QUANTITY, ON\n!! comment\n!OUTPUT_VIS\nDISP, ON\n!END\n";
        let parsed = read(text);
        assert!(parsed.import_issues.is_empty());
        let mut setup = AnalysisSetup::default();
        parsed.merge_into(&mut setup);
        assert!(crate::cnt_writer::build_cnt(&setup, &[]).contains("UNKNOWN_QUANTITY, ON"));
    }

    #[test]
    fn blockers_survive_merges_edits_cloning_and_assembly_until_new_model() {
        let mut setup = AnalysisSetup::default();
        read("!SOLUTION, TYPE=HEAT").merge_into(&mut setup);
        let original = setup.import_issues.clone();
        assert!(!setup.is_empty());
        read("!SOLUTION, TYPE=STATIC").merge_into(&mut setup);
        read("!SOLUTION, TYPE=HEAT").merge_into(&mut setup);
        setup.solver.analysis_type = AnalysisType::Static;
        assert_eq!(setup.import_issues, original);
        assert_eq!(
            crate::remap_setup_for_assembly(&setup, &[(0, 0)]).import_issues,
            original
        );
        assert!(!crate::cnt_writer::export_blockers(&setup).is_empty());
        setup.clear();
        assert!(setup.import_issues.is_empty());
    }

    #[test]
    fn import_source_and_export_failure_preserve_existing_files() {
        let dir = std::env::temp_dir().join(format!(
            "bevyistr-compat-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let source = dir.join("original.cnt");
        std::fs::write(&source, "!SOLUTION, TYPE=HEAT\n!END\n").unwrap();
        let model = FemModel::demo_hex8();
        let data = crate::load_cnt_file(&source, &model.meshes[0], 0).unwrap();
        assert_eq!(data.import_issues[0].source, source.display().to_string());
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        for name in ["hecmw_ctrl.dat", "test.msh", "test.cnt"] {
            std::fs::write(dir.join(name), "keep original").unwrap();
        }
        assert!(crate::write_frontistr_project(&dir, "test", &model, &setup).is_err());
        assert!(crate::write_cnt_file(dir.join("test.cnt"), &setup).is_err());
        for name in ["hecmw_ctrl.dat", "test.msh", "test.cnt"] {
            assert_eq!(
                std::fs::read_to_string(dir.join(name)).unwrap(),
                "keep original"
            );
            std::fs::remove_file(dir.join(name)).unwrap();
        }
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn newmark_dynamic_settings_round_trip_without_enabling_incomplete_project_export() {
        let source = concat!(
            "!SOLUTION, TYPE=DYNAMIC\n",
            "!DYNAMIC, TYPE=NONLINEAR\n",
            "1,1\n0,1d5,10000,10\n0.5,0.25\n1,1,0,0\n",
            "50,MONITOR,50\n1,0,0,0,2,3\n!END\n",
        );
        let data = read(source);
        assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        let settings = setup.solver.dynamic.as_ref().unwrap();
        assert!(settings.nonlinear);
        assert_eq!(settings.fallback_substeps, 10000);
        assert_eq!(settings.fallback_increment, 10.);
        assert_eq!(settings.monitor_node_or_group, "MONITOR");
        assert_eq!(settings.output_flags, [1, 0, 0, 0, 2, 3]);
        let generated = crate::cnt_writer::build_cnt(&setup, &[]);
        let reloaded = read(&generated);
        assert_eq!(reloaded.solver.unwrap().dynamic, setup.solver.dynamic);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dynamic.cnt");
        let error = crate::write_cnt_file(&path, &setup).unwrap_err().to_string();
        assert!(error.contains("Full STEP and boundary/load group controls"));
        assert!(!path.exists());

        for (before, after) in [
            ("1,1\n0,1d5", "2,1\n0,1d5"),
            ("1,1\n0,1d5", "11,1\n0,1d5"),
            ("1,1\n0,1d5", "1,2\n0,1d5"),
            ("10000,10", "10000,0"),
            ("0.5,0.25", "0.5,NaN"),
            ("50,MONITOR,50", "50,,50"),
            ("0,2,3\n", "0,2,4\n"),
        ] {
            let data = read(&source.replace(before, after));
            assert!(data.import_issues.iter().any(|issue| issue.keyword.starts_with("!DYNAMIC")), "{after}");
            assert!(data.solver.unwrap().dynamic.is_none());
        }
    }

    #[test]
    fn freshly_authored_dynamic_cannot_write_header_only_input() {
        let mut setup = AnalysisSetup::default();
        setup.solver.analysis_type = AnalysisType::Dynamic;
        assert!(crate::validate_frontistr_project(&FemModel::demo_hex8(), &setup).has_errors());
        assert!(!crate::cnt_writer::export_blockers(&setup).is_empty());
    }

    #[test]
    fn lowercase_material_subcards_and_comments_are_read() {
        let parsed = read(
            "!material, name=steel\n!! comment\n!elastic\n!! inside elastic\n100,0.3,0\n!density\n# inside density\n7.85e-9,0\n!end",
        );
        assert!(
            parsed.import_issues.is_empty(),
            "{:?}",
            parsed.import_issues
        );
        assert_eq!(parsed.materials[0].young_modulus, Some(100.0));
        assert_eq!(parsed.materials[0].density, Some(7.85e-9));
    }

    #[test]
    fn comments_do_not_truncate_boundary_load_or_inline_node_groups() {
        let parsed = read(
            "!ngroup, NGRP=FIXED\n1\n!! comment\n2\n!SOLUTION, TYPE=EIGEN # comment\n!EIGEN\n3\n!BOUNDARY, GRPID=1\n!! comment\nFIXED,1,3,0\n!CLOAD, GRPID=1\n1,1,10\n!! comment\n2,1,20\n!END",
        );
        assert!(
            parsed.import_issues.is_empty(),
            "{:?}",
            parsed.import_issues
        );
        assert_eq!(parsed.boundary_conditions[0].nodes.len(), 2);
        assert_eq!(parsed.nodal_loads.len(), 2);
        assert_eq!(parsed.solver.unwrap().analysis_type, AnalysisType::Eigen);
    }
}
