//! Process stats for the footer and `concordia doctor`: memory and CPU of Concordia itself and of
//! the provider processes it started (whole process trees).
//!
//! Sampling is Win32 (`GetProcessMemoryInfo`, `GetProcessTimes`, `CreateToolhelp32Snapshot`),
//! declared directly so no dependency is added. A background thread samples every
//! [`INTERVAL`] and publishes the last [`Stats`]; the UI thread only reads it. The thread exists
//! only while stats are switched on (`[ui] stats = true` or `/stats`). Provider turns and
//! delegated workers register their pid with [`register`]; the guard unregisters on drop.
//!
//! The functions below the platform layer are pure so the arithmetic is tested without a clock.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const INTERVAL: Duration = Duration::from_secs(2);

/// Snapshot of host capacity for the resource governor.
pub fn machine_resources(
    workspace: &std::path::Path,
) -> crate::resource_governor::MachineResources {
    let (total, free, cores) = sys::machine_memory_and_cores();
    crate::resource_governor::MachineResources {
        total_ram_bytes: total,
        free_ram_bytes: free,
        logical_cores: cores,
        free_disk_bytes: sys::free_disk_bytes(workspace).unwrap_or(u64::MAX),
    }
}

/// One published reading. Memory is working set in bytes; CPU is percent of the whole machine.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub self_mem: u64,
    pub self_cpu: f64,
    pub agents_mem: u64,
    pub agents_cpu: f64,
    pub agents_count: usize,
    /// False until the first sample lands.
    pub ready: bool,
}

// ---- registry of provider pids ------------------------------------------------------------

static AGENT_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Keeps a provider pid in the registry until dropped.
#[derive(Debug)]
pub struct PidGuard(u32);

impl Drop for PidGuard {
    fn drop(&mut self) {
        if let Ok(mut v) = AGENT_PIDS.lock() {
            if let Some(i) = v.iter().position(|p| *p == self.0) {
                v.swap_remove(i);
            }
        }
    }
}

/// Register a running provider process (pid 0 is ignored). Hold the guard while it runs.
pub fn register(pid: u32) -> PidGuard {
    if pid != 0 {
        if let Ok(mut v) = AGENT_PIDS.lock() {
            v.push(pid);
        }
    }
    PidGuard(pid)
}

pub fn registered() -> Vec<u32> {
    AGENT_PIDS.lock().map(|v| v.clone()).unwrap_or_default()
}

// ---- pure helpers -------------------------------------------------------------------------

/// CPU percent of the whole machine: `cpu_100ns` of process time over `wall`, on `ncpu` cores.
pub fn cpu_percent(cpu_100ns: u64, wall: Duration, ncpu: u32) -> f64 {
    let wall_100ns = wall.as_nanos() as f64 / 100.0;
    if wall_100ns <= 0.0 || ncpu == 0 {
        return 0.0;
    }
    (cpu_100ns as f64 / (wall_100ns * f64::from(ncpu)) * 100.0).clamp(0.0, 100.0)
}

/// `41 MB`, `1.2 GB`.
pub fn format_mem(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{} MB", mb.round() as u64)
    }
}

fn format_cpu(pct: f64) -> String {
    format!("{}%", pct.round() as u64)
}

/// Footer text: Concordia's own figures, and the agents segment when any run.
pub fn footer_parts(s: &Stats) -> (String, Option<String>) {
    let own = format!(
        "mem {} - cpu {}",
        format_mem(s.self_mem),
        format_cpu(s.self_cpu)
    );
    let agents = (s.agents_count > 0).then(|| {
        format!(
            "+ agents {} - {} ({})",
            format_mem(s.agents_mem),
            format_cpu(s.agents_cpu),
            s.agents_count
        )
    });
    (own, agents)
}

/// `roots` and every descendant, from `(pid, parent_pid)` pairs. Safe against pid reuse loops.
pub fn tree_pids(roots: &[u32], procs: &[(u32, u32)]) -> Vec<u32> {
    let mut seen: HashSet<u32> = roots.iter().copied().collect();
    let mut out: Vec<u32> = roots.to_vec();
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        for &(pid, ppid) in procs {
            if ppid == parent && pid != 0 && seen.insert(pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
}

// ---- Win32 --------------------------------------------------------------------------------

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;

    type Handle = *mut c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        lo: u32,
        hi: u32,
    }

    impl FileTime {
        fn ticks(self) -> u64 {
            (u64::from(self.hi) << 32) | u64::from(self.lo)
        }
    }

    #[repr(C)]
    struct MemCounters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }

    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        pid: u32,
        heap_id: usize,
        module_id: u32,
        threads: u32,
        parent_pid: u32,
        pri_class_base: i32,
        flags: u32,
        exe: [u16; 260],
    }

    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(h: Handle) -> i32;
        fn K32GetProcessMemoryInfo(h: Handle, c: *mut MemCounters, cb: u32) -> i32;
        fn GetProcessTimes(
            h: Handle,
            create: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Process32FirstW(h: Handle, e: *mut ProcessEntry) -> i32;
        fn Process32NextW(h: Handle, e: *mut ProcessEntry) -> i32;
        fn GetActiveProcessorCount(group: u16) -> u32;
        fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
        fn GetDiskFreeSpaceExW(
            path: *const u16,
            free: *mut u64,
            total: *mut u64,
            avail: *mut u64,
        ) -> i32;
    }
    #[repr(C)]
    struct MemoryStatus {
        length: u32,
        load: u32,
        total: u64,
        avail: u64,
        pages: u64,
        avail_pages: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended: u64,
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const TH32CS_SNAPPROCESS: u32 = 2;
    const INVALID: isize = -1;

    /// An open process handle, closed on drop. The pseudo handle of the current process is not
    /// closed.
    struct Proc(Handle, bool);

    impl Proc {
        fn open(pid: u32) -> Option<Proc> {
            if pid == std::process::id() {
                // SAFETY: GetCurrentProcess has no preconditions.
                return Some(Proc(unsafe { GetCurrentProcess() }, false));
            }
            // SAFETY: plain FFI call; a null result is handled.
            let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            (!h.is_null()).then_some(Proc(h, true))
        }
    }

    impl Drop for Proc {
        fn drop(&mut self) {
            if self.1 {
                // SAFETY: the handle came from OpenProcess and is closed once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    pub fn ncpu() -> u32 {
        // SAFETY: 0xFFFF is ALL_PROCESSOR_GROUPS.
        unsafe { GetActiveProcessorCount(0xFFFF) }.max(1)
    }
    pub fn machine_memory_and_cores() -> (u64, u64, u32) {
        let mut s = MemoryStatus {
            length: std::mem::size_of::<MemoryStatus>() as u32,
            load: 0,
            total: 0,
            avail: 0,
            pages: 0,
            avail_pages: 0,
            total_virtual: 0,
            avail_virtual: 0,
            avail_extended: 0,
        };
        unsafe {
            GlobalMemoryStatusEx(&mut s);
        }
        (s.total, s.avail, ncpu())
    }
    pub fn free_disk_bytes(path: &std::path::Path) -> Option<u64> {
        use std::os::windows::ffi::OsStrExt;
        let mut p: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut f = 0;
        (unsafe {
            GetDiskFreeSpaceExW(
                p.as_mut_ptr(),
                &mut f,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } != 0)
            .then_some(f)
    }

    /// Working set in bytes.
    pub fn working_set(pid: u32) -> Option<u64> {
        let p = Proc::open(pid)?;
        // SAFETY: the struct is zeroed, correctly sized and `cb` says so.
        unsafe {
            let mut c: MemCounters = std::mem::zeroed();
            c.cb = std::mem::size_of::<MemCounters>() as u32;
            (K32GetProcessMemoryInfo(p.0, &mut c, c.cb) != 0).then_some(c.working_set as u64)
        }
    }

    /// Kernel plus user time, in 100 ns ticks.
    pub fn cpu_ticks(pid: u32) -> Option<u64> {
        let p = Proc::open(pid)?;
        let (mut c, mut e, mut k, mut u) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        // SAFETY: four valid out pointers.
        let ok = unsafe { GetProcessTimes(p.0, &mut c, &mut e, &mut k, &mut u) };
        (ok != 0).then(|| k.ticks() + u.ticks())
    }

    /// Every process as `(pid, parent_pid)`.
    pub fn process_list() -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        // SAFETY: snapshot handle is checked and closed; entry is zeroed with `size` set.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap as isize == INVALID {
                return out;
            }
            let mut e: ProcessEntry = std::mem::zeroed();
            e.size = std::mem::size_of::<ProcessEntry>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                out.push((e.pid, e.parent_pid));
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
        out
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn ncpu() -> u32 {
        1
    }
    pub fn machine_memory_and_cores() -> (u64, u64, u32) {
        (0, 0, 1)
    }
    pub fn free_disk_bytes(_path: &std::path::Path) -> Option<u64> {
        None
    }
    pub fn working_set(_pid: u32) -> Option<u64> {
        None
    }
    pub fn cpu_ticks(_pid: u32) -> Option<u64> {
        None
    }
    pub fn process_list() -> Vec<(u32, u32)> {
        Vec::new()
    }
}

/// Working set of this process in bytes (`concordia doctor`).
pub fn self_memory_bytes() -> Option<u64> {
    sys::working_set(std::process::id())
}

/// Working set of `pid` and its descendants, with the number of processes found.
pub fn tree_memory_bytes(roots: &[u32]) -> (u64, usize) {
    let pids = tree_pids(roots, &sys::process_list());
    let mut total = 0;
    let mut n = 0;
    for p in pids {
        if let Some(m) = sys::working_set(p) {
            total += m;
            n += 1;
        }
    }
    (total, n)
}

// ---- sampler ------------------------------------------------------------------------------

/// Keeps the previous CPU times so each sample reports the delta over its interval.
pub struct Sampler {
    ncpu: u32,
    last: Option<Instant>,
    prev_self: Option<u64>,
    prev_agents: HashMap<u32, u64>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Sampler {
            ncpu: sys::ncpu(),
            last: None,
            prev_self: None,
            prev_agents: HashMap::new(),
        }
    }

    /// Sample this process and the trees of `roots`. The first call has no interval, so its CPU
    /// figures are 0.
    pub fn sample(&mut self, roots: &[u32]) -> Stats {
        let now = Instant::now();
        let wall = self.last.map(|t| now - t);
        self.last = Some(now);
        let me = std::process::id();

        let self_ticks = sys::cpu_ticks(me);
        let self_cpu = match (wall, self.prev_self, self_ticks) {
            (Some(w), Some(a), Some(b)) => cpu_percent(b.saturating_sub(a), w, self.ncpu),
            _ => 0.0,
        };
        self.prev_self = self_ticks;

        let pids = if roots.is_empty() {
            Vec::new()
        } else {
            tree_pids(roots, &sys::process_list())
        };
        let mut agents_mem = 0;
        let mut delta = 0u64;
        let mut current = HashMap::new();
        for p in &pids {
            if let Some(m) = sys::working_set(*p) {
                agents_mem += m;
            }
            if let Some(t) = sys::cpu_ticks(*p) {
                // A process that appeared since the last sample ran for at most one interval.
                let before = self
                    .prev_agents
                    .get(p)
                    .copied()
                    .unwrap_or(if wall.is_none() { t } else { 0 });
                delta += t.saturating_sub(before);
                current.insert(*p, t);
            }
        }
        self.prev_agents = current;
        let agents_cpu = wall.map_or(0.0, |w| cpu_percent(delta, w, self.ncpu));
        Stats {
            self_mem: sys::working_set(me).unwrap_or(0),
            self_cpu,
            agents_mem,
            agents_cpu,
            agents_count: roots.len(),
            ready: true,
        }
    }
}

/// A running sampler thread and what it last published. Dropping it stops the thread.
pub struct StatsHandle {
    latest: Arc<Mutex<Stats>>,
    stop: Arc<AtomicBool>,
}

impl StatsHandle {
    /// Start sampling every [`INTERVAL`] on a background thread.
    pub fn start() -> StatsHandle {
        let latest = Arc::new(Mutex::new(Stats::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (l, s) = (latest.clone(), stop.clone());
        let spawned = std::thread::Builder::new()
            .name("concordia-stats".into())
            .spawn(move || {
                let mut sampler = Sampler::new();
                while !s.load(Ordering::Relaxed) {
                    let stats = sampler.sample(&registered());
                    if let Ok(mut g) = l.lock() {
                        *g = stats;
                    }
                    // Sleep in short steps so a switch-off is noticed quickly.
                    let until = Instant::now() + INTERVAL;
                    while Instant::now() < until && !s.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            });
        drop(spawned);
        StatsHandle { latest, stop }
    }

    /// A handle with a fixed reading and no thread (tests).
    pub fn fixed(stats: Stats) -> StatsHandle {
        StatsHandle {
            latest: Arc::new(Mutex::new(stats)),
            stop: Arc::new(AtomicBool::new(true)),
        }
    }

    /// The last sample (default, not ready, before the first lands).
    pub fn latest(&self) -> Stats {
        self.latest.lock().map(|g| *g).unwrap_or_default()
    }
}

impl Drop for StatsHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_is_normalised_by_cores() {
        // One full core for 2 s on 4 cores is 25 %.
        let one_core_2s = 2 * 10_000_000;
        let p = cpu_percent(one_core_2s, Duration::from_secs(2), 4);
        assert!((p - 25.0).abs() < 0.01, "{p}");
        assert_eq!(cpu_percent(5, Duration::ZERO, 4), 0.0);
        assert_eq!(cpu_percent(5, Duration::from_secs(1), 0), 0.0);
        // Never above 100 even with a skewed clock.
        assert_eq!(cpu_percent(u64::MAX / 4, Duration::from_secs(1), 1), 100.0);
    }

    #[test]
    fn memory_and_footer_formatting() {
        assert_eq!(format_mem(41 * 1024 * 1024), "41 MB");
        assert_eq!(format_mem(1536 * 1024 * 1024), "1.5 GB");
        let s = Stats {
            self_mem: 41 << 20,
            self_cpu: 1.2,
            agents_mem: 380 << 20,
            agents_cpu: 5.6,
            agents_count: 2,
            ready: true,
        };
        let (own, agents) = footer_parts(&s);
        assert_eq!(own, "mem 41 MB - cpu 1%");
        assert_eq!(agents.as_deref(), Some("+ agents 380 MB - 6% (2)"));
        let idle = Stats {
            agents_count: 0,
            ..s
        };
        assert!(footer_parts(&idle).1.is_none());
    }

    #[test]
    fn tree_pids_follow_children_and_survive_loops() {
        let procs = [(2, 1), (3, 2), (4, 2), (5, 9), (1, 3)];
        let mut t = tree_pids(&[2], &procs);
        t.sort_unstable();
        assert_eq!(t, vec![1, 2, 3, 4]);
        assert_eq!(tree_pids(&[7], &procs), vec![7]);
    }

    #[test]
    fn registry_guard_unregisters_on_drop() {
        // Pids no real process uses in this test, so parallel tests do not interfere.
        let a = register(4_000_001);
        let b = register(4_000_002);
        let _zero = register(0);
        assert!(registered().contains(&4_000_001) && registered().contains(&4_000_002));
        drop(a);
        assert!(!registered().contains(&4_000_001));
        assert!(registered().contains(&4_000_002));
        drop(b);
        assert!(!registered().contains(&4_000_002));
        assert!(!registered().contains(&0));
    }

    #[cfg(windows)]
    #[test]
    fn sampling_self_returns_plausible_memory() {
        let mem = self_memory_bytes().expect("working set");
        assert!(mem > 1 << 20 && mem < 64 << 30, "{mem}");
        let mut s = Sampler::new();
        let first = s.sample(&[]);
        assert!(first.ready && first.self_mem > 0 && first.agents_count == 0);
        // Burn a little CPU so the second sample has a non-negative, bounded figure.
        let t = Instant::now();
        let mut x = 0u64;
        while t.elapsed() < Duration::from_millis(60) {
            x = x.wrapping_add(1);
        }
        std::hint::black_box(x);
        let second = s.sample(&[]);
        assert!((0.0..=100.0).contains(&second.self_cpu));
    }

    #[cfg(windows)]
    #[test]
    fn tree_enumeration_finds_a_spawned_child_and_its_memory() {
        let mut child = crate::process_ui::command("ping")
            .args(["-n", "4", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("ping");
        let me = std::process::id();
        let tree = tree_pids(&[me], &sys::process_list());
        assert!(tree.contains(&child.id()), "child not in {tree:?}");
        let (mem, n) = tree_memory_bytes(&[child.id()]);
        assert!(n >= 1 && mem > 0);
        let g = register(child.id());
        let mut s = Sampler::new();
        s.sample(&registered());
        let st = s.sample(&registered());
        assert!(st.agents_count >= 1);
        assert!(st.agents_mem > 0);
        drop(g);
        let _ = child.kill();
        let _ = child.wait();
    }
}
