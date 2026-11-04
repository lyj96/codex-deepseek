//! Command construction for background processes that must not allocate a Windows console.
//! Callers retain control of environment, stdio, and process lifetime. Interactive,
//! detached, and private-desktop launches must select their own console policy.

use std::ffi::OsStr;
use std::process::Command;

/// Read startup arguments, including from musl constructors before Rust initializes argv.
/// Normal entry points use the standard library. Only an uninitialized Linux musl
/// argv falls back to the kernel's NUL-delimited command line; no shell is involved.
pub fn startup_args_os() -> std::vec::IntoIter<std::ffi::OsString> {
    let args: Vec<_> = std::env::args_os().collect();
    #[cfg(all(target_os = "linux", target_env = "musl"))]
    if args.is_empty() {
        use std::os::unix::ffi::OsStringExt;

        let bytes = std::fs::read("/proc/self/cmdline").unwrap_or_else(|error| {
            eprintln!("failed to read process startup arguments: {error}");
            std::process::exit(/*code*/ 1);
        });
        let Some(bytes) = bytes.strip_suffix(&[0]) else {
            eprintln!("process startup arguments are not NUL-terminated");
            std::process::exit(/*code*/ 1);
        };
        return bytes
            .split(|byte| *byte == 0)
            .map(|arg| std::ffi::OsString::from_vec(arg.to_vec()))
            .collect::<Vec<_>>()
            .into_iter();
    }
    args.into_iter()
}

/// UTF-8 startup arguments for helpers whose command-line protocol requires text.
pub fn startup_args() -> impl Iterator<Item = String> {
    startup_args_os().map(|arg| {
        arg.into_string().unwrap_or_else(|_| {
            eprintln!("process startup argument is not valid UTF-8");
            std::process::exit(/*code*/ 1);
        })
    })
}

/// Construct a background command, suppressing console allocation on Windows.
/// Convert with `tokio::process::Command::from` for asynchronous execution.
/// Subsequent Windows `creation_flags` calls must retain `CREATE_NO_WINDOW`.
pub fn background_command(program: impl AsRef<OsStr>) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let mut command = Command::new(program);
        command.creation_flags(/*flags*/ 0x0800_0000); // CREATE_NO_WINDOW
        command
    }
    #[cfg(not(windows))]
    Command::new(program)
}

#[cfg(all(test, windows))]
#[path = "background_command_tests.rs"]
mod tests;
