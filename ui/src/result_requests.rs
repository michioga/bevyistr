//! Ordering shared by manual opening and Solve's automatic result handoff.
use bevy::prelude::Resource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResultRequestId(u64);

#[derive(Resource, Default)]
pub(crate) struct ResultRequests {
    latest: u64,
}

impl ResultRequests {
    pub fn begin(&mut self) -> ResultRequestId {
        self.latest = self.latest.wrapping_add(1);
        ResultRequestId(self.latest)
    }

    pub fn is_current(&self, request: ResultRequestId) -> bool {
        request.0 == self.latest
    }
}
