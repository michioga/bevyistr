use fem_core::{FrontistrPartitionMethod as Method, FrontistrPartitionSettings as Settings};
use std::{collections::HashMap, fmt::Write, fs, io, path::Path};

pub fn load(path: impl AsRef<Path>) -> io::Result<Settings> {
    parse(&fs::read_to_string(path)?)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn parse(text: &str) -> io::Result<Settings> {
    let mut lines = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty() && !line.starts_with("!!"));
    let header = lines
        .next()
        .ok_or_else(|| invalid("Missing PARTITION card"))?;
    let mut fields = header.split(',').map(str::trim);
    if !fields
        .next()
        .is_some_and(|key| key.eq_ignore_ascii_case("!PARTITION"))
    {
        return Err(invalid("Expected !PARTITION"));
    }
    let mut values = HashMap::new();
    for field in fields {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| invalid("Invalid PARTITION parameter"))?;
        let key = key.trim().to_ascii_uppercase();
        if values.insert(key, value.trim()).is_some() {
            return Err(invalid("Repeated PARTITION parameter"));
        }
    }
    let upper = |value: &str| value.to_ascii_uppercase();
    let element_based = match values.remove("TYPE").map(upper).as_deref() {
        Some("NODE-BASED") => false,
        Some("ELEMENT-BASED") => true,
        _ => return Err(invalid("Invalid partition TYPE")),
    };
    let method = match values.remove("METHOD").map(upper).as_deref() {
        Some("PMETIS") => Method::Pmetis,
        Some("KMETIS") => Method::Kmetis,
        Some("RCB") => {
            let axes = lines.next().unwrap_or("");
            let axes = if axes.is_empty() {
                Vec::new()
            } else {
                axes.split(',')
                    .map(|axis| match axis.trim().to_ascii_lowercase().as_str() {
                        "x" => Ok('x'),
                        "y" => Ok('y'),
                        "z" => Ok('z'),
                        _ => Err(invalid("Invalid RCB partition axis")),
                    })
                    .collect::<io::Result<_>>()?
            };
            Method::Rcb(axes)
        }
        _ => return Err(invalid("Unsupported partition METHOD")),
    };
    let domains = values
        .remove("DOMAIN")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| invalid("Invalid or missing partition DOMAIN"))?;
    let depth = values
        .remove("DEPTH")
        .map(|value| {
            value
                .parse()
                .map_err(|_| invalid("Invalid partition DEPTH"))
        })
        .transpose()?;
    let ucd = values.remove("UCD").map(str::to_owned);
    let contact = values.remove("CONTACT").map(upper);
    let contact_owner = values.remove("CONTACT_OWNER").map(upper);
    if !values.is_empty() || lines.next().is_some() {
        return Err(invalid("Unsupported additional partition settings"));
    }
    let settings = Settings {
        element_based,
        method,
        domains,
        depth,
        ucd,
        contact,
        contact_owner,
    };
    validate(&settings, settings.domains.get(), false)?;
    Ok(settings)
}

pub fn rcb_split_count(ranks: u16) -> Option<usize> {
    ranks.is_power_of_two().then(|| ranks.ilog2() as usize)
}

pub fn validate(settings: &Settings, ranks: u16, has_contact: bool) -> io::Result<()> {
    if ranks == 0 {
        return Err(invalid("Partition DOMAIN must be positive"));
    }
    if let Method::Rcb(axes) = &settings.method
        && (rcb_split_count(ranks) != Some(axes.len())
            || !axes.iter().all(|axis| matches!(axis, 'x' | 'y' | 'z')))
    {
        return Err(invalid(
            "RCB requires a power-of-two DOMAIN and one axis per split",
        ));
    }
    if settings.element_based
        && (settings.depth.is_some() || settings.contact_owner.is_some() || has_contact)
    {
        return Err(invalid(
            "ELEMENT-BASED is incompatible with DEPTH, CONTACT_OWNER or a mesh with contact pairs",
        ));
    }
    if settings.ucd.as_ref().is_some_and(|name| {
        !super::staged_inputs::is_project_name(name)
            || name.contains(|c: char| c.is_whitespace() || matches!(c, ',' | '#' | '!'))
    }) {
        return Err(invalid("UCD must be a simple relative output filename"));
    }
    if settings
        .contact
        .as_deref()
        .is_some_and(|value| !matches!(value, "DEFAULT" | "AGGREGATE" | "DISTRIBUTE" | "SIMPLE"))
    {
        return Err(invalid("Unsupported partition CONTACT value"));
    }
    if settings
        .contact_owner
        .as_deref()
        .is_some_and(|value| !matches!(value, "MASTER" | "SLAVE"))
    {
        return Err(invalid("Unsupported partition CONTACT_OWNER value"));
    }
    Ok(())
}

pub(crate) fn format(settings: Option<&Settings>, ranks: u16) -> io::Result<String> {
    let defaults = Settings::default();
    let settings = settings.unwrap_or(&defaults);
    validate(settings, ranks, false)?;
    let kind = if settings.element_based {
        "ELEMENT-BASED"
    } else {
        "NODE-BASED"
    };
    let mut out = format!(
        "!PARTITION, TYPE={kind}, METHOD={}, DOMAIN={ranks}",
        settings.method.keyword()
    );
    if let Some(depth) = settings.depth {
        write!(out, ", DEPTH={depth}").unwrap();
    }
    for (name, value) in [
        ("UCD", &settings.ucd),
        ("CONTACT", &settings.contact),
        ("CONTACT_OWNER", &settings.contact_owner),
    ] {
        if let Some(value) = value {
            write!(out, ", {name}={value}").unwrap();
        }
    }
    out.push('\n');
    if let Method::Rcb(axes) = &settings.method {
        if !axes.is_empty() {
            for (index, axis) in axes.iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                out.push(*axis);
            }
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_rcb_partition_and_contact_options() {
        let source = "!PARTITION, TYPE=NODE-BASED, METHOD=RCB, DOMAIN=8, DEPTH=2, UCD=mesh.inp, CONTACT=DISTRIBUTE, CONTACT_OWNER=SLAVE\nx,y,z\n";
        let settings = parse(source).unwrap();
        assert_eq!(
            parse(&format(Some(&settings), 8).unwrap()).unwrap(),
            settings
        );
        assert!(format(Some(&settings), 4).is_err());
    }

    #[test]
    fn rejects_lossy_or_incompatible_partition_settings() {
        assert!(
            parse("!PARTITION, TYPE=ELEMENT-BASED, METHOD=PMETIS, DOMAIN=2, CONTACT_OWNER=SLAVE\n")
                .is_err()
        );
        assert!(parse("!PARTITION, TYPE=NODE-BASED, METHOD=PMETIS, DOMAIN=2, SORT=YES\n").is_err());
        assert!(
            parse("!PARTITION, TYPE=NODE-BASED, METHOD=PMETIS, DOMAIN=2, UCD=../mesh.inp\n")
                .is_err()
        );
    }
}
