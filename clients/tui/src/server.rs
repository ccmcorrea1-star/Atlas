use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use clap::Subcommand;

use crate::runtime::DEFAULT_RUNTIME_SOCKET_PATH;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const RUNTIME_PROCESS_MARKER: &str = "ATLAS_RUNTIME_PROCESS=1";
const RUNTIME_PROGRAM_ENV: &str = "ATLAS_RUNTIME_PROGRAM";
const RUNTIME_ARGS_ENV: &str = "ATLAS_RUNTIME_ARGS";
const RUNTIME_CWD_ENV: &str = "ATLAS_RUNTIME_CWD";

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Inicia o Atlas Runtime em primeiro plano.
    Run,
    /// Desliga o Atlas Runtime em execução.
    Stop,
    /// Desliga o Atlas Runtime e inicia uma nova instância.
    Restart,
}

pub fn execute(
    command: ServerCommand,
    socket_override: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    match command {
        ServerCommand::Run => run_runtime(socket_override),
        ServerCommand::Stop => stop_runtime_command(socket_override),
        ServerCommand::Restart => {
            stop_existing_runtime(&RuntimePaths::from_environment(socket_override))?;
            run_runtime(socket_override)
        }
    }
}

fn run_runtime(socket_override: Option<&str>) -> Result<(), Box<dyn Error>> {
    let program = std::env::var(RUNTIME_PROGRAM_ENV).unwrap_or_else(|_| "npm".to_owned());
    let args = std::env::var(RUNTIME_ARGS_ENV)
        .map(|value| {
            value
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| vec!["run".to_owned(), "runtime".to_owned()]);
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(cwd) = std::env::var_os(RUNTIME_CWD_ENV).filter(|path| !path.is_empty()) {
        command.current_dir(cwd);
    }
    if let Some(socket_path) = socket_override {
        command.env("ATLAS_RUNTIME_SOCKET", socket_path);
    }
    command.env("ATLAS_RUNTIME_PROCESS", "1");
    let status = command.status()?;

    if status.success() || status.code().is_none() {
        return Ok(());
    }

    Err(io::Error::other(format!("Atlas Runtime exited with status {status}")).into())
}

fn stop_runtime_command(socket_override: Option<&str>) -> Result<(), Box<dyn Error>> {
    let paths = RuntimePaths::from_environment(socket_override);
    let stopped = stop_existing_runtime(&paths)?;
    if stopped {
        println!("Atlas Runtime stopped.");
    } else {
        println!("Atlas Runtime is not running.");
    }
    Ok(())
}

fn stop_existing_runtime(paths: &RuntimePaths) -> Result<bool, Box<dyn Error>> {
    let had_runtime_state = paths.pid_path.exists() || paths.socket_path.exists();
    if let Some(pid) = read_pid(&paths.pid_path)? {
        if process_is_alive(pid) {
            if !runtime_process_matches(pid) {
                return Err(io::Error::other(format!(
                    "Recusando desligar o PID {pid}: ele nao foi iniciado pelo Atlas."
                ))
                .into());
            }
            stop_runtime(pid, &paths.socket_path)?;
        } else {
            remove_if_exists(&paths.pid_path)?;
        }
    }

    if socket_is_active(&paths.socket_path) {
        return Err(io::Error::other(format!(
            "Atlas Runtime is active at {} but has no valid PID file.",
            paths.socket_path.display()
        ))
        .into());
    }

    remove_if_exists(&paths.socket_path)?;
    remove_if_exists(&paths.pid_path)?;
    Ok(had_runtime_state)
}

fn stop_runtime(pid: u32, socket_path: &Path) -> Result<(), Box<dyn Error>> {
    let status = Command::new("kill").arg(pid.to_string()).status()?;
    if !status.success() && process_is_alive(pid) {
        return Err(io::Error::other(format!("Failed to stop Atlas Runtime process {pid}")).into());
    }

    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while process_is_alive(pid) || socket_is_active(socket_path) {
        if Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "Timed out while stopping Atlas Runtime process {pid}."
            ))
            .into());
        }
        sleep(Duration::from_millis(50));
    }

    Ok(())
}

fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn socket_is_active(socket_path: &Path) -> bool {
    UnixStream::connect(socket_path).is_ok()
}

fn runtime_process_matches(pid: u32) -> bool {
    let environment_path = format!("/proc/{pid}/environ");
    fs::read(environment_path)
        .map(|environment| {
            environment
                .split(|byte| *byte == 0)
                .any(|variable| variable == RUNTIME_PROCESS_MARKER.as_bytes())
        })
        .unwrap_or(false)
}

fn read_pid(pid_path: &Path) -> io::Result<Option<u32>> {
    match fs::read_to_string(pid_path) {
        Ok(contents) => contents
            .trim()
            .parse::<u32>()
            .map(Some)
            .map_err(|error| io::Error::other(format!("Invalid Runtime PID file: {error}"))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

struct RuntimePaths {
    socket_path: PathBuf,
    pid_path: PathBuf,
}

impl RuntimePaths {
    fn from_environment(socket_override: Option<&str>) -> Self {
        let socket_path = socket_override
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("ATLAS_RUNTIME_SOCKET").map(PathBuf::from))
            .or_else(|| {
                std::env::var_os("XDG_RUNTIME_DIR")
                    .filter(|path| !path.is_empty())
                    .map(|path| PathBuf::from(path).join("atlas-runtime.sock"))
            })
            .unwrap_or_else(|| PathBuf::from(DEFAULT_RUNTIME_SOCKET_PATH));
        let mut pid_path = socket_path.clone().into_os_string();
        pid_path.push(".pid");

        Self {
            socket_path,
            pid_path: PathBuf::from(pid_path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimePaths;

    #[test]
    fn cli_socket_override_takes_precedence_over_environment() {
        let paths = RuntimePaths::from_environment(Some("/tmp/atlas-explicit.sock"));

        assert_eq!(
            paths.socket_path,
            std::path::PathBuf::from("/tmp/atlas-explicit.sock")
        );
        assert_eq!(
            paths.pid_path,
            std::path::PathBuf::from("/tmp/atlas-explicit.sock.pid")
        );
    }
}
