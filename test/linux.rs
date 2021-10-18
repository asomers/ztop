cfg_if! {
    if #[cfg(target_os = "linux")] {
        mod linux_tests {
            use super::super::*;

            const MOCK_DIR: &str = "linux/data/zfs";

            fn tank_set() -> HashSet<Snapshot> {
                [
                    Snapshot {
                        name: "tank/vm/chimera".to_string(),
                        writes: 2391634,
                        nwritten: 3953804,
                        reads: 55453,
                        nread: 2032404,
                        nunlinks: 451747,
                        nunlinked: 1441696,
                    },
                    Snapshot {
                        name: "tank/vm".to_string(),
                        writes: 854952,
                        nwritten: 3762136,
                        reads: 3020704,
                        nread: 2502570,
                        nunlinks: 1512800,
                        nunlinked: 3014868,
                    },
                    Snapshot {
                        name: "tank/vm/steam".to_string(),
                        writes: 2684173,
                        nwritten: 2951911,
                        reads: 794351,
                        nread: 3313516,
                        nunlinks: 1671663,
                        nunlinked: 2998217,
                    },
                    Snapshot {
                        name: "tank/backups".to_string(),
                        writes: 57307,
                        nwritten: 3688922381,
                        reads: 3910907,
                        nread: 2953618,
                        nunlinks: 11364,
                        nunlinked: 11364,
                    },
                    Snapshot {
                        name: "tank".to_string(),
                        writes: 314246,
                        nwritten: 3891438,
                        reads: 2348128,
                        nread: 3148052,
                        nunlinks: 1180639,
                        nunlinked: 2072994,
                    }].iter().cloned().collect::<HashSet<Snapshot>>()
            }

            /// Reading one pool returns its datasets
            #[test]
            fn read_one_pool() {
                let expected = tank_set();
                let snaps = SnapshotIter::new_from_basepath(MOCK_DIR, Some("tank"));
                assert!(snaps.is_ok());
                let actual = snaps
                    .unwrap()
                    .filter_map(|res| res.map_or(None, Some))
                    .collect::<HashSet<Snapshot>>();
                assert_eq!(actual, expected);
            }
        }
    }
}
