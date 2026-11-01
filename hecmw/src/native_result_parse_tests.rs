use super::*;

const V2: &str = "*fstrresult 2.0\n*comment\n解析結果\n*global\n2\n1 1\nTOTALTIME EIGENVALUE\n1.25D+0 7.8306921036862833d+6\n*data\n2 1\n2 1\n3 1\nDISPLACEMENT TEMP\n20\n1E0 2D0 3d0 300\n10\n-4e0 -5D0 -6d0 400\n1\nDAMAGE\n99\n0.5D-3\n";

fn sample(source: &str) -> StepResult {
    NativeResult::parse(source)
        .unwrap()
        .step(&[NodeId(10), NodeId(20)], &[ElementId(99)], 7)
        .unwrap()
}

#[test]
fn borrowed_sections_preserve_crlf_unicode_comments_and_fortran_numbers() {
    let expected = sample(V2);
    assert_eq!(expected.step, 7);
    assert_eq!(expected.time, 1.25);
    assert_eq!(expected.eigenvalue, Some(7.8306921036862833e6));
    let ResultField::NodeVector { values, .. } = expected.field_by_name("Displacement").unwrap()
    else {
        panic!("missing vector")
    };
    assert_eq!(
        values,
        &[Vec3::new(-4.0, -5.0, -6.0), Vec3::new(1.0, 2.0, 3.0)]
    );
    let ResultField::ElementScalar { values, .. } =
        expected.field_by_name("DAMAGE (element)").unwrap()
    else {
        panic!("missing element field")
    };
    assert_eq!(values, &[0.0005]);
    for source in [
        V2.replace('\n', "\r\n"),
        V2.trim_end().to_owned(),
        V2.replace("*global\n", "  *global \n")
            .replace("*data\n", "\t*data \n"),
        V2.replace("D0", "E0")
            .replace("d0", "e0")
            .replace("D+", "E+")
            .replace("d+", "e+")
            .replace("D-", "E-"),
    ] {
        let actual = sample(&source);
        assert_eq!(actual.fields, expected.fields);
        assert_eq!(actual.time, expected.time);
        assert_eq!(actual.eigenvalue, expected.eigenvalue);
    }
    // V1 has no section markers/global metadata; its record layout remains identical.
    let v1 = format!("*fstrresult\n{}", V2.split_once("*data\n").unwrap().1);
    assert_eq!(sample(&v1).fields, expected.fields);
    assert_eq!(sample(&v1.replace('\n', "\r\n")).fields, expected.fields);
}

#[test]
fn malformed_sections_truncation_and_nonfinite_values_remain_errors() {
    for source in [
        V2.replace("*global", "*missing"),
        V2.replace("*data", "*missing"),
        V2.replace("*global", "*swap")
            .replace("*data", "*global")
            .replace("*swap", "*data"),
        V2.replace("1.25D+0", "1.25D+0 EXTRA"),
        V2.replace("1E0 2D0 3d0 300", "NaN 2D0 3d0 300"),
        V2.replace("1E0 2D0 3d0 300", "1e99 2D0 3d0 300"),
        V2.replace("1E0 2D0 3d0 300", "1D99 2D0 3d0 300"),
        V2.replace("20\n1E0", "10\n1E0"), // duplicate record ID
        V2.trim_end_matches("0.5D-3\n").to_owned(),
        format!("{V2}EXTRA"),
        "*fstrresult 2.0".into(),
        "*fstrresult 2.0\n*global\n0\n*data".into(),
        V2.replace("2.0", "3.0"),
    ] {
        assert!(
            NativeResult::parse(&source).is_err(),
            "unexpected success: {source}"
        );
    }
}
