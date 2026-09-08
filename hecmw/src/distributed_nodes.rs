//! Read only the node ownership prefix of HEC-MW's distributed ASCII mesh.
//! Layout follows hecmw_io_dist.c, get_global_info/get_node_info. Coordinates
//! and element records are deliberately not loaded for this result query.
use fem_core::NodeId;
use std::{collections::HashSet, io::BufRead};

pub fn owned_nodes(
    mut reader: impl BufRead,
    rank: u16,
    ranks: u16,
) -> Result<HashSet<NodeId>, String> {
    fn line(reader: &mut impl BufRead) -> Result<String, String> {
        let mut text = String::new();
        if reader.read_line(&mut text).map_err(|e| e.to_string())? == 0 {
            return Err("Truncated distributed mesh header".into());
        }
        Ok(text.trim().to_string())
    }
    fn number(reader: &mut impl BufRead) -> Result<usize, String> {
        line(reader)?
            .parse()
            .map_err(|_| "Invalid distributed mesh header number".into())
    }
    let header = line(&mut reader)?;
    let version: usize = header
        .strip_prefix("!HECMW-DMD-ASCII version=")
        .ok_or("Expected a distributed ASCII mesh")?
        .parse()
        .map_err(|_| "Invalid mesh version")?;
    if !(1..=5).contains(&version) {
        return Err("Unsupported distributed mesh version".into());
    }
    if number(&mut reader)? != 0 {
        return Err("Adaptive distributed results are not supported".into());
    }
    let _init = number(&mut reader)?;
    let part_type = number(&mut reader)?;
    if part_type > 2 {
        return Err("Unsupported partition type".into());
    }
    let _depth = number(&mut reader)?;
    if number(&mut reader)? != version {
        return Err("Distributed mesh versions disagree".into());
    }
    if version >= 4 {
        let _contact = number(&mut reader)?;
    }
    let _gridfile = line(&mut reader)?;
    let files = number(&mut reader)?;
    for _ in 0..files {
        line(&mut reader)?;
    }
    match number(&mut reader)? {
        0 => {}
        1 => {
            line(&mut reader)?;
        }
        _ => return Err("Invalid mesh header flag".into()),
    }
    line(&mut reader)?
        .parse::<f64>()
        .map_err(|_| "Invalid mesh reference temperature")?;
    // Integers below may wrap across lines (arrays), unlike string headers.
    let mut tokens = reader.lines().flat_map(|line| match line {
        Ok(line) => line
            .split_whitespace()
            .map(|v| v.parse::<u32>().map_err(|e| e.to_string()))
            .collect::<Vec<_>>(),
        Err(error) => vec![Err(error.to_string())],
    });
    let mut next = || {
        tokens
            .next()
            .ok_or_else(|| "Truncated distributed node map".to_string())?
    };
    let n_node = next()? as usize;
    let gross = if version >= 2 {
        next()? as usize
    } else {
        n_node
    };
    let middle = if version >= 4 {
        next()? as usize
    } else {
        n_node
    };
    let internal = next()? as usize;
    if n_node > gross || middle > gross || internal > n_node {
        return Err("Invalid distributed node counts".into());
    }
    if part_type != 1 {
        for _ in 0..internal {
            let id = next()? as usize;
            if id == 0 || id > gross {
                return Err("Invalid internal node index".into());
            }
        }
    }
    let mut owners = Vec::new();
    for _ in 0..gross {
        let local_id = next()?;
        let owner = next()?;
        if local_id == 0 || owner >= u32::from(ranks) {
            return Err("Invalid distributed node owner".into());
        }
        owners.push(owner == u32::from(rank));
    }
    let mut seen = HashSet::new();
    let mut owned = HashSet::new();
    for owner in owners {
        let id = NodeId(next()?);
        if !seen.insert(id) {
            return Err("Duplicate distributed global node ID".into());
        }
        if owner {
            owned.insert(id);
        }
    }
    if owned.len() != internal {
        return Err("Distributed ownership count disagrees".into());
    }
    Ok(owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_owned_global_ids_not_ghosts_or_line_positions() {
        let text = "!HECMW-DMD-ASCII version=5\n0\n0\n1\n1\n5\n0\nmesh with spaces\n1\ninput path.msh\n1\nheader words\n0.0\n3 3 3 2\n1 0\n1 1\n2 0\n10 20 30\n";
        assert_eq!(
            owned_nodes(text.as_bytes(), 0, 2).unwrap(),
            HashSet::from([NodeId(10), NodeId(30)])
        );
        assert!(owned_nodes(text.replace("10 20 30", "10 20").as_bytes(), 0, 2).is_err());
        assert!(owned_nodes(text.replace("10 20 30", "10 20 10").as_bytes(), 0, 2).is_err());
        assert!(owned_nodes(text.replace("1 1\n2 0", "1 9\n2 0").as_bytes(), 0, 2).is_err());
    }
}
