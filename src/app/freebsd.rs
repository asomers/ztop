// vim: tw=80
use std::{
    error::Error,
    mem
};
use super::Snapshot;
use sysctl::{Ctl, CtlIter, Sysctl, SysctlError, CtlValue};

#[derive(Default)]
struct Builder {
    dataset_name: Option<String>,
    nunlinked: Option<u64>,
    nunlinks: Option<u64>,
    nread: Option<u64>,
    reads: Option<u64>,
    nwritten: Option<u64>,
    writes: Option<u64>,
}

impl Builder {
    fn build(&mut self, name: &str, value: CtlValue) {
        let mut fields = name.split('.');
        let field = fields.nth(5).unwrap();
        match value {
            CtlValue::String(s) => {
                if field != "dataset_name" {
                    eprintln!("Unknown sysctl {:?}", name);
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
                    _ => eprintln!("Unknown sysctl {:?}", name),
                }
            },
            _ => eprintln!("Unknown sysctl {:?}", name),
        };
    }

    fn finish(mut self) -> Snapshot {
        Snapshot {
            name: self.dataset_name.take().unwrap(),
            // On FreeBSD 12.2 and earlier, unlinked and nunlinks will not be
            // present.  Set them to zero.
            nunlinked: self.nunlinked.take().unwrap_or(0),
            nunlinks: self.nunlinks.take().unwrap_or(0),
            nread: self.nread.take().unwrap(),
            reads: self.reads.take().unwrap(),
            nwritten: self.nwritten.take().unwrap(),
            writes: self.writes.take().unwrap(),
        }
    }
}

pub(super) struct SnapshotIter {
    inner: Box<dyn Iterator<Item=Result<(String, CtlValue), SysctlError>>>,
    objset_name: Option<String>,
    finished: bool,
    builder: Builder
}

impl SnapshotIter {
    pub(crate) fn new(pool: Option<&str>) -> Result<Self, Box<dyn Error>> {
        Ok(Self::with_inner(SysctlIter::new(pool)))
    }

    fn with_inner<T>(inner: T) -> Self
        where T: Iterator<Item=Result<(String, CtlValue), SysctlError>> + 'static
    {
        SnapshotIter {
            inner: Box::new(inner),
            objset_name: None,
            finished: false,
            builder: Builder::default()
        }
    }

    /// Progressively build the next Snapshot
    ///
    /// # Returns
    ///
    /// If all of the sysctls relevant to the snapshot have been received,
    /// returns `Some(snapshot)` and prepares `self` to build the next Snapshot.
    fn build(&mut self, name: String, value: CtlValue) -> Option<Snapshot> {
        let mut fields = name.split('.');
        let on = fields.nth(4).unwrap();
        match &self.objset_name {
            None => {
                self.builder.build(&name, value);
                self.objset_name = Some(on.to_owned());
                None
            },
            Some(son) if son == on => {
                self.builder.build(&name, value);
                None
            },
            _ => {
                self.objset_name = Some(on.to_owned());
                let new = Builder::default();
                let old = mem::replace(&mut self.builder, new);
                self.builder.build(&name, value);
                Some(old.finish())
            }
        }
    }

}

impl Iterator for SnapshotIter {
    type Item=Result<Snapshot, Box<SysctlError>>;

    fn next(&mut self) -> Option<Self::Item> {
        // We need to read several values from the internal iterator to assemble
        // a Snapshot.  We can't rely on them always being returned in the same
        // order.
        if self.finished {
            return None;
        }
        loop {
            match self.inner.next() {
                Some(Ok((name, value))) => {
                    if let Some(snapshot) = self.build(name, value) {
                        break Some(Ok(snapshot));
                    }
                    // else continue
                }
                Some(Err(e)) => {break Some(Err(Box::new(e)))},
                None => {
                    self.finished = true;
                    let new = Builder::default();
                    let old = mem::replace(&mut self.builder, new);
                    break Some(Ok(old.finish()));
                }
            }
        }
    }
}

/// Iterate through all of the sysctls, but only return the ones we care about.
struct SysctlIter(CtlIter);

impl SysctlIter {
    fn new(pool: Option<&str>) -> Self {
        let root = if let Some(s) = pool {
            Ctl::new(&format!("kstat.zfs.{}.dataset", s.replace(".", "%25")))
                .unwrap_or_else(|_e| {
                    eprintln!("Statistics not found for pool {}", s);
                    std::process::exit(1);
                })
        } else {
            Ctl::new("kstat.zfs").unwrap()
        };
        Self(CtlIter::below(root))
    }
}

impl Iterator for SysctlIter {
    type Item = Result<(String, CtlValue), SysctlError>;

    /// Return the next Ctl that ztop cares about
    fn next(&mut self) -> Option<Self::Item> {
        loop  {
            match self.0.next() {
                Some(Ok(ctl)) => {
                    match ctl.name() {
                        Ok(name) => {
                            if name.splitn(4, '.')
                                .last()
                                .map(|l| l.starts_with("dataset"))
                                .unwrap_or(false)
                            {
                                break Some(
                                    ctl.value()
                                    .map(|v| (name, v))
                                )
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

#[cfg(test)]
mod t {
    mod builder {
        use super::super::*;

        #[test]
        fn like_freebsd_12_2()
        {
            let names = vec![
                "kstat.zfs.tank.dataset.objset-0x58c.nread",
                "kstat.zfs.tank.dataset.objset-0x58c.reads",
                "kstat.zfs.tank.dataset.objset-0x58c.nwritten",
                "kstat.zfs.tank.dataset.objset-0x58c.writes",
                "kstat.zfs.tank.dataset.objset-0x58c.dataset_name"
            ].into_iter();
            let values = vec![
                CtlValue::U64(3),
                CtlValue::U64(4),
                CtlValue::U64(5),
                CtlValue::U64(6),
                CtlValue::String("tank/foo".to_owned())
            ].into_iter();
            let mut builder = Builder::default();
            for (n, v) in names.zip(values) {
                builder.build(n, v);
            }
            let r = builder.finish();
            assert_eq!(r.name, "tank/foo");
            assert_eq!(r.nunlinked, 0);
            assert_eq!(r.nunlinks, 0);
            assert_eq!(r.nread, 3);
            assert_eq!(r.reads, 4);
            assert_eq!(r.nwritten, 5);
            assert_eq!(r.writes, 6);
        }

        #[test]
        fn like_freebsd_13_0()
        {
            let names = vec![
                "kstat.zfs.tank.dataset.objset-0x58c.nunlinked",
                "kstat.zfs.tank.dataset.objset-0x58c.nunlinks",
                "kstat.zfs.tank.dataset.objset-0x58c.nread",
                "kstat.zfs.tank.dataset.objset-0x58c.reads",
                "kstat.zfs.tank.dataset.objset-0x58c.nwritten",
                "kstat.zfs.tank.dataset.objset-0x58c.writes",
                "kstat.zfs.tank.dataset.objset-0x58c.dataset_name"
            ].into_iter();
            let values = vec![
                CtlValue::U64(1),
                CtlValue::U64(2),
                CtlValue::U64(3),
                CtlValue::U64(4),
                CtlValue::U64(5),
                CtlValue::U64(6),
                CtlValue::String("tank/foo".to_owned())
            ].into_iter();
            let mut builder = Builder::default();
            for (n, v) in names.zip(values) {
                builder.build(n, v);
            }
            let r = builder.finish();
            assert_eq!(r.name, "tank/foo");
            assert_eq!(r.nunlinked, 1);
            assert_eq!(r.nunlinks, 2);
            assert_eq!(r.nread, 3);
            assert_eq!(r.reads, 4);
            assert_eq!(r.nwritten, 5);
            assert_eq!(r.writes, 6);
        }
    }

    mod snapshot_iter {
        use super::super::*;

        #[test]
        fn like_freebsd_12_2()
        {
            let kv = vec![
                ("kstat.zfs.tank.dataset.objset-0x58c.nread".to_string(),
                CtlValue::U64(1)),
                ("kstat.zfs.tank.dataset.objset-0x58c.reads".to_string(),
                CtlValue::U64(2)),
                ("kstat.zfs.tank.dataset.objset-0x58c.nwritten".to_string(),
                CtlValue::U64(3)),
                ("kstat.zfs.tank.dataset.objset-0x58c.writes".to_string(),
                CtlValue::U64(4)),
                ("kstat.zfs.tank.dataset.objset-0x58c.dataset_name".to_string(),

                CtlValue::String("tank/foo".to_string())),
                ("kstat.zfs.tank.dataset.objset-0x58d.nread".to_string(),
                CtlValue::U64(11)),
                ("kstat.zfs.tank.dataset.objset-0x58d.reads".to_string(),
                CtlValue::U64(12)),
                ("kstat.zfs.tank.dataset.objset-0x58d.nwritten".to_string(),
                CtlValue::U64(13)),
                ("kstat.zfs.tank.dataset.objset-0x58d.writes".to_string(),
                CtlValue::U64(14)),
                ("kstat.zfs.tank.dataset.objset-0x58d.dataset_name".to_string(),

                CtlValue::String("tank/bar".to_string())),
            ].into_iter()
            .map(Ok);
            let mut iter = SnapshotIter::with_inner(kv);
            let ss = iter.next().unwrap().unwrap();
            assert_eq!(ss.name, "tank/foo");
            assert_eq!(ss.nunlinked, 0);
            assert_eq!(ss.nunlinks, 0);
            assert_eq!(ss.nread, 1);
            assert_eq!(ss.reads, 2);
            assert_eq!(ss.nwritten, 3);
            assert_eq!(ss.writes, 4);
            let ss = iter.next().unwrap().unwrap();
            assert_eq!(ss.name, "tank/bar");
            assert_eq!(ss.nunlinked, 0);
            assert_eq!(ss.nunlinks, 0);
            assert_eq!(ss.nread, 11);
            assert_eq!(ss.reads, 12);
            assert_eq!(ss.nwritten, 13);
            assert_eq!(ss.writes, 14);
            assert!(iter.next().is_none());
        }

        #[test]
        fn like_freebsd_13_0()
        {
            let kv = vec![
                ("kstat.zfs.tank.dataset.objset-0x58c.nunlinked".to_string(),
                CtlValue::U64(5)),
                ("kstat.zfs.tank.dataset.objset-0x58c.nunlinks".to_string(),
                CtlValue::U64(6)),
                ("kstat.zfs.tank.dataset.objset-0x58c.nread".to_string(),
                CtlValue::U64(1)),
                ("kstat.zfs.tank.dataset.objset-0x58c.reads".to_string(),
                CtlValue::U64(2)),
                ("kstat.zfs.tank.dataset.objset-0x58c.nwritten".to_string(),
                CtlValue::U64(3)),
                ("kstat.zfs.tank.dataset.objset-0x58c.writes".to_string(),
                CtlValue::U64(4)),
                ("kstat.zfs.tank.dataset.objset-0x58c.dataset_name".to_string(),

                CtlValue::String("tank/foo".to_string())),
                ("kstat.zfs.tank.dataset.objset-0x58d.nunlinked".to_string(),
                CtlValue::U64(15)),
                ("kstat.zfs.tank.dataset.objset-0x58d.nunlinks".to_string(),
                CtlValue::U64(16)),
                ("kstat.zfs.tank.dataset.objset-0x58d.nread".to_string(),
                CtlValue::U64(11)),
                ("kstat.zfs.tank.dataset.objset-0x58d.reads".to_string(),
                CtlValue::U64(12)),
                ("kstat.zfs.tank.dataset.objset-0x58d.nwritten".to_string(),
                CtlValue::U64(13)),
                ("kstat.zfs.tank.dataset.objset-0x58d.writes".to_string(),
                CtlValue::U64(14)),
                ("kstat.zfs.tank.dataset.objset-0x58d.dataset_name".to_string(),

                CtlValue::String("tank/bar".to_string())),
            ].into_iter()
            .map(Ok);
            let mut iter = SnapshotIter::with_inner(kv);
            let ss = iter.next().unwrap().unwrap();
            assert_eq!(ss.name, "tank/foo");
            assert_eq!(ss.nunlinked, 5);
            assert_eq!(ss.nunlinks, 6);
            assert_eq!(ss.nread, 1);
            assert_eq!(ss.reads, 2);
            assert_eq!(ss.nwritten, 3);
            assert_eq!(ss.writes, 4);
            let ss = iter.next().unwrap().unwrap();
            assert_eq!(ss.name, "tank/bar");
            assert_eq!(ss.nunlinked, 15);
            assert_eq!(ss.nunlinks, 16);
            assert_eq!(ss.nread, 11);
            assert_eq!(ss.reads, 12);
            assert_eq!(ss.nwritten, 13);
            assert_eq!(ss.writes, 14);
            assert!(iter.next().is_none());
        }
    }
}
