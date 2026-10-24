use super::*;
use fem_core::{AnalysisSetup, AnalysisType, FemMesh, FemModel};

fn read(text: &str) -> super::super::CntData {
    super::super::parse_cnt(text, &FemMesh::demo_hex8(), 0)
}

#[test]
fn optional_columns_comments_and_fortran_exponents_use_documented_defaults() {
    for row in ["5", "5,", "5,,,", "5,1d-6,60,1D-2"] {
        assert_eq!(
            parse_rows(&[row]).unwrap(),
            EigenSettings {
                mode_count: 5,
                ..Default::default()
            }
        );
    }
    let data = read("!solution,type=eigen\n!eigen\n!! comment\n5,1.0E-8,60 # spring example\n!end");
    assert!(data.import_issues.is_empty(), "{:?}", data.import_issues);
    let e = data.solver.unwrap().eigen;
    assert_eq!(e.mode_count, 5);
    assert_eq!(e.tolerance, 1e-8);
    assert_eq!(e.shift, 0.01);
}

#[test]
fn round_trip_preserves_all_four_values_and_independent_solver_controls() {
    let solution = "!SOLUTION, TYPE=EIGEN\n";
    let eigen = "!EIGEN\n7,1.2345678901234567e-12,87,-0.12345678901234567\n";
    for text in [format!("{solution}{eigen}"), format!("{eigen}{solution}")] {
        let data = read(&text);
        assert!(data.import_issues.is_empty());
        let mut setup = AnalysisSetup::default();
        data.merge_into(&mut setup);
        setup.solver.max_iterations = 432;
        setup.solver.convergence_tol = 2e-7;
        let output = crate::cnt_writer::build_cnt(&setup, &[]);
        let reloaded = read(&output);
        assert!(
            reloaded.import_issues.is_empty(),
            "{:?}",
            reloaded.import_issues
        );
        assert_eq!(reloaded.solver.unwrap(), setup.solver);
        assert!(!crate::validate_frontistr_project(&FemModel::demo_hex8(), &setup).has_errors());
    }
}

#[test]
fn malformed_or_extended_cards_block_export_without_partial_substitution() {
    for card in [
        "!EIGEN",
        "!EIGEN\n",
        "!EIGEN\n,1e-6,60,0",
        "!EIGEN\n0",
        "!EIGEN\n3.5",
        "!EIGEN\n5,-1,60,0",
        "!EIGEN\n5,NaN,60,0",
        "!EIGEN\n5,1e-8,5,0",
        "!EIGEN\n5,1e-8,60,inf",
        "!EIGEN\n5,1e-8,60,0,99",
        "!EIGEN\n5\n6",
        "!EIGEN\n5\n!EIGEN\n6",
        "!EIGEN, EXTRA=1\n5",
        "!EIGEN\n2147483648",
        "!EIGEN\n5,1e-8,4294967295,0",
    ] {
        let mut setup = AnalysisSetup::default();
        let data = read(&format!("!SOLUTION,TYPE=EIGEN\n{card}\n!END"));
        assert!(!data.import_issues.is_empty(), "{card}");
        data.merge_into(&mut setup);
        assert!(!crate::cnt_writer::export_blockers(&setup).is_empty());
    }
    assert!(!read("!SOLUTION,TYPE=EIGEN\n!END").import_issues.is_empty());
    assert!(
        !read("!SOLUTION,TYPE=STATIC\n!EIGEN\n5")
            .import_issues
            .is_empty()
    );
    assert!(
        !read("!SOLUTION,TYPE=STATICEIGEN\n!EIGEN\n5")
            .import_issues
            .is_empty()
    );
}

#[test]
fn invalid_authored_eigen_settings_preserve_existing_export() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bevyistr-eigen-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("original.cnt");
    std::fs::write(&path, "original").unwrap();
    let mut setup = AnalysisSetup::default();
    setup.solver.analysis_type = AnalysisType::Eigen;
    setup.solver.eigen.max_iterations = setup.solver.eigen.mode_count;
    assert!(crate::cnt_writer::write_cnt_file(&path, &setup).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    assert!(crate::validate_frontistr_project(&FemModel::demo_hex8(), &setup).has_errors());
    // Hidden eigen settings do not affect static export, and are retained.
    setup.solver.analysis_type = AnalysisType::Static;
    crate::cnt_writer::write_cnt_file(&path, &setup).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("!EIGEN"));
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn signed_shift_and_small_tolerance_are_not_narrowed_to_f32() {
    for shift in [-1.2345678901234567e100, 0., 0.12345678901234567] {
        let mut setup = AnalysisSetup::default();
        setup.solver.analysis_type = AnalysisType::Eigen;
        setup.solver.eigen.shift = shift;
        setup.solver.eigen.tolerance = 1.2345678901234567e-100;
        assert_eq!(
            read(&crate::cnt_writer::build_cnt(&setup, &[]))
                .solver
                .unwrap(),
            setup.solver
        );
    }
}
