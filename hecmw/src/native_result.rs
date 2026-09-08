//! HEC-MW ASCII result records (v1/v2), following hecmw_result_io_txt.c.
//! Counts, component widths/labels and IDs are explicit; missing values must
//! never turn into plausible zero results when joining MPI partitions.
use bevy::prelude::Vec3;
use fem_core::{NodeId, ResultField, StepResult};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
struct Block {
    layout: Vec<(String, usize)>,
    values: HashMap<u32, Vec<f32>>,
}

#[derive(Clone, Debug)]
pub struct NativeResult {
    time: Option<f32>,
    nodes: Block,
    element_layout: Vec<(String, usize)>,
}

struct Tokens<'a>(std::str::SplitWhitespace<'a>);
impl<'a> Tokens<'a> {
    fn next(&mut self) -> Result<&'a str, String> {
        self.0
            .next()
            .ok_or_else(|| "Truncated HEC-MW result record".into())
    }
    fn count(&mut self) -> Result<usize, String> {
        let token = self.next()?;
        token
            .parse()
            .map_err(|_| format!("Expected a count/ID, got {token:?}"))
    }
    fn number(&mut self) -> Result<f32, String> {
        let token = self.next()?;
        token
            .replace(['D', 'd'], "E")
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("Invalid/non-finite result value {token:?}"))
    }
    fn layout(&mut self, count: usize) -> Result<Vec<(String, usize)>, String> {
        let mut widths = Vec::new();
        for _ in 0..count {
            let width = self.count()?;
            if width == 0 || width > 1024 {
                return Err("Invalid result component width".into());
            }
            widths.push(width);
        }
        let mut layout = Vec::new();
        for width in widths {
            let name = self.next()?.to_string();
            if layout.iter().any(|(n, _)| n == &name) {
                return Err(format!("Duplicate result label {name}"));
            }
            layout.push((name, width));
        }
        Ok(layout)
    }
    fn block(&mut self, count: usize, components: usize) -> Result<Block, String> {
        let layout = self.layout(components)?;
        let width: usize = layout.iter().map(|(_, n)| n).sum();
        let mut block = Block {
            layout,
            ..Default::default()
        };
        if width == 0 {
            return Ok(block);
        }
        for _ in 0..count {
            let id = u32::try_from(self.count()?).map_err(|_| "Result ID exceeds u32")?;
            let mut values = Vec::new();
            for _ in 0..width {
                values.push(self.number()?);
            }
            if block.values.insert(id, values).is_some() {
                return Err(format!("Duplicate result ID {id}"));
            }
        }
        Ok(block)
    }
}

impl NativeResult {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut lines = source.lines();
        let header = lines.next().unwrap_or("").trim();
        if !header.starts_with("*fstrresult") {
            return Err(
                "Expected FrontISTR ASCII *fstrresult header (binary results are not supported)"
                    .into(),
            );
        }
        let mut time = None;
        let remainder = lines.collect::<Vec<_>>().join("\n");
        let data = if header.split_whitespace().nth(1).is_some() {
            if header.split_whitespace().nth(1) != Some("2.0") {
                return Err(format!("Unsupported HEC-MW result version: {header}"));
            }
            let rows: Vec<_> = remainder.lines().collect();
            let global = rows
                .iter()
                .position(|s| s.trim() == "*global")
                .ok_or("Missing *global section")?;
            let data = rows
                .iter()
                .position(|s| s.trim() == "*data")
                .ok_or("Missing *data section")?;
            if global >= data {
                return Err("Invalid result section order".into());
            }
            let text = rows[global + 1..data].join("\n");
            let mut tokens = Tokens(text.split_whitespace());
            let count = tokens.count()?;
            let layout = tokens.layout(count)?;
            for (name, width) in layout {
                for component in 0..width {
                    let value = tokens.number()?;
                    if name.eq_ignore_ascii_case("TOTALTIME") && component == 0 {
                        time = Some(value);
                    }
                }
            }
            if tokens.0.next().is_some() {
                return Err("Extra global result data".into());
            }
            rows[data + 1..].join("\n")
        } else {
            remainder
        };
        let mut tokens = Tokens(data.split_whitespace());
        let nodes = tokens.count()?;
        let elements = tokens.count()?;
        let node_components = tokens.count()?;
        let element_components = tokens.count()?;
        let nodes = tokens.block(nodes, node_components)?;
        let elements = tokens.block(elements, element_components)?;
        if tokens.0.next().is_some() {
            return Err("Extra data after HEC-MW result records".into());
        }
        if nodes.layout.is_empty() {
            return Err("No nodal result fields to display".into());
        }
        Ok(Self {
            time,
            nodes,
            element_layout: elements.layout,
        })
    }

    pub fn merge(&mut self, other: Self) -> Result<(), String> {
        fn close(a: f32, b: f32) -> bool {
            (a - b).abs() <= 1e-7 + 1e-5 * a.abs().max(b.abs())
        }
        match (self.time, other.time) {
            (Some(a), Some(b)) if close(a, b) => {}
            (None, None) => {}
            _ => return Err("MPI result times disagree".into()),
        }
        // This API publishes nodal fields only. Element records were checked
        // while parsing; they cannot be merged without element ownership.
        if self.element_layout != other.element_layout {
            return Err("MPI element layouts disagree".into());
        }
        let target = &mut self.nodes;
        let source = other.nodes;
        if target.layout != source.layout {
            return Err("MPI result fields disagree".into());
        }
        for (id, values) in source.values {
            if let Some(previous) = target.values.get(&id) {
                if let Some((i, (a, b))) = previous
                    .iter()
                    .zip(&values)
                    .enumerate()
                    .find(|(_, (a, b))| !close(**a, **b))
                {
                    return Err(format!(
                        "Conflicting MPI node values at ID {id}, column {i}: {a} / {b}; layout {:?}",
                        target.layout
                    ));
                }
            } else {
                target.values.insert(id, values);
            }
        }
        Ok(())
    }

    /// Drop ghost copies, using the distributed mesh's owner rank (not file
    /// order or averaging). Each owned node must have a result record.
    pub fn retain_owned_nodes(
        &mut self,
        ids: &std::collections::HashSet<NodeId>,
    ) -> Result<(), String> {
        for id in ids {
            if !self.nodes.values.contains_key(&id.0) {
                return Err(format!("Missing owned node result {}", id.0));
            }
        }
        self.nodes.values.retain(|id, _| ids.contains(&NodeId(*id)));
        Ok(())
    }

    /// Every requested node must be present. Range caches use the entire
    /// merged result so equal colors mean equal values across assembly parts.
    pub fn nodal_step(&self, node_ids: &[NodeId], step: u32) -> Result<StepResult, String> {
        for id in node_ids {
            if !self.nodes.values.contains_key(&id.0) {
                return Err(format!("Missing result for node {}", id.0));
            }
        }
        let mut fields = Vec::new();
        let mut offset = 0;
        for (name, width) in &self.nodes.layout {
            if name.eq_ignore_ascii_case("DISPLACEMENT") && *width >= 2 {
                let vector = |v: &Vec<f32>| {
                    Vec3::new(
                        v[offset],
                        v[offset + 1],
                        if *width >= 3 { v[offset + 2] } else { 0.0 },
                    )
                };
                fields.push(ResultField::NodeVector {
                    name: "Displacement".into(),
                    values: node_ids
                        .iter()
                        .map(|id| vector(&self.nodes.values[&id.0]))
                        .collect(),
                    min_mag: self
                        .nodes
                        .values
                        .values()
                        .map(|v| vector(v).length())
                        .fold(f32::INFINITY, f32::min),
                    max_mag: self
                        .nodes
                        .values
                        .values()
                        .map(|v| vector(v).length())
                        .fold(0.0, f32::max),
                });
            }
            // Components remain named; stress components are never guessed
            // to be displacements or eigenmodes based on column count.
            for component in 0..*width {
                let label = if *width == 1 {
                    name.clone()
                } else {
                    format!("{name}[{}]", component + 1)
                };
                fields.push(ResultField::NodeScalar {
                    name: label,
                    values: node_ids
                        .iter()
                        .map(|id| self.nodes.values[&id.0][offset + component])
                        .collect(),
                    min: self
                        .nodes
                        .values
                        .values()
                        .map(|v| v[offset + component])
                        .fold(f32::INFINITY, f32::min),
                    max: self
                        .nodes
                        .values
                        .values()
                        .map(|v| v[offset + component])
                        .fold(f32::NEG_INFINITY, f32::max),
                });
            }
            offset += width;
        }
        Ok(StepResult {
            step,
            time: self.time.unwrap_or(0.0),
            fields,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const A: &str = "*fstrresult 2.0\n*comment\nstatic\n*global\n1\n1\nTOTALTIME\n1.25D+0\n*data\n2 1\n2 1\n3 1\nDISPLACEMENT\nMISES\n10\n1 2 3 9\n20\n4 5 6 10\n1\nElementMISES\n50\n8\n";
    #[test]
    fn native_counts_wrapped_values_labels_and_ids() {
        let raw = NativeResult::parse(A).unwrap();
        let step = raw.nodal_step(&[NodeId(20), NodeId(10)], 7).unwrap();
        assert_eq!(step.time, 1.25);
        let ResultField::NodeVector { values, .. } = &step.fields[0] else {
            panic!()
        };
        assert_eq!(values[0], Vec3::new(4.0, 5.0, 6.0));
        assert!(step.field_by_name("MISES").is_some());
        assert!(raw.nodal_step(&[NodeId(30)], 1).is_err());
        assert!(NativeResult::parse(&A.replace("4 5 6 10", "4 bad 6 10")).is_err());
        assert!(NativeResult::parse(A.trim_end_matches("8\n")).is_err());
    }
    #[test]
    fn partitions_merge_without_inventing_missing_values() {
        let mut a = NativeResult::parse(A).unwrap();
        a.merge(NativeResult::parse(A).unwrap()).unwrap();
        assert!(
            a.merge(NativeResult::parse(&A.replace("1 2 3 9", "9 2 3 9")).unwrap())
                .is_err()
        );
        let b = NativeResult::parse("*fstrresult\n1 0\n1 0\n1\nTEMPERATURE\n5\n300\n").unwrap();
        assert!(b.nodal_step(&[NodeId(5)], 1).is_ok());
    }

    #[test]
    fn owner_values_override_different_ghost_copies_without_averaging() {
        let mut a = NativeResult::parse(A).unwrap();
        let mut b = NativeResult::parse(
            &A.replace("1 2 3 9", "99 2 3 9")
                .replace("4 5 6 10", "8 5 6 10"),
        )
        .unwrap();
        assert!(a.retain_owned_nodes(&[NodeId(99)].into()).is_err());
        a.retain_owned_nodes(&[NodeId(10)].into()).unwrap();
        b.retain_owned_nodes(&[NodeId(20)].into()).unwrap();
        a.merge(b).unwrap();
        let step = a.nodal_step(&[NodeId(10), NodeId(20)], 1).unwrap();
        let ResultField::NodeVector { values, .. } = &step.fields[0] else {
            panic!()
        };
        assert_eq!(values[0].x, 1.0);
        assert_eq!(values[1].x, 8.0);
    }
}
