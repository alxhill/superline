//! Renders the config being edited by running `superline preview` on a copy of
//! it, on a background thread. A separate process picks up a changed custom
//! theme (themes load once per process) and keeps a slow widget from freezing
//! the editor.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use serde_json::Value;

pub struct Request {
    pub config: Value,
    /// The theme being edited, drawn instead of the file the config names.
    pub theme: Option<Value>,
    pub columns: u16,
}

/// The printed prompt, or why it could not be drawn.
pub type Rendered = Result<String, String>;

pub struct Preview {
    requests: Sender<Request>,
    results: Receiver<Rendered>,
}

impl Preview {
    pub fn spawn(config_path: &Path) -> Preview {
        let (requests, request_rx) = mpsc::channel::<Request>();
        let (result_tx, results) = mpsc::channel();
        let config_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let temp = std::env::temp_dir().join(format!(
            "superline-config-preview-{}.json",
            std::process::id()
        ));
        let temp_theme = temp.with_file_name(format!(
            "superline-theme-preview-{}.json",
            std::process::id()
        ));

        thread::spawn(move || {
            while let Ok(mut request) = request_rx.recv() {
                // Only the newest config is worth drawing.
                loop {
                    match request_rx.try_recv() {
                        Ok(newer) => request = newer,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            let _ = std::fs::remove_file(&temp);
                            let _ = std::fs::remove_file(&temp_theme);
                            return;
                        }
                    }
                }
                let rendered = render(&request, &config_dir, &temp, &temp_theme);
                if result_tx.send(rendered).is_err() {
                    break;
                }
            }
            let _ = std::fs::remove_file(&temp);
            let _ = std::fs::remove_file(&temp_theme);
        });

        Preview { requests, results }
    }

    pub fn request(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// The latest finished render, if one arrived since the last call.
    pub fn poll(&self) -> Option<Rendered> {
        self.results.try_iter().last()
    }
}

fn render(request: &Request, config_dir: &Path, temp: &Path, temp_theme: &Path) -> Rendered {
    let mut config = request.config.clone();
    if let Some(theme) = &request.theme {
        let text = serde_json::to_string(theme).map_err(|e| e.to_string())?;
        std::fs::write(temp_theme, text)
            .map_err(|e| format!("could not write preview theme: {e}"))?;
        config["theme"] = Value::from(temp_theme.to_string_lossy().into_owned());
    } else if let Some(theme) = config.get_mut("theme") {
        // The copy lives elsewhere, so point a relative theme path back at the
        // real config directory.
        if let Some(name) = theme.as_str() {
            if name != "rainbow" && name != "simple" {
                *theme = Value::from(config_dir.join(name).to_string_lossy().into_owned());
            }
        }
    }

    let text = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    std::fs::write(temp, text).map_err(|e| format!("could not write preview config: {e}"))?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = Command::new(exe)
        .arg("preview")
        .arg("--config")
        .arg(temp)
        .args(["-s", "0", "-c", &request.columns.to_string()])
        .arg(shell_name())
        // A sample duration so last_cmd_duration has something to show.
        .arg("1234")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run superline: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

/// The user's login shell, for the `shell` widget.
fn shell_name() -> &'static str {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let name = Path::new(&shell)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match name.as_str() {
        "bash" => "bash",
        "zsh" => "zsh",
        "pwsh" | "powershell" => "pwsh",
        "nu" => "nu",
        _ if cfg!(windows) && shell.is_empty() => "pwsh",
        _ => "fish",
    }
}
