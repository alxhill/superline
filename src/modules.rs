use crate::cache::{refresh_from_json, Source};
use crate::powerline::Powerline;

mod cmd;
mod cwd;
mod error_message;
mod exit_code;
mod git;
mod host;
mod pr;
mod readonly;
mod user;

mod cargo;
mod cmd_duration;
mod java;
mod nvm;
mod python_env;
mod shell_name;
mod spacer;
mod time;
mod unknown;
mod usage;

pub use cargo::{Cargo, CargoScheme};
pub use cmd::{Cmd, CmdScheme};
pub use cmd_duration::{LastCmdDuration, LastCmdDurationScheme};
pub use cwd::{Cwd, CwdScheme};
pub use error_message::{ErrorMessage, ErrorMessageScheme};
pub use exit_code::{ExitCode, ExitCodeScheme};
pub use git::{Git, GitScheme, GitStatus};
pub use host::{Host, HostScheme};
pub use java::{Java, JavaScheme};
pub use nvm::{Nvm, NvmScheme};
pub use pr::{Pr, PrLookup, PrScheme};
pub use python_env::{PythonEnv, PythonEnvScheme, PythonVersion};
pub use readonly::{ReadOnly, ReadOnlyScheme};
pub use shell_name::{ShellName, ShellScheme};
pub use spacer::{Spacer, SpacerScheme};
pub use time::{Time, TimeScheme};
pub use unknown::{Unknown, UnknownScheme};
pub use usage::{Usage, UsageLookup, UsageScheme, UsageWindow, UsageWindows};
pub use user::{User, UserScheme};

pub trait Module {
    fn append_segments(&mut self, powerline: &mut Powerline);
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
        _ => false,
    }
}
