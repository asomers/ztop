// vim: tw=80
use cfg_if::cfg_if;
use ieee754::Ieee754;
use nix::{
    sys::time::TimeSpec,
    time::{ClockId, clock_gettime},
};
use regex::Regex;
use std::{
    collections::HashMap,
    error::Error,
    mem,
    num::NonZeroUsize,
    slice,
};

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
struct Snapshot {
    name: String,
    nunlinked: u64,
    nunlinks: u64,
    nread: u64,
    reads: u64,
    nwritten: u64,
    writes: u64,
}

impl Snapshot {
    fn compute(&self, prev: Option<&Self>, etime: f64) -> Element {
        if let Some(prev) = prev {
            Element {
                name: self.name.clone(),
                ops_r: (self.reads - prev.reads ) as f64 / etime,
                r_s: (self.nread - prev.nread ) as f64 / etime,
                ops_w: (self.writes - prev.writes ) as f64 / etime,
                w_s: (self.nwritten - prev.nwritten ) as f64 / etime,
                ops_d: (self.nunlinks - prev.nunlinks ) as f64 / etime,
                d_s: (self.nunlinked - prev.nunlinked ) as f64 / etime,
            }
        } else {
            Element {
                name: self.name.clone(),
                ops_r: self.reads as f64 / etime,
                r_s: self.nread as f64 / etime,
                ops_w: self.writes as f64 / etime,
                w_s: self.nwritten as f64 / etime,
                ops_d: self.nunlinks as f64 / etime,
                d_s: self.nunlinked as f64 / etime,
            }
        }
    }

    /// Iterate through ZFS datasets, returning stats for each.
    ///
    /// Iterates through every dataset beneath each of the given pools, or
    /// through all datasets if no pool is supplied.
    pub fn iter(pool: Option<&str>) -> Result<SnapshotIter, Box<dyn Error>> {
        SnapshotIter::new(pool)
    }
}

#[derive(Default)]
struct DataSource {
    prev: HashMap<String, Snapshot>,
    prev_ts: Option<TimeSpec>,
    cur: Vec<Snapshot>,
    cur_ts: Option<TimeSpec>,
    pools: Vec<String>
}

impl DataSource {
    fn new(pools: Vec<String>) -> Self {
        DataSource {
            pools,
            .. Default::default()
        }
    }

    /// Iterate through all the datasets, returning current stats
    fn iter(&mut self) -> impl Iterator<Item=Element> + '_ {
        let etime = if let Some(prev_ts) = self.prev_ts.as_ref() {
            let delta = *self.cur_ts.as_ref().unwrap() - *prev_ts;
            delta.tv_sec() as f64 + delta.tv_nsec() as f64 * 1e-9
        } else {
            let boottime = clock_gettime(ClockId::CLOCK_UPTIME).unwrap();
            boottime.tv_sec() as f64 + boottime.tv_nsec() as f64 * 1e-9
        };
        DataSourceIter {
            inner_iter: self.cur.iter(),
            ds: self,
            etime
        }
    }

    fn refresh(&mut self) -> Result<(), Box<dyn Error>> {
        let now = clock_gettime(ClockId::CLOCK_MONOTONIC)?;
        self.prev = mem::take(&mut self.cur)
            .into_iter()
            .map(|ss| (ss.name.clone(), ss))
            .collect();
        self.prev_ts = self.cur_ts.replace(now);
        if self.pools.is_empty() {
            for rss in Snapshot::iter(None).unwrap() {
                self.cur.push(rss?);
            }
        } else {
            for pool in self.pools.iter() {
                for rss in Snapshot::iter(Some(pool)).unwrap() {
                    self.cur.push(rss?);
                }
            }
        }
        Ok(())
    }
}

struct DataSourceIter<'a> {
    inner_iter: slice::Iter<'a, Snapshot>,
    ds: &'a DataSource,
    etime: f64
}

impl<'a> Iterator for DataSourceIter<'a> {
    type Item = Element;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner_iter.next()
            .map(|ss| ss.compute(self.ds.prev.get(&ss.name), self.etime))
    }
}

/// One thing to display in the table
#[derive(Clone, Debug)]
pub struct Element {
    pub name: String,
    /// Read IOPs
    pub ops_r: f64,
    /// Read B/s
    pub r_s: f64,
    /// Delete IOPs
    pub ops_d: f64,
    /// Delete B/s
    pub d_s: f64,
    /// Write IOPs
    pub ops_w: f64,
    /// Write B/s
    pub w_s: f64,
}

#[derive(Default)]
pub struct App {
    auto: bool,
    data: DataSource,
    depth: Option<NonZeroUsize>,
    filter: Option<Regex>,
    reverse: bool,
    should_quit: bool,
    /// 0-based index of the column to sort by, if any
    sort_idx: Option<usize>
}

impl App {
    pub fn new(
        auto: bool,
        pools: Vec<String>,
        depth: Option<NonZeroUsize>,
        filter: Option<Regex>
    ) -> Self {
        let mut data = DataSource::new(pools);
        data.refresh().unwrap();
        App {
            auto,
            data,
            depth,
            filter,
            .. Default::default()
        }
    }

    pub fn clear_filter(&mut self) {
        self.filter = None;
    }

    /// Return the elements that should be displayed, in order
    pub fn elements(&mut self) -> Vec<Element> {
        let auto = self.auto;
        let depth = self.depth;
        let filter = &self.filter;
        let mut v = self.data.iter()
            .filter(move |elem| {
                if let Some(limit) = depth {
                    let edepth = elem.name.split('/').count();
                    edepth <= limit.get()
                } else {
                    true
                }
            }).filter(|elem|
                 filter.as_ref()
                 .map(|f| f.is_match(&elem.name))
                 .unwrap_or(true)
            ).filter(|elem| !auto || (elem.r_s + elem.w_s + elem.d_s > 1.0))
            .collect::<Vec<_>>();
        match (self.reverse, self.sort_idx) {
            // TODO: when the total_cmp feature stabilities, use f64::total_cmp
            // instead.
            // https://github.com/rust-lang/rust/issues/72599
            (false, Some(0)) => v.sort_by(|x, y| Ieee754::total_cmp(&x.ops_r, &y.ops_r)),
            (true,  Some(0)) => v.sort_by(|x, y| Ieee754::total_cmp(&y.ops_r, &x.ops_r)),
            (false, Some(1)) => v.sort_by(|x, y| Ieee754::total_cmp(&x.r_s, &y.r_s)),
            (true,  Some(1)) => v.sort_by(|x, y| Ieee754::total_cmp(&y.r_s, &x.r_s)),
            (false, Some(2)) => v.sort_by(|x, y| Ieee754::total_cmp(&x.ops_w, &y.ops_w)),
            (true,  Some(2)) => v.sort_by(|x, y| Ieee754::total_cmp(&y.ops_w, &x.ops_w)),
            (false, Some(3)) => v.sort_by(|x, y| Ieee754::total_cmp(&x.w_s, &y.w_s)),
            (true,  Some(3)) => v.sort_by(|x, y| Ieee754::total_cmp(&y.w_s, &x.w_s)),
            (false, Some(4)) => v.sort_by(|x, y| Ieee754::total_cmp(&x.ops_d, &y.ops_d)),
            (true,  Some(4)) => v.sort_by(|x, y| Ieee754::total_cmp(&y.ops_d, &x.ops_d)),
            (false, Some(5)) => v.sort_by(|x, y| Ieee754::total_cmp(&x.d_s, &y.d_s)),
            (true,  Some(5)) => v.sort_by(|x, y| Ieee754::total_cmp(&y.d_s, &x.d_s)),
            (false, Some(6)) => v.sort_by(|x, y| x.name.cmp(&y.name)),
            (true,  Some(6)) => v.sort_by(|x, y| y.name.cmp(&x.name)),
            _ => ()
        }
        v
    }

    pub fn on_a(&mut self) {
        self.auto ^= true;
    }

    pub fn on_d(&mut self, more_depth: bool) {
        self.depth = if more_depth {
            match self.depth {
                None => NonZeroUsize::new(1),
                Some(x) => NonZeroUsize::new(x.get() + 1)
            }
        } else {
            match self.depth {
                None => None,
                Some(x) => NonZeroUsize::new(x.get() - 1)
            }
        }
    }

    pub fn on_minus(&mut self) {
        self.sort_idx = match self.sort_idx {
            Some(0) => None,
            Some(old) => Some(old - 1),
            None => Some(6)
        }
    }

    pub fn on_plus(&mut self) {
        self.sort_idx = match self.sort_idx {
            Some(old) if old >= 6 => None,
            Some(old) => Some(old + 1),
            None => Some(0)
        }
    }

    pub fn on_q(&mut self) {
        self.should_quit = true;
    }

    pub fn on_r(&mut self) {
        self.reverse ^= true;
    }

    pub fn on_tick(&mut self) {
        self.data.refresh().unwrap();
    }

    pub fn set_filter(&mut self, filter: Regex) {
        self.filter = Some(filter);
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn sort_idx(&self) -> Option<usize> {
        self.sort_idx
    }
}


