//! Processo language server: JSON-RPC por stdio, handshake initialize,
//! roteamento de respostas e coleta de publishDiagnostics com timeout.

use crate::lsp;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Tempo para o handshake initialize (servidor frio carrega o projeto).
pub const INIT_TIMEOUT: Duration = Duration::from_secs(60);
/// Tempo de espera por publishDiagnostics apos sincronizar o documento.
pub const DIAG_TIMEOUT: Duration = Duration::from_secs(30);

/// Uma publicacao recebida do servidor.
struct Published {
    seq: u64,
    uri: String,
    version: Option<i64>,
    diagnostics: Vec<Value>,
}

struct Shared {
    pending: HashMap<u64, mpsc::Sender<Result<Value, String>>>,
    published: Vec<Published>,
    seq: u64,
}

impl Shared {
    fn new() -> Self {
        Shared {
            pending: HashMap::new(),
            published: Vec::new(),
            seq: 0,
        }
    }
}

/// Conexao com um language server; thread de leitura dedicada roteia
/// respostas (por id) e notificacoes publishDiagnostics (por uri).
pub struct ServerConn {
    next_id: AtomicU64,
    state: Mutex<Shared>,
    cvar: Condvar,
    dead: AtomicBool,
    dead_reason: Mutex<String>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Option<Child>>,
    shutdown_extra: Box<dyn Fn() + Send + Sync>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
}

impl ServerConn {
    /// Inicia o servidor real com cwd na raiz do projeto.
    pub fn spawn(
        command: &str,
        args: &[&str],
        current_dir: &std::path::Path,
    ) -> io::Result<Arc<ServerConn>> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(current_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("language server stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("language server stdout unavailable"))?;
        Ok(Self::from_streams(
            BufReader::new(stdout),
            stdin,
            Some(child),
            Box::new(|| {}),
        ))
    }

    /// Constroi sobre streams quaisquer; testes injetam um servidor falso.
    pub fn from_streams<R, W>(
        reader: R,
        writer: W,
        child: Option<Child>,
        shutdown_extra: Box<dyn Fn() + Send + Sync>,
    ) -> Arc<ServerConn>
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        let conn = Arc::new(ServerConn {
            next_id: AtomicU64::new(1),
            state: Mutex::new(Shared::new()),
            cvar: Condvar::new(),
            dead: AtomicBool::new(false),
            dead_reason: Mutex::new(String::new()),
            writer: Mutex::new(Box::new(writer)),
            child: Mutex::new(child),
            shutdown_extra,
            reader_thread: Mutex::new(None),
        });
        let worker = Arc::clone(&conn);
        let handle = thread::spawn(move || reader_loop(&worker, reader));
        *conn.reader_thread.lock().unwrap() = Some(handle);
        conn
    }

    /// Marca o servidor como morto e falha todo request pendente.
    fn mark_dead(&self, reason: String) {
        if self.dead.swap(true, Ordering::SeqCst) {
            return;
        }
        *self.dead_reason.lock().unwrap() = reason.clone();
        let mut state = self.state.lock().unwrap();
        for (_, sender) in state.pending.drain() {
            let _ = sender.send(Err(reason.clone()));
        }
        // Nao limpa published: o ultimo diagnostico continua valido para
        // quem ja sincronizou o documento.
        self.cvar.notify_all();
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }

    fn dead_error(&self) -> String {
        let reason = self.dead_reason.lock().unwrap();
        if reason.is_empty() {
            "language server exited".to_string()
        } else {
            reason.clone()
        }
    }

    /// Maior seq visto; quem sincroniza registra antes de enviar didOpen.
    pub fn current_seq(&self) -> u64 {
        self.state.lock().unwrap().seq
    }

    /// Envia request e aguarda a resposta com o id correspondente.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        if self.is_dead() {
            return Err(self.dead_error());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        {
            self.state.lock().unwrap().pending.insert(id, sender);
        }
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = lsp::write_message(&mut *self.writer.lock().unwrap(), &message)
            .map_err(|error| error.to_string())
        {
            self.state.lock().unwrap().pending.remove(&id);
            self.mark_dead(format!("language server write failed: {error}"));
            return Err(self.dead_error());
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.state.lock().unwrap().pending.remove(&id);
                Err(format!("language server request '{method}' timed out"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.dead_error()),
        }
    }

    /// Envia notificacao (sem resposta esperada).
    pub fn notify(&self, message: &Value) -> Result<(), String> {
        if self.is_dead() {
            return Err(self.dead_error());
        }
        lsp::write_message(&mut *self.writer.lock().unwrap(), message).map_err(|error| {
            self.mark_dead(format!("language server write failed: {error}"));
            self.dead_error()
        })
    }

    /// Handshake initialize + initialized.
    pub fn initialize(&self, root_uri: &str, timeout: Duration) -> Result<(), String> {
        let params = lsp::initialize_request(0, root_uri)["params"].clone();
        self.request("initialize", params, timeout)?;
        self.notify(&lsp::initialized_notification())
    }

    /// Aguarda publishDiagnostics do uri com seq posterior a `since_seq`.
    /// Com `baseline` (diagnosticos do ultimo retorno e texto alterado desde
    /// entao), ecos — republicacao do conteudo anterior antes da reanalise —
    /// nao assentam a espera: o retorno so sai no conteudo genuinamente novo
    /// (inclusive analise tardia de versao anterior), ou no ultimo eco apos
    /// `changed_grace` de quietude. Sem baseline, comportamento original.
    #[allow(clippy::too_many_arguments)]
    pub fn wait_diagnostics(
        &self,
        uri: &str,
        min_version: Option<i64>,
        since_seq: u64,
        timeout: Duration,
        grace: Duration,
        baseline: Option<&[Value]>,
        changed_grace: Duration,
    ) -> Result<Vec<Value>, String> {
        let deadline = std::time::Instant::now() + timeout;
        let mut guard = self.state.lock().unwrap();
        let mut latest_seq = since_seq;
        let mut latest_diags: Option<Vec<Value>> = None;
        let mut settle_from: Option<std::time::Instant> = None;
        let mut same_since: Option<std::time::Instant> = None;
        loop {
            let mut fresh: Option<(u64, Vec<Value>)> = None;
            let mut echo: Option<(u64, Vec<Value>)> = None;
            for item in guard
                .published
                .iter()
                .filter(|item| item.seq > latest_seq && item.uri == uri)
            {
                let echoes = baseline.is_some_and(|base| base == item.diagnostics.as_slice());
                let version_ok = item
                    .version
                    .is_none_or(|v| min_version.is_none_or(|min| v >= min));
                // Versao antiga com conteudo novo e analise tardia: vale como
                // fresh, senao o filtro de versao a descartaria para sempre a
                // cada novo sync.
                let slot = if echoes {
                    &mut echo
                } else if version_ok || item.version.is_some() {
                    &mut fresh
                } else {
                    continue;
                };
                let replace = match slot {
                    None => true,
                    Some((seq, _)) => item.seq > *seq,
                };
                if replace {
                    *slot = Some((item.seq, item.diagnostics.clone()));
                }
            }
            let now = std::time::Instant::now();
            if let Some((seq, diags)) = fresh {
                latest_seq = seq;
                latest_diags = Some(diags);
                settle_from = Some(now);
            } else if let Some((seq, diags)) = echo {
                latest_seq = seq;
                if settle_from.is_none() {
                    // Eco antes do conteudo novo: registra sem assentar.
                    latest_diags = Some(diags);
                    same_since = Some(now);
                }
            }
            if settle_from.is_some_and(|since| since.elapsed() >= grace) {
                guard.published.clear();
                return Ok(latest_diags.unwrap_or_default());
            }
            if baseline.is_some()
                && settle_from.is_none()
                && same_since.is_some_and(|since| since.elapsed() >= changed_grace)
            {
                // So ecos e quietude: a edicao nao alterou os diagnosticos.
                guard.published.clear();
                return Ok(latest_diags.unwrap_or_default());
            }
            if self.is_dead() {
                if let Some(diags) = latest_diags {
                    guard.published.clear();
                    return Ok(diags);
                }
                return Err(self.dead_error());
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                if let Some(diags) = latest_diags {
                    guard.published.clear();
                    return Ok(diags);
                }
                return Err("language server did not publish diagnostics in time".to_string());
            }
            let slice = (deadline - now).min(Duration::from_millis(100));
            let (next, _) = self.cvar.wait_timeout(guard, slice).unwrap();
            guard = next;
        }
    }

    /// Encerra o processo e a thread de leitura.
    pub fn kill(&self) {
        (self.shutdown_extra)();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.reader_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

/// Roteia mensagens do stdout: respostas por id, publishDiagnostics por uri.
fn reader_loop<R: BufRead>(conn: &Arc<ServerConn>, mut reader: R) {
    loop {
        match lsp::read_message(&mut reader) {
            Ok(message) => {
                if let Some(id) = message.get("id").and_then(Value::as_u64) {
                    let sender = conn.state.lock().unwrap().pending.remove(&id);
                    if let Some(sender) = sender {
                        let _ = sender.send(response_result(&message));
                    }
                    continue;
                }
                if message.get("method").and_then(Value::as_str)
                    == Some("textDocument/publishDiagnostics")
                {
                    push_published(conn, &message);
                }
            }
            Err(error) => {
                let _ = error;
                conn.mark_dead("language server exited".to_string());
                break;
            }
        }
    }
}

fn response_result(message: &Value) -> Result<Value, String> {
    if let Some(error) = message.get("error") {
        let detail = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!("language server error: {detail}"));
    }
    Ok(message.get("result").cloned().unwrap_or(Value::Null))
}

fn push_published(conn: &Arc<ServerConn>, message: &Value) {
    let params = match message.get("params") {
        Some(params) => params,
        None => return,
    };
    let uri = match params.get("uri").and_then(Value::as_str) {
        Some(uri) => uri.to_string(),
        None => return,
    };
    let version = params.get("version").and_then(Value::as_i64);
    let diagnostics = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut state = conn.state.lock().unwrap();
    state.seq += 1;
    let seq = state.seq;
    // Guarda só a publicacao mais recente por uri para limitar memoria.
    state.published.retain(|item| item.uri != uri);
    state.published.push(Published {
        seq,
        uri,
        version,
        diagnostics,
    });
    conn.cvar.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    /// Par bidirecional: a conn le de um lado e escreve no outro; o servidor
    /// falso usa o lado oposto de cada par.
    pub(crate) fn mock_pair() -> (Arc<ServerConn>, BufReader<UnixStream>, UnixStream) {
        let (client_reader, server_writer) = UnixStream::pair().unwrap();
        let (server_reader, client_writer) = UnixStream::pair().unwrap();
        // O killer clona o lado do leitor: libera a thread sem segurar ponta
        // do servidor, entao crash do falso gera EOF de verdade.
        let unblock = client_reader.try_clone().unwrap();
        let conn = ServerConn::from_streams(
            BufReader::new(client_reader),
            client_writer,
            None,
            Box::new(move || {
                let _ = unblock.shutdown(std::net::Shutdown::Both);
            }),
        );
        (conn, BufReader::new(server_reader), server_writer)
    }

    fn read_server_message(reader: &mut BufReader<UnixStream>) -> Value {
        lsp::read_message(reader).unwrap()
    }

    /// Servidor falso mínimo: responde initialize e publica diagnostico fixo.
    fn scripted_server(mut reader: BufReader<UnixStream>, mut writer: UnixStream, publish: Value) {
        let init = read_server_message(&mut reader);
        assert_eq!(init["method"], "initialize");
        let id = init["id"].as_u64().unwrap();
        lsp::write_message(
            &mut writer,
            &json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {}}}),
        )
        .unwrap();
        let _ = read_server_message(&mut reader); // initialized
        let open = read_server_message(&mut reader); // didOpen ou didChange
        assert!(
            open["method"]
                .as_str()
                .unwrap()
                .starts_with("textDocument/did")
        );
        let uri = open["params"]["textDocument"]["uri"]
            .as_str()
            .unwrap_or("file:///x")
            .to_string();
        let mut params = publish.clone();
        params["uri"] = Value::String(uri);
        lsp::write_message(
            &mut writer,
            &json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params}),
        )
        .unwrap();
    }

    fn canned_publish() -> Value {
        json!({
            "diagnostics": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                "severity": 1,
                "message": "erro falso",
                "source": "falso",
            }],
            "version": 1,
        })
    }

    #[test]
    fn lifecycle_handshake_e_coleta() {
        let (conn, server_reader, server_writer) = mock_pair();
        let server = thread::spawn(move || {
            scripted_server(server_reader, server_writer, canned_publish());
        });
        conn.initialize("file:///projeto", INIT_TIMEOUT).unwrap();
        let since = conn.current_seq();
        conn.notify(&lsp::did_open(
            "file:///projeto/a.rs",
            "rust",
            1,
            "fn main() {}\n",
        ))
        .unwrap();
        let diags = conn
            .wait_diagnostics(
                "file:///projeto/a.rs",
                Some(1),
                since,
                Duration::from_secs(5),
                Duration::from_millis(200),
                None,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(diags.len(), 1);
        let contract = lsp::to_contract_diagnostic(&diags[0]).unwrap();
        assert_eq!(contract["severity"], "error");
        assert_eq!(contract["message"], "erro falso");
        server.join().unwrap();
        conn.kill();
        assert!(conn.is_dead());
    }

    #[test]
    fn protocolo_request_expira() {
        let (conn, mut server_reader, _server_writer) = mock_pair();
        let server = thread::spawn(move || {
            let init = read_server_message(&mut server_reader);
            assert_eq!(init["method"], "initialize");
            // Nunca responde: o cliente deve expirar sozinho.
            thread::sleep(Duration::from_secs(1));
        });
        let error = conn
            .request("initialize", json!({}), Duration::from_millis(200))
            .unwrap_err();
        assert!(error.contains("timed out"), "inesperado: {error}");
        conn.kill();
        server.join().unwrap();
    }

    #[test]
    fn protocolo_filtra_por_uri_e_versao() {
        let (conn, mut server_reader, mut server_writer) = mock_pair();
        let uri = "file:///projeto/a.rs";
        let server = thread::spawn(move || {
            handshake(&mut server_reader, &mut server_writer);
            for version in [1, 2] {
                let open = read_server_message(&mut server_reader);
                assert!(
                    open["method"]
                        .as_str()
                        .unwrap()
                        .starts_with("textDocument/did")
                );
                publish(
                    &mut server_writer,
                    "file:///projeto/outro.rs",
                    version,
                    "estranho",
                );
                publish(&mut server_writer, uri, version, format!("v{version}"));
            }
        });
        conn.initialize("file:///projeto", INIT_TIMEOUT).unwrap();
        for version in [1, 2] {
            let since = conn.current_seq();
            let sync = if version == 1 {
                lsp::did_open(uri, "rust", version, "um\n")
            } else {
                lsp::did_change(uri, version, "dois\n")
            };
            conn.notify(&sync).unwrap();
            let diags = conn
                .wait_diagnostics(
                    uri,
                    Some(version),
                    since,
                    Duration::from_secs(5),
                    Duration::from_millis(200),
                    None,
                    Duration::from_secs(1),
                )
                .unwrap();
            assert_eq!(diags.len(), 1);
            assert_eq!(diags[0]["message"], format!("v{version}"));
        }
        server.join().unwrap();
        conn.kill();
    }

    #[test]
    fn lifecycle_crash_falha_requests() {
        let (conn, mut server_reader, mut server_writer) = mock_pair();
        let server = thread::spawn(move || {
            handshake(&mut server_reader, &mut server_writer);
            // Encerra sem aviso: queda do servidor.
        });
        conn.initialize("file:///projeto", INIT_TIMEOUT).unwrap();
        server.join().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !conn.is_dead() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(conn.is_dead());
        let error = conn
            .request("x", json!({}), Duration::from_secs(1))
            .unwrap_err();
        assert!(error.contains("exited"), "inesperado: {error}");
        conn.kill();
    }

    fn handshake(reader: &mut BufReader<UnixStream>, writer: &mut UnixStream) {
        let init = read_server_message(reader);
        let id = init["id"].as_u64().unwrap();
        lsp::write_message(
            writer,
            &json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {}}}),
        )
        .unwrap();
        let _ = read_server_message(reader); // initialized
    }

    fn publish(writer: &mut UnixStream, uri: &str, version: i64, message: impl Into<String>) {
        lsp::write_message(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": uri,
                    "version": version,
                    "diagnostics": [{
                        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                        "severity": 2,
                        "message": message.into(),
                    }],
                },
            }),
        )
        .unwrap();
    }

    /// Servidor que modela o rust-analyzer real apos edicao externa: primeiro
    /// republica o conteudo anterior carimbado com a versao nova (eco) e so
    /// depois publica a reanalise.
    fn echo_then_fresh_server(
        mut reader: BufReader<UnixStream>,
        mut writer: UnixStream,
        uri: &str,
        stale: Vec<Value>,
        fresh: Vec<Value>,
        done: mpsc::Receiver<()>,
    ) {
        handshake(&mut reader, &mut writer);
        let open = read_server_message(&mut reader); // didOpen v1
        assert_eq!(open["params"]["textDocument"]["version"], 1);
        publish_raw(&mut writer, uri, 1, &stale);
        let change = read_server_message(&mut reader); // didChange v2
        assert_eq!(change["method"], "textDocument/didChange");
        publish_raw(&mut writer, uri, 2, &stale); // eco com versao nova
        thread::sleep(Duration::from_millis(50));
        publish_raw(&mut writer, uri, 2, &fresh); // reanalise genuina
        // Aguarda o teste liberar: sair aqui derrubaria o writer (EOF) e o
        // ramo morto-devolucao mascararia a espera. kill() nao da EOF no lado
        // do servidor, entao o desbloqueio e por canal, antes do join.
        let _ = done.recv();
    }

    fn publish_raw(writer: &mut UnixStream, uri: &str, version: i64, diagnostics: &[Value]) {
        lsp::write_message(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {"uri": uri, "version": version, "diagnostics": diagnostics},
            }),
        )
        .unwrap();
    }

    fn diag(message: &str) -> Vec<Value> {
        vec![json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "severity": 1,
            "message": message,
        })]
    }

    #[test]
    fn espera_conteudo_novo_em_vez_do_eco() {
        let (conn, server_reader, server_writer) = mock_pair();
        let uri = "file:///projeto/a.rs";
        let stale = diag("antigo");
        let fresh = diag("novo");
        let stale_clone = stale.clone();
        let fresh_clone = fresh.clone();
        let (done_tx, done_rx): (mpsc::Sender<()>, mpsc::Receiver<()>) = mpsc::channel();
        let server = thread::spawn(move || {
            echo_then_fresh_server(
                server_reader,
                server_writer,
                uri,
                stale_clone,
                fresh_clone,
                done_rx,
            );
        });
        conn.initialize("file:///projeto", INIT_TIMEOUT).unwrap();
        let since = conn.current_seq();
        conn.notify(&lsp::did_open(uri, "rust", 1, "um\n")).unwrap();
        let first = conn
            .wait_diagnostics(
                uri,
                Some(1),
                since,
                Duration::from_secs(5),
                Duration::from_millis(200),
                None,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(first, stale);
        let since = conn.current_seq();
        conn.notify(&lsp::did_change(uri, 2, "dois\n")).unwrap();
        // Sem baseline o eco assentaria aqui; com baseline, o retorno e o fresco.
        let second = conn
            .wait_diagnostics(
                uri,
                Some(2),
                since,
                Duration::from_secs(5),
                Duration::from_millis(200),
                Some(&stale),
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(second, fresh);
        drop(done_tx);
        server.join().unwrap();
        conn.kill();
    }

    #[test]
    fn eco_solitario_retorna_apos_quietude_sem_assentar_errado() {
        let (conn, mut server_reader, mut server_writer) = mock_pair();
        let uri = "file:///projeto/a.rs";
        let stale = diag("mesmo");
        let stale_clone = stale.clone();
        let (done_tx, done_rx): (mpsc::Sender<()>, mpsc::Receiver<()>) = mpsc::channel();
        let server = thread::spawn(move || {
            handshake(&mut server_reader, &mut server_writer);
            let _ = read_server_message(&mut server_reader); // didOpen
            publish_raw(&mut server_writer, uri, 1, &stale_clone);
            let _ = read_server_message(&mut server_reader); // didChange
            publish_raw(&mut server_writer, uri, 2, &stale_clone); // so eco
            // Aguarda o teste liberar (kill() nao da EOF no lado do servidor).
            let _ = done_rx.recv();
        });
        conn.initialize("file:///projeto", INIT_TIMEOUT).unwrap();
        let since = conn.current_seq();
        conn.notify(&lsp::did_open(uri, "rust", 1, "um\n")).unwrap();
        let first = conn
            .wait_diagnostics(
                uri,
                Some(1),
                since,
                Duration::from_secs(5),
                Duration::from_millis(200),
                None,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(first, stale);
        let since = conn.current_seq();
        conn.notify(&lsp::did_change(uri, 2, "dois\n")).unwrap();
        let start = std::time::Instant::now();
        let second = conn
            .wait_diagnostics(
                uri,
                Some(2),
                since,
                Duration::from_secs(5),
                Duration::from_millis(50),
                Some(&stale),
                Duration::from_millis(400),
            )
            .unwrap();
        // Conteudo correto (a edicao nao mudou nada), sem assentar no eco imediato.
        assert_eq!(second, stale);
        assert!(
            start.elapsed() >= Duration::from_millis(400),
            "assentou no eco em {:?}",
            start.elapsed()
        );
        drop(done_tx);
        server.join().unwrap();
        conn.kill();
    }
}
