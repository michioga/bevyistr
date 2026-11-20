use fem_core::SetupImportIssue;
use std::path::Path;

pub(crate) fn audit(source: &str, path: &Path) -> Vec<SetupImportIssue> {
    let mut issues = Vec::new();
    let mut lines = source.lines().map(str::trim).enumerate().peekable();
    while let Some((index, header)) = lines.next() {
        if !header.starts_with('!') || header.starts_with("!!") {
            continue;
        }
        let keyword = header
            .split(',')
            .next()
            .unwrap_or(header)
            .trim()
            .to_ascii_uppercase();
        let problem = match keyword.as_str() {
            "!NODE" | "!ELEMENT" | "!NGROUP" | "!EGROUP" | "!SGROUP" => {
                if super::keyword_parameter(header, "INPUT").is_some() {
                    Some("External mesh data referenced by INPUT is not preserved")
                } else if super::keyword_parameter(header, "MATITEM").is_some() {
                    Some("Element material item values are not preserved")
                } else {
                    None
                }
            }
            "!SYSTEM" | "!AMPLITUDE" | "!EMBEDPAIR" | "!EMBED PAIR" | "!ZERO" | "!INCLUDE" => {
                Some("This mesh setting is not preserved")
            }
            "!MATERIAL" => {
                let count = super::keyword_parameter(header, "ITEM");
                if count.is_some_and(|count| !matches!(count.as_str(), "1" | "2")) {
                    Some("Only elastic and density material items are preserved")
                } else {
                    None
                }
            }
            "!SECTION" => {
                let section_type = super::keyword_parameter(header, "TYPE").unwrap_or_default();
                if super::keyword_parameter(header, "INPUT").is_some()
                    || super::keyword_parameter(header, "SECOPT").is_some_and(|value| value != "0")
                {
                    Some("Section INPUT and non-default SECOPT are not preserved")
                } else if section_type.eq_ignore_ascii_case("SOLID") {
                    let rows = data_rows(&mut lines);
                    if !matches!(rows.as_slice(), [] | [_])
                        || rows.first().is_some_and(|row| {
                            row.trim_end_matches(',').trim().parse::<f64>() != Ok(1.0)
                        })
                    {
                        Some("Non-default solid thickness or cross-sectional area is not preserved")
                    } else {
                        None
                    }
                } else if section_type.eq_ignore_ascii_case("BEAM")
                    || section_type.eq_ignore_ascii_case("SHELL")
                {
                    if data_rows(&mut lines).len() != 1 {
                        Some("This section requires one supported data row")
                    } else {
                        None
                    }
                } else {
                    Some("This section type is not preserved")
                }
            }
            _ if keyword.starts_with("!ITEM")
                || matches!(keyword.as_str(), "!ELASTIC" | "!DENSITY") =>
            {
                let expected = match super::keyword_assignment_value(header) {
                    Some("1") => Some(2),
                    Some("2") => Some(1),
                    None if keyword == "!ELASTIC" => Some(2),
                    None if keyword == "!DENSITY" => Some(1),
                    _ => None,
                };
                let rows = data_rows(&mut lines);
                if expected.is_none_or(|expected| !constant_material_row(header, &rows, expected)) {
                    Some(
                        "Material tables, temperature dependence, or extra material properties are not preserved",
                    )
                } else {
                    None
                }
            }
            "!END" => break,
            _ => None,
        };
        if let Some(message) = problem {
            issues.push(SetupImportIssue {
                source: path.display().to_string(),
                line: index + 1,
                keyword: header.into(),
                message: message.into(),
            });
        }
    }
    issues
}

fn data_rows<'a>(
    lines: &mut std::iter::Peekable<impl Iterator<Item = (usize, &'a str)>>,
) -> Vec<&'a str> {
    let mut rows = Vec::new();
    while let Some(&(_, line)) = lines.peek() {
        if line.starts_with('!') && !line.starts_with("!!") {
            break;
        }
        lines.next();
        if !line.is_empty() && !line.starts_with("!!") && !line.starts_with('#') {
            rows.push(line);
        }
    }
    rows
}

fn constant_material_row(header: &str, rows: &[&str], expected: usize) -> bool {
    let declared = super::keyword_parameter(header, "SUBITEM");
    let native_item = super::keyword_assignment_value(header).is_some();
    let default = if native_item { 1 } else { expected };
    if super::keyword_parameter(header, "INPUT").is_some()
        || declared.map_or(Ok(default), |value| value.parse()) != Ok(expected)
    {
        return false;
    }
    let [row] = rows else {
        return false;
    };
    let fields: Vec<_> = row.split(',').map(str::trim).collect();
    (fields.len() == expected || (fields.len() == expected + 1 && fields[expected] == ""))
        && fields[..expected]
            .iter()
            .all(|field| field.parse::<f32>().is_ok_and(f32::is_finite))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_validation_and_import_skip_the_same_comments() {
        let source = "!MATERIAL, NAME=STEEL, ITEM=2\n!ITEM=1,SUBITEM=2\n!! Elastic constants\n210000,0.3\n!ITEM=2\n# Density\n7.85e-9\n";
        assert!(audit(source, Path::new("project.msh")).is_empty());
        let (materials, _) = crate::parse_msh_setup(source, 0);
        assert_eq!(materials[0].young_modulus, Some(210000.0));
        assert_eq!(materials[0].poisson_ratio, Some(0.3));
        assert_eq!(materials[0].density, Some(7.85e-9));

        let wrong_item_width = source.replace("!ITEM=1,SUBITEM=2", "!ITEM=1");
        assert_eq!(audit(&wrong_item_width, Path::new("project.msh")).len(), 1);
    }

    #[test]
    fn blocks_temperature_tables_without_blocking_constant_materials() {
        let source = "!MATERIAL, NAME=STEEL, ITEM=2\n!ITEM=1,SUBITEM=2\n210000,0.3,0\n190000,0.29,100\n!ITEM=2\n7.85e-9\n!SECTION,TYPE=SOLID,EGRP=ALL,MATERIAL=STEEL\n1.0\n";
        let issues = audit(source, Path::new("project.msh"));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].line, 2);
        assert!(issues[0].message.contains("temperature"));
    }
}
