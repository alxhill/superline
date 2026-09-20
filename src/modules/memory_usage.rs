//! The current machine's memory usage.
//!
//! Memory is read directly from the operating system rather than through a
//! command such as `free` or `vm_stat`. The read is a small, local operation,
//! so it does not need the asynchronous cache used by network and repository
//! lookups.

use std::marker::PhantomData;

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// The Nerd Font `memory` glyph. It is deliberately fixed for now: the
/// module has no user-facing formatting knobs yet.
const MEMORY_ICON: &str = "\u{f035b}";

/// A snapshot of physical and swap memory, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MemoryStats {
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
    fn memory_usage_fg() -> Color {
        Self::default_fg()
    }

    fn memory_usage_bg() -> Color {
        Self::default_bg()
    }
}

pub struct MemoryUsage<S: MemoryUsageScheme> {
    scheme: PhantomData<S>,
}

impl<S: MemoryUsageScheme> Default for MemoryUsage<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: MemoryUsageScheme> MemoryUsage<S> {
    pub fn new() -> Self {
        Self {
            scheme: PhantomData,
        }
    }
}

impl<S: MemoryUsageScheme> Module for MemoryUsage<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(stats) = system_memory() else {
            return;
        };

        powerline.add_segment(
            format_memory(stats),
            Style::simple(S::memory_usage_fg(), S::memory_usage_bg()),
        );
    }
}

fn format_memory(stats: MemoryStats) -> String {
    let mut text = format!(
        "{MEMORY_ICON} {}/{}",
        display_bytes(stats.used()),
        display_bytes(stats.total),
    );

    // Match starship's useful default of showing swap only when the machine
    // has a swap device/file configured. The explicit label keeps the second
    // pair understandable in a compact prompt.
    if stats.swap_total > 0 {
        text.push_str(&format!(
            " | swap {}/{}",
            display_bytes(stats.swap_used()),
            display_bytes(stats.swap_total),
        ));
    }

    text
}

/// Render a byte count with a short binary unit and no decimal places.
///
/// Keeping the display to whole units makes the segment stable from one
/// prompt to the next and mirrors starship's compact memory presentation.
fn display_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

    let mut value = bytes;
    let mut unit = 0;
    while value >= 1024 && unit < UNITS.len() - 1 {
        value /= 1024;
        unit += 1;
    }

    format!("{value}{}", UNITS[unit])
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

    #[test]
    fn displays_binary_units_without_decimals() {
        assert_eq!(display_bytes(0), "0B");
        assert_eq!(display_bytes(1023), "1023B");
        assert_eq!(display_bytes(1024), "1KiB");
        assert_eq!(display_bytes(1024 * 1024 * 1024), "1GiB");
    }

    #[test]
    fn formats_used_and_swap_memory() {
        let stats = MemoryStats {
            total: 16 * 1024 * 1024 * 1024,
            available: 6 * 1024 * 1024 * 1024,
            swap_total: 2 * 1024 * 1024 * 1024,
            swap_available: 1024 * 1024 * 1024,
        };
        assert_eq!(display_bytes(stats.used()), "10GiB");
        assert_eq!(display_bytes(stats.swap_used()), "1GiB");
        assert_eq!(
            format_memory(stats),
            "\u{f035b} 10GiB/16GiB | swap 1GiB/2GiB"
        );
    }

    #[test]
    fn omits_swap_when_the_machine_has_none() {
        let stats = MemoryStats {
            total: 8 * 1024 * 1024 * 1024,
            available: 2 * 1024 * 1024 * 1024,
            swap_total: 0,
            swap_available: 0,
        };

        assert_eq!(format_memory(stats), "\u{f035b} 6GiB/8GiB");
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
