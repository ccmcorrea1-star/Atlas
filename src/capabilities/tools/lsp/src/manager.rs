//! Cliente residente: detecta o servidor, inicia sob demanda, reutiliza
//! ativos, sincroniza documentos e coleta publishDiagnostics. O daemon Unix
//! mantem servidores vivos entre chamadas do runtime executable.

use crate::detect::{self, ServerSpec};
use crate::lsp;
use crate::protocol;
use crate::server::{self, ServerConn};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Servidores sao chaveados por comando + argumentos + raiz do projeto.
#[derive(Hash, PartialEq, Eq, Clone)]
struct ServerKey {
    command: String,
    args: Vec<String>,
    root: PathBuf,
}

struct OpenDoc {
    version: i64,
    text: String,
}

type Spawner = Box<dyn Fn(&ServerSpec, &Path) -> std::io::Result<Arc<ServerConn>> + Send>;

/// Quietude apos a ultima publicacao em servidor frio (vazio e depois real).
const FRESH_GRACE: Duration = Duration::from_secs(4);
/// Quietude em servidor reutilizado (quente, publica rapido).
const REUSE_GRACE: Duration = Duration::from_millis(1500);

/// Resultado de uma analise no vocabulario do contrato Atlas.
pub struct Outcome {
    pub status: &'static str,
    pub error: String,
    pub path: Option<String>,
    pub diagnostics: Vec<Value>,
}

fn failed(error: String) -> Outcome {
    Outcome {
        status: "failed",
        error,
        path: None,
        diagnostics: Vec::new(),
    }
}

fn unavailable(error: String) -> Outcome {
    Outcome {
        status: "unavailable",
        error,
        path: None,
        diagnostics: Vec::new(),
    }
}

pub struct Client {
    servers: HashMap<ServerKey, Arc<ServerConn>>,
    docs: HashMap<String, OpenDoc>,
    spawner: Spawner,
    diag_timeout: Duration,
    fresh_grace: Duration,
    reuse_grace: Duration,
}

impl Client {
    pub fn new() -> Self {
        Client {
            servers: HashMap::new(),
            docs: HashMap::new(),
            spawner: Box::new(|spec, root| ServerConn::spawn(spec.command, spec.args, root)),
            diag_timeout: server::DIAG_TIMEOUT,
            fresh_grace: FRESH_GRACE,
            reuse_grace: REUSE_GRACE,
        }
    }

    #[cfg(test)]
    fn with_spawner(spawner: Spawner) -> Self {
        Client {
            servers: HashMap::new(),
            docs: HashMap::new(),
            spawner,
            diag_timeout: Duration::from_secs(2),
            fresh_grace: Duration::from_millis(300),
            reuse_grace: Duration::from_millis(200),
        }
    }

    pub fn diagnose(&mut self, path: &str) -> Outcome {
        self.diagnose_attempts(path, 2)
    }

    /// Até 2 tentativas: queda do servidor no meio da chamada recria tudo.
    fn diagnose_attempts(&mut self, path: &str, attempts: u32) -> Outcome {
        let canonical = match std::fs::canonicalize(path) {
            Ok(candidate) if candidate.is_file() => candidate,
            _ => return failed(format!("path '{path}' does not exist or is not a file")),
        };
        let extension = canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();
        let spec = match detect::spec_for_extension(&extension) {
            Some(spec) => spec,
            None => {
                return unavailable(format!(
                    "no language server registered for '.{extension}' files"
                ));
            }
        };
        // Servidor ausente eh indisponibilidade, nunca fallback para build/lint.
        if detect::find_in_path(spec.command).is_none() {
            return unavailable(format!(
                "language server '{}' is not installed",
                spec.command
            ));
        }
        let text = match std::fs::read_to_string(&canonical) {
            Ok(text) => text,
            Err(error) => return failed(format!("cannot read '{path}': {error}")),
        };
        let uri = format!("file://{}", canonical.display());
        let parent = canonical.parent().unwrap_or(Path::new("/"));
        let root = detect::project_root(parent, spec.markers);
        let key = ServerKey {
            command: spec.command.to_string(),
            args: spec.args.iter().map(|arg| arg.to_string()).collect(),
            root: root.clone(),
        };
        if let Some(dead) = self.servers.get(&key).filter(|conn| conn.is_dead()) {
            let dead = Arc::clone(dead);
            dead.kill();
            self.servers.remove(&key);
        }
        let mut fresh = false;
        if !self.servers.contains_key(&key) {
            fresh = true;
            let conn = match (self.spawner)(spec, &root) {
                Ok(conn) => conn,
                Err(error) => {
                    return failed(format!(
                        "language server '{}' failed to start: {error}",
                        spec.command
                    ));
                }
            };
            let root_uri = format!("file://{}", root.display());
            if let Err(error) = conn.initialize(&root_uri, server::INIT_TIMEOUT) {
                conn.kill();
                // Queda durante o handshake tambem recria (1x).
                if attempts > 1 {
                    return self.diagnose_attempts(path, attempts - 1);
                }
                return failed(format!(
                    "language server '{}' failed to start: {error}",
                    spec.command
                ));
            }
            self.servers.insert(key.clone(), conn);
        }
        let conn = Arc::clone(&self.servers[&key]);
        let since = conn.current_seq();
        let version = match self.docs.get(&uri) {
            None => 1,
            Some(doc) => doc.version + 1,
        };
        // Servidores deduplicam didChange de conteudo identico e nada
        // republicam: texto igual reabre o documento (quente, rapido).
        let reopen = matches!(self.docs.get(&uri), Some(doc) if doc.text == text);
        if reopen && conn.notify(&lsp::did_close(&uri)).is_err() {
            return self.restart_and_retry(path, &key, &uri, attempts);
        }
        let sync = if !reopen && self.docs.contains_key(&uri) {
            lsp::did_change(&uri, version, &text)
        } else {
            lsp::did_open(&uri, spec.language_id, version, &text)
        };
        if conn.notify(&sync).is_err() {
            return self.restart_and_retry(path, &key, &uri, attempts);
        }
        self.docs.insert(
            uri.clone(),
            OpenDoc {
                version,
                text: text.clone(),
            },
        );
        let grace = if fresh {
            self.fresh_grace
        } else {
            self.reuse_grace
        };
        match conn.wait_diagnostics(&uri, Some(version), since, self.diag_timeout, grace) {
            Ok(raw) => {
                let diagnostics = raw.iter().filter_map(lsp::to_contract_diagnostic).collect();
                Outcome {
                    status: "success",
                    error: String::new(),
                    path: Some(canonical.to_string_lossy().into_owned()),
                    diagnostics,
                }
            }
            Err(error) if error.contains("in time") => {
                // Timeout autodestrói o servidor travado; proxima chamada recria.
                conn.kill();
                self.servers.remove(&key);
                self.docs.remove(&uri);
                Outcome {
                    status: "timed_out",
                    error,
                    path: Some(canonical.to_string_lossy().into_owned()),
                    diagnostics: Vec::new(),
                }
            }
            Err(_) => self.restart_and_retry(path, &key, &uri, attempts),
        }
    }

    /// Descarta servidor/documento e tenta de novo (1x); sem tentativas, falha.
    fn restart_and_retry(
        &mut self,
        path: &str,
        key: &ServerKey,
        uri: &str,
        attempts: u32,
    ) -> Outcome {
        if let Some(conn) = self.servers.remove(key) {
            conn.kill();
        }
        self.docs.remove(uri);
        if attempts > 1 {
            return self.diagnose_attempts(path, attempts - 1);
        }
        failed("language server exited".to_string())
    }
}

/// Caminho do socket do daemon residente com namespace por instalacao:
/// diretorio de runtime do usuario + hash estavel da raiz do Atlas.
/// Duas instalacoes legitimas nunca dividem um daemon; a mesma instalacao
/// sempre reutiliza o seu.
pub fn socket_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    socket_path_for(&exe)
}

/// Deriva o socket para um binario dado (testavel sem processo real).
pub fn socket_path_for(exe: &Path) -> PathBuf {
    let root = install_root(exe);
    let key = std::fs::canonicalize(&root)
        .unwrap_or(root)
        .to_string_lossy()
        .into_owned();
    runtime_dir().join(format!("atlas-lsp-{:016x}.sock", fnv1a64(&key)))
}

/// Sobe do binario ate a raiz do Atlas (package.json + capabilities);
/// sem marcadores, usa o diretorio do binario.
fn install_root(exe: &Path) -> PathBuf {
    let mut current = exe.parent().map(Path::to_path_buf);
    while let Some(directory) = current {
        if directory.join("package.json").is_file()
            && directory.join("src/capabilities/tools/lsp").is_dir()
        {
            return directory;
        }
        current = directory.parent().map(Path::to_path_buf);
    }
    exe.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir)
}

/// Diretorio de runtime do usuario (privado 0700) ou temp como fallback.
fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute())
        .unwrap_or_else(std::env::temp_dir)
}

/// FNV-1a 64 bits: hash deterministico entre processos (DefaultHasher usa
/// chaves aleatorias e nao serve para nomes estaveis).
fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Socket legado sem namespace (versoes anteriores); limpeza best-effort.
fn legacy_socket_path() -> PathBuf {
    std::env::temp_dir().join("atlas-lsp-manager.sock")
}

/// Laco do daemon: uma thread por conexao, Client compartilhado. Se o
/// binario foi rebuildado (mtime mudou), responde a chamada e se aposenta
/// para a proxima invocar um daemon com o codigo novo.
pub fn run_manager(sock: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(sock);
    let listener = UnixListener::bind(sock)?;
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_mtime = std::fs::metadata(&exe)
        .and_then(|meta| meta.modified())
        .ok();
    let client = Arc::new(Mutex::new(Client::new()));
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let client = Arc::clone(&client);
        let exe = exe.clone();
        let sock = sock.to_path_buf();
        thread::spawn(move || {
            handle_connection(stream, client);
            if exe_mtime.is_some() && is_stale(&exe, exe_mtime.as_ref()) {
                // Some com o socket para o proximo frontend criar outro daemon.
                let _ = std::fs::remove_file(&sock);
                std::process::exit(0);
            }
        });
    }
    Ok(())
}

/// Binario trocado ou sumido (unlink de rebuild) significa codigo obsoleto.
fn is_stale(exe: &Path, mtime: Option<&std::time::SystemTime>) -> bool {
    match (
        std::fs::metadata(exe).and_then(|meta| meta.modified()).ok(),
        mtime,
    ) {
        (Some(current), Some(born)) => current != *born,
        _ => true,
    }
}

fn handle_connection(stream: UnixStream, client: Arc<Mutex<Client>>) {
    let mut reader = match stream.try_clone() {
        Ok(clone) => BufReader::new(clone),
        Err(_) => return,
    };
    let mut writer = stream;
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let response = match protocol::parse_request(&line) {
        Err(error) => protocol::response("", "failed", &error, None, &[]),
        Ok(request) => {
            let outcome = client.lock().unwrap().diagnose(&request.path);
            protocol::response(
                &request.target,
                outcome.status,
                &outcome.error,
                outcome.path.as_deref(),
                &outcome.diagnostics,
            )
        }
    };
    let _ = writer.write_all(response.as_bytes());
    let _ = writer.write_all(b"\n");
}

/// Garante o daemon no ar: conecta, ou inicia destacado e aguarda o socket.
/// Remove com seguranca o socket legado sem namespace (so o arquivo; o
/// daemon antigo, se vivo, aposenta-se sozinho ao proximo rebuild).
pub fn ensure_manager(exe: &Path, sock: &Path) -> Result<(), String> {
    let legacy = legacy_socket_path();
    if legacy != sock {
        let _ = std::fs::remove_file(&legacy);
    }
    if UnixStream::connect(sock).is_ok() {
        return Ok(());
    }
    let _ = std::fs::remove_file(sock);
    Command::new(exe)
        .arg("--manager")
        .arg(sock)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start lsp manager: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if UnixStream::connect(sock).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("lsp manager did not come up".to_string())
}

/// Envia a requisicao (linha unica) e le a resposta (linha unica).
pub fn call_manager(sock: &Path, request: &Value) -> Result<Value, String> {
    let mut stream =
        UnixStream::connect(sock).map_err(|error| format!("cannot reach lsp manager: {error}"))?;
    let mut line =
        serde_json::to_string(request).map_err(|error| format!("invalid request: {error}"))?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|error| format!("cannot reach lsp manager: {error}"))?;
    let mut reader = BufReader::new(stream);
    let mut output = String::new();
    reader
        .read_line(&mut output)
        .map_err(|error| format!("lsp manager reply failed: {error}"))?;
    serde_json::from_str(&output).map_err(|error| format!("lsp manager reply failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream as StdUnixStream;
    use std::sync::MutexGuard;

    static PATH_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn obsoleto_detecta_binario_trocado_ou_sumido() {
        let file = std::env::temp_dir().join(format!("atlas-lsp-stale-{}", std::process::id()));
        std::fs::write(&file, "x").unwrap();
        let born = std::fs::metadata(&file)
            .and_then(|meta| meta.modified())
            .ok();
        assert!(!is_stale(&file, born.as_ref()));
        assert!(is_stale(&file.join("nao-existe"), born.as_ref()));
        std::fs::remove_file(&file).unwrap();
        assert!(is_stale(&file, born.as_ref()));
    }

    /// Duas instalacoes do Atlas nunca dividem socket/daemon; a mesma raiz
    /// deriva sempre o mesmo caminho (reutilizacao preservada).
    #[test]
    fn socket_isola_instalacoes() {
        let base = std::env::temp_dir().join(format!("atlas-lsp-roots-{}", std::process::id()));
        let exe_a = fake_install(&base, "a");
        let exe_b = fake_install(&base, "b");
        let sock_a = socket_path_for(&exe_a);
        let sock_b = socket_path_for(&exe_b);
        assert_ne!(sock_a, sock_b, "instalacoes distintas colidiram");
        assert_eq!(sock_a, socket_path_for(&exe_a), "mesma instalacao variou");
        assert!(sock_a.starts_with(runtime_dir()));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn socket_respeita_runtime_dir_do_usuario() {
        let custom = std::env::temp_dir().join(format!("atlas-lsp-rtdir-{}", std::process::id()));
        std::fs::create_dir_all(&custom).unwrap();
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &custom) };
        let parent = socket_path_for(Path::new("/opt/atlas/bin/runtime"))
            .parent()
            .unwrap()
            .to_path_buf();
        unsafe {
            match &previous {
                Some(value) => std::env::set_var("XDG_RUNTIME_DIR", value),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
        }
        std::fs::remove_dir_all(&custom).unwrap();
        assert_eq!(parent, custom);
    }

    /// Monta uma instalacao falsa (marcadores package.json + capabilities).
    fn fake_install(base: &Path, name: &str) -> PathBuf {
        let root = base.join(name);
        let bin = root.join("src/capabilities/tools/lsp/diagnostics");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(root.join("package.json"), "{}\n").unwrap();
        bin.join("runtime")
    }

    /// Poe um binario falso no PATH para passar na verificacao de
    /// disponibilidade; restaura tudo no drop.
    struct FakePath {
        _guard: MutexGuard<'static, ()>,
        directory: PathBuf,
        previous: Option<std::ffi::OsString>,
    }

    impl FakePath {
        fn install(command: &str) -> Self {
            let guard = PATH_LOCK.lock().unwrap();
            let directory =
                std::env::temp_dir().join(format!("atlas-lsp-fakebin-{}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            let binary = directory.join(command);
            std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            let previous = std::env::var_os("PATH");
            let mut paths = vec![directory.clone().into_os_string()];
            if let Some(ref current) = previous {
                // Estende com as entradas (PATH inteira como um elemento quebra).
                paths.extend(std::env::split_paths(current).map(|item| item.into_os_string()));
            }
            let joined = std::env::join_paths(paths).unwrap();
            unsafe { std::env::set_var("PATH", &joined) };
            FakePath {
                _guard: guard,
                directory,
                previous,
            }
        }
    }

    impl Drop for FakePath {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(path) => std::env::set_var("PATH", path),
                    None => std::env::remove_var("PATH"),
                }
            }
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    /// Servidor falso que responde initialize e publica um erro por sync.
    fn fake_rs_server(mut reader: BufReader<StdUnixStream>, mut writer: StdUnixStream) {
        let init = match lsp::read_message(&mut reader) {
            Ok(message) => message,
            Err(_) => return,
        };
        let id = init["id"].as_u64().unwrap_or(0);
        if lsp::write_message(
            &mut writer,
            &json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {}}}),
        )
        .is_err()
        {
            return;
        }
        let _ = lsp::read_message(&mut reader); // initialized
        loop {
            let sync = match lsp::read_message(&mut reader) {
                Ok(message) => message,
                Err(_) => return,
            };
            let document = &sync["params"]["textDocument"];
            let (Some(uri), Some(version)) =
                (document["uri"].as_str(), document["version"].as_i64())
            else {
                continue;
            };
            let reply = json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": uri,
                    "version": version,
                    "diagnostics": [{
                        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                        "severity": 1,
                        "message": "falso",
                        "source": "falso",
                    }],
                },
            });
            if lsp::write_message(&mut writer, &reply).is_err() {
                return;
            }
        }
    }

    fn spawn_fake(spawns: Arc<Mutex<usize>>) -> Spawner {
        Box::new(move |_, _| {
            *spawns.lock().unwrap() += 1;
            let (client_reader, server_writer) = StdUnixStream::pair().unwrap();
            let (server_reader, client_writer) = StdUnixStream::pair().unwrap();
            let unblock = client_reader.try_clone().unwrap();
            let conn = ServerConn::from_streams(
                BufReader::new(client_reader),
                client_writer,
                None,
                Box::new(move || {
                    let _ = unblock.shutdown(std::net::Shutdown::Both);
                }),
            );
            thread::spawn(move || fake_rs_server(BufReader::new(server_reader), server_writer));
            Ok(conn)
        })
    }

    fn fixture(name: &str, content: &str) -> PathBuf {
        let file = std::env::temp_dir().join(format!("atlas-lsp-{name}-{}.rs", std::process::id()));
        std::fs::write(&file, content).unwrap();
        file
    }

    #[test]
    fn lifecycle_reutiliza_servidor_ativo() {
        let _path = FakePath::install("rust-analyzer");
        let spawns = Arc::new(Mutex::new(0));
        let mut client = Client::with_spawner(spawn_fake(Arc::clone(&spawns)));
        let file = fixture("reuse", "fn main() {}\n");
        for _ in 0..2 {
            let outcome = client.diagnose(file.to_str().unwrap());
            assert_eq!(outcome.status, "success", "erro: {}", outcome.error);
            assert_eq!(outcome.diagnostics.len(), 1);
            assert_eq!(outcome.diagnostics[0]["severity"], "error");
        }
        assert_eq!(*spawns.lock().unwrap(), 1);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn lifecycle_crash_recria_servidor() {
        let _path = FakePath::install("rust-analyzer");
        let spawns = Arc::new(Mutex::new(0));
        let spawns_clone = Arc::clone(&spawns);
        let spawner: Spawner = Box::new(move |_, _| {
            let count = {
                *spawns_clone.lock().unwrap() += 1;
                *spawns_clone.lock().unwrap()
            };
            let (client_reader, mut server_writer) = StdUnixStream::pair().unwrap();
            let (server_reader, client_writer) = StdUnixStream::pair().unwrap();
            let unblock = client_reader.try_clone().unwrap();
            let conn = ServerConn::from_streams(
                BufReader::new(client_reader),
                client_writer,
                None,
                Box::new(move || {
                    let _ = unblock.shutdown(std::net::Shutdown::Both);
                }),
            );
            if count == 1 {
                // Primeira instancia cai logo apos o handshake, sem publicar.
                thread::spawn(move || {
                    let mut reader = BufReader::new(server_reader);
                    let init = lsp::read_message(&mut reader).unwrap();
                    let id = init["id"].as_u64().unwrap_or(0);
                    let _ = lsp::write_message(
                        &mut server_writer,
                        &json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    );
                });
            } else {
                thread::spawn(move || fake_rs_server(BufReader::new(server_reader), server_writer));
            }
            Ok(conn)
        });
        let mut client = Client::with_spawner(spawner);
        let file = fixture("crash", "fn main() {}\n");
        let outcome = client.diagnose(file.to_str().unwrap());
        assert_eq!(outcome.status, "success", "erro: {}", outcome.error);
        assert_eq!(*spawns.lock().unwrap(), 2);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn protocolo_timeout_descarta_servidor_travado() {
        let _path = FakePath::install("rust-analyzer");
        let spawns = Arc::new(Mutex::new(0));
        let spawns_clone = Arc::clone(&spawns);
        let spawner: Spawner = Box::new(move |_, _| {
            *spawns_clone.lock().unwrap() += 1;
            let (client_reader, mut server_writer) = StdUnixStream::pair().unwrap();
            let (server_reader, client_writer) = StdUnixStream::pair().unwrap();
            let unblock = client_reader.try_clone().unwrap();
            let conn = ServerConn::from_streams(
                BufReader::new(client_reader),
                client_writer,
                None,
                Box::new(move || {
                    let _ = unblock.shutdown(std::net::Shutdown::Both);
                }),
            );
            // Handshake ok, mas nunca publica: servidor travado.
            thread::spawn(move || {
                let mut reader = BufReader::new(server_reader);
                let init = lsp::read_message(&mut reader).unwrap();
                let id = init["id"].as_u64().unwrap_or(0);
                let _ = lsp::write_message(
                    &mut server_writer,
                    &json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                );
                let _ = lsp::read_message(&mut reader);
                let _ = lsp::read_message(&mut reader);
                thread::sleep(Duration::from_secs(30));
            });
            Ok(conn)
        });
        let mut client = Client::with_spawner(spawner);
        client.diag_timeout = Duration::from_millis(300);
        let file = fixture("stuck", "fn main() {}\n");
        let outcome = client.diagnose(file.to_str().unwrap());
        assert_eq!(outcome.status, "timed_out");
        assert!(client.servers.is_empty());
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn diagnostics_extensao_desconhecida_e_indisponivel() {
        let spawns = Arc::new(Mutex::new(0));
        let mut client = Client::with_spawner(spawn_fake(Arc::clone(&spawns)));
        let file = fixture("unknown", "x").with_extension("xyz");
        std::fs::write(&file, "x").unwrap();
        let outcome = client.diagnose(file.to_str().unwrap());
        assert_eq!(outcome.status, "unavailable");
        assert!(outcome.error.contains(".xyz"));
        assert_eq!(*spawns.lock().unwrap(), 0);
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn diagnostics_arquivo_inexistente_falha() {
        let spawns = Arc::new(Mutex::new(0));
        let mut client = Client::with_spawner(spawn_fake(Arc::clone(&spawns)));
        let outcome = client.diagnose("/tmp/atlas-lsp-nao-existe-12345.rs");
        assert_eq!(outcome.status, "failed");
        assert_eq!(*spawns.lock().unwrap(), 0);
    }

    #[test]
    fn diagnostics_rust_analyzer_real() {
        // Serializa contra os PATHs falsos: sem isso o dummy shadowing o real.
        let _lock = PATH_LOCK.lock().unwrap();
        if detect::find_in_path("rust-analyzer").is_none() {
            eprintln!("skipped: rust-analyzer is not installed");
            return;
        }
        let base = std::env::temp_dir().join(format!("atlas-lsp-ra-{}", std::process::id()));
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            base.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let main = src.join("main.rs");
        std::fs::write(&main, "fn main() {\n    let numero: i32 = \"nao e numero\";\n    println!(\"{numero}\");\n}\n").unwrap();
        let mut client = Client::new();
        let mut found_error = false;
        for _ in 0..2 {
            let outcome = client.diagnose(main.to_str().unwrap());
            assert_eq!(outcome.status, "success", "erro: {}", outcome.error);
            if outcome
                .diagnostics
                .iter()
                .any(|diag| diag["severity"] == "error")
            {
                found_error = true;
                break;
            }
        }
        std::fs::remove_dir_all(&base).unwrap();
        assert!(found_error, "rust-analyzer nao reportou o erro de tipo");
    }
}
