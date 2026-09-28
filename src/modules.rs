use std::fmt;

use crate::cache::{refresh_from_json, Source};
use crate::config::SegmentPadding;
use crate::powerline::Powerline;
use crate::update::UpdateLookup;
use crate::upgrade::AutoUpgrade;

mod battery;
mod cmd;
mod cwd;
mod error_message;
mod exit_code;
mod git;
mod host;
mod jobs;
mod local_ip;
mod pr;
mod readonly;
mod user;

mod cargo;
mod cmd_duration;
mod java;
mod kubernetes;
mod memory_usage;
mod node;
mod os;
mod python;
mod shell_name;
mod spacer;
mod sudo;
mod text;
mod time;
mod unknown;
mod usage;

pub use battery::{Battery, BatteryScheme};
pub use cargo::{Cargo, CargoScheme};
pub use cmd::{Cmd, CmdScheme};
pub use cmd_duration::{LastCmdDuration, LastCmdDurationScheme};
pub use cwd::{Cwd, CwdScheme};
pub use error_message::{ErrorMessage, ErrorMessageScheme};
pub use exit_code::{ExitCode, ExitCodeScheme};
pub use git::{preresolve_system_gitconfig, Git, GitScheme, GitStatus};
pub use host::{Host, HostScheme, Hostname};
pub use java::{Java, JavaScheme};
pub use jobs::{Jobs, JobsScheme};
pub use kubernetes::{Kubernetes, KubernetesContext, KubernetesLookup, KubernetesScheme};
pub use local_ip::{LocalIp, LocalIpScheme};
pub use memory_usage::{MemoryUsage, MemoryUsageScheme};
pub use node::{Node, NodeScheme};
pub use os::{Os, OsKind, OsScheme};
pub use pr::{Pr, PrLookup, PrScheme};
pub use python::{Python, PythonScheme, PythonVersion};
pub use readonly::{ReadOnly, ReadOnlyScheme};
pub use shell_name::{ShellName, ShellScheme};
pub use spacer::{Spacer, SpacerScheme};
pub use sudo::{Sudo, SudoLookup, SudoScheme};
pub use text::Text;
pub use time::{Time, TimeScheme};
pub use unknown::{Unknown, UnknownScheme};
pub use usage::{Usage, UsageLookup, UsageScheme, UsageWindow, UsageWindows};
pub use user::{User, UserScheme, Username};

pub trait Module {
    /// The padding of this module's segments when neither the config nor the
    /// theme sets one. The editor shows it next to the `padding` option.
    fn default_padding(&self) -> DefaultPadding;

    fn append_segments(&mut self, powerline: &mut Powerline);
}

/// The padding a module declares for its segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultPadding {
    /// What [`Powerline::add_segment`] draws the module's segments with.
    pub padding: SegmentPadding,
    /// A segment drawn with its own padding instead, passed to
    /// [`Powerline::add_padded_segment`], and what the editor calls it.
    pub exception: Option<(&'static str, SegmentPadding)>,
}

impl From<SegmentPadding> for DefaultPadding {
    fn from(padding: SegmentPadding) -> Self {
        DefaultPadding {
            padding,
            exception: None,
        }
    }
}

/// As the editor shows it, e.g. `large` or `large; venv label right`.
impl fmt::Display for DefaultPadding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.padding.name())?;
        if let Some((segment, padding)) = self.exception {
            write!(f, "; {segment} {}", padding.name())?;
        }
        Ok(())
    }
}

/// Runs the background half of a cached lookup: the hidden `refresh`
/// subcommand lands here with the source's [`Source::KIND`] and its JSON
/// parameters. Every type implementing [`Source`] needs a line in this match.
/// Returns whether a fresh value was written.
pub fn run_refresh(kind: &str, source: &str) -> bool {
    match kind {
        GitStatus::KIND => refresh_from_json::<GitStatus>(source),
        PrLookup::KIND => refresh_from_json::<PrLookup>(source),
        UsageLookup::KIND => refresh_from_json::<UsageLookup>(source),
        PythonVersion::KIND => refresh_from_json::<PythonVersion>(source),
        SudoLookup::KIND => refresh_from_json::<SudoLookup>(source),
        KubernetesLookup::KIND => refresh_from_json::<KubernetesLookup>(source),
        UpdateLookup::KIND => refresh_from_json::<UpdateLookup>(source),
        AutoUpgrade::KIND => refresh_from_json::<AutoUpgrade>(source),
        _ => false,
    }
}
