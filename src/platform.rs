//! Small cross-platform helpers that paper over the differences between Unix
//! and Windows for the handful of OS-specific things the prompt needs: the
//! home/cache directories, the current user, root/elevation, and whether the
//! current directory is writable.
//!
//! The directory lookups are split into a pure resolver (which takes an
//! environment-variable getter and a `windows` flag) and a thin public wrapper
//! that feeds it the real environment. That lets the tests exercise both the
//! Unix and Windows resolution rules regardless of the host they run on.

use std::ffi::OsString;
use std::path::PathBuf;

/// A function that looks up an environment variable, mirroring
/// [`std::env::var_os`]. Taken as a parameter so tests can inject a fake
/// environment.
type EnvGetter<'a> = dyn Fn(&str) -> Option<OsString> + 'a;

fn real_env(key: &str) -> Option<OsString> {
    std::env::var_os(key)
}

/// Resolve the user's home directory from the given environment.
///
/// * Unix: `$HOME`.
/// * Windows: `%USERPROFILE%`, falling back to `%HOMEDRIVE%%HOMEPATH%`.
fn resolve_home(env: &EnvGetter, windows: bool) -> Option<PathBuf> {
    let non_empty = |v: OsString| (!v.is_empty()).then_some(v);

    if windows {
        if let Some(profile) = env("USERPROFILE").and_then(non_empty) {
            return Some(PathBuf::from(profile));
        }
        // `%HOMEDRIVE%%HOMEPATH%` is a plain concatenation (e.g. `C:` + `\Users\x`),
        // so join the strings directly rather than via `PathBuf::push`, which
        // would insert the host's separator.
        let mut home = env("HOMEDRIVE").and_then(non_empty)?;
        home.push(env("HOMEPATH").and_then(non_empty)?);
        Some(PathBuf::from(home))
    } else {
        env("HOME").and_then(non_empty).map(PathBuf::from)
    }
}

/// Resolve the cache directory from the given environment.
///
/// * Unix: `$XDG_CACHE_HOME`, falling back to `$HOME/.cache`.
/// * Windows: `%LOCALAPPDATA%`, falling back to `<home>/.cache`.
fn resolve_cache(env: &EnvGetter, home: Option<PathBuf>, windows: bool) -> Option<PathBuf> {
    let non_empty = |v: OsString| (!v.is_empty()).then_some(v);

    let explicit = if windows {
        "LOCALAPPDATA"
    } else {
        "XDG_CACHE_HOME"
    };
    if let Some(dir) = env(explicit).and_then(non_empty) {
        return Some(PathBuf::from(dir));
    }
    home.map(|h| h.join(".cache"))
}

/// The current user's home directory, or `None` if it can't be determined.
pub fn home_dir() -> Option<PathBuf> {
    resolve_home(&real_env, cfg!(windows))
}

/// The base cache directory superline should use, or `None` if it can't be
/// determined.
pub fn cache_dir() -> Option<PathBuf> {
    resolve_cache(&real_env, home_dir(), cfg!(windows))
}

/// The first file named `name` in a `PATH` directory. On Windows the usual
/// executable extensions are tried as well.
pub fn resolve_binary(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        for extension in ["exe", "cmd", "bat"] {
            let candidate = directory.join(format!("{name}.{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Whether the current process is running as root (Unix) / is the closest
/// equivalent on other platforms.
///
/// On Windows there is no uid; the prompt simply treats the session as
/// non-elevated (showing the normal user symbol). Detecting an elevated
/// "Run as administrator" session would require Win32 token APIs and is left as
/// a future enhancement.
#[cfg(unix)]
pub fn is_root() -> bool {
    users::get_current_uid() == 0
}

#[cfg(not(unix))]
pub fn is_root() -> bool {
    false
}

/// The current user's login name.
#[cfg(unix)]
pub fn current_username() -> Option<String> {
    users::get_user_by_uid(users::get_current_uid())
        .map(|user| user.name().to_string_lossy().into_owned())
}

#[cfg(not(unix))]
pub fn current_username() -> Option<String> {
    std::env::var("USERNAME")
        .ok()
        .or_else(|| std::env::var("USER").ok())
}

/// Whether the current working directory is not writable by this process.
#[cfg(unix)]
pub fn cwd_is_readonly() -> bool {
    use std::ffi::CString;
    // `access(W_OK)` answers the real question - can we write here - taking the
    // effective uid and directory permissions into account.
    let path = CString::new("./").expect("static path is valid");
    unsafe { libc::access(path.as_ptr(), libc::W_OK) != 0 }
}

#[cfg(windows)]
pub fn cwd_is_readonly() -> bool {
    // Best effort on Windows: the directory's read-only attribute. Real write
    // access is governed by ACLs that this doesn't fully capture, but it matches
    // the common case and never blocks the prompt.
    std::fs::metadata(".")
        .map(|m| m.permissions().readonly())
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
pub fn cwd_is_readonly() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build an [`EnvGetter`] backed by a fixed map, so resolution can be tested
    /// independently of the host's real environment.
    fn fake_env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        let map: HashMap<&str, &str> = vars.iter().copied().collect();
        move |key| map.get(key).map(OsString::from)
    }

    #[test]
    fn home_uses_home_on_unix() {
        let env = fake_env(&[("HOME", "/home/alex"), ("USERPROFILE", r"C:\Users\alex")]);
        assert_eq!(resolve_home(&env, false), Some(PathBuf::from("/home/alex")));
    }

    #[test]
    fn home_uses_userprofile_on_windows() {
        let env = fake_env(&[("HOME", "/home/alex"), ("USERPROFILE", r"C:\Users\alex")]);
        assert_eq!(
            resolve_home(&env, true),
            Some(PathBuf::from(r"C:\Users\alex"))
        );
    }

    #[test]
    fn home_falls_back_to_homedrive_homepath_on_windows() {
        let env = fake_env(&[("HOMEDRIVE", "C:"), ("HOMEPATH", r"\Users\alex")]);
        assert_eq!(
            resolve_home(&env, true),
            Some(PathBuf::from(r"C:\Users\alex"))
        );
    }

    #[test]
    fn home_is_none_when_nothing_is_set() {
        let env = fake_env(&[]);
        assert_eq!(resolve_home(&env, false), None);
        assert_eq!(resolve_home(&env, true), None);
    }

    #[test]
    fn empty_values_are_ignored() {
        let env = fake_env(&[("HOME", ""), ("USERPROFILE", "")]);
        assert_eq!(resolve_home(&env, false), None);
        assert_eq!(resolve_home(&env, true), None);
    }

    #[test]
    fn cache_prefers_xdg_on_unix() {
        let env = fake_env(&[("XDG_CACHE_HOME", "/xdg/cache")]);
        assert_eq!(
            resolve_cache(&env, Some(PathBuf::from("/home/alex")), false),
            Some(PathBuf::from("/xdg/cache"))
        );
    }

    #[test]
    fn cache_falls_back_to_home_dot_cache_on_unix() {
        let env = fake_env(&[]);
        assert_eq!(
            resolve_cache(&env, Some(PathBuf::from("/home/alex")), false),
            Some(PathBuf::from("/home/alex/.cache"))
        );
    }

    #[test]
    fn cache_prefers_localappdata_on_windows() {
        let env = fake_env(&[("LOCALAPPDATA", r"C:\Users\alex\AppData\Local")]);
        assert_eq!(
            resolve_cache(&env, Some(PathBuf::from(r"C:\Users\alex")), true),
            Some(PathBuf::from(r"C:\Users\alex\AppData\Local"))
        );
    }

    #[test]
    fn cache_ignores_xdg_on_windows() {
        // XDG_CACHE_HOME must not leak into Windows resolution.
        let env = fake_env(&[("XDG_CACHE_HOME", "/xdg/cache")]);
        assert_eq!(
            resolve_cache(&env, Some(PathBuf::from(r"C:\Users\alex")), true),
            Some(PathBuf::from(r"C:\Users\alex").join(".cache"))
        );
    }
}
