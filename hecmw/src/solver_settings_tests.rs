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
        assert_eq!(setup.solver.max_iterations, Some(432));
        assert_eq!(setup.solver.convergence_tol, Some(1.2345679e-9));
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
    assert_eq!(settings.max_iterations, Some(100));
    assert_eq!(settings.convergence_tol, Some(1e-6));
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
        "100,0\n1e-8,1,0",
        "100,1,30,20,3,1\n1e-8,1,0",
        "100.0,1\n1e-8,1,0",
        "100,1\n1e-8,1.1,NaN",
        "100,1,0\n1e-8,1,0",
        "100,1,,0\n1e-8,1,0",
        "100,1,,20,-1\n1e-8,1,0",
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
fn iterative_solver_rows_round_trip_with_positional_omissions() {
    for (rows, expected) in [
        (
            "10000,1,40,100\n1e-8,1,0",
            (Some(40), Some(100), None, Some(1.0), Some(0.0)),
        ),
        (
            "10000,2,,100,0\n1e-8,,1.2345678901234567",
            (None, Some(100), Some(0), None, Some(1.2345678901234567)),
        ),
        (
            "10000,1\n1e-8,1.2345678901234567",
            (None, None, None, Some(1.2345678901234567), None),
        ),
    ] {
        let data = parsed(&format!("!SOLVER,METHOD=GMRES,PRECOND=1\n{rows}\n!END"));
        assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        let s = &setup.solver;
        assert_eq!(
            (s.gmres_subspaces, s.ordering_colors, s.preconditioner_reuse,
             s.diagonal_scale, s.ssor_relaxation),
            expected,
        );
        let reloaded = parsed(&crate::cnt_writer::build_cnt(&setup, &[]));
        assert!(reloaded.import_issues.is_empty(), "{:?}", reloaded.import_issues);
        assert_eq!(reloaded.solver.unwrap(), setup.solver);
    }
}

#[test]
fn omitted_solver_rows_and_columns_preserve_frontistr_defaults() {
    let header = "!SOLVER, METHOD=GMRES, PRECOND=1";
    for (rows, expected_values, emitted_rows) in [
        ("", (None, None, None), "\n"),
        ("123\n", (Some(123), None, None), " 123\n\n"),
        (",2\n", (None, Some(2), None), " , 2\n\n"),
        (
            ",\n1.2345678901234567e-12\n",
            (None, None, Some(1.2345678901234567e-12)),
            " ,\n 1.2345678901234567e-12\n\n",
        ),
        (",,40\n,,0\n", (None, None, None), " , , 40\n , , 0e0\n\n"),
    ] {
        let input = format!("{header}\n{rows}!END\n");
        let data = parsed(&input);
        assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        let solver = &setup.solver;
        assert_eq!(
            (solver.max_iterations, solver.preconditioner_iterations, solver.convergence_tol),
            expected_values,
        );
        let exported = crate::cnt_writer::build_cnt(&setup, &[]);
        assert!(exported.contains(&format!("{header}\n{emitted_rows}")), "{exported}");
        let reloaded = parsed(&exported);
        assert!(reloaded.import_issues.is_empty(), "{:?}", reloaded.import_issues);
        assert_eq!(reloaded.solver.unwrap(), setup.solver);
    }
}

#[test]
fn hinge_solver_options_and_implicit_group_one_survive_export() {
    let text = concat!(
        "!SOLUTION, TYPE=STATIC\n!BOUNDARY\n1,1,3,0\n!CLOAD\n2,1,10\n",
        "!SOLVER,METHOD=CG,PRECOND=1,ITERLOG=YES,TIMELOG=YES\n",
        "10000,2\n1e-8,1,0\n!END\n",
    );
    let data = parsed(text);
    assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
    let mut setup = AnalysisSetup::default();
    data.merge_into(&mut setup);
    assert_eq!(setup.solver.preconditioner_iterations, Some(2));
    let output = crate::cnt_writer::build_cnt(&setup, &[]);
    assert!(output.contains("!BOUNDARY, GRPID=1"));
    assert!(output.contains("!CLOAD, GRPID=1"));
    assert!(output.contains("10000, 2"));
    let reloaded = parsed(&output);
    assert!(
        reloaded.import_issues.is_empty(),
        "{:?}",
        reloaded.import_issues
    );
    assert_eq!(reloaded.solver.unwrap(), setup.solver);
    let other_group = text.replace("!CLOAD", "!CLOAD,GRPID=2");
    assert!(!parsed(&other_group).import_issues.is_empty());
}

#[test]
fn solver_log_settings_preserve_explicit_values_and_omissions() {
    for (solver, rows) in [
        ("METHOD=CG, PRECOND=2", "100,1\n1e-6,1,0\n"),
        ("METHOD=MUMPS", ""),
    ] {
        for (iter_param, iteration_log) in [
            ("", None),
            (", ITERLOG=yes", Some(true)),
            (", ITERLOG=NO", Some(false)),
        ] {
            for (time_param, time_log) in [
                ("", None),
                (", TIMELOG=YES", Some(SolverTimeLog::Summary)),
                (", TIMELOG=no", Some(SolverTimeLog::Off)),
                (", TIMELOG=Verbose", Some(SolverTimeLog::Verbose)),
            ] {
                let imported = parsed(&format!(
                    "!SOLVER, {solver}{iter_param}{time_param}\n{rows}!END"
                ));
                assert!(imported.import_issues.is_empty(), "{:?}", imported.import_issues);
                let mut setup = AnalysisSetup::default();
                imported.merge_into(&mut setup);
                assert_eq!(setup.solver.iteration_log, iteration_log);
                assert_eq!(setup.solver.time_log, time_log);

                let output = crate::cnt_writer::build_cnt(&setup, &[]);
                assert_eq!(output.contains("ITERLOG="), iteration_log.is_some());
                assert_eq!(output.contains("TIMELOG="), time_log.is_some());
                let reloaded = parsed(&output);
                assert!(reloaded.import_issues.is_empty(), "{:?}", reloaded.import_issues);
                assert_eq!(reloaded.solver.unwrap(), setup.solver);
            }
        }
    }
}

#[test]
fn cavity_solver_controls_round_trip_with_explicit_or_default_scaling() {
    for (parameter, expected) in [
        ("", None),
        (", SCALING=NO", Some(false)),
        (", SCALING=yes", Some(true)),
    ] {
        let input = format!(
            "!SOLVER, METHOD=bicgstab, PRECOND=1, ITERLOG=NO, TIMELOG=YES{parameter}\n100000, 1\n1.0e-8, 1.0, 1.0\n!END"
        );
        let data = parsed(&input);
        assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        assert_eq!(setup.solver.solver_method, LinearSolverMethod::BiCgStab);
        assert_eq!(setup.solver.matrix_scaling, expected);
        assert_eq!(setup.solver.max_iterations, Some(100000));
        assert_eq!(setup.solver.ssor_relaxation, Some(1.0));
        let output = crate::cnt_writer::build_cnt(&setup, &[]);
        assert!(output.contains("METHOD=BiCGSTAB, PRECOND=1"));
        assert_eq!(output.contains("SCALING="), expected.is_some());
        let reloaded = parsed(&output);
        assert!(reloaded.import_issues.is_empty(), "{:?}", reloaded.import_issues);
        assert_eq!(reloaded.solver.unwrap(), setup.solver);
    }

    for (parameters, message) in [
        ("METHOD=BiCGSTAB, PRECOND=1, SCALING=ON", "SCALING must be YES or NO"),
        ("METHOD=BiCGSTAB, PRECOND=1, SCALING=", "SCALING must be YES or NO"),
        ("METHOD=MUMPS, SCALING=YES", "only for iterative"),
    ] {
        let data = parsed(&format!("!SOLVER, {parameters}\n!END"));
        assert!(
            data.import_issues.iter().any(|issue| issue.message.contains(message)),
            "{parameters}: {:?}", data.import_issues
        );
    }
}

#[test]
fn matrix_dump_settings_round_trip_for_iterative_and_direct_solvers() {
    for (header, rows) in [
        (
            "METHOD=CG, PRECOND=1, ITERLOG=YES, TIMELOG=YES",
            "100,1\n1e-6,1,0\n",
        ),
        ("METHOD=MUMPS", ""),
    ] {
        for (input_format, format) in [
            ("NONE", MatrixDumpFormat::None),
            ("mm", MatrixDumpFormat::MatrixMarket),
            ("2", MatrixDumpFormat::Csr),
            ("BSR", MatrixDumpFormat::Bsr),
        ] {
            for stop in [false, true] {
                let exit = if stop { "yes" } else { "NO" };
                let cnt = format!(
                    "!SOLVER, {header}, DUMPTYPE={input_format}, DUMPEXIT={exit}\n{rows}!END"
                );
                let imported = parsed(&cnt);
                assert!(
                    imported.import_issues.is_empty(),
                    "{:?}",
                    imported.import_issues
                );
                let mut setup = AnalysisSetup::default();
                imported.merge_into(&mut setup);
                assert_eq!(setup.solver.matrix_dump_format, format);
                assert_eq!(setup.solver.exit_after_matrix_dump, stop);
                let exported = crate::cnt_writer::build_cnt(&setup, &[]);
                let reloaded = parsed(&exported);
                assert!(
                    reloaded.import_issues.is_empty(),
                    "{:?}",
                    reloaded.import_issues
                );
                assert_eq!(reloaded.solver.unwrap(), setup.solver);
            }
        }
    }
}

#[test]
fn unsupported_matrix_dump_parameters_still_block_export() {
    for options in [
        "DUMPTYPE=COO",
        "DUMPTYPE=4",
        "DUMPTYPE",
        "DUMPEXIT=TRUE",
        "DUMPEXIT=",
        "DUMPTYPE=NONE, DUMPTYPE=MM",
        "DUMPEXIT=YES, DUMPEXIT=NO",
        "DUMPTYPE=MM, MATRIXFORMAT=BSR",
    ] {
        let data = parsed(&format!("!SOLVER, METHOD=MUMPS, {options}\n!END"));
        assert!(!data.import_issues.is_empty(), "{options}");
    }
}

#[test]
fn editing_one_tolerance_does_not_change_the_other_on_export() {
    let mut setup = AnalysisSetup::default();
    setup.solver.analysis_type = AnalysisType::NlStatic;
    for value in [1.2345678901234567e-18, 2.3456789e-4, f64::MIN_POSITIVE] {
        setup.solver.convergence_tol = Some(value);
        let after_linear = parsed(&crate::cnt_writer::build_cnt(&setup, &[]))
            .solver
            .unwrap();
        assert_eq!(after_linear.convergence_tol.unwrap().to_bits(), value.to_bits());
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
    setup.solver.max_iterations = Some(u32::MAX);
    assert!(
        crate::validate_frontistr_project(&model, &setup)
            .summary(100)
            .contains("Linear solver")
    );
}
