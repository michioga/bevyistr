//! Shared strict interpretation for the reader and compatibility guard.
use super::KeywordHeader;

pub(super) fn parse(h: &KeywordHeader) -> Result<(bool, bool, u32), &'static str> {
    if h.params
        .keys()
        .any(|key| !matches!(key.as_str(), "RESULT" | "VISUAL" | "FREQUENCY"))
    {
        return Err("Only WRITE RESULT / VISUAL with a positive FREQUENCY are supported");
    }
    let res = h.params.contains_key("RESULT");
    let vis = h.params.contains_key("VISUAL");
    if (!res && !vis)
        || ["RESULT", "VISUAL"]
            .iter()
            .any(|key| h.params.get(*key).is_some_and(|v| !v.is_empty()))
    {
        return Err("WRITE requires a bare RESULT or VISUAL flag");
    }
    let frequency = match h.params.get("FREQUENCY") {
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|v| *v > 0 && *v <= i32::MAX as u32)
            .ok_or("WRITE FREQUENCY must be an integer between 1 and 2147483647")?,
        None => 1,
    };
    Ok((res, vis, frequency))
}

#[cfg(test)]
#[path = "write_settings_tests.rs"]
mod tests;
