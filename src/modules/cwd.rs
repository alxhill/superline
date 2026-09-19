use std::env;
use std::ffi::OsString;
use std::marker::PhantomData;
use std::path::{PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR};

use crate::colors::Color;
use crate::platform;
use crate::themes::DefaultColors;
use crate::{Segments, Style};

use super::Module;

pub struct Cwd<S: CwdScheme> {
    max_length: usize,
    wanted_seg_num: usize,
    resolve_symlinks: bool,
    scheme: PhantomData<S>,
}

pub trait CwdScheme: DefaultColors {
    fn path_fg() -> Color {
        Self::default_fg()
    }

    fn path_bg_colors() -> Vec<Color>;
}

impl<S: CwdScheme> Cwd<S> {
    pub fn new(max_length: usize, wanted_seg_num: usize, resolve_symlinks: bool) -> Cwd<S> {
        Cwd {
            max_length,
            wanted_seg_num,
            resolve_symlinks,
            scheme: PhantomData,
        }
    }
}

macro_rules! rainbow_segment {
    ($segments:ident, $iter_var:ident, $value:expr) => {
        let r_col = S::path_bg_colors()[$iter_var % S::path_bg_colors().len()];
        $segments.add_short_segment(format!(" {}", $value), Style::simple(S::path_fg(), r_col));
        $iter_var = $iter_var.wrapping_add(1);
    };
}

/// Resolve the directory the prompt should display.
///
/// When `resolve_symlinks` is set we always use the real (symlink-resolved)
/// cwd. Otherwise we prefer the shell's logical `$PWD`, which preserves
/// symlinks - but only off Windows. On Windows `$PWD` is either unset
/// (cmd / PowerShell) or an MSYS-style POSIX path such as `/c/Users/alex` under
/// Git Bash. A native Windows binary uses `\` as its path separator, so it
/// can't split or home-contract a forward-slash `$PWD`: the whole path becomes
/// a single unsplittable segment that the leading `skip(1)` then discards,
/// leaving the module empty. So on Windows we always fall back to the real cwd,
/// which yields a proper `C:\...` path.
fn resolve_cwd(
    resolve_symlinks: bool,
    windows: bool,
    pwd: Option<OsString>,
    current_dir: impl FnOnce() -> PathBuf,
) -> PathBuf {
    if resolve_symlinks || windows {
        return current_dir();
    }
    pwd.map(PathBuf::from).unwrap_or_else(current_dir)
}

impl<S: CwdScheme> Module for Cwd<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        let current_dir = resolve_cwd(
            self.resolve_symlinks,
            cfg!(windows),
            env::var_os("PWD"),
            || env::current_dir().unwrap(),
        );

        let current_dir = current_dir.to_string_lossy();
        let mut cwd: &str = &current_dir;

        let mut current_bg = 0usize;

        // Sitting at the filesystem root ("/" on Unix) - just show the glyph.
        #[allow(unused_assignments)]
        if cwd == MAIN_SEPARATOR_STR {
            rainbow_segment!(segments, current_bg, "~");
            return;
        }

        if let Some(home) = platform::home_dir() {
            let home = home.to_string_lossy();
            if cwd.starts_with(home.as_ref()) {
                rainbow_segment!(segments, current_bg, "~");
                cwd = &cwd[home.len()..]
            }
        }

        let depth = cwd.matches(MAIN_SEPARATOR).count();

        if (cwd.len() > self.max_length) && (depth > self.wanted_seg_num) {
            let left = self.wanted_seg_num / 2;
            let right = self.wanted_seg_num - left;

            let start = cwd.split(MAIN_SEPARATOR).skip(1).take(left);
            let end = cwd.split(MAIN_SEPARATOR).skip(depth - right + 1);

            for val in start {
                rainbow_segment!(segments, current_bg, val);
            }

            rainbow_segment!(segments, current_bg, "...");

            for val in end {
                rainbow_segment!(segments, current_bg, val);
            }
        } else {
            for val in cwd.split(MAIN_SEPARATOR).skip(1) {
                rainbow_segment!(segments, current_bg, val);
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never_called() -> PathBuf {
        panic!("current_dir should not have been called");
    }

    #[test]
    fn prefers_pwd_off_windows() {
        let dir = resolve_cwd(
            false,
            false,
            Some(OsString::from("/home/alex/src")),
            never_called,
        );
        assert_eq!(dir, PathBuf::from("/home/alex/src"));
    }

    #[test]
    fn falls_back_to_current_dir_when_pwd_unset_off_windows() {
        let dir = resolve_cwd(false, false, None, || PathBuf::from("/home/alex/src"));
        assert_eq!(dir, PathBuf::from("/home/alex/src"));
    }

    #[test]
    fn ignores_msys_pwd_on_windows() {
        // Git Bash sets `$PWD` to a forward-slash POSIX path; a native Windows
        // binary must ignore it and use the real cwd, otherwise the module
        // renders empty (regression test).
        let dir = resolve_cwd(
            false,
            true,
            Some(OsString::from("/c/Users/alex/src")),
            || PathBuf::from(r"C:\Users\alex\src"),
        );
        assert_eq!(dir, PathBuf::from(r"C:\Users\alex\src"));
    }

    #[test]
    fn resolve_symlinks_always_uses_current_dir() {
        let dir = resolve_cwd(true, false, Some(OsString::from("/logical/pwd")), || {
            PathBuf::from("/real/cwd")
        });
        assert_eq!(dir, PathBuf::from("/real/cwd"));
    }
}
