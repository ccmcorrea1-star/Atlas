//! Runtime executable da tool lsp.diagnostics: fala o protocolo Atlas na
//! entrada padrao e delega ao manager residente via socket Unix.

mod detect;
mod lsp;
mod manager;
mod protocol;
mod server;

use std::io::Read;

/// Modo interno do daemon residente (iniciado sob demanda, sem output).
fn run_manager_mode(sock: &str) -> i32 {
    match manager::run_manager(std::path::Path::new(sock)) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("atlas-lsp manager failed: {error}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--manager") {
        let sock = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| manager::socket_path().to_string_lossy().into_owned());
        std::process::exit(run_manager_mode(&sock));
    }

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).unwrap_or(0) == 0 {
        print!(
            "{}",
            protocol::response("", "failed", "empty request", None, &[])
        );
        return;
    }
    // Requisicao valida vira linha unica antes de ir ao manager.
    let request: serde_json::Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(error) => {
            print!(
                "{}",
                protocol::response(
                    "",
                    "failed",
                    &format!("invalid JSON request: {error}"),
                    None,
                    &[]
                )
            );
            return;
        }
    };
    let target = request
        .get("target")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let respond = |status: &str, error: &str| {
        println!("{}", protocol::response(&target, status, error, None, &[]));
    };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            respond("failed", &format!("cannot locate runtime binary: {error}"));
            return;
        }
    };
    let sock = manager::socket_path();
    if let Err(error) = manager::ensure_manager(&exe, &sock) {
        respond("failed", &error);
        return;
    }
    match manager::call_manager(&sock, &request) {
        Ok(response) => print_response(&target, response),
        Err(_) => {
            // O daemon pode ter morrido na concorrencia (crash, rebuild com
            // socket pendente): recria do zero e tenta uma ultima vez.
            let _ = std::fs::remove_file(&sock);
            match manager::ensure_manager(&exe, &sock)
                .map_err(|error| error.to_string())
                .and_then(|_| manager::call_manager(&sock, &request))
            {
                Ok(response) => print_response(&target, response),
                Err(error) => respond("failed", &error),
            }
        }
    }
}

fn print_response(target: &str, response: serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string(&response).unwrap_or_else(|_| protocol::response(
            target,
            "failed",
            "response serialization failed",
            None,
            &[]
        ))
    );
}
