//! How a command ends: the exit code the shell sees, and whether the command
//! failed. A failed command exits 1 unless it says otherwise; the one
//! exception is the bare `agent hooks` invocation (see
//! [`Exit::zero_on_failure`]).
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

    /// A failure whose error was already printed: exit code 1.
    pub fn failed(kind: ErrorKind) -> Self {
        Self::failed_with(kind, 1)
    }

    pub fn failed_with(kind: ErrorKind, code: i32) -> Self {
        Self {
            code,
            failure: Some(kind),
        }
    }

    /// Keeps a failure at exit code 0, as every command did before failures
    /// exited 1. Only for invocations other tools run and read the exit code
    /// of, whose codes must not change; telemetry still sees the failure.
    pub fn zero_on_failure(self) -> Self {
        match self.failure {
            Some(_) => Self { code: 0, ..self },
            None => self,
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
    /// A failed command exits 1, except a child process that failed
    /// (`CommandFailed`), which keeps its own exit code.
    fn from(error: anyhow::Error) -> Self {
        if let Some(reported) = error.downcast_ref::<ReportedFailure>() {
            return Self::failed(reported.kind);
        }
        let code = error
            .downcast_ref::<CommandFailed>()
            .map_or(1, CommandFailed::exit_code);
        Self::failed_with(classify_error(&error), code)
    }
}

/// An error a handler has already printed. Returning it fails the command
/// (exit 1) without printing anything more, for handlers that format their
/// own error output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedFailure {
    pub kind: ErrorKind,
}

impl ReportedFailure {
    pub fn new(kind: ErrorKind) -> anyhow::Error {
        anyhow::Error::new(Self { kind })
    }
}

impl std::fmt::Display for ReportedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("command failed")
    }
}

impl std::error::Error for ReportedFailure {}

/// Whether an error still has to be printed: one a handler already printed
/// (`ReportedFailure`) has not.
pub fn needs_printing(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ReportedFailure>().is_none()
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
    fn failed_reports_a_failure_and_exits_1() {
        assert_eq!(
            Exit::failed(ErrorKind::Validation),
            Exit {
                code: 1,
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
    fn an_error_is_classified_by_type_and_exits_1() {
        let validation = anyhow::anyhow!(InputValidationError::MissingApiKey);
        assert_eq!(
            Exit::from(validation),
            Exit {
                code: 1,
                failure: Some(ErrorKind::Validation)
            }
        );
        assert_eq!(
            Exit::from(anyhow::anyhow!("anything else")),
            Exit {
                code: 1,
                failure: Some(ErrorKind::Other)
            }
        );
    }

    #[test]
    fn a_reported_failure_exits_1_with_its_kind_and_is_not_printed_again() {
        let reported = ReportedFailure::new(ErrorKind::NotFound);
        assert!(!needs_printing(&reported));
        assert_eq!(
            Exit::from(reported),
            Exit {
                code: 1,
                failure: Some(ErrorKind::NotFound)
            }
        );
        assert!(needs_printing(&anyhow::anyhow!("not printed yet")));
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
    fn zero_on_failure_only_changes_failures() {
        assert_eq!(
            Exit::failed(ErrorKind::Validation).zero_on_failure(),
            Exit {
                code: 0,
                failure: Some(ErrorKind::Validation)
            }
        );
        assert_eq!(Exit::ok().zero_on_failure(), Exit::ok());
        assert_eq!(Exit::code(3).zero_on_failure(), Exit::code(3));
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
