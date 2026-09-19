use std::env;
use std::fs::{self, File};
use std::io::read_to_string;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::colors::Color;
use crate::mise;
use crate::themes::DefaultColors;
use crate::{Segments, Style};

use super::Module;

pub struct Python<S: PythonScheme> {
    /// Whether to show the interpreter version.
    show_version: bool,
    /// Whether to show the active virtual env's name.
    show_venv: bool,
    scheme: PhantomData<S>,
}

pub trait PythonScheme: DefaultColors {
    fn pyenv_fg() -> Color {
        Self::default_fg()
    }
    fn pyenv_bg() -> Color {
        Self::default_bg()
    }
    fn pyver_fg() -> Color {
        Self::default_fg()
    }
    fn pyver_bg() -> Color {
        Self::default_bg()
    }

    /// Marks a version that came from a mise config rather than
    /// `.python-version`.
    fn mise_icon() -> &'static str {
        mise::DEFAULT_ICON
    }
}

impl<S: PythonScheme> Default for Python<S> {
    fn default() -> Self {
        Self::new(true, true)
    }
}

impl<S: PythonScheme> Python<S> {
    pub fn new(show_version: bool, show_venv: bool) -> Python<S> {
        Python {
            show_version,
            show_venv,
            scheme: PhantomData,
        }
    }
}

const PYTHON_VERSION_CMD: &str =
    r#"from sys import version_info as v; print(f"{v.major}.{v.minor}.{v.micro}")"#;
const PYTHON_LOGO: &str = "\u{e73c}";
const SNAKE_ICON: &str = "\u{f150e}";
const LOADING_MARKER: &str = "\u{2026}";

/// The version a virtual env was created from, read from the files the env
/// tooling leaves behind, so the interpreter never has to start.
///
/// `pyvenv.cfg` is written by the stdlib `venv` module (`version`), uv
/// (`version_info`) and virtualenv (both). Conda envs have no `pyvenv.cfg`
/// but record the package in `conda-meta/python-<version>-<build>.json`.
fn version_from_env_files(venv: &Path) -> Option<String> {
    if let Ok(cfg) = fs::read_to_string(venv.join("pyvenv.cfg")) {
        if let Some(version) = parse_pyvenv_cfg(&cfg) {
            return Some(version);
        }
    }

    fs::read_dir(venv.join("conda-meta"))
        .ok()?
        .flatten()
        .find_map(|entry| conda_python_version(&entry.file_name().to_string_lossy()))
}

fn parse_pyvenv_cfg(contents: &str) -> Option<String> {
    let mut version = None;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            // virtualenv writes both keys; `version_info` is the more precise
            // one (`3.13.8.final.0`), so it wins when present.
            "version_info" => return release_version(value),
            "version" => version = release_version(value),
            _ => {}
        }
    }
    version
}

/// `python-3.13.8-h1234abc_0.json` -> `3.13.8`. Other conda packages whose
/// names merely start with `python` (`python-dateutil`, `python_abi`) do not
/// have a digit straight after the dash.
fn conda_python_version(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix("python-")?.strip_suffix(".json")?;
    let (version, _build) = rest.split_once('-')?;
    release_version(version)
}

/// The leading `major.minor.micro` of a version string, dropping any release
/// level suffix such as `.final.0`.
fn release_version(value: &str) -> Option<String> {
    let value = value.trim();
    let numeric: &str = value
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()?;
    let parts = numeric
        .split('.')
        .filter(|part| !part.is_empty())
        .take(3)
        .collect::<Vec<_>>();
    (!parts.is_empty() && parts[0].chars().all(|c| c.is_ascii_digit())).then(|| parts.join("."))
}

/// The version reported by one interpreter, for envs without a readable
/// `pyvenv.cfg` or `conda-meta`. Asking `python` is the slow step in this
/// module, so the answer is cached and refreshed in the background.
#[derive(Clone, Serialize, Deserialize)]
pub struct PythonVersion {
    pub interpreter: PathBuf,
}

impl Source for PythonVersion {
    type Value = String;
    const KIND: &'static str = "python";
    /// A virtual env's interpreter changes only when the env is rebuilt.
    const TTL: Duration = Duration::from_secs(10 * 60);

    fn cache_id(&self) -> String {
        hash_id(&self.interpreter)
    }

    fn fetch(&self) -> Option<String> {
        let output = Command::new(&self.interpreter)
            .args(["-c", PYTHON_VERSION_CMD])
            .output()
            .ok()?;
        parse_version(&output.stdout)
    }
}

fn parse_version(stdout: &[u8]) -> Option<String> {
    let version = std::str::from_utf8(stdout).ok()?.trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// The interpreter a virtual env activates. Falls back to whatever `python`
/// is on `PATH` for envs that are named rather than located (conda's
/// `CONDA_DEFAULT_ENV`).
fn interpreter_for(venv: &Path) -> PathBuf {
    let candidate = if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    };
    if candidate.is_file() {
        candidate
    } else {
        PathBuf::from("python")
    }
}

impl<S: PythonScheme> Module for Python<S> {
    fn append_segments(&mut self, segments: &mut Segments) {
        let venv = env::var("VIRTUAL_ENV")
            .or_else(|_| env::var("CONDA_ENV_PATH"))
            .or_else(|_| env::var("CONDA_DEFAULT_ENV"));

        let pylogo = if let Ok(cwd) = env::current_dir() {
            if cwd.join("pyproject.toml").exists() {
                format!("{} {}", PYTHON_LOGO, SNAKE_ICON)
            } else {
                PYTHON_LOGO.to_string()
            }
        } else {
            "".into()
        };

        if let Ok(venv_path) = venv {
            // file_name is always some, because env variable is a valid directory path.
            let venv_name = Path::new(&venv_path).file_name().unwrap().to_string_lossy();

            let label = if self.show_venv {
                format!("{} {} ", pylogo, venv_name)
            } else {
                format!("{} ", pylogo)
            };
            segments.add_short_segment(label, Style::simple(S::pyenv_fg(), S::pyenv_bg()));

            if self.show_version {
                let venv_dir = Path::new(&venv_path);
                let version = match version_from_env_files(venv_dir) {
                    Some(version) => version,
                    None => {
                        let interpreter = interpreter_for(venv_dir);
                        match Cached::new(PythonVersion { interpreter }).load() {
                            Lookup::Ready(version) => version,
                            Lookup::Loading => LOADING_MARKER.to_string(),
                            Lookup::Unavailable => return,
                        }
                    }
                };
                segments.add_segment(version, Style::simple(S::pyver_fg(), S::pyver_bg()));
            }
        } else if let Ok(cwd) = env::current_dir() {
            // A mise config wins over `.python-version`: it is what puts an
            // interpreter on the path, and repos often keep both.
            let mise_ver = mise::tool_version("python");
            let py_ver = mise_ver.map(str::to_string).or_else(|| {
                File::open(cwd.join(".python-version"))
                    .and_then(read_to_string)
                    .ok()
            });

            if py_ver.is_some() || cwd.join("pyproject.toml").exists() {
                // One segment, unlike the venv path: without an env name there
                // is nothing for the version to be set apart from.
                let py_ver = py_ver.filter(|_| self.show_version);
                let label = [
                    mise_ver.map(|_| S::mise_icon()),
                    Some(pylogo.as_str()),
                    py_ver.as_deref().map(str::trim),
                ]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");

                segments.add_segment(label, Style::simple(S::pyenv_fg(), S::pyenv_bg()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyvenv_cfg_from_each_tool_yields_the_release_version() {
        let uv = "home = /x/bin\nimplementation = CPython\nversion_info = 3.13.8\n";
        let stdlib = "home = /x/bin\ninclude-system-site-packages = false\nversion = 3.14.7\n";
        let virtualenv = "version_info = 3.13.8.final.0\nversion = 3.13.8\n";
        let prerelease = "version = 3.15.0rc1\n";
        assert_eq!(parse_pyvenv_cfg(uv).as_deref(), Some("3.13.8"));
        assert_eq!(parse_pyvenv_cfg(stdlib).as_deref(), Some("3.14.7"));
        assert_eq!(parse_pyvenv_cfg(virtualenv).as_deref(), Some("3.13.8"));
        assert_eq!(parse_pyvenv_cfg(prerelease).as_deref(), Some("3.15.0"));
        assert_eq!(parse_pyvenv_cfg("home = /x/bin\n"), None);
        assert_eq!(parse_pyvenv_cfg("version = \n"), None);
    }

    #[test]
    fn conda_meta_python_package_yields_its_version() {
        assert_eq!(
            conda_python_version("python-3.13.8-h1234abc_0.json").as_deref(),
            Some("3.13.8")
        );
        assert_eq!(
            conda_python_version("python-dateutil-2.9.0-py_0.json"),
            None
        );
        assert_eq!(conda_python_version("python_abi-3.13-5_cp313.json"), None);
        assert_eq!(conda_python_version("history"), None);
    }

    #[test]
    fn interpreter_version_output_is_trimmed_and_empty_output_is_a_failure() {
        assert_eq!(parse_version(b"3.12.4\n").as_deref(), Some("3.12.4"));
        assert_eq!(parse_version(b"\n"), None);
        assert_eq!(parse_version(b""), None);
    }

    #[test]
    fn a_named_env_without_an_interpreter_falls_back_to_path_lookup() {
        assert_eq!(interpreter_for(Path::new("base")), PathBuf::from("python"));
    }
}
