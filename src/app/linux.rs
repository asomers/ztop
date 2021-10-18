// vim: tw=80
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    fs,
    io,
    path::{Path, PathBuf},
};

use super::Snapshot;

/// SnapshotIter allocates a vector of the paths to objsets
/// under the desired pools on initialization.  Reading the
/// objsets into Snapshots is lazy.
pub(super) struct SnapshotIter {
    idx:     usize,
    objsets: Vec<PathBuf>,
}

/// This is the default filepath for zfs stats using ZoL.
const DEFAULT_ZFS_STATS_PATH: &str = "/proc/spl/kstat/zfs";

impl SnapshotIter {
    /// Simply use the default path to the stats files in procfs.  A value of `None`
    /// for `pool` will get statistics for all pools.
    pub(crate) fn new(pool: Option<&str>) -> Result<Self, Box<dyn Error>> {
        Self::new_from_basepath(DEFAULT_ZFS_STATS_PATH, pool)
    }

    // Useful for testing on a mock directory.
    fn new_from_basepath(
        basepath: &str,
        pool: Option<&str>,
    ) -> Result<SnapshotIter, Box<dyn Error>> {
        let objsets = Self::required_pools_from_basepath(basepath, pool)?
            .into_iter()
            .flat_map(Self::enumerate_pool)
            .collect::<Vec<PathBuf>>();
        Ok(SnapshotIter { idx: 0, objsets })
    }

    // Get all objset paths for a given pool.
    // At this point all the paths should exist, because they have been
    // checked by `required_pools_from_basepath`.  Should this somehow be
    // expressed as an invariant?
    fn enumerate_pool(pool: PathBuf) -> Vec<PathBuf> {
        let is_dataset = |entry: &fs::DirEntry| {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_str().unwrap_or("");
            path.is_file() && name.starts_with("objset")
        };
        fs::read_dir(pool).map_or(vec![], |dir| {
            dir.filter_map(|e| {
                e.map_or(None, |entry| {
                    if is_dataset(&entry) {
                        Some(entry.path())
                    } else {
                        None
                    }
                })
            })
            .collect()
        })
    }

    fn required_pools_from_basepath(
        basepath: &str,
        pool: Option<&str>,
    ) -> Result<HashSet<PathBuf>, Box<dyn Error>> {
        let zfs_stats_path = PathBuf::from(basepath);
        let mut pools = SnapshotIter::get_pools(zfs_stats_path.as_path())
            .map_or_else(|err| Err(Box::new(err)), Ok)?;
        if let Some(pool) = pool {
            let pool: PathBuf =
                [zfs_stats_path, PathBuf::from(pool)].iter().collect();
            pools.retain(|p| p == &pool);
            if pools.is_empty() {
                let name = pool
                    .file_name()
                    .map(|ostr| ostr.to_str().unwrap_or(""))
                    .unwrap_or("")
                    .to_string();
                return Err(Box::new(ZTopError::PoolDoesNotExist {
                    pool: name,
                }));
            }
        }
        Ok(pools)
    }

    fn get_pools(zfs_stat_path: &Path) -> io::Result<HashSet<PathBuf>> {
        fs::read_dir(zfs_stat_path).map(|dir| {
            dir.filter_map(|entry| {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_dir() {
                        Some(path)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<HashSet<PathBuf>>()
        })
    }
}

impl Iterator for SnapshotIter {
    type Item = Result<Snapshot, Box<dyn Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.idx += 1;
        self.objsets.get(self.idx - 1).map(|objset| {
            fs::read_to_string(objset)
                .map_err(|err| Box::new(err) as Box<dyn Error>)
                .and_then(|data| {
                    parse_snapshot(data.as_ref())
                        .map_err(|err| Box::new(err) as Box<dyn Error>)
                })
        })
    }
}

fn snapshot_from_hash_map<'a>(
    stats: &'a HashMap<&str, SnapshotParseData>,
) -> Option<Snapshot> {
    use SnapshotParseData::{Name, Number};
    let get_name = |data: &'a SnapshotParseData| match data {
        Name(name) => Some(name),
        _ => None,
    };
    let get_number = |data: &'a SnapshotParseData| match data {
        Number(n) => Some(*n),
        _ => None,
    };
    let name = stats.get("dataset_name").and_then(get_name)?.to_string();
    let nunlinked = stats.get("nunlinked").and_then(get_number)?;
    let nunlinks = stats.get("nunlinks").and_then(get_number)?;
    let nread = stats.get("nread").and_then(get_number)?;
    let reads = stats.get("reads").and_then(get_number)?;
    let nwritten = stats.get("nwritten").and_then(get_number)?;
    let writes = stats.get("writes").and_then(get_number)?;
    Some(Snapshot {
        name,
        nunlinked,
        nunlinks,
        nread,
        reads,
        nwritten,
        writes,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum ZTopError {
    PoolDoesNotExist { pool: String },
    SnapshotParseError { src: String },
}

impl Error for ZTopError {}

impl fmt::Display for ZTopError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ZTopError::SnapshotParseError { src } => {
                write!(f, "Failed to parse Snapshot from string:\n {}", src)
            }
            ZTopError::PoolDoesNotExist { pool } => {
                write!(f, "ZFS pool does not exist: {}", pool)
            }
        }
    }
}

#[derive(Debug)]
enum SnapshotParseData<'a> {
    Name(&'a str),
    Number(u64),
}

fn parse_snapshot(s: &str) -> Result<Snapshot, ZTopError> {
    use SnapshotParseData::{Name, Number};
    let mut stats = HashMap::new();
    for row in s.split('\n') {
        let fields: Vec<_> = row.split_whitespace().collect();
        match fields[..] {
            [name, _, data] if name != "name" => {
                let data =
                    data.parse::<u64>().map_or_else(|_| Name(data), Number);
                stats.insert(name, data);
            }
            _ => {}
        }
    }
    if let Some(snap) = snapshot_from_hash_map(&stats) {
        Ok(snap)
    } else {
        Err(ZTopError::SnapshotParseError { src: s.to_owned() })
    }
}

#[cfg(test)]
mod t {
    mod parsing {
        use super::super::*;

        const OBJSET: [&str; 9] = [
            "26 1 0x01 7 2160 15278395714 17176723400350",
            "name                            type data",
            "dataset_name                    7    tank",
            "writes                          4    14",
            "nwritten                        4    256",
            "reads                           4    100",
            "nread                           4    1024",
            "nunlinks                        4    1",
            "nunlinked                       4    4",
        ];

        /// Empty string is insufficient data
        #[test]
        fn empty() {
            assert!(parse_snapshot("").is_err());
        }

        /// Parses full objset content including the header
        #[test]
        fn full_objset() {
            let objset =
                OBJSET.iter().copied().collect::<Vec<&str>>().join("\n");
            let expected: Result<Snapshot, ZTopError> = Ok(Snapshot {
                name:      "tank".to_owned(),
                nunlinked: 4,
                nunlinks:  1,
                nread:     1024,
                reads:     100,
                nwritten:  256,
                writes:    14,
            });
            assert_eq!(expected, parse_snapshot(&objset));
        }

        /// Parses without the header lines (just in case)
        #[test]
        fn without_header() {
            let objset = &OBJSET[2..9]
                .iter()
                .copied()
                .collect::<Vec<&str>>()
                .join("\n");
            let expected: Result<Snapshot, ZTopError> = Ok(Snapshot {
                name:      "tank".to_owned(),
                nunlinked: 4,
                nunlinks:  1,
                nread:     1024,
                reads:     100,
                nwritten:  256,
                writes:    14,
            });
            assert_eq!(expected, parse_snapshot(&objset));
        }

        /// The order of the file/data doesn't matter.
        #[test]
        fn out_of_order() {
            let mut objset = OBJSET.iter().copied().collect::<Vec<&str>>();
            objset.reverse();
            let objset = objset.join("\n");
            let expected: Result<Snapshot, ZTopError> = Ok(Snapshot {
                name:      "tank".to_owned(),
                nunlinked: 4,
                nunlinks:  1,
                nread:     1024,
                reads:     100,
                nwritten:  256,
                writes:    14,
            });
            assert_eq!(expected, parse_snapshot(&objset));
        }

        /// Missing field fails parsing
        ///
        /// We should probably have a test for each field missing?
        #[test]
        fn missing_fields() {
            // Leave off the end field.
            let objset = &OBJSET[2..8]
                .iter()
                .copied()
                .collect::<Vec<&str>>()
                .join("\n");
            assert!(parse_snapshot(objset).is_err());
            // Strip the dataset_name field.
            let objset = &OBJSET[3..9]
                .iter()
                .copied()
                .collect::<Vec<&str>>()
                .join("\n");
            assert!(parse_snapshot(objset).is_err());
        }
    }

    mod read_stats {
        use super::super::*;
        const MOCK_DIR: &str = "./test-data/linux/zfs";

        fn tank_set() -> HashSet<Snapshot> {
            [
                Snapshot {
                    name:      "tank/vm/chimera".to_string(),
                    writes:    2391634,
                    nwritten:  3953804,
                    reads:     55453,
                    nread:     2032404,
                    nunlinks:  451747,
                    nunlinked: 1441696,
                },
                Snapshot {
                    name:      "tank/vm".to_string(),
                    writes:    854952,
                    nwritten:  3762136,
                    reads:     3020704,
                    nread:     2502570,
                    nunlinks:  1512800,
                    nunlinked: 3014868,
                },
                Snapshot {
                    name:      "tank/vm/steam".to_string(),
                    writes:    2684173,
                    nwritten:  2951911,
                    reads:     794351,
                    nread:     3313516,
                    nunlinks:  1671663,
                    nunlinked: 2998217,
                },
                Snapshot {
                    name:      "tank/backups".to_string(),
                    writes:    57307,
                    nwritten:  3688922381,
                    reads:     3910907,
                    nread:     2953618,
                    nunlinks:  11364,
                    nunlinked: 11364,
                },
                Snapshot {
                    name:      "tank".to_string(),
                    writes:    314246,
                    nwritten:  3891438,
                    reads:     2348128,
                    nread:     3148052,
                    nunlinks:  1180639,
                    nunlinked: 2072994,
                },
            ]
            .iter()
            .cloned()
            .collect::<HashSet<Snapshot>>()
        }

        /// Reading one pool returns its datasets
        #[test]
        fn read_one_pool() {
            let expected = tank_set();
            let snaps = SnapshotIter::new_from_basepath(MOCK_DIR, Some("tank"));

            println!("Current directory is {:?}", std::env::current_dir());
            assert!(snaps.is_ok());
            let actual = snaps
                .unwrap()
                .filter_map(|res| res.map_or(None, Some))
                .collect::<HashSet<Snapshot>>();
            assert_eq!(actual, expected);
        }
    }
}
