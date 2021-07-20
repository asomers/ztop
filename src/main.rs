// vim: tw=80
use cfg_if::cfg_if;
use std::error::Error;

cfg_if! {
    if #[cfg(target_os = "freebsd")] {
        mod freebsd;
        use freebsd::{SnapshotIter};
    }
}


/// A snapshot in time of a dataset's statistics.
///
/// The various fields are not saved atomically, but ought to be close.
#[derive(Clone, Debug)]
pub struct Snapshot {
    name: String,
    nunlinked: u64,
    nunlinks: u64,
    nread: u64,
    reads: u64,
    nwritten: u64,
    writes: u64,
}

impl Snapshot {
    /// Iterate through all ZFS datasets, returning stats for each.
    pub fn iter() -> Result<SnapshotIter, Box<dyn Error>> {
        SnapshotIter::new()
    }
}


fn main() -> Result<(), Box<dyn Error>> {
    println!("{:40} {:>13} {:>10} {:>13} {:>10} {:>10} {:>10}",
             "name",
             "bytes read",
             "read ops",
             "bytes written",
             "write ops",
             "bytes freed",
             "free ops"
    );
    for rss in Snapshot::iter().unwrap() {
        let ss = rss.unwrap();
        println!("{:40} {:13} {:10} {:13} {:10} {:10} {:10}",
                 ss.name,
                 ss.nread,
                 ss.reads,
                 ss.nwritten,
                 ss.writes,
                 ss.nunlinked,
                 ss.nunlinks,
         );
    }
    Ok(())
}
