//! How a command ends: the exit code the shell sees, and whether the command
//! failed. They are separate on purpose, because many failures print an error
//! and still exit 0.
//!
//! `handle_cli` returns an [`Exit`], and `main` ends the process with
//! [`Exit::terminate`], the only place that reports to telemetry and exits.

use std::sync::atomic::Ordering;

use crate::{
    handlers::run::subprocess::CommandFailed,
    telemetry::event::{classify_error, ErrorKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    /// The process exit code.
    pub code: i32,
    /// What telemetry reports; `None` means the command succeeded.
    pub failure: Option<ErrorKind>,
}

impl Exit {
    /// Success, exit code 0.
    pub fn ok() -> Self {
        Self {
            code: 0,
            failure: None,
        }
    }

    /// Passes an exit code through, for example a child process's. Telemetry
    /// still reports any non-zero code as a failure.
    pub fn code(code: i32) -> Self {
        Self {
            code,
            failure: None,
        }
    }

    /// A failure that exits 0: the error was printed and the command returned
    /// normally. Changing such a code is a product decision, not a refactor.
    pub fn failed(kind: ErrorKind) -> Self {
        Self::failed_with(kind, 0)
    }

    pub fn failed_with(kind: ErrorKind, code: i32) -> Self {
        Self {
            code,
            failure: Some(kind),
        }
    }

    /// Reports the outcome to telemetry, then exits the process. Telemetry
    /// never changes the exit code.
    pub fn terminate(self) -> ! {
        crate::telemetry::finish(self.code, self.failure);
        let aborted = crate::REQUEST_ABORTED.load(Ordering::SeqCst);
        std::process::exit(process_exit_code(self.code, aborted))
    }
}

impl From<anyhow::Error> for Exit {
    /// A failed command exits 0 unless the error says otherwise: a child
    /// process that failed (`CommandFailed`) keeps its own exit code.
    fn from(error: anyhow::Error) -> Self {
        let code = error
            .downcast_ref::<CommandFailed>()
            .map_or(0, CommandFailed::exit_code);
        Self::failed_with(classify_error(&error), code)
    }
}

/// Lets the `Result<()>` most handlers return stand in for a command's
/// outcome: success is `Exit::ok()`, an error is left for the caller to print.
pub trait IntoExit {
    fn into_exit(self) -> anyhow::Result<Exit>;
}

impl IntoExit for anyhow::Result<()> {
    fn into_exit(self) -> anyhow::Result<Exit> {
        self.map(|()| Exit::ok())
    }
}

/// The code the process exits with. A command that returned normally after
/// Ctrl-C aborted its request exits 130, as the shell expects.
fn process_exit_code(code: i32, aborted: bool) -> i32 {
    if code == 0 && aborted {
        130
    } else {
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::validation::InputValidationError;

    #[test]
    fn ok_is_a_success_with_code_0() {
        assert_eq!(
            Exit::ok(),
            Exit {
                code: 0,
                failure: None
            }
        );
    }

    #[test]
    fn code_passes_the_code_through_without_naming_a_failure() {
        assert_eq!(
            Exit::code(3),
            Exit {
                code: 3,
                failure: None
            }
        );
    }

    #[test]
    fn failed_reports_a_failure_but_keeps_exit_code_0() {
        assert_eq!(
            Exit::failed(ErrorKind::Validation),
            Exit {
                code: 0,
                failure: Some(ErrorKind::Validation)
            }
        );
    }

    #[test]
    fn failed_with_sets_both() {
        assert_eq!(
            Exit::failed_with(ErrorKind::Auth, 1),
            Exit {
                code: 1,
                failure: Some(ErrorKind::Auth)
            }
        );
    }

    #[test]
    fn an_error_is_classified_by_type_and_exits_0() {
        let validation = anyhow::anyhow!(InputValidationError::MissingApiKey);
        assert_eq!(
            Exit::from(validation),
            Exit {
                code: 0,
                failure: Some(ErrorKind::Validation)
            }
        );
        assert_eq!(
            Exit::from(anyhow::anyhow!("anything else")),
            Exit {
                code: 0,
                failure: Some(ErrorKind::Other)
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_child_keeps_its_exit_code() {
        use std::os::unix::process::ExitStatusExt;

        use crate::handlers::run::subprocess::CommandFailed;

        let exited_7 = CommandFailed {
            status: std::process::ExitStatus::from_raw(7 << 8),
        };
        assert_eq!(
            Exit::from(anyhow::Error::from(exited_7)),
            Exit {
                code: 7,
                failure: Some(ErrorKind::Other)
            }
        );
        // Killed by a signal: there is no exit code, which is reported as 1.
        let killed = CommandFailed {
            status: std::process::ExitStatus::from_raw(9),
        };
        assert_eq!(Exit::from(anyhow::Error::from(killed)).code, 1);
    }

    #[test]
    fn a_normal_return_after_ctrl_c_exits_130_and_nothing_else_changes() {
        assert_eq!(process_exit_code(0, true), 130);
        assert_eq!(process_exit_code(0, false), 0);
        // An explicit code is never replaced.
        assert_eq!(process_exit_code(1, true), 1);
        assert_eq!(process_exit_code(143, false), 143);
    }
}
