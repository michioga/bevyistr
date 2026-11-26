use fem_core::{ElementType, FemMesh, Section, SectionKind, SetupImportIssue};
use std::{collections::HashSet, path::Path};

pub(crate) fn audit(
    source: &str,
    path: &Path,
    mesh: &FemMesh,
    sections: &[Section],
) -> (Vec<SetupImportIssue>, HashSet<String>) {
    let mut issues = Vec::new();
    let mut contacts = HashSet::new();
    let mut material_names = HashSet::new();
    let mut fluid_placeholders = HashSet::new();
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
            "!CONTACT PAIR" => {
                check_contact_pair(header, &data_rows(&mut lines), mesh, &mut contacts)
            }
            "!CONTACTPAIR" => Some("Expected !CONTACT PAIR with a space"),
            "!SYSTEM" | "!AMPLITUDE" | "!EMBEDPAIR" | "!EMBED PAIR" | "!ZERO" | "!INCLUDE" => {
                Some("This mesh setting is not preserved")
            }
            "!MATERIAL" => {
                let name = super::keyword_parameter(header, "NAME").unwrap_or_default();
                if !name.is_empty() && !material_names.insert(name.to_ascii_uppercase()) {
                    fluid_placeholders.retain(|item: &String| !item.eq_ignore_ascii_case(&name));
                    Some("A mesh material name is defined more than once")
                } else {
                    match super::keyword_parameter(header, "ITEM").as_deref() {
                        None | Some("1" | "2") => None,
                        Some("3") if is_fluid_solid_material(&name, mesh, sections) => {
                            if known_parameters(header, &["NAME", "ITEM"])
                                && fluid_placeholder_items(&mut lines)
                            {
                                fluid_placeholders.insert(name);
                                None
                            } else {
                                Some(
                                    "Fluid MSH material requires exactly three zero-valued placeholder items",
                                )
                            }
                        }
                        Some("3") => Some(
                            "Only elastic and density material items are preserved for structural materials",
                        ),
                        _ => Some("Unsupported MSH material item count"),
                    }
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
    (issues, fluid_placeholders)
}

fn is_fluid_solid_material(name: &str, mesh: &FemMesh, sections: &[Section]) -> bool {
    let mut assignments = sections
        .iter()
        .filter(|section| section.material_name == name)
        .peekable();
    if name.is_empty() || assignments.peek().is_none() || mesh.elements.is_empty() {
        return false;
    }
    let fluid_ids: HashSet<_> = mesh
        .elements
        .iter()
        .filter(|element| element.element_type == ElementType::FluidTet4)
        .map(|element| element.id)
        .collect();
    assignments.all(|section| {
        if section.kind != SectionKind::Solid {
            return false;
        }
        let Some(group) = section.element_set_name.as_deref() else {
            return false;
        };
        if group.eq_ignore_ascii_case("ALL") {
            return mesh
                .elements
                .iter()
                .all(|element| element.element_type == ElementType::FluidTet4);
        }
        mesh.element_sets.iter().any(|set| {
            set.name == group
                && !set.elements.is_empty()
                && set.elements.iter().all(|id| fluid_ids.contains(id))
        })
    })
}

fn known_parameters(header: &str, allowed: &[&str]) -> bool {
    let mut fields = HashSet::new();
    header
        .split(',')
        .skip(1)
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .all(|field| {
            field.split_once('=').is_some_and(|(key, _)| {
                let key = key.trim().to_ascii_uppercase();
                allowed.contains(&key.as_str()) && fields.insert(key)
            })
        })
}

fn fluid_placeholder_items<'a>(
    lines: &mut std::iter::Peekable<impl Iterator<Item = (usize, &'a str)>>,
) -> bool {
    if !data_rows(lines).is_empty() {
        return false;
    }
    let mut seen = [false; 3];
    while let Some(&(_, header)) = lines.peek() {
        let (keyword, _) = header.split_once('=').unwrap_or((header, ""));
        if !keyword.trim().eq_ignore_ascii_case("!ITEM") {
            break;
        }
        lines.next();
        let rows = data_rows(lines);
        let index = match super::keyword_assignment_value(header) {
            Some("1") => 0,
            Some("2") => 1,
            Some("3") => 2,
            _ => return false,
        };
        let expected = if index == 0 { 2 } else { 1 };
        if seen[index]
            || !known_parameters(header, &["SUBITEM"])
            || super::keyword_parameter(header, "SUBITEM").map_or(Ok(1), |width| width.parse())
                != Ok(expected)
        {
            return false;
        }
        let [row] = rows.as_slice() else {
            return false;
        };
        let row = row.split('#').next().unwrap_or("").trim();
        let fields: Vec<_> = row
            .strip_suffix(',')
            .unwrap_or(row)
            .split(',')
            .map(str::trim)
            .collect();
        if fields.len() != expected
            || !fields
                .iter()
                .all(|value| value.replace(['d', 'D'], "e").parse::<f64>() == Ok(0.0))
        {
            return false;
        }
        seen[index] = true;
    }
    seen.into_iter().all(|item| item)
        && !lines.peek().is_some_and(|(_, header)| {
            let keyword = header
                .split(',')
                .next()
                .unwrap_or(header)
                .trim()
                .to_ascii_uppercase();
            matches!(
                keyword.as_str(),
                "!ELASTIC"
                    | "!DENSITY"
                    | "!PLASTIC"
                    | "!HYPERELASTIC"
                    | "!VISCOSITY"
                    | "!ISOTROPIC"
            )
        })
}

fn check_contact_pair(
    header: &str,
    rows: &[&str],
    mesh: &FemMesh,
    names: &mut HashSet<String>,
) -> Option<&'static str> {
    if !known_parameters(header, &["NAME", "TYPE"]) {
        return Some("Unsupported or repeated contact pair parameters");
    }
    let name = super::keyword_parameter(header, "NAME").unwrap_or_default();
    let kind = super::keyword_parameter(header, "TYPE").map(|value| value.to_ascii_uppercase());
    if name.is_empty() || !names.insert(name.to_ascii_uppercase()) {
        return Some("A contact pair must have a unique non-empty name");
    }
    if !matches!(kind.as_deref(), None | Some("NODE-SURF" | "SURF-SURF")) {
        return Some("Unsupported contact pair TYPE");
    }
    let [row] = rows else {
        return Some("Only one slave/master row per contact pair can be preserved");
    };
    let row = row.split('#').next().unwrap_or("");
    let fields: Vec<_> = row
        .trim()
        .trim_end_matches(',')
        .split(',')
        .map(str::trim)
        .collect();
    let [slave, master] = fields.as_slice() else {
        return Some("Expected slave and master group names on the contact pair data row");
    };
    let surface_exists = |name: &str| {
        mesh.surface_sets
            .iter()
            .any(|group| group.name.eq_ignore_ascii_case(name) && !group.surfaces.is_empty())
    };
    let slave_exists = if kind.as_deref() == Some("SURF-SURF") {
        surface_exists(slave)
    } else {
        mesh.node_sets
            .iter()
            .any(|group| group.name.eq_ignore_ascii_case(slave) && !group.nodes.is_empty())
    };
    if !slave_exists || !surface_exists(master) {
        return Some("The contact pair references a missing or empty slave/master group");
    }
    None
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
    fn blocks_multiple_rows_and_unresolved_contact_group() {
        let mesh = FemMesh::demo_hex8();
        for (rows, reason) in [
            ("slave,master\nslave2,master2", "one slave/master row"),
            ("slave,master", "missing or empty"),
        ] {
            let input = format!("!CONTACT PAIR,NAME=CP\n{rows}\n!END\n");
            let issues = audit(&input, Path::new("project.msh"), &mesh, &[]).0;
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0].line, 1);
            assert!(issues[0].message.contains(reason));
        }
    }

    #[test]
    fn material_validation_and_import_skip_the_same_comments() {
        let source = "!MATERIAL, NAME=STEEL, ITEM=2\n!ITEM=1,SUBITEM=2\n!! Elastic constants\n210000,0.3\n!ITEM=2\n# Density\n7.85e-9\n";
        assert!(
            audit(source, Path::new("project.msh"), &FemMesh::demo_hex8(), &[])
                .0
                .is_empty()
        );
        let (materials, _) = crate::parse_msh_setup(source, 0);
        assert_eq!(materials[0].young_modulus, Some(210000.0));
        assert_eq!(materials[0].poisson_ratio, Some(0.3));
        assert_eq!(materials[0].density, Some(7.85e-9));

        let wrong_item_width = source.replace("!ITEM=1,SUBITEM=2", "!ITEM=1");
        assert_eq!(
            audit(
                &wrong_item_width,
                Path::new("project.msh"),
                &FemMesh::demo_hex8(),
                &[],
            )
            .0
            .len(),
            1
        );
    }

    #[test]
    fn fluid_placeholder_requires_three_zero_items_and_only_fluid_solid_assignments() {
        let source = concat!(
            "!NODE\n1,0,0,0\n2,1,0,0\n3,0,1,0\n4,0,0,1\n5,0,0,-1\n",
            "!ELEMENT, TYPE=3414, EGRP=LIQUID\n1,1,2,3,4\n",
            "!ELEMENT, TYPE=341, EGRP=METAL\n2,1,3,2,5\n",
            "!SECTION, TYPE=SOLID, EGRP=LIQUID, MATERIAL=F1\n",
            "!MATERIAL, NAME=F1, ITEM=3\n",
            "!ITEM=2\n0d0\n",
            "!ITEM=1, SUBITEM=2\n!! placeholder\n0.0,0.0\n",
            "!ITEM=3\n0.0\n!END\n",
        );
        let check = |input: &str| {
            let mesh = crate::parse_mesh_str(input).unwrap();
            let (_, sections) = crate::parse_msh_setup(input, 0);
            audit(input, Path::new("project.msh"), &mesh, &sections)
        };
        let (issues, placeholders) = check(source);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(placeholders, HashSet::from(["F1".into()]));
        for (from, to) in [
            ("0d0", "1e-15"),
            ("0.0,0.0", "0.0,0.0,20.0"),
            ("!ITEM=3\n0.0", "!ITEM=3\n0.0\n0.0"),
            ("!ITEM=3\n0.0\n", ""),
            ("!ITEM=3", "!ITEM=2"),
            ("!ITEM=3", "!ITEM=3, INPUT=material.dat"),
            (
                "!END",
                "!MATERIAL, NAME=f1, ITEM=1\n!ITEM=1,SUBITEM=2\n100,0.3\n!END",
            ),
            ("NAME=F1, ITEM=3", "NAME=F1, ITEM=3, INPUT=material.dat"),
            ("EGRP=LIQUID, MATERIAL", "EGRP=METAL, MATERIAL"),
            (
                "!MATERIAL, NAME=F1",
                "!SECTION, TYPE=SOLID, EGRP=METAL, MATERIAL=F1\n!MATERIAL, NAME=F1",
            ),
            (
                "!ITEM=3\n0.0\n!END",
                "!ITEM=3\n0.0\n!ELASTIC\n2000,0.3\n!END",
            ),
        ] {
            let (issues, placeholders) = check(&source.replace(from, to));
            assert!(placeholders.is_empty(), "{to}");
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.keyword.starts_with("!MATERIAL")),
                "{to}: {issues:?}"
            );
        }
    }

    #[test]
    fn blocks_temperature_tables_without_blocking_constant_materials() {
        let source = "!MATERIAL, NAME=STEEL, ITEM=2\n!ITEM=1,SUBITEM=2\n210000,0.3,0\n190000,0.29,100\n!ITEM=2\n7.85e-9\n!SECTION,TYPE=SOLID,EGRP=ALL,MATERIAL=STEEL\n1.0\n";
        let issues = audit(source, Path::new("project.msh"), &FemMesh::demo_hex8(), &[]).0;
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].line, 2);
        assert!(issues[0].message.contains("temperature"));
    }
}
