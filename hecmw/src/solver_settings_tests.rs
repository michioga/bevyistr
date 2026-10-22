use super::*;

fn parsed(text: &str) -> CntData {
    parse_cnt(text, &FemMesh::demo_hex8(), 0)
}

#[test]
fn linear_and_nonlinear_values_are_independent_in_both_card_orders() {
    let step = "!STEP, SUBSTEPS=8, MAXITER=27, CONVERG=2.3456789e-4\nBOUNDARY,1\nLOAD,1\n";
    let linear = "!SOLVER, METHOD=GMRES, PRECOND=1, ITERLOG=YES, TIMELOG=YES\n!! comment\n432,1\n1.2345679e-9,1,0\n";
    for (first, second) in [(step, linear), (linear, step)] {
        let text = format!(
            "!SOLUTION, TYPE=STATIC, NONLINEAR\n!BOUNDARY,GRPID=1\n1,1,3,0\n!CLOAD,GRPID=1\n2,1,10\n{first}{second}!END\n"
        );
        let data = parsed(&text);
        assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        assert_eq!(setup.solver.substeps, 8);
        assert_eq!(setup.solver.max_iterations, 432);
        assert_eq!(setup.solver.convergence_tol, 1.2345679e-9);
        assert_eq!(setup.solver.nonlinear_max_iterations, 27);
        assert_eq!(setup.solver.nonlinear_convergence_tol, 2.3456789e-4);
        let output = crate::cnt_writer::build_cnt(&setup, &[]);
        let reloaded = parsed(&output);
        assert!(
            reloaded.import_issues.is_empty(),
            "{:?}",
            reloaded.import_issues
        );
        assert_eq!(reloaded.solver.unwrap(), setup.solver);
        assert!(
            !crate::validate_frontistr_project(&fem_core::FemModel::demo_hex8(), &setup)
                .has_errors()
        );
    }
}

#[test]
fn step_defaults_are_frontistr_defaults_not_linear_editor_values() {
    let data = parsed("!SOLUTION, TYPE=NLSTATIC\n!STEP\n!END");
    assert!(data.import_issues.is_empty());
    let settings = data.solver.unwrap();
    assert_eq!(settings.nonlinear_max_iterations, 50);
    assert_eq!(settings.nonlinear_convergence_tol, 1e-3);
    assert_eq!(settings.max_iterations, 100);
    assert_eq!(settings.convergence_tol, 1e-6);
}

#[test]
fn direct_solver_identifiers_are_not_conflated() {
    for (token, method, output) in [
        ("MUMPS", LinearSolverMethod::Mumps, "MUMPS"),
        ("DIRECTmkl", LinearSolverMethod::DirectMkl, "DIRECTmkl"),
        ("MKL", LinearSolverMethod::DirectMkl, "DIRECTmkl"),
        ("DIRECT", LinearSolverMethod::Direct, "DIRECT"),
    ] {
        let data = parsed(&format!(
            "!SOLUTION, TYPE=STATIC\n!SOLVER, METHOD={token}, PRECOND=1, ITERLOG=YES, TIMELOG=YES\n100,1\n1e-6,1,0\n!END"
        ));
        assert!(
            data.import_issues.is_empty(),
            "{token}: {:?}",
            data.import_issues
        );
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        assert_eq!(setup.solver.solver_method, method);
        let cnt = crate::cnt_writer::build_cnt(&setup, &[]);
        assert!(cnt.contains(&format!("METHOD={output},")));
        assert_eq!(parsed(&cnt).solver.unwrap().solver_method, method);
    }
    assert!(!parsed("!SOLVER, METHOD=PARDISO").import_issues.is_empty());
}

#[test]
fn unsupported_step_features_and_invalid_values_remain_blocked() {
    for step in [
        "!STEP\n!STEP",
        "!STEP, INC_TYPE=AUTO\n0.1,1,0.01,0.2",
        "!STEP, TYPE=VISCO",
        "!STEP, CONVERG_DDISP=1e-9",
        "!STEP, MAXITER=0",
        "!STEP, MAXITER=5000000000",
        "!STEP, SUBSTEPS=2.5",
        "!STEP, CONVERG=NaN",
        "!STEP\n0.1,1",
        "!STEP\nBOUNDARY,2",
        "!STEP\nCONTACT,1",
    ] {
        let data = parsed(&format!("!SOLUTION, TYPE=NLSTATIC\n{step}\n!END"));
        assert!(!data.import_issues.is_empty(), "{step}");
    }
    // A defined BC that is NOT activated must not silently become active.
    let data = parsed("!SOLUTION, TYPE=NLSTATIC\n!BOUNDARY, GRPID=1\n1,1,3,0\n!STEP\n!END");
    assert!(!data.import_issues.is_empty());
    for row in [
        "100,2\n1e-8,1,0",
        "100,1,30\n1e-8,1,0",
        "100.0,1\n1e-8,1,0",
        "100,1\n1e-8,1.1,0",
    ] {
        assert!(
            !parsed(&format!(
                "!SOLVER, METHOD=CG, PRECOND=1, ITERLOG=YES, TIMELOG=YES\n{row}\n!END"
            ))
            .import_issues
            .is_empty()
        );
    }
}

#[test]
fn editing_one_tolerance_does_not_change_the_other_on_export() {
    let mut setup = AnalysisSetup::default();
    setup.solver.analysis_type = AnalysisType::NlStatic;
    for value in [1.2345679e-18, 2.3456789e-4, f32::MIN_POSITIVE] {
        setup.solver.convergence_tol = value;
        let after_linear = parsed(&crate::cnt_writer::build_cnt(&setup, &[]))
            .solver
            .unwrap();
        assert_eq!(after_linear.convergence_tol.to_bits(), value.to_bits());
        assert_eq!(after_linear.nonlinear_convergence_tol, 1e-3);
    }
    setup.solver.nonlinear_convergence_tol = 5.6789123e-5;
    let after_nonlinear = parsed(&crate::cnt_writer::build_cnt(&setup, &[]))
        .solver
        .unwrap();
    assert_eq!(after_nonlinear, setup.solver);
}

#[test]
fn invalid_authoring_values_are_rejected_by_preflight() {
    let model = fem_core::FemModel::demo_hex8();
    let mut setup = AnalysisSetup::default();
    setup.solver.analysis_type = AnalysisType::NlStatic;
    setup.solver.nonlinear_max_iterations = 0;
    assert!(
        crate::validate_frontistr_project(&model, &setup)
            .summary(100)
            .contains("Nonlinear step")
    );
    setup.solver.nonlinear_max_iterations = 50;
    setup.solver.nonlinear_convergence_tol = f32::NAN;
    assert!(crate::validate_frontistr_project(&model, &setup).has_errors());
    setup.solver.nonlinear_convergence_tol = 1e-3;
    setup.solver.max_iterations = u32::MAX;
    assert!(
        crate::validate_frontistr_project(&model, &setup)
            .summary(100)
            .contains("Linear solver")
    );
}
