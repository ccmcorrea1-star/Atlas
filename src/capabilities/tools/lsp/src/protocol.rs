//! Protocolo executable do Atlas: requisicao JSON pela entrada padrao e
//! resposta JSON em linha unica ({campos..., target, status, error}).

use serde_json::{Value, json};

/// Requisicao da tool lsp.diagnostics.
pub struct ToolRequest {
    pub target: String,
    pub path: String,
}

/// Extrai target e path da requisicao; espelha as mensagens do adapter C++.
pub fn parse_request(text: &str) -> Result<ToolRequest, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON request: {error}"))?;
    let object = value.as_object().ok_or("request must be a JSON object")?;
    let target = object
        .get("target")
        .and_then(Value::as_str)
        .ok_or("field 'target' must be a string")?;
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or("field 'path' must be a string")?;
    Ok(ToolRequest {
        target: target.to_string(),
        path: path.to_string(),
    })
}

/// Monta a resposta final; status segue ExecutionStatus (success, failed,
/// timed_out, unavailable).
pub fn response(
    target: &str,
    status: &str,
    error: &str,
    path: Option<&str>,
    diagnostics: &[Value],
) -> String {
    let mut output = json!({
        "target": target,
        "status": status,
        "error": error,
        "diagnostics": diagnostics,
    });
    if let Some(path) = path {
        output["path"] = Value::String(path.to_string());
    }
    serde_json::to_string(&output).unwrap_or_else(|_| {
        "{\"target\":\"\",\"status\":\"failed\",\"error\":\"response serialization failed\",\"diagnostics\":[]}".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracao_basica() {
        let request = parse_request(r#"{"target":"local","path":"src/main.rs"}"#).unwrap();
        assert_eq!(request.target, "local");
        assert_eq!(request.path, "src/main.rs");
    }

    #[test]
    fn parse_rejeita_json_invalido() {
        assert!(parse_request("{invalido}").is_err());
    }

    #[test]
    fn parse_exige_path_string() {
        assert!(parse_request(r#"{"target":"local"}"#).is_err());
        assert!(parse_request(r#"{"target":"local","path":42}"#).is_err());
    }

    #[test]
    fn resposta_em_linha_unica_com_contrato() {
        let line = response("local", "success", "", Some("a.rs"), &[]);
        assert!(!line.contains('\n'));
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["target"], "local");
        assert_eq!(value["status"], "success");
        assert_eq!(value["path"], "a.rs");
        assert_eq!(value["diagnostics"], json!([]));
    }
}
