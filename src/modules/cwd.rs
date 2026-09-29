use std::env;
use std::ffi::OsString;
use std::marker::PhantomData;
use std::path::{PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR};

use crate::colors::Color;
use crate::config::SegmentPadding;
use crate::platform;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

pub struct Cwd<S: CwdScheme> {
    max_length: usize,
    wanted_seg_num: usize,
    resolve_symlinks: bool,
    scheme: PhantomData<S>,
}

pub trait CwdScheme: DefaultColors {
    const CWD_HOME_ICON: &'static str = "~";
    const CWD_ROOT_ICON: &'static str = "~";
    const CWD_ELLIPSIS_ICON: &'static str = "...";

    fn path_fg() -> Color {
        Self::default_fg()
    }

    fn path_bg_colors() -> Vec<Color>;

    /// Stands in for the home directory at the start of the path.
    fn cwd_home_icon() -> &'static str {
        Self::CWD_HOME_ICON
    }

    /// Shown on its own at the filesystem root.
    fn cwd_root_icon() -> &'static str {
        Self::CWD_ROOT_ICON
    }

    /// Stands in for the components a long path leaves out.
    fn cwd_ellipsis_icon() -> &'static str {
        Self::CWD_ELLIPSIS_ICON
    }
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

/// The directory the prompt shows, split at the home directory.
pub enum CwdPath {
    /// The filesystem root.
    Root,
    /// Under the home directory: the rest of the path after it, starting with
    /// a separator (empty at home itself).
    Home(String),
    /// Anywhere else: the whole path.
    Other(String),
}

impl<S: CwdScheme> Module for Cwd<S> {
    type Data = CwdPath;

    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Left.into()
    }

    fn fetch(&self) -> CwdPath {
        let current_dir = resolve_cwd(
            self.resolve_symlinks,
            cfg!(windows),
            env::var_os("PWD"),
            || env::current_dir().unwrap(),
        );

        let current_dir = current_dir.to_string_lossy();
        let cwd: &str = &current_dir;

        // Sitting at the filesystem root ("/" on Unix) - just show the glyph.
        if cwd == MAIN_SEPARATOR_STR {
            return CwdPath::Root;
        }

        if let Some(home) = platform::home_dir() {
            let home = home.to_string_lossy();
            if cwd.starts_with(home.as_ref()) {
                return CwdPath::Home(cwd[home.len()..].to_owned());
            }
        }

        CwdPath::Other(cwd.to_owned())
    }

    fn sample(&self) -> CwdPath {
        CwdPath::Home(format!("{MAIN_SEPARATOR}dev{MAIN_SEPARATOR}superline"))
    }

    fn render(&self, path: CwdPath, powerline: &mut Powerline) {
        let colors = S::path_bg_colors();
        for (index, label) in self.labels(&path).into_iter().enumerate() {
            let bg = colors[index % colors.len()];
            powerline.add_segment(label, Style::simple(S::path_fg(), bg));
        }
    }
}

impl<S: CwdScheme> Cwd<S> {
    /// The segments `path` draws as, one rainbow colour each. An icon the
    /// theme set to `""` draws nothing and leaves the next segment its colour.
    fn labels<'a>(&self, path: &'a CwdPath) -> Vec<&'a str> {
        let mut labels = Vec::new();
        let icon = |icon: &'static str, labels: &mut Vec<&'a str>| {
            if !icon.is_empty() {
                labels.push(icon);
            }
        };

        let cwd = match path {
            CwdPath::Root => {
                icon(S::cwd_root_icon(), &mut labels);
                return labels;
            }
            CwdPath::Home(rest) => {
                icon(S::cwd_home_icon(), &mut labels);
                rest.as_str()
            }
            CwdPath::Other(cwd) => cwd.as_str(),
        };

        let depth = cwd.matches(MAIN_SEPARATOR).count();

        if (cwd.len() > self.max_length) && (depth > self.wanted_seg_num) {
            let left = self.wanted_seg_num / 2;
            let right = self.wanted_seg_num - left;

            let start = cwd.split(MAIN_SEPARATOR).skip(1).take(left);
            let end = cwd.split(MAIN_SEPARATOR).skip(depth - right + 1);

            labels.extend(start);
            icon(S::cwd_ellipsis_icon(), &mut labels);
            labels.extend(end);
        } else {
            labels.extend(cwd.split(MAIN_SEPARATOR).skip(1));
        }
        labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colors::{black, blue, green};

    struct TestTheme;

    impl DefaultColors for TestTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            green()
        }
    }

    impl CwdScheme for TestTheme {
        fn path_bg_colors() -> Vec<Color> {
            vec![black(), blue()]
        }
    }

    struct NoIconTheme;

    impl DefaultColors for NoIconTheme {
        fn default_bg() -> Color {
            black()
        }

        fn default_fg() -> Color {
            green()
        }
    }

    impl CwdScheme for NoIconTheme {
        fn path_bg_colors() -> Vec<Color> {
            vec![black()]
        }

        fn cwd_home_icon() -> &'static str {
            ""
        }

        fn cwd_ellipsis_icon() -> &'static str {
            ""
        }
    }

    fn path(parts: &[&str]) -> String {
        parts
            .iter()
            .map(|part| format!("{MAIN_SEPARATOR}{part}"))
            .collect()
    }

    #[test]
    fn the_sample_is_a_project_under_home() {
        let cwd = Cwd::<TestTheme>::new(50, 4, false);
        assert_eq!(cwd.labels(&cwd.sample()), ["~", "dev", "superline"]);
        let _ = crate::terminal::SHELL.set(crate::terminal::Shell::Bare);
        cwd.render(cwd.sample(), &mut Powerline::new());
    }

    #[test]
    fn the_root_shows_only_its_icon() {
        let cwd = Cwd::<TestTheme>::new(50, 4, false);
        assert_eq!(cwd.labels(&CwdPath::Root), ["~"]);
    }

    #[test]
    fn a_long_path_elides_its_middle() {
        let cwd = Cwd::<TestTheme>::new(10, 2, false);
        let long = CwdPath::Other(path(&["usr", "local", "share", "doc"]));
        assert_eq!(cwd.labels(&long), ["usr", "...", "doc"]);
    }

    #[test]
    fn empty_icons_draw_nothing() {
        let cwd = Cwd::<NoIconTheme>::new(10, 2, false);
        let long = CwdPath::Home(path(&["dev", "a", "b", "superline"]));
        assert_eq!(cwd.labels(&long), ["dev", "superline"]);
    }

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
