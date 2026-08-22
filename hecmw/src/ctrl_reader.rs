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

use std::{io, path::{Path, PathBuf}};

#[derive(Debug, Clone, Default)]
pub struct HecmwCtrlContent {
    pub mesh_path:   Option<String>,
    pub cnt_path:    Option<String>,
    pub result_path: Option<String>,
}

pub fn load_hecmw_ctrl(path: impl AsRef<Path>) -> io::Result<HecmwCtrlContent> {
    let text = std::fs::read_to_string(path.as_ref())?;
    Ok(parse_ctrl(&text))
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
        if p.exists() { return Some(p); }
        // Some ctrl files omit the extension — try appending it
        let with_ext = dir.join(format!("{}.msh", rel.trim()));
        with_ext.exists().then_some(with_ext)
    };

    let mesh = content.mesh_path.as_deref().and_then(resolve_file);
    let cnt  = content.cnt_path.as_deref().and_then(|p| {
        let full = dir.join(p.trim());
        full.exists().then_some(full)
    });

    (mesh, cnt)
}

fn parse_ctrl(text: &str) -> HecmwCtrlContent {
    let mut c = HecmwCtrlContent::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Skip blank lines and comments (# or !!)
        if line.is_empty() || line.starts_with('#') || line.starts_with("!!") {
            i += 1; continue;
        }

        if line.starts_with('!') {
            let upper = line.to_ascii_uppercase();

            // Identify the block role by NAME parameter or keyword
            let role = if upper.contains("NAME=FSTRMSH") || upper.starts_with("!MESH") {
                Some("mesh")
            } else if upper.contains("NAME=FSTRCNT") || upper.starts_with("!CONTROL") {
                Some("cnt")
            } else if upper.contains("NAME=FSTRRES") {
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
                        j += 1; continue;
                    }
                    if next.starts_with('!') { break; }
                    match role {
                        "mesh" => c.mesh_path   = Some(next.to_string()),
                        "cnt"  => c.cnt_path    = Some(next.to_string()),
                        "res"  => c.result_path = Some(next.to_string()),
                        _      => {}
                    }
                    break;
                }
            }
        }

        i += 1;
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str =
        "#\n!MESH, NAME=fstrMSH, TYPE=HECMW-ENTIRE\n hinge.msh\n\
         !CONTROL, NAME=fstrCNT\n hinge.cnt\n\
         !RESULT, NAME=fstrRES, IO=OUT\n hinge.res\n";

    #[test]
    fn parses_mesh_and_cnt() {
        let c = parse_ctrl(SAMPLE);
        assert_eq!(c.mesh_path.as_deref(), Some("hinge.msh"));
        assert_eq!(c.cnt_path.as_deref(),  Some("hinge.cnt"));
    }
}
