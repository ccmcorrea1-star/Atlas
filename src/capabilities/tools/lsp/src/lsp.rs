//! Framing Content-Length e mensagens JSON-RPC do LSP.

use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

/// Lê uma mensagem LSP: cabecalhos até linha vazia + corpo Content-Length.
pub fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "lsp stream closed",
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
            })?);
        }
    }
    let length = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Escreve uma mensagem LSP com framing Content-Length.
pub fn write_message(writer: &mut dyn Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

pub fn initialize_request(id: u64, root_uri: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": { "textDocument": { "publishDiagnostics": { "relatedInformation": true } } },
        },
    })
}

pub fn initialized_notification() -> Value {
    json!({"jsonrpc": "2.0", "method": "initialized", "params": {}})
}

pub fn did_open(uri: &str, language_id: &str, version: i64, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": version,
                "text": text,
            },
        },
    })
}

pub fn did_close(uri: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": uri } },
    })
}

pub fn did_change(uri: &str, version: i64, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }],
        },
    })
}

/// Nome do contrato para cada DiagnosticSeverity do LSP.
pub fn severity_name(severity: Option<i64>) -> &'static str {
    match severity {
        Some(1) => "error",
        Some(2) => "warning",
        Some(3) => "information",
        Some(4) => "hint",
        _ => "information",
    }
}

/// Converte um Diagnostic LSP para o objeto do contrato (range, severity,
/// message, source?, code?); range preserva a base zero do LSP.
pub fn to_contract_diagnostic(diagnostic: &Value) -> Option<Value> {
    let object = diagnostic.as_object()?;
    let mut output = serde_json::Map::new();
    output.insert(
        "range".to_string(),
        object.get("range").cloned().unwrap_or(Value::Null),
    );
    let severity = object.get("severity").and_then(Value::as_i64);
    output.insert(
        "severity".to_string(),
        Value::String(severity_name(severity).to_string()),
    );
    output.insert(
        "message".to_string(),
        object.get("message").cloned().unwrap_or(Value::Null),
    );
    if let Some(source) = object.get("source").and_then(Value::as_str) {
        output.insert("source".to_string(), Value::String(source.to_string()));
    }
    match object.get("code") {
        Some(code @ Value::Number(_)) | Some(code @ Value::String(_)) => {
            output.insert("code".to_string(), code.clone());
        }
        _ => {}
    }
    Some(Value::Object(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn framing_ida_e_volta() {
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let mut buffer = Vec::new();
        write_message(&mut buffer, &message).unwrap();
        let text = String::from_utf8(buffer.clone()).unwrap();
        assert!(text.starts_with("Content-Length:"));
        assert!(text.contains("\r\n\r\n"));
        let decoded = read_message(&mut Cursor::new(buffer)).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn leitura_sequencial_de_duas_mensagens() {
        let first = json!({"jsonrpc": "2.0", "method": "a"});
        let second = json!({"jsonrpc": "2.0", "id": 7, "result": {}});
        let mut buffer = Vec::new();
        write_message(&mut buffer, &first).unwrap();
        write_message(&mut buffer, &second).unwrap();
        let mut cursor = Cursor::new(buffer);
        assert_eq!(read_message(&mut cursor).unwrap(), first);
        assert_eq!(read_message(&mut cursor).unwrap(), second);
    }

    #[test]
    fn mapeamento_completo_de_diagnostico() {
        let diagnostic = json!({
            "range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 9}},
            "severity": 1,
            "message": "mismatched types",
            "source": "rust-analyzer",
            "code": "E0308",
        });
        let contract = to_contract_diagnostic(&diagnostic).unwrap();
        assert_eq!(contract["severity"], "error");
        assert_eq!(contract["message"], "mismatched types");
        assert_eq!(contract["source"], "rust-analyzer");
        assert_eq!(contract["code"], "E0308");
        assert_eq!(contract["range"]["start"]["line"], 1);
    }

    #[test]
    fn mapeamento_com_code_numerico_e_sem_source() {
        let diagnostic = json!({
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
            "severity": 2,
            "message": "unused variable",
            "code": 6133,
        });
        let contract = to_contract_diagnostic(&diagnostic).unwrap();
        assert_eq!(contract["severity"], "warning");
        assert_eq!(contract["code"], 6133);
        assert!(contract.get("source").is_none());
    }

    #[test]
    fn severidade_ausente_vira_information_sem_code() {
        let diagnostic = json!({
            "range": {},
            "message": "nota",
        });
        let contract = to_contract_diagnostic(&diagnostic).unwrap();
        assert_eq!(contract["severity"], "information");
        assert!(contract.get("code").is_none());
    }
}
