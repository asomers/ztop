// vim: tw=80
use std::error::Error;
use super::Snapshot;

pub(super) struct SnapshotIter {}

impl SnapshotIter {
    pub(crate) fn new(_pool: Option<&str>) -> Result<Self, Box<dyn Error>> {
        unimplemented!()
    }
}

impl Iterator for SnapshotIter {
    type Item=Result<Snapshot, Box<dyn Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        unimplemented!()
    }
}
