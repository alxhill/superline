use std::env;
use std::fs::File;
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
use crate::{Powerline, Style};

use super::Module;

pub struct PythonEnv<S: PythonEnvScheme> {
    /// Whether to show the interpreter version.
    show_version: bool,
    /// Whether to show the active virtual env's name.
    show_venv: bool,
    scheme: PhantomData<S>,
}

pub trait PythonEnvScheme: DefaultColors {
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

impl<S: PythonEnvScheme> Default for PythonEnv<S> {
    fn default() -> Self {
        Self::new(false, true)
    }
}

impl<S: PythonEnvScheme> PythonEnv<S> {
    pub fn new(show_version: bool, show_venv: bool) -> PythonEnv<S> {
        PythonEnv {
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

/// The version reported by one interpreter. Asking `python` is the slow step in
/// this module, so the answer is cached and refreshed in the background.
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

impl<S: PythonEnvScheme> Module for PythonEnv<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
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
            powerline.add_short_segment(label, Style::simple(S::pyenv_fg(), S::pyenv_bg()));

            if self.show_version {
                let interpreter = interpreter_for(Path::new(&venv_path));
                let version = match Cached::new(PythonVersion { interpreter }).load() {
                    Lookup::Ready(version) => version,
                    Lookup::Loading => LOADING_MARKER.to_string(),
                    Lookup::Unavailable => return,
                };
                powerline.add_segment(version, Style::simple(S::pyver_fg(), S::pyver_bg()));
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

                powerline.add_segment(label, Style::simple(S::pyenv_fg(), S::pyenv_bg()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
