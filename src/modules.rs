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
pub use git::{refresh_git, Git, GitScheme};
pub use host::{Host, HostScheme};
pub use java::{Java, JavaScheme};
pub use nvm::{Nvm, NvmScheme};
pub use pr::{refresh_pr, Pr, PrScheme};
pub use python_env::{PythonEnv, PythonEnvScheme};
pub use readonly::{ReadOnly, ReadOnlyScheme};
pub use shell_name::{ShellName, ShellScheme};
pub use spacer::{Spacer, SpacerScheme};
pub use time::{Time, TimeScheme};
pub use unknown::{Unknown, UnknownScheme};
pub use usage::{refresh_usage, Usage, UsageScheme, UsageWindow, UsageWindows};
pub use user::{User, UserScheme};

pub trait Module {
    fn append_segments(&mut self, powerline: &mut Powerline);
}
