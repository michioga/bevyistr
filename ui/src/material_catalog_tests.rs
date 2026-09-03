use super::*;
use crate::material_library::{MaterialLibraryState, use_material};

pub(crate) fn sample_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data/materials.toml")
}

#[test]
fn external_catalogue_has_sourced_records_and_explicit_units() {
    let catalog = Catalog::read(&sample_path()).unwrap();
    assert_eq!(catalog.materials.len(), 4);
    for entry in &catalog.materials {
        assert!(!entry.source.is_empty());
        assert!(entry.source_url.starts_with("https://"));
    }
    let entry = &catalog.materials[0];
    let si = LibraryUnits::Metres.material(entry);
    let mm = LibraryUnits::Millimetres.material(entry);
    assert_eq!(si.young_modulus, Some(210e9));
    assert_eq!(mm.young_modulus, Some(210000.0));
    assert_eq!(mm.density, Some(7.85e-9));
    assert_eq!(si.poisson_ratio, mm.poisson_ratio);
    let mut setup = fem_core::AnalysisSetup::default();
    assert_eq!(use_material(&mut setup, si.clone()), "STEEL");
    assert_eq!(use_material(&mut setup, mm.clone()), "STEEL_2");
    assert_eq!(use_material(&mut setup, mm), "STEEL_2");
    assert_eq!(setup.materials[0], si);
    assert_eq!(setup.materials.len(), 2);
}

const CUSTOM: &str = "schema_version = 1\n[[materials]]\nname = 'MY_ALLOY'\nlabel = '試験材'\nyoung_pa = 70e9\npoisson = 0.3\n";

#[test]
fn custom_comments_unicode_bom_and_optional_density_are_supported() {
    let catalog = Catalog::parse(&format!("\u{feff}# user library\n{CUSTOM}")).unwrap();
    assert_eq!(catalog.materials[0].label, "試験材");
    assert_eq!(catalog.materials[0].density_kg_m3, None);
    let text = CUSTOM
        .replace("label = '試験材'\n", "")
        .replace("70e9", "70000000000");
    assert_eq!(
        Catalog::parse(&text).unwrap().materials[0].label,
        "MY_ALLOY"
    );
}

#[test]
fn invalid_or_misspelled_constants_fail_closed() {
    for bad in ["nan", "inf", "-1", "0", "1e300", "1e-300", "'70e9'"] {
        assert!(
            Catalog::parse(&CUSTOM.replace("70e9", bad)).is_err(),
            "{bad}"
        );
    }
    for bad in ["-1", "0.5", "nan", "0.49999999999999999"] {
        assert!(
            Catalog::parse(&CUSTOM.replace("0.3", bad)).is_err(),
            "{bad}"
        );
    }
    for text in [
        CUSTOM.replace("young_pa", "young_mpa"),
        CUSTOM.replace("schema_version = 1", "schema_version = 2"),
        format!("{CUSTOM}density_kg_m3 = -1\n"),
        format!("{CUSTOM}{}", CUSTOM.split_once('\n').unwrap().1),
        "schema_version = 1\nmaterials = []".into(),
    ] {
        assert!(Catalog::parse(&text).is_err());
    }
}

#[test]
fn file_edits_reload_without_compilation_and_invalid_reload_clears_draft() {
    let dir = std::env::temp_dir().join(format!(
        "bevyistr-library-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("カスタム.toml");
    std::fs::write(&path, CUSTOM).unwrap();
    let mut state = MaterialLibraryState::from_path(path.clone());
    state.selected = Some("MY_ALLOY".into());
    state.units = Some(LibraryUnits::Metres);
    assert_eq!(state.draft().unwrap().young_modulus, Some(70e9));
    std::fs::write(&path, CUSTOM.replace("70e9", "71e9")).unwrap();
    state.install(path.clone(), Catalog::read(&path));
    assert!(state.draft().is_none()); // must explicitly choose after reloading
    state.selected = Some("MY_ALLOY".into());
    assert_eq!(state.draft().unwrap().young_modulus, Some(71e9));
    std::fs::write(&path, "[broken").unwrap();
    state.install(path.clone(), Catalog::read(&path));
    assert!(state.catalog.is_none());
    assert!(state.draft().is_none());
    assert!(state.status.contains("Cannot load"));
    std::fs::remove_file(&path).unwrap();
    state.install(path.clone(), Catalog::read(&path));
    assert!(state.catalog.is_none());
    std::fs::remove_dir(&dir).unwrap();
}
