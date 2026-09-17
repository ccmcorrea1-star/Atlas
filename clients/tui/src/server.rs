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

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Inicia o Atlas Runtime em primeiro plano.
    Run,
    /// Desliga o Atlas Runtime em execução.
    Stop,
    /// Desliga o Atlas Runtime e inicia uma nova instância.
    Restart,
}

pub fn execute(command: ServerCommand) -> Result<(), Box<dyn Error>> {
    match command {
        ServerCommand::Run => run_runtime(),
        ServerCommand::Stop => stop_runtime_command(),
        ServerCommand::Restart => {
            stop_existing_runtime(&RuntimePaths::from_environment())?;
            run_runtime()
        }
    }
}

fn run_runtime() -> Result<(), Box<dyn Error>> {
    let status = Command::new("npm")
        .args(["run", "runtime"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if status.success() || status.code().is_none() {
        return Ok(());
    }

    Err(io::Error::other(format!("Atlas Runtime exited with status {status}")).into())
}

fn stop_runtime_command() -> Result<(), Box<dyn Error>> {
    let paths = RuntimePaths::from_environment();
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
                    "Refusing to stop PID {pid}: it is not an Atlas Runtime process."
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

fn runtime_process_matches(pid: u32) -> bool {
    let command_line_path = format!("/proc/{pid}/cmdline");
    fs::read(command_line_path)
        .map(|command_line| String::from_utf8_lossy(&command_line).contains("runtime/server"))
        .unwrap_or(false)
}

fn socket_is_active(socket_path: &Path) -> bool {
    UnixStream::connect(socket_path).is_ok()
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
    fn from_environment() -> Self {
        let socket_path = std::env::var_os("ATLAS_RUNTIME_SOCKET")
            .map(PathBuf::from)
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
