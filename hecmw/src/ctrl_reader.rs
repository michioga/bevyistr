//! Parser for the FrontISTR global control file (`hecmw_ctrl.dat`).
//!
//! Actual FrontISTR format uses !KEYWORD headers with file paths on the next line:
//!
//! ```text
//! !MESH, NAME=fstrMSH, TYPE=HECMW-ENTIRE
//!  hinge.msh
//! !CONTROL, NAME=fstrCNT
//!  hinge.cnt
//! !RESULT, NAME=fstrRES, IO=OUT
//!  hinge.res
//! ```

use fem_core::{FrontistrControlSettings, FrontistrSubdirectories, SetupImportIssue};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default)]
pub struct HecmwCtrlContent {
    /// Editable entire mesh. For a distributed solve, this is `part_in`,
    /// not the rank-file prefix used by `fstrMSH`.
    pub mesh_path: Option<String>,
    /// Rank mesh prefix for a solve using HECMW-DIST.
    pub distributed_mesh_path: Option<String>,
    pub cnt_path: Option<String>,
    pub result_path: Option<String>,
    pub settings: FrontistrControlSettings,
    pub has_partition_input: bool,
    pub import_issues: Vec<SetupImportIssue>,
}

pub fn load_hecmw_ctrl(path: impl AsRef<Path>) -> io::Result<HecmwCtrlContent> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)?;
    let mut control = parse_ctrl(&text);
    for issue in &mut control.import_issues {
        issue.source = path.display().to_string();
    }
    Ok(control)
}

/// Resolves the relative paths in `content` against the directory containing
/// `ctrl_path`, returning the mesh and cnt paths that actually exist on disk.
pub fn resolve_paths(
    ctrl_path: &Path,
    content: &HecmwCtrlContent,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let dir = ctrl_path.parent().unwrap_or(Path::new("."));

    let resolve_file = |rel: &str| -> Option<PathBuf> {
        let p = dir.join(rel.trim());
        if p.exists() {
            return Some(p);
        }
        // Some ctrl files omit the extension — try appending it
        let with_ext = dir.join(format!("{}.msh", rel.trim()));
        with_ext.exists().then_some(with_ext)
    };

    let mesh = content.mesh_path.as_deref().and_then(resolve_file);
    let cnt = content.cnt_path.as_deref().and_then(|p| {
        let full = dir.join(p.trim());
        full.exists().then_some(full)
    });

    (mesh, cnt)
}

fn parse_ctrl(text: &str) -> HecmwCtrlContent {
    let mut c = HecmwCtrlContent::default();
    let mut mesh_priority = 0;
    let mut seen_result = false;
    let mut seen_visual = false;
    let mut seen_subdir = false;
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Skip blank lines and comments (# or !!)
        if line.is_empty() || line.starts_with('#') || line.starts_with("!!") {
            i += 1;
            continue;
        }

        if line.starts_with('!') {
            let keyword = line.split(',').next().unwrap_or(line).trim();
            let parameter = |key: &str| {
                line.split(',').skip(1).find_map(|field| {
                    let (name, value) = field.split_once('=')?;
                    name.trim()
                        .eq_ignore_ascii_case(key)
                        .then_some(value.trim())
                })
            };
            let name = parameter("NAME").unwrap_or("");
            if keyword.eq_ignore_ascii_case("!MESH") && name.eq_ignore_ascii_case("part_in") {
                c.has_partition_input = true;
            }
            let result = keyword.eq_ignore_ascii_case("!RESULT");
            let filename = lines[i + 1..]
                .iter()
                .map(|line| line.trim())
                .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("!!"))
                .filter(|line| !line.starts_with('!'));
            let mut problem = None;
            if keyword.eq_ignore_ascii_case("!SUBDIR") {
                if seen_subdir {
                    problem = Some("Repeated SUBDIR cards cannot be preserved");
                } else {
                    seen_subdir = true;
                    c.settings.subdirectories = parse_subdir(line);
                    if c.settings.subdirectories.is_none() {
                        problem = Some("Invalid SUBDIR options");
                    }
                }
            } else if result {
                if name.eq_ignore_ascii_case("fstrRES") {
                    c.result_path = filename.map(str::to_owned);
                    problem = read_result_output(
                        line, filename, &mut seen_result,
                        &mut c.settings.result_header, &mut c.settings.binary_result,
                    ).err();
                } else if name.eq_ignore_ascii_case("vis_out") {
                    problem = read_result_output(
                        line, filename, &mut seen_visual,
                        &mut c.settings.visual_header, &mut c.settings.binary_visual,
                    ).err();
                } else {
                    problem = Some("This global control setting cannot be regenerated");
                }
            } else if keyword.eq_ignore_ascii_case("!RESTART")
                || (keyword.eq_ignore_ascii_case("!MESH") && parameter("REFINE").is_some())
            {
                problem = Some("This global control setting cannot be regenerated");
            }
            if let Some(message) = problem {
                c.import_issues.push(SetupImportIssue {
                    source: "hecmw_ctrl.dat".into(),
                    line: i + 1,
                    keyword: line.into(),
                    message: message.into(),
                });
            }
            let candidate_priority = if keyword.eq_ignore_ascii_case("!MESH")
                && parameter("TYPE").is_none_or(|kind| !kind.eq_ignore_ascii_case("HECMW-DIST"))
            {
                if name.eq_ignore_ascii_case("fstrMSH") {
                    3
                } else if name.eq_ignore_ascii_case("part_in") {
                    2
                } else {
                    1
                }
            } else {
                0
            };

            // Identify the block role by NAME parameter or keyword
            let role = if candidate_priority > 0 {
                Some("mesh")
            } else if keyword.eq_ignore_ascii_case("!MESH")
                && name.eq_ignore_ascii_case("fstrMSH")
                && parameter("TYPE").is_some_and(|kind| kind.eq_ignore_ascii_case("HECMW-DIST"))
            {
                Some("distributed")
            } else if name.eq_ignore_ascii_case("fstrCNT")
                || keyword.eq_ignore_ascii_case("!CONTROL")
            {
                Some("cnt")
            } else {
                None
            };

            if let (Some(role), Some(path)) = (role, filename) {
                match role {
                    "mesh" if candidate_priority >= mesh_priority => {
                        c.mesh_path = Some(path.to_owned());
                        mesh_priority = candidate_priority;
                    }
                    "distributed" => c.distributed_mesh_path = Some(path.to_owned()),
                    "cnt" => c.cnt_path = Some(path.to_owned()),
                    _ => {}
                }
            }
        }

        i += 1;
    }

    c
}

fn read_result_output(
    line: &str,
    filename: Option<&str>,
    seen: &mut bool,
    header: &mut Option<String>,
    binary: &mut Option<bool>,
) -> Result<(), &'static str> {
    if std::mem::replace(seen, true) {
        return Err("Repeated result output cards cannot be preserved");
    }
    let mut name = false;
    let mut output = false;
    let mut format = None;
    for field in line.split(',').skip(1) {
        let (key, value) = field.split_once('=').ok_or("Invalid result output option")?;
        let value = value.trim().to_ascii_uppercase();
        match (key.trim().to_ascii_uppercase().as_str(), value.as_str()) {
            ("NAME", _) if !name => name = true,
            ("IO", "OUT") if !output => output = true,
            ("TYPE", "BINARY") if format.is_none() => format = Some(true),
            ("TYPE", "TEXT") if format.is_none() => format = Some(false),
            _ => return Err("Unsupported or repeated result output option"),
        }
    }
    if !output {
        return Err("Only result cards with IO=OUT can be regenerated");
    }
    let filename = filename.ok_or("Missing result output file header")?;
    if !super::ctrl_writer::is_relative_output_header(filename) {
        return Err("Result output must name a relative file within the output folder");
    }
    *header = Some(filename.to_owned());
    *binary = format;
    Ok(())
}

fn parse_subdir(line: &str) -> Option<FrontistrSubdirectories> {
    let mut enabled = None;
    let mut limit = None;
    for field in line.split(',').skip(1).map(str::trim) {
        match field.to_ascii_uppercase().as_str() {
            "ON" if enabled.is_none() => enabled = Some(true),
            "OFF" if enabled.is_none() => enabled = Some(false),
            _ => {
                let (key, value) = field.split_once('=')?;
                if !key.trim().eq_ignore_ascii_case("LIMIT") || limit.is_some() {
                    return None;
                }
                limit = Some(value.trim().parse().ok()?);
            }
        }
    }
    Some(FrontistrSubdirectories { enabled: enabled?, limit })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = "#\n!MESH, NAME=fstrMSH, TYPE=HECMW-ENTIRE\n hinge.msh\n\
         !CONTROL, NAME=fstrCNT\n hinge.cnt\n\
         !RESULT, NAME=fstrRES, IO=OUT\n hinge.res\n";

    #[test]
    fn reports_frequency_result_roles_before_export() {
        let data = parse_ctrl(&format!(
            "{SAMPLE}!RESULT, NAME=fstrDYNA, IO=OUT\n history.res\n!RESULT, NAME=result-in, IO=IN\n modes.res\n"
        ));
        assert_eq!(data.import_issues.len(), 2);
        assert_eq!(data.result_path.as_deref(), Some("hinge.res"));
    }

    #[test]
    fn reports_unsafe_or_unwritable_result_cards_with_line_numbers() {
        for card in [
            "!RESULT,NAME=vis_out,IO=IN\nplots/view",
            "!RESULT,NAME=fstrRES,IO=OUT\n../escape.res",
            "!RESULT,NAME=fstrRES,IO=OUT\nC:/external/saved.res",
            "!RESULT,NAME=vis_out,IO=OUT\n/absolute/view",
            "!RESULT,NAME=fstrRES,IO=OUT\nresults/dir/../../escape.res",
            "!RESULT,NAME=vis_out,IO=OUT\n",
            "!RESULT,NAME=vis_out,IO=OUT,TYPE=UNKNOWN\nplot",
            "!RESULT,NAME=vis_out,IO=OUT\nplot\n!RESULT,NAME=vis_out,IO=OUT\nother",
        ] {
            let data = parse_ctrl(card);
            assert_eq!(data.import_issues.len(), 1, "{card}");
            let issue = &data.import_issues[0];
            assert_eq!(issue.line, if card.ends_with("other") { 3 } else { 1 });
        }
    }

    #[test]
    fn parses_mesh_and_cnt() {
        let c = parse_ctrl(SAMPLE);
        assert_eq!(c.mesh_path.as_deref(), Some("hinge.msh"));
        assert_eq!(c.cnt_path.as_deref(), Some("hinge.cnt"));
    }

    #[test]
    fn distributed_project_uses_partition_input_in_either_order() {
        let entire = "!MESH, NAME = part_in, TYPE = HECMW-ENTIRE\n hinge.msh\n";
        let distributed = "!MESH, NAME=part_out, TYPE=HECMW-DIST\n hinge_2\n\
            !MESH, NAME=fstrMSH, TYPE=HECMW-DIST\n hinge_2\n";
        for text in [
            format!("{entire}{distributed}"),
            format!("{distributed}{entire}"),
        ] {
            assert_eq!(parse_ctrl(&text).mesh_path.as_deref(), Some("hinge.msh"));
        }
        assert!(parse_ctrl(distributed).mesh_path.is_none());
    }

    #[test]
    fn direct_solver_mesh_takes_priority_over_partition_input() {
        let text = format!("{SAMPLE}!MESH, NAME=part_in, TYPE=HECMW-ENTIRE\n other.msh\n");
        assert_eq!(parse_ctrl(&text).mesh_path.as_deref(), Some("hinge.msh"));
    }

    #[test]
    fn non_distributed_import_types_remain_readable() {
        let text = "!MESH, NAME=fstrMSH, TYPE=ABAQUS\n hinge.inp\n";
        assert_eq!(parse_ctrl(text).mesh_path.as_deref(), Some("hinge.inp"));
    }
}
