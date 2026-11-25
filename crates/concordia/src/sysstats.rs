use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const INTERVAL: Duration = Duration::from_secs(2);

pub fn machine_resources(
    _workspace: &std::path::Path,
) -> crate::resource_governor::MachineResources {
    crate::resource_governor::MachineResources {
        logical_cores: sys::ncpu(),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub self_mem: u64,
    pub self_cpu: f64,
    pub agents_mem: u64,
    pub agents_cpu: f64,
    pub agents_count: usize,
    pub ready: bool,
}

static AGENT_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

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

pub fn cpu_percent(cpu_100ns: u64, wall: Duration, ncpu: u32) -> f64 {
    let wall_100ns = wall.as_nanos() as f64 / 100.0;
    if wall_100ns <= 0.0 || ncpu == 0 {
        return 0.0;
    }
    (cpu_100ns as f64 / (wall_100ns * f64::from(ncpu)) * 100.0).clamp(0.0, 100.0)
}

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
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const TH32CS_SNAPPROCESS: u32 = 2;
    const INVALID: isize = -1;

    struct Proc(Handle, bool);

    impl Proc {
        fn open(pid: u32) -> Option<Proc> {
            if pid == std::process::id() {
                return Some(Proc(unsafe { GetCurrentProcess() }, false));
            }
            let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            (!h.is_null()).then_some(Proc(h, true))
        }
    }

    impl Drop for Proc {
        fn drop(&mut self) {
            if self.1 {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    pub fn ncpu() -> u32 {
        unsafe { GetActiveProcessorCount(0xFFFF) }.max(1)
    }
    pub fn working_set(pid: u32) -> Option<u64> {
        let p = Proc::open(pid)?;
        unsafe {
            let mut c: MemCounters = std::mem::zeroed();
            c.cb = std::mem::size_of::<MemCounters>() as u32;
            (K32GetProcessMemoryInfo(p.0, &mut c, c.cb) != 0).then_some(c.working_set as u64)
        }
    }

    pub fn cpu_ticks(pid: u32) -> Option<u64> {
        let p = Proc::open(pid)?;
        let (mut c, mut e, mut k, mut u) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        let ok = unsafe { GetProcessTimes(p.0, &mut c, &mut e, &mut k, &mut u) };
        (ok != 0).then(|| k.ticks() + u.ticks())
    }

    pub fn process_list() -> Vec<(u32, u32)> {
        let mut out = Vec::new();
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

pub fn process_pairs() -> Vec<(u32, u32)> {
    sys::process_list()
}

pub fn self_memory_bytes() -> Option<u64> {
    sys::working_set(std::process::id())
}

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

pub struct StatsHandle {
    latest: Arc<Mutex<Stats>>,
    stop: Arc<AtomicBool>,
}

impl StatsHandle {
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
                    let until = Instant::now() + INTERVAL;
                    while Instant::now() < until && !s.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            });
        drop(spawned);
        StatsHandle { latest, stop }
    }

    pub fn fixed(stats: Stats) -> StatsHandle {
        StatsHandle {
            latest: Arc::new(Mutex::new(stats)),
            stop: Arc::new(AtomicBool::new(true)),
        }
    }

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
        let one_core_2s = 2 * 10_000_000;
        let p = cpu_percent(one_core_2s, Duration::from_secs(2), 4);
        assert!((p - 25.0).abs() < 0.01, "{p}");
        assert_eq!(cpu_percent(5, Duration::ZERO, 4), 0.0);
        assert_eq!(cpu_percent(5, Duration::from_secs(1), 0), 0.0);
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
