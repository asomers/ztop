// vim: tw=80
use std::error::Error;
use super::Snapshot;
use sysctl::{Ctl, CtlIter, Sysctl, SysctlError, CtlValue};

pub struct SnapshotIter {
    ctl_iter: CtlIter,
    objset_name: Option<String>,
    dataset_name: Option<String>,
    nunlinked: Option<u64>,
    nunlinks: Option<u64>,
    nread: Option<u64>,
    reads: Option<u64>,
    nwritten: Option<u64>,
    writes: Option<u64>,
}

impl SnapshotIter {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let ctl_iter = CtlIter::below(Ctl::new("kstat.zfs")?);
        Ok(SnapshotIter{
            ctl_iter,
            objset_name: None,
            dataset_name: None,
            nunlinked: None,
            nunlinks: None,
            nread: None,
            reads: None,
            nwritten: None,
            writes: None,
        })
    }

    /// Progressively build the next Snapshot
    ///
    /// # Returns
    ///
    /// If all of the sysctls relevant to the snapshot have been received,
    /// returns `Some(snapshot)` and prepares `self` to build the next Snapshot.
    fn build(&mut self, name: String, value: CtlValue) -> Option<Snapshot> {
        let mut fields = name.split(".");
        let on = fields.nth(4).unwrap();
        if let Some(son) = &self.objset_name {
            assert_eq!(son, on);
        } else {
            self.objset_name = Some(on.to_owned());
        }
        let field = fields.next().unwrap();
        match value {
            CtlValue::String(s) => {
                if field != "dataset_name" {
                    unimplemented!("Unknown sysctl {:?}", name);
                }
                assert_eq!(self.dataset_name.replace(s), None);
            },
            CtlValue::U64(x) => {
                match field {
                    "nunlinked" => {self.nunlinked = Some(x);}
                    "nunlinks" => {self.nunlinks = Some(x);}
                    "nread" => {self.nread = Some(x);}
                    "reads" => {self.reads = Some(x);}
                    "nwritten" => {self.nwritten = Some(x);}
                    "writes" => {self.writes = Some(x);}
                    _ => unimplemented!("Unknown sysctl {:?}", name)
                }
            },
            _ => unimplemented!("Unknown sysctl {:?} = {:?}", name, value)
        };
        if self.dataset_name.is_some() &&
            self.nunlinked.is_some() &&
            self.nunlinks.is_some() &&
            self.nread.is_some() &&
            self.reads.is_some() &&
            self.nwritten.is_some() &&
            self.writes.is_some()
        {
            self.objset_name = None;
            Some(Snapshot {
                name: self.dataset_name.take().unwrap(),
                nunlinked: self.nunlinked.take().unwrap(),
                nunlinks: self.nunlinks.take().unwrap(),
                nread: self.nread.take().unwrap(),
                reads: self.reads.take().unwrap(),
                nwritten: self.nwritten.take().unwrap(),
                writes: self.writes.take().unwrap(),
            })
        } else {
            None
        }
    }

    /// Return the next Ctl that ztop cares about
    fn next_ztop(&mut self) -> Option<Result<(Ctl, String), SysctlError>> {
        loop  {
            match self.ctl_iter.next() {
                Some(Ok(ctl)) => {
                    match ctl.name() {
                        Ok(name) => {
                            if name.splitn(4, ".")
                                .last()
                                .map(|l| l.starts_with("dataset"))
                                .unwrap_or(false)
                            {
                                break Some(Ok((ctl, name)));
                            } else {
                                continue;
                            }
                        }
                        Err(e) => {return Some(Err(e));}
                    }
                }
                Some(Err(e)) => {return Some(Err(e));}
                None => {return None;}
            }
        }
    }
}

impl Iterator for SnapshotIter {
    type Item=Result<Snapshot, Box<SysctlError>>;

    fn next(&mut self) -> Option<Self::Item> {
        // We need to read several values from the internal iterator to assemble
        // a Snapshot.  AFAIK they will always be returned in the same order on
        // every system.  If not this code will grow more complicated.
        loop {
            match self.next_ztop() {
                Some(Ok((ctl, name))) => {
                    match ctl.value() {
                        Ok(value) => {
                            if let Some(snapshot) = self.build(name, value) {
                                break Some(Ok(snapshot));
                            }
                            // else continue
                        }
                        Err(e) => {break Some(Err(Box::new(e)))}
                    }
                }
                Some(Err(e)) => {break Some(Err(Box::new(e)))},
                None => {break None}
            }
        }
    }
}
