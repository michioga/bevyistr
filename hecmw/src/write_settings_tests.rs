use crate::{
    cnt::parse_cnt,
    cnt_writer::{build_cnt, export_blockers, write_cnt_file},
};
use fem_core::{AnalysisSetup, FemModel, OutputSettings};

fn read(text: &str) -> AnalysisSetup {
    let mut setup = AnalysisSetup::default();
    parse_cnt(text, &FemModel::demo_hex8().meshes[0], 0).merge_into(&mut setup);
    setup
}

#[test]
fn intervals_round_trip_independently_and_last_card_wins() {
    let input = "!WRITE, RESULT, VISUAL, FREQUENCY=5\n!WRITE, RESULT, FREQUENCY=2\n!END\n";
    let setup = read(input);
    assert!(setup.import_issues.is_empty(), "{:?}", setup.import_issues);
    assert_eq!(setup.output.write_res.frequency, 2);
    assert_eq!(setup.output.write_vis.frequency, 5);
    let written = build_cnt(&setup, &[]);
    assert!(written.contains("!WRITE, RESULT, FREQUENCY=2\n"));
    assert!(written.contains("!WRITE, VISUAL, FREQUENCY=5\n"));
    assert_eq!(read(&written).output, setup.output);
    assert!(read(&written).import_issues.is_empty());
}

#[test]
fn absent_write_is_not_enabled_by_quantity_cards_or_export() {
    for input in [
        "!END\n",
        "!OUTPUT_RES\nDISP,ON\n!END\n",
        "!WRITE, RESULT\n!END\n",
    ] {
        let setup = read(input);
        let written = build_cnt(&setup, &[]);
        assert!(!written.contains("!WRITE, VISUAL"));
        assert_eq!(read(&written).output, setup.output);
    }
    assert_eq!(read("!END\n").output, OutputSettings::inherited());
    let setup = AnalysisSetup::default();
    assert!(build_cnt(&setup, &[]).contains("!WRITE, RESULT\n!WRITE, VISUAL\n"));
}

#[test]
fn invalid_or_extended_write_remains_inspection_only() {
    for card in [
        "!WRITE",
        "!WRITE, FREQUENCY=2",
        "!WRITE, RESULT=YES",
        "!WRITE, RESULT, FILE=custom",
        "!WRITE, FEMAP",
        "!WRITE, RESULT, FREQUENCY=2, FREQUENCY=3",
        "!WRITE, RESULT\n2",
    ] {
        let setup = read(card);
        assert!(!export_blockers(&setup).is_empty(), "{card}");
    }
    for value in ["0", "-1", "1.5", "abc", "2147483648", ""] {
        let setup = read(&format!("!WRITE, RESULT, FREQUENCY={value}\n"));
        assert!(!export_blockers(&setup).is_empty(), "{value}");
    }
    assert!(
        read("!WRITE, VISUAL, FREQUENCY=2147483647\n")
            .import_issues
            .is_empty()
    );
}

#[test]
fn invalid_authored_interval_does_not_touch_existing_file() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("bevyistr-write-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("existing.cnt");
    std::fs::write(&path, "original").unwrap();
    let mut setup = AnalysisSetup::default();
    setup.output.write_res.frequency = 0;
    assert!(write_cnt_file(&path, &setup).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
