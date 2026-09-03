//! External, user-editable TOML catalogue. Never executes content or changes
//! the model while loading; unit conversion occurs only in an assignment draft.
use fem_core::FemMaterial;
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 1_048_576;

#[derive(Debug)]
pub(crate) struct Catalog {
    schema_version: u32,
    pub materials: Vec<LibraryMaterial>,
}

#[derive(Debug)]
pub(crate) struct LibraryMaterial {
    pub name: String,
    pub label: String,
    pub young_pa: f64,
    pub poisson: f64,
    pub density_kg_m3: Option<f64>,
    pub source: String,
    pub source_url: String,
    pub note: String,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Self, String> {
        let doc = text
            .trim_start_matches('\u{feff}')
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| error.to_string())?;
        check_keys(doc.as_table(), &["schema_version", "materials"])?;
        if doc.get("schema_version").and_then(|v| v.as_integer()) != Some(1) {
            return Err("Expected schema_version = 1".into());
        }
        let rows = doc
            .get("materials")
            .and_then(|v| v.as_array_of_tables())
            .ok_or("Expected [[materials]] tables")?;
        let mut materials = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let parse = || -> Result<LibraryMaterial, String> {
                check_keys(
                    row,
                    &[
                        "name",
                        "label",
                        "young_pa",
                        "poisson",
                        "density_kg_m3",
                        "source",
                        "source_url",
                        "note",
                    ],
                )?;
                Ok(LibraryMaterial {
                    name: string_field(row, "name", true)?,
                    label: string_field(row, "label", false)?,
                    young_pa: number_field(row, "young_pa", true)?.unwrap(),
                    poisson: number_field(row, "poisson", true)?.unwrap(),
                    density_kg_m3: number_field(row, "density_kg_m3", false)?,
                    source: string_field(row, "source", false)?,
                    source_url: string_field(row, "source_url", false)?,
                    note: string_field(row, "note", false)?,
                })
            };
            materials.push(parse().map_err(|e| format!("materials[{}]: {e}", i + 1))?);
        }
        let mut data = Self {
            schema_version: 1,
            materials,
        };
        if data.schema_version != 1 || data.materials.is_empty() {
            return Err("Expected schema_version = 1 and at least one [[materials]]".into());
        }
        let mut names = BTreeSet::new();
        for entry in &mut data.materials {
            if entry.name.is_empty()
                || !entry
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !names.insert(entry.name.clone())
            {
                return Err(format!(
                    "Invalid/duplicate material name: {} (use letters, digits, underscore)",
                    entry.name
                ));
            }
            if !entry.young_pa.is_finite()
                || entry.young_pa <= 0.0
                || !entry.poisson.is_finite()
                || entry.poisson <= -1.0
                || entry.poisson >= 0.5
                || entry
                    .density_kg_m3
                    .is_some_and(|rho| !rho.is_finite() || rho <= 0.0)
            {
                return Err(format!(
                    "{}: require E > 0, -1 < nu < 0.5, density > 0 (or omit density)",
                    entry.name
                ));
            }
            for units in [LibraryUnits::Metres, LibraryUnits::Millimetres] {
                let material = units.material(entry);
                if material
                    .young_modulus
                    .is_none_or(|v| !v.is_finite() || v <= 0.0)
                    || material.poisson_ratio.is_none_or(|v| v <= -1.0 || v >= 0.5)
                    || material.density.is_some_and(|v| !v.is_finite() || v <= 0.0)
                {
                    return Err(format!(
                        "{}: constants exceed supported precision",
                        entry.name
                    ));
                }
            }
            if entry.label.trim().is_empty() {
                entry.label = entry.name.clone();
            }
        }
        Ok(data)
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Library exceeds the 1 MiB limit".into());
        }
        let text =
            std::str::from_utf8(&bytes).map_err(|_| "Save the TOML file as UTF-8".to_string())?;
        Self::parse(text)
    }
}

fn check_keys(table: &toml_edit::Table, allowed: &[&str]) -> Result<(), String> {
    for (key, _) in table.iter() {
        if !allowed.contains(&key) {
            return Err(format!("Unknown field: {key}"));
        }
    }
    Ok(())
}
fn string_field(table: &toml_edit::Table, name: &str, required: bool) -> Result<String, String> {
    match table.get(name) {
        None if !required => Ok(String::new()),
        Some(item) => item
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{name} must be a string")),
        _ => Err(format!("Missing {name}")),
    }
}
fn number_field(
    table: &toml_edit::Table,
    name: &str,
    required: bool,
) -> Result<Option<f64>, String> {
    match table.get(name) {
        None if !required => Ok(None),
        Some(item) => item
            .as_float()
            .or_else(|| item.as_integer().map(|v| v as f64))
            .map(Some)
            .ok_or_else(|| format!("{name} must be a number")),
        _ => Err(format!("Missing {name}")),
    }
}

pub(crate) fn default_path() -> PathBuf {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .join("materials.toml");
    if cwd.is_file() {
        return cwd;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("materials.toml")))
        .filter(|path| path.is_file())
        .unwrap_or(cwd)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryUnits {
    Metres,
    Millimetres,
}
impl LibraryUnits {
    pub fn label(self) -> &'static str {
        match self {
            Self::Metres => "m / kg / N / s",
            Self::Millimetres => "mm / t / N / s",
        }
    }
    pub fn material(self, entry: &LibraryMaterial) -> FemMaterial {
        let (e_scale, rho_scale) = match self {
            Self::Metres => (1.0, 1.0),
            Self::Millimetres => (1e-6, 1e-12),
        };
        FemMaterial {
            name: entry.name.clone(),
            young_modulus: Some((entry.young_pa * e_scale) as f32),
            poisson_ratio: Some(entry.poisson as f32),
            density: entry.density_kg_m3.map(|v| (v * rho_scale) as f32),
        }
    }
}

#[cfg(test)]
#[path = "material_catalog_tests.rs"]
mod tests;
