//! Project-local FrontISTR output controls. Unedited cards remain intact.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTarget {
    Res,
    Vis,
}

impl OutputTarget {
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Res => "!OUTPUT_RES",
            Self::Vis => "!OUTPUT_VIS",
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Res => "RES",
            Self::Vis => "VTK",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputCard {
    pub header: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputControl {
    pub cards: Vec<OutputCard>,
}

fn item(line: &str) -> Option<(&str, bool)> {
    let mut parts = line.split(',').map(str::trim);
    let key = parts.next()?;
    let value = parts.next()?;
    if key.is_empty() || parts.next().is_some() {
        return None;
    }
    if value.eq_ignore_ascii_case("ON") {
        Some((key, true))
    } else if value.eq_ignore_ascii_case("OFF") {
        Some((key, false))
    } else {
        None
    }
}

impl OutputControl {
    /// GROUP/ACTION and extended data syntax are retained, but not flattened
    /// into the global ON/OFF editor. Unknown *names* with ON/OFF are safe.
    pub fn editable(&self, target: OutputTarget) -> bool {
        self.cards.iter().all(|card| {
            card.header.trim().eq_ignore_ascii_case(target.keyword())
                && card.lines.iter().all(|line| {
                    let line = line.trim();
                    line.is_empty()
                        || line.starts_with("!!")
                        || line.starts_with('#')
                        || item(line).is_some()
                })
        })
    }

    /// None means solver default, not OFF. Defaults vary by solver version.
    pub fn value(&self, keyword: &str) -> Option<bool> {
        self.cards
            .iter()
            .flat_map(|card| &card.lines)
            .filter_map(|line| item(line))
            .filter(|(key, _)| key.eq_ignore_ascii_case(keyword))
            .map(|(_, value)| value)
            .last()
    }

    pub fn set(&mut self, target: OutputTarget, keyword: &str, value: Option<bool>) -> bool {
        if !self.editable(target) || !OUTPUT_QUANTITIES.iter().any(|q| q.0 == keyword) {
            return false;
        }
        for card in &mut self.cards {
            card.lines.retain(|line| {
                !item(line).is_some_and(|(key, _)| key.eq_ignore_ascii_case(keyword))
            });
        }
        if let Some(value) = value {
            if self.cards.is_empty() {
                self.cards.push(OutputCard {
                    header: target.keyword().into(),
                    lines: vec![],
                });
            }
            self.cards
                .last_mut()
                .unwrap()
                .lines
                .push(format!("{keyword}, {}", if value { "ON" } else { "OFF" }));
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSettings {
    pub res: OutputControl,
    pub vis: OutputControl,
}

impl OutputSettings {
    /// An imported file without cards inherits the solver defaults unchanged.
    pub fn inherited() -> Self {
        Self {
            res: OutputControl::default(),
            vis: OutputControl::default(),
        }
    }
    pub fn get(&self, target: OutputTarget) -> &OutputControl {
        match target {
            OutputTarget::Res => &self.res,
            OutputTarget::Vis => &self.vis,
        }
    }
    pub fn get_mut(&mut self, target: OutputTarget) -> &mut OutputControl {
        match target {
            OutputTarget::Res => &mut self.res,
            OutputTarget::Vis => &mut self.vis,
        }
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        let mut settings = Self::inherited();
        for target in [OutputTarget::Res, OutputTarget::Vis] {
            for keyword in ["DISP", "NMISES"] {
                settings.get_mut(target).set(target, keyword, Some(true));
            }
        }
        settings
    }
}

/// Common structural quantities supported by FrontISTR's result and VIS
/// builders (m_out.f90 / analysis/static/make_result.f90). Availability still
/// depends on analysis, element type and computed data; ON is not a guarantee.
pub const OUTPUT_QUANTITIES: &[(&str, &str)] = &[
    ("DISP", "Displacement"),
    ("NMISES", "Nodal von Mises"),
    ("REACTION", "Reaction force"),
    ("NSTRAIN", "Nodal strain"),
    ("NSTRESS", "Nodal stress"),
    ("ESTRAIN", "Element strain"),
    ("ESTRESS", "Element stress"),
    ("EMISES", "Element von Mises"),
    ("VEL", "Velocity (dynamic)"),
    ("ACC", "Acceleration (dynamic)"),
    ("ROT", "Rotation (shell)"),
    ("PRINC_NSTRESS", "Nodal principal stress"),
    ("PRINC_ESTRESS", "Element principal stress"),
    ("PRINC_NSTRAIN", "Nodal principal strain"),
    ("PRINC_ESTRAIN", "Element principal strain"),
    ("PL_ESTRAIN", "Element plastic strain"),
    ("CONTACT_NFORCE", "Contact normal force"),
    ("CONTACT_FRICTION", "Contact friction force"),
    ("CONTACT_RELVEL", "Contact relative velocity"),
    ("CONTACT_STATE", "Contact state"),
    ("CONTACT_NTRACTION", "Contact normal traction"),
    ("CONTACT_FTRACTION", "Contact friction traction"),
    ("MATERIAL_ID", "Material ID"),
    ("NODE_ID", "Node ID"),
    ("ELEM_ID", "Element ID"),
    ("SECTION_ID", "Section ID"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_projects_enable_basics_but_imports_inherit() {
        for target in [OutputTarget::Res, OutputTarget::Vis] {
            assert_eq!(
                OutputSettings::default().get(target).value("DISP"),
                Some(true)
            );
            assert_eq!(OutputSettings::inherited().get(target).value("DISP"), None);
        }
    }
    #[test]
    fn editing_preserves_unknown_items_and_res_vis_independence() {
        let mut settings = OutputSettings::inherited();
        settings.res.cards.push(OutputCard {
            header: "!OUTPUT_RES".into(),
            lines: vec![
                "FUTURE_FIELD, ON".into(),
                "NSTRESS, OFF".into(),
                "NSTRESS, ON".into(),
            ],
        });
        assert_eq!(settings.res.value("NSTRESS"), Some(true));
        assert!(settings.res.set(OutputTarget::Res, "NSTRESS", None));
        assert_eq!(settings.res.value("NSTRESS"), None);
        assert_eq!(settings.res.cards[0].lines, ["FUTURE_FIELD, ON"]);
        assert!(settings.vis.cards.is_empty());
    }
    #[test]
    fn scoped_or_extended_cards_cannot_be_overwritten_by_global_editor() {
        for (header, line) in [
            ("!OUTPUT_RES, GROUP=FIX, ACTION=SUM", "DISP, OFF"),
            ("!OUTPUT_RES", "DISP, ON, VECTOR"),
        ] {
            let mut control = OutputControl {
                cards: vec![OutputCard {
                    header: header.into(),
                    lines: vec![line.into()],
                }],
            };
            let before = control.clone();
            assert!(!control.set(OutputTarget::Res, "DISP", Some(true)));
            assert_eq!(control, before);
        }
    }
}
