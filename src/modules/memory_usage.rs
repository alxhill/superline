//! The current machine's memory usage.
//!
//! Memory is read directly from the operating system rather than through a
//! command such as `free` or `vm_stat`. The read is a small, local operation,
//! so it does not need the asynchronous cache used by network and repository
//! lookups.

use std::marker::PhantomData;

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::themes::DefaultColors;
use crate::utils::join_non_empty;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

/// A fraction of a percent is too noisy to call out in a prompt. Round-down
/// percentage formatting means this also keeps a displayed `0%` out of the
/// swap lane.
const SWAP_DISPLAY_THRESHOLD_PERCENT: u8 = 1;

/// A snapshot of physical and swap memory, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    total: u64,
    available: u64,
    swap_total: u64,
    swap_available: u64,
}

impl MemoryStats {
    fn used(self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    fn swap_used(self) -> u64 {
        self.swap_total.saturating_sub(self.swap_available)
    }
}

/// Theme hooks for the memory segment.
pub trait MemoryUsageScheme: DefaultColors {
    /// The Nerd Font `memory` glyph.
    const MEMORY_USAGE_ICON: &'static str = "\u{f035b}";

    fn memory_usage_fg() -> Color {
        Self::alert_fg()
    }

    fn memory_usage_bg() -> Color {
        Self::alert_bg()
    }

    fn memory_usage_icon() -> &'static str {
        Self::MEMORY_USAGE_ICON
    }
}

pub struct MemoryUsage<S: MemoryUsageScheme> {
    threshold: Option<u8>,
    scheme: PhantomData<S>,
}

impl<S: MemoryUsageScheme> Default for MemoryUsage<S> {
    fn default() -> Self {
        Self::new(None)
    }
}

impl<S: MemoryUsageScheme> MemoryUsage<S> {
    pub fn new(threshold: Option<u8>) -> Self {
        Self {
            threshold,
            scheme: PhantomData,
        }
    }
}

impl<S: MemoryUsageScheme> Module for MemoryUsage<S> {
    /// The system's memory, if the operating system reports it.
    type Data = Option<MemoryStats>;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn fetch(&self) -> Option<MemoryStats> {
        system_memory()
    }

    /// 16 GiB of RAM with the larger of 72% and the threshold in use, plus a
    /// little swap.
    fn sample(&self) -> Option<MemoryStats> {
        const GIB: u64 = 1024 * 1024 * 1024;
        let used_percent = u64::from(self.threshold.unwrap_or(0).clamp(72, 100));
        Some(MemoryStats {
            total: 16 * GIB,
            available: 16 * GIB * (100 - used_percent) / 100,
            swap_total: 4 * GIB,
            swap_available: 4 * GIB * 95 / 100,
        })
    }

    fn render(&self, stats: Option<MemoryStats>, powerline: &mut Powerline) {
        let Some(stats) = stats else {
            return;
        };

        if let Some(text) = format_memory(S::memory_usage_icon(), stats, self.threshold) {
            powerline.add_segment(
                text,
                Style::simple(S::memory_usage_fg(), S::memory_usage_bg()),
            );
        }
    }
}

fn format_memory(icon: &str, stats: MemoryStats, threshold: Option<u8>) -> Option<String> {
    let ram_percent = usage_percent(stats.used(), stats.total)?;
    if threshold.is_some_and(|threshold| ram_percent < threshold) {
        return None;
    }
    let mut text = join_non_empty([icon, format!("{ram_percent}%").as_str()]);

    if let Some(swap_percent) = meaningful_swap_percent(stats) {
        text.push_str(&format!(" | swap {swap_percent}%"));
    }

    Some(text)
}

fn meaningful_swap_percent(stats: MemoryStats) -> Option<u8> {
    usage_percent(stats.swap_used(), stats.swap_total)
        .filter(|percent| *percent >= SWAP_DISPLAY_THRESHOLD_PERCENT)
}

/// Return a whole-number percentage without floating-point rounding at the
/// visibility boundary. A zero total is not a usable reading.
fn usage_percent(used: u64, total: u64) -> Option<u8> {
    (total > 0).then(|| {
        (u128::from(used.min(total)) * 100 / u128::from(total))
            .min(100)
            .try_into()
            .expect("a clamped percentage always fits in u8")
    })
}

#[cfg(target_os = "linux")]
fn system_memory() -> Option<MemoryStats> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo(&contents)
}

#[cfg(target_os = "macos")]
fn system_memory() -> Option<MemoryStats> {
    use std::ffi::c_void;
    use std::mem::{size_of, MaybeUninit};
    use std::ptr;

    let total = sysctl_u64(c"hw.memsize")?;

    let mut vm_stat = MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    #[allow(deprecated)]
    let host = unsafe { libc::mach_host_self() };
    let result = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            vm_stat.as_mut_ptr().cast::<libc::integer_t>(),
            &mut count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return None;
    }
    let vm_stat = unsafe { vm_stat.assume_init() };
    let page_size = unsafe { libc::vm_page_size as u64 };
    let available_pages = vm_stat
        .free_count
        .saturating_add(vm_stat.inactive_count)
        .saturating_add(vm_stat.speculative_count);
    let available = u64::from(available_pages).saturating_mul(page_size);

    let mut swap = MaybeUninit::<libc::xsw_usage>::zeroed();
    let mut swap_size = size_of::<libc::xsw_usage>();
    let swap_result = unsafe {
        libc::sysctlbyname(
            c"vm.swapusage".as_ptr(),
            swap.as_mut_ptr().cast::<c_void>(),
            &mut swap_size,
            ptr::null_mut(),
            0,
        )
    };
    let (swap_total, swap_available) = if swap_result == 0 {
        let swap = unsafe { swap.assume_init() };
        (swap.xsu_total, swap.xsu_avail)
    } else {
        (0, 0)
    };

    Some(MemoryStats {
        total,
        available: available.min(total),
        swap_total,
        swap_available: swap_available.min(swap_total),
    })
}

#[cfg(target_os = "macos")]
fn sysctl_u64(name: &std::ffi::CStr) -> Option<u64> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr;

    let mut value = 0_u64;
    let mut size = size_of::<u64>();
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast::<c_void>(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    (result == 0 && size == size_of::<u64>()).then_some(value)
}

#[cfg(target_os = "windows")]
fn system_memory() -> Option<MemoryStats> {
    #[repr(C)]
    #[allow(non_snake_case)]
    struct MemoryStatusEx {
        dwLength: u32,
        dwMemoryLoad: u32,
        ullTotalPhys: u64,
        ullAvailPhys: u64,
        ullTotalPageFile: u64,
        ullAvailPageFile: u64,
        ullTotalVirtual: u64,
        ullAvailVirtual: u64,
        ullAvailExtendedVirtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
    }

    let mut status = MemoryStatusEx {
        dwLength: std::mem::size_of::<MemoryStatusEx>() as u32,
        dwMemoryLoad: 0,
        ullTotalPhys: 0,
        ullAvailPhys: 0,
        ullTotalPageFile: 0,
        ullAvailPageFile: 0,
        ullTotalVirtual: 0,
        ullAvailVirtual: 0,
        ullAvailExtendedVirtual: 0,
    };

    let success = unsafe { GlobalMemoryStatusEx(&mut status) != 0 };
    success.then_some(MemoryStats {
        total: status.ullTotalPhys,
        available: status.ullAvailPhys,
        swap_total: status.ullTotalPageFile.saturating_sub(status.ullTotalPhys),
        swap_available: status.ullAvailPageFile.saturating_sub(status.ullAvailPhys),
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn system_memory() -> Option<MemoryStats> {
    None
}

#[cfg(target_os = "linux")]
fn parse_meminfo(contents: &str) -> Option<MemoryStats> {
    let mut total = None;
    let mut available = None;
    let mut free = None;
    let mut swap_total = None;
    let mut swap_free = None;

    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(key) = fields.next().and_then(|key| key.strip_suffix(':')) else {
            continue;
        };
        let Some(value) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        let multiplier = match fields.next().unwrap_or("B") {
            "B" => 1,
            "kB" => 1024,
            "MB" => 1024 * 1024,
            "GB" => 1024 * 1024 * 1024,
            _ => continue,
        };
        let Some(value) = value.checked_mul(multiplier) else {
            continue;
        };

        match key {
            "MemTotal" => total = Some(value),
            "MemAvailable" => available = Some(value),
            "MemFree" => free = Some(value),
            "SwapTotal" => swap_total = Some(value),
            "SwapFree" => swap_free = Some(value),
            _ => {}
        }
    }

    let total = total?;
    Some(MemoryStats {
        total,
        available: available.or(free)?.min(total),
        swap_total: swap_total.unwrap_or(0),
        swap_available: swap_free.unwrap_or(0).min(swap_total.unwrap_or(0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: &str = "\u{f035b}";

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            crate::colors::black()
        }

        fn default_fg() -> Color {
            crate::colors::white()
        }
    }

    impl MemoryUsageScheme for TestTheme {}

    fn stats(ram_used: u64, ram_total: u64, swap_used: u64, swap_total: u64) -> MemoryStats {
        MemoryStats {
            total: ram_total,
            available: ram_total.saturating_sub(ram_used),
            swap_total,
            swap_available: swap_total.saturating_sub(swap_used),
        }
    }

    #[test]
    fn hides_memory_below_threshold_and_shows_at_threshold() {
        assert_eq!(format_memory(ICON, stats(74, 100, 0, 0), Some(75)), None);
        assert_eq!(
            format_memory(ICON, stats(75, 100, 0, 0), Some(75)),
            Some("\u{f035b} 75%".into())
        );
        assert_eq!(
            format_memory(ICON, stats(76, 100, 0, 0), Some(75)),
            Some("\u{f035b} 76%".into())
        );
        assert_eq!(
            format_memory(ICON, stats(1, 100, 0, 0), None),
            Some("\u{f035b} 1%".into())
        );
    }

    #[test]
    fn includes_swap_only_when_at_least_one_percent_is_used() {
        assert_eq!(meaningful_swap_percent(stats(80, 100, 0, 100)), None);
        assert_eq!(meaningful_swap_percent(stats(80, 100, 1, 100)), Some(1));
        assert_eq!(
            format_memory(ICON, stats(80, 100, 0, 100), None),
            Some("\u{f035b} 80%".into())
        );
        assert_eq!(
            format_memory(ICON, stats(80, 100, 1, 100), None),
            Some("\u{f035b} 80% | swap 1%".into())
        );
        assert_eq!(
            format_memory(ICON, stats(80, 100, 100, 100), None),
            Some("\u{f035b} 80% | swap 100%".into())
        );
    }

    #[test]
    fn an_empty_icon_leaves_only_the_percentages() {
        assert_eq!(
            format_memory("", stats(80, 100, 1, 100), None),
            Some("80% | swap 1%".into())
        );
    }

    #[test]
    fn sample_is_shown_whatever_the_threshold() {
        for threshold in [None, Some(50), Some(90), Some(100)] {
            let stats = MemoryUsage::<TestTheme>::new(threshold)
                .sample()
                .expect("the sample has memory stats");
            assert!(format_memory(ICON, stats, threshold).is_some());
        }
        let stats = MemoryUsage::<TestTheme>::new(None).sample().unwrap();
        assert_eq!(
            format_memory(ICON, stats, None),
            Some("\u{f035b} 72% | swap 5%".into())
        );
    }

    #[test]
    fn unusable_totals_do_not_render() {
        assert_eq!(format_memory(ICON, stats(0, 0, 0, 0), None), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_linux_meminfo_and_prefers_available_memory() {
        let stats = parse_meminfo(
            "MemTotal:       16384 kB\nMemFree:         4096 kB\nMemAvailable:    6144 kB\nSwapTotal:       2048 kB\nSwapFree:        1024 kB\n",
        )
        .expect("valid meminfo should parse");

        assert_eq!(stats.total, 16 * 1024 * 1024);
        assert_eq!(stats.available, 6 * 1024 * 1024);
        assert_eq!(stats.swap_total, 2 * 1024 * 1024);
        assert_eq!(stats.swap_available, 1024 * 1024);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn falls_back_to_memfree_when_memavailable_is_missing() {
        let stats = parse_meminfo("MemTotal: 4 kB\nMemFree: 1 kB\n").expect("valid meminfo");
        assert_eq!(stats.available, 1024);
    }
}
