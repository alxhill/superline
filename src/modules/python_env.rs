use std::env;
use std::fs::File;
use std::io::read_to_string;
use std::marker::PhantomData;
use std::path::Path;
use std::process::Command;

use crate::colors::Color;
use crate::mise;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

pub struct PythonEnv<S: PythonEnvScheme> {
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
        Self::new()
    }
}

impl<S: PythonEnvScheme> PythonEnv<S> {
    pub fn new() -> PythonEnv<S> {
        PythonEnv {
            scheme: PhantomData,
        }
    }
}

const PYTHON_VERSION_CMD: &str =
    r#"from sys import version_info as v; print(f"{v.major}.{v.minor}.{v.micro}")"#;
const PYTHON_LOGO: &str = "\u{e73c}";
const SNAKE_ICON: &str = "\u{f150e}";

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

            let py_ver_str = Command::new("python")
                .args(["-c", PYTHON_VERSION_CMD])
                .output()
                .ok()
                .and_then(|output| {
                    std::str::from_utf8(&output.stdout)
                        .map(|s| s.to_owned())
                        .ok()
                })
                .unwrap_or("".into());

            powerline.add_short_segment(
                format!("{} {} ", pylogo, venv_name),
                Style::simple(S::pyenv_fg(), S::pyenv_bg()),
            );
            powerline.add_segment(
                py_ver_str.trim().to_string(),
                Style::simple(S::pyver_fg(), S::pyver_bg()),
            );
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
                let source_icon = if mise_ver.is_some() {
                    format!("{} ", S::mise_icon())
                } else {
                    String::new()
                };

                powerline.add_short_segment(
                    format!("{}{} ", source_icon, pylogo),
                    Style::simple(S::pyenv_fg(), S::pyenv_bg()),
                );

                if let Some(py_ver) = py_ver {
                    powerline.add_segment(
                        py_ver.trim().to_string(),
                        Style::simple(S::pyver_fg(), S::pyenv_bg()),
                    );
                }
            }
        }
    }
}
