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
            let result = keyword.eq_ignore_ascii_case("!RESULT");
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
            } else if result && name.eq_ignore_ascii_case("fstrRES") {
                if seen_result {
                    problem = Some("Repeated fstrRES results cannot be preserved");
                } else {
                    seen_result = true;
                    c.settings.binary_result = match parameter("TYPE").map(str::to_ascii_uppercase).as_deref() {
                        Some("BINARY") => Some(true),
                        Some("TEXT") => Some(false),
                        None => None,
                        _ => {
                            problem = Some("Unsupported result file TYPE");
                            None
                        }
                    };
                    if parameter("IO").is_some_and(|io| !io.eq_ignore_ascii_case("OUT")) {
                        problem = Some("Only fstrRES output files can be regenerated");
                    }
                }
            } else if keyword.eq_ignore_ascii_case("!RESTART")
                || (result && name.eq_ignore_ascii_case("fstrTEMP"))
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
            } else if name.eq_ignore_ascii_case("fstrRES") {
                Some("res")
            } else {
                None
            };

            if let Some(role) = role {
                // Next non-blank, non-comment, non-! line is the path
                let mut j = i + 1;
                while j < lines.len() {
                    let next = lines[j].trim();
                    if next.is_empty() || next.starts_with('#') || next.starts_with("!!") {
                        j += 1;
                        continue;
                    }
                    if next.starts_with('!') {
                        break;
                    }
                    match role {
                        "mesh" if candidate_priority >= mesh_priority => {
                            c.mesh_path = Some(next.to_string());
                            mesh_priority = candidate_priority;
                        }
                        "distributed" => c.distributed_mesh_path = Some(next.to_string()),
                        "cnt" => c.cnt_path = Some(next.to_string()),
                        "res" => c.result_path = Some(next.to_string()),
                        _ => {}
                    }
                    break;
                }
            }
        }

        i += 1;
    }

    c
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
