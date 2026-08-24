use bevy::prelude::*;
use fem_core::FemEntityRef;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionOperation {
    Replace,
    Add,
    Toggle,
    Remove,
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
            self.highlight_targets
                .retain(|target| !removed_highlights.contains(target));
            return;
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
}
