use bevy::prelude::*;
use fem_core::FemEntityRef;
use std::collections::HashSet;

const MULTI_CLICK_MAX_GAP_SECS: f64 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionOperation {
    Replace,
    Add,
    Toggle,
    Remove,
}

/// Counts repeated clicks on the same FEM target without depending on the
/// platform window backend's double-click handling.
#[derive(Resource, Debug, Clone)]
pub struct ClickSequence {
    last_target: Option<FemEntityRef>,
    last_time_secs: f64,
    count: u8,
}

impl Default for ClickSequence {
    fn default() -> Self {
        Self {
            last_target: None,
            last_time_secs: f64::NEG_INFINITY,
            count: 0,
        }
    }
}

impl ClickSequence {
    pub fn register(&mut self, target: FemEntityRef, now_secs: f64) -> u8 {
        let repeated = self.last_target == Some(target)
            && now_secs - self.last_time_secs <= MULTI_CLICK_MAX_GAP_SECS;

        self.count = if repeated && self.count < 3 {
            self.count + 1
        } else {
            1
        };
        self.last_target = Some(target);
        self.last_time_secs = now_secs;
        self.count
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl SelectionOperation {
    pub const fn from_modifiers(ctrl: bool, shift: bool, alt: bool) -> Self {
        if alt || (ctrl && shift) {
            Self::Remove
        } else if shift {
            Self::Toggle
        } else if ctrl {
            Self::Add
        } else {
            Self::Replace
        }
    }
}

#[derive(Resource, Default)]
pub struct SelectionState {
    pub entities: Vec<Entity>,

    pub targets: Vec<FemEntityRef>,

    /// Geometry shown by the selected overlay. Usually identical to
    /// `targets`; surface-grown Element selections retain element IDs in
    /// `targets` but show their originating boundary Face patch here.
    pub highlight_targets: Vec<FemEntityRef>,
}

impl SelectionState {
    pub fn clear(&mut self) {
        self.entities.clear();
        self.targets.clear();
        self.highlight_targets.clear();
    }

    pub fn len(&self) -> usize {
        if self.targets.is_empty() {
            self.entities.len()
        } else {
            self.targets.len()
        }
    }

    pub fn will_remove_group(
        &self,
        targets: &[FemEntityRef],
        operation: SelectionOperation,
    ) -> bool {
        operation == SelectionOperation::Remove
            || (operation == SelectionOperation::Toggle
                && !targets.is_empty()
                && targets.iter().all(|target| self.targets.contains(target)))
    }

    pub fn apply_group(
        &mut self,
        targets: &[FemEntityRef],
        highlight_targets: &[FemEntityRef],
        operation: SelectionOperation,
    ) {
        let remove = self.will_remove_group(targets, operation);

        if operation == SelectionOperation::Replace {
            self.clear();
        }

        if remove {
            let removed: HashSet<_> = targets.iter().copied().collect();
            let removed_highlights: HashSet<_> = highlight_targets.iter().copied().collect();
            self.targets.retain(|target| !removed.contains(target));
            self.highlight_targets.retain(|target| {
                !removed.contains(target) && !removed_highlights.contains(target)
            });
            return;
        }

        // A grown Element group replaces the direct whole-element overlay
        // with boundary Face highlights. This matters when Ctrl+double-click
        // expands an Element that was added by the first click.
        for target in targets {
            if !highlight_targets.contains(target) {
                self.highlight_targets.retain(|highlight| highlight != target);
            }
        }

        for &target in targets {
            if !self.targets.contains(&target) {
                self.targets.push(target);
            }
        }
        for &target in highlight_targets {
            if !self.highlight_targets.contains(&target) {
                self.highlight_targets.push(target);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{FaceId, NodeId};

    #[test]
    fn modifier_mapping_matches_viewport_conventions() {
        assert_eq!(
            SelectionOperation::from_modifiers(false, false, false),
            SelectionOperation::Replace
        );
        assert_eq!(
            SelectionOperation::from_modifiers(true, false, false),
            SelectionOperation::Add
        );
        assert_eq!(
            SelectionOperation::from_modifiers(false, true, false),
            SelectionOperation::Toggle
        );
        assert_eq!(
            SelectionOperation::from_modifiers(true, true, false),
            SelectionOperation::Remove
        );
        assert_eq!(
            SelectionOperation::from_modifiers(false, false, true),
            SelectionOperation::Remove
        );
    }

    #[test]
    fn toggle_adds_then_removes_the_same_group_and_its_highlight() {
        let targets = [FemEntityRef::node(0, NodeId(3))];
        let highlights = [FemEntityRef::face(0, FaceId(4))];
        let mut selection = SelectionState::default();

        selection.apply_group(&targets, &highlights, SelectionOperation::Toggle);
        assert_eq!(selection.targets, targets);
        assert_eq!(selection.highlight_targets, highlights);

        selection.apply_group(&targets, &highlights, SelectionOperation::Toggle);
        assert!(selection.targets.is_empty());
        assert!(selection.highlight_targets.is_empty());
    }

    #[test]
    fn repeated_clicks_count_to_three_then_start_over() {
        let target = FemEntityRef::node(0, NodeId(3));
        let mut sequence = ClickSequence::default();

        assert_eq!(sequence.register(target, 1.00), 1);
        assert_eq!(sequence.register(target, 1.20), 2);
        assert_eq!(sequence.register(target, 1.35), 3);
        assert_eq!(sequence.register(target, 1.50), 1);
    }

    #[test]
    fn a_different_target_or_long_gap_starts_a_new_sequence() {
        let first = FemEntityRef::node(0, NodeId(3));
        let second = FemEntityRef::node(0, NodeId(4));
        let mut sequence = ClickSequence::default();

        assert_eq!(sequence.register(first, 1.00), 1);
        assert_eq!(sequence.register(second, 1.10), 1);
        assert_eq!(sequence.register(second, 2.00), 1);
        sequence.reset();
        assert_eq!(sequence.register(second, 2.10), 1);
    }

    #[test]
    fn a_surface_highlight_replaces_the_same_elements_direct_overlay() {
        use fem_core::ElementId;

        let element = FemEntityRef::element(0, ElementId(7));
        let face = FemEntityRef::face(0, FaceId(9));
        let mut selection = SelectionState::default();

        selection.apply_group(&[element], &[element], SelectionOperation::Add);
        selection.apply_group(&[element], &[face], SelectionOperation::Add);

        assert_eq!(selection.targets, vec![element]);
        assert_eq!(selection.highlight_targets, vec![face]);

        selection.apply_group(&[element], &[face], SelectionOperation::Remove);
        assert!(selection.targets.is_empty());
        assert!(selection.highlight_targets.is_empty());
    }
}
