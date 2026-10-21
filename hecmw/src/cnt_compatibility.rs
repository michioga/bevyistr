//! Conservative import/export compatibility checks, independent of the lenient
//! reader. Recognizing a card is not enough: ignored parameters/data must also
//! be reported. Expand this allow-list only with round-trip regression tests.

use super::{CntData, KeywordHeader, parse_keyword_header, strip_comment};
use fem_core::{SetupImportIssue, SolverSettings};
use std::collections::HashSet;

pub(super) fn audit(text: &str, data: &CntData) -> Vec<SetupImportIssue> {
    let lines: Vec<_> = text.lines().collect();
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    let mut material = false;
    let mut i = 0;
    while i < lines.len() {
        let raw = strip_comment(lines[i]);
        if !raw.starts_with('!') || raw.starts_with("!!") {
            i += 1;
            continue;
        }
        let line = i + 1;
        let header = parse_keyword_header(raw);
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
        let reason = if matches!(name, "SOLUTION" | "SOLVER" | "STATIC" | "EIGEN" | "VERSION")
            && !seen.insert(header.name.clone())
        {
            Some("Repeated control cards cannot be preserved".to_string())
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
            || (material && (matches!(name, "ELASTIC" | "DENSITY") || name.starts_with("ITEM")));
    }
    issues
}

fn check_card(h: &KeywordHeader, body: &[&str], data: &CntData, material: bool) -> Option<String> {
    let name = h.name.as_str();
    // These cards are stored verbatim by the reader, including extension items.
    if matches!(name, "OUTPUT_RES" | "OUTPUT_VIS") {
        return None;
    }
    let allowed: &[&str] = match name {
        "SOLUTION" => &["TYPE", "NONLINEAR"],
        "SOLVER" => &["METHOD", "PRECOND", "ITERLOG", "TIMELOG"],
        "BOUNDARY" | "CLOAD" => &["GRPID", "ROT_CENTER"],
        "DLOAD" => &["GRPID"],
        "CONTACT" => &["GRPID", "INTERACTION"],
        "MATERIAL" => &["NAME"],
        "NGROUP" | "NGRP" => &["NGRP"],
        "WRITE" => &["RESULT", "VISUAL"],
        "VISUAL" => &["METHOD"],
        "ELASTIC" => &["TYPE"],
        "VERSION" | "END" | "STATIC" | "EIGEN" | "DENSITY"
        | "SURFACE_NUM=1" | "SURFACE 1" | "OUTPUT_TYPE=VTK" | "ITEM=1" | "ITEM=2" => &[],
        "STEP" => return Some("Step timing, activation groups and nonlinear iteration controls are not fully preserved yet".into()),
        "DYNAMIC" => return Some("Dynamic integration settings cannot yet be regenerated".into()),
        "SECTION" => return Some("CNT section formulation/orientation is not preserved by the editor".into()),
        _ => return Some("Unsupported CNT keyword; its settings would be omitted on export".into()),
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
    let value = |key: &str| h.params.get(key).map(|v| v.to_ascii_uppercase());
    let is = |key: &str, expected: &str| value(key).as_deref() == Some(expected);
    let reason = match name {
        "SOLUTION" => {
            if !matches!(
                value("TYPE").as_deref(),
                Some("STATIC" | "NLSTATIC" | "DYNAMIC" | "EIGEN" | "EIGENVALUE")
            ) {
                Some(
                    "Unsupported or missing analysis TYPE; displayed solver defaults are not the imported analysis",
                )
            } else if is("TYPE", "DYNAMIC") {
                Some("Dynamic setup authoring is incomplete; inspection only")
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
        "SOLVER" => {
            let solver = data.solver.clone().unwrap_or_else(SolverSettings::default);
            if !matches!(
                value("METHOD").as_deref(),
                Some("CG" | "GMRES" | "MUMPS" | "DIRECT")
            ) {
                Some(
                    "Unsupported or missing solver METHOD; aliases such as DIRECTMKL must not be converted to DIRECT",
                )
            } else if !is("PRECOND", "1") || !is("ITERLOG", "YES") || !is("TIMELOG", "YES") {
                Some(
                    "Solver options/defaults are not preserved (writer uses PRECOND=1, ITERLOG=YES, TIMELOG=YES)",
                )
            } else if body.len() != 2
                || !numbers(body[0], &[solver.max_iterations as f32, 1.0])
                || !numbers(body[1], &[solver.convergence_tol, 1.0, 0.0])
            {
                Some(
                    "Linear solver data/defaults are not preserved yet; separate linear/nonlinear settings are planned",
                )
            } else {
                None
            }
        }
        "STATIC" if body.len() != 1 || !numbers(body[0], &[1.0, 1.0]) => {
            Some("Static increment/time settings are not preserved (writer uses 1, 1)")
        }
        "EIGEN" if body.len() != 1 || !numbers(body[0], &[3.0]) => Some(
            "Eigen mode count/tolerance/iterations/shift are not preserved (writer currently requests 3 modes)",
        ),
        "VERSION" if body.len() != 1 || !numbers(body[0], &[3.0]) => {
            Some("Only VERSION 3 is emitted")
        }
        "BOUNDARY" | "CLOAD" | "DLOAD" if !is("GRPID", "1") => {
            Some("Only explicit GRPID=1 is preserved for boundary/load groups")
        }
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
        "CLOAD"
            if body.iter().any(|row| {
                let target = row.split(',').next().unwrap().trim();
                target.parse::<u32>().is_err()
                    && !data
                        .nodal_loads
                        .iter()
                        .any(|load| load.ngrp_name.as_deref() == Some(target))
            }) =>
        {
            Some("Unresolved CLOAD node group would produce no loads")
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
        "CONTACT" => Some(
            "Contact activation groups and detailed interaction settings are not fully preserved yet",
        ),
        "ELASTIC" | "DENSITY" | "ITEM=1" | "ITEM=2" => {
            let elastic = matches!(name, "ELASTIC" | "ITEM=1");
            let values: Option<Vec<f32>> = body.first().and_then(|row| {
                row.split(',')
                    .map(|s| s.trim().parse::<f32>().ok())
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
        "WRITE"
            if h.params.len() != 1
                || !(is("RESULT", "") || is("VISUAL", ""))
                || !body.is_empty() =>
        {
            Some("Output frequency/WRITE options are not preserved")
        }
        "VISUAL" if !is("METHOD", "PSR") || !body.is_empty() => {
            Some("Only the default PSR/VTK visualization is regenerated")
        }
        "MATERIAL" | "END" | "SURFACE_NUM=1" | "SURFACE 1" | "OUTPUT_TYPE=VTK"
            if !body.is_empty() =>
        {
            Some("Unexpected data would be omitted")
        }
        _ => None,
    };
    reason.map(str::to_string)
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
            LinearSolverMethod::Mumps,
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
            "!SOLVER, METHOD=DIRECTMKL",
            "!SOLVER, METHOD=UNKNOWN",
            "!SOLVER, METHOD=CG, PRECOND=2",
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
            "!WRITE, RESULT, FREQUENCY=10",
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
    fn repeated_cards_and_nondefault_solver_data_are_reported() {
        assert!(
            !read("!SOLUTION, TYPE=STATIC\n!SOLUTION, TYPE=EIGEN")
                .import_issues
                .is_empty()
        );
        let text = "!SOLVER, METHOD=CG, PRECOND=1, ITERLOG=YES, TIMELOG=YES\n200,1\n1e-8,1,0\n";
        assert_eq!(read(text).import_issues.len(), 1);
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
            "!ngroup, NGRP=FIXED\n1\n!! comment\n2\n!SOLUTION, TYPE=EIGEN # comment\n!BOUNDARY, GRPID=1\n!! comment\nFIXED,1,3,0\n!CLOAD, GRPID=1\n1,1,10\n!! comment\n2,1,20\n!END",
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
