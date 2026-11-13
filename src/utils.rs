use std::env;

use crate::backend::{self, Probe};

/// Whether the shell came in over SSH. `true` or `false` in fixtures.
pub fn is_remote_shell() -> bool {
    backend::probe(RemoteShell).unwrap_or(false)
}

struct RemoteShell;

impl Probe for RemoteShell {
    type Value = bool;
    const KEY: &'static str = "ssh";

    fn probe(&self) -> Option<bool> {
        Some(
            env::var_os("SSH_CLIENT").is_some()
                || env::var_os("SSH_TTY").is_some()
                || env::var_os("SSH_CONNECTION").is_some(),
        )
    }
}

/// Joins the non-empty `parts` with single spaces, so an icon a theme sets to
/// `""` takes its separating space with it.
pub fn join_non_empty<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_non_empty_skips_empty_parts_and_their_spaces() {
        assert_eq!(join_non_empty(["a", "b"]), "a b");
        assert_eq!(join_non_empty(["", "b"]), "b");
        assert_eq!(join_non_empty(["a", "", "c"]), "a c");
        assert_eq!(join_non_empty(["", ""]), "");
    }
}
