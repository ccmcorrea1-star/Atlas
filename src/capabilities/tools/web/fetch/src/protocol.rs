//! Protocolo executable do Atlas: requisicao JSON pela entrada padrao e
//! resposta JSON em linha unica ({campos..., target, status, error}).

use serde_json::{json, Value};

/// Requisicao da tool web.fetch.
pub struct ToolRequest {
    pub target: String,
    pub url: String,
}

/// Extrai target e url da requisicao; espelha as mensagens do adapter C++.
pub fn parse_request(text: &str) -> Result<ToolRequest, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON request: {error}"))?;
    let object = value.as_object().ok_or("request must be a JSON object")?;
    let target = object
        .get("target")
        .and_then(Value::as_str)
        .ok_or("field 'target' must be a string")?;
    let url = object
        .get("url")
        .and_then(Value::as_str)
        .ok_or("field 'url' must be a string")?;
    Ok(ToolRequest {
        target: target.to_string(),
        url: url.to_string(),
    })
}

/// Monta a resposta final; status segue ExecutionStatus (success, failed).
pub fn response(
    target: &str,
    status: &str,
    error: &str,
    url: Option<&str>,
    content_type: Option<&str>,
    title: Option<&str>,
    content: Option<&str>,
) -> String {
    let mut output = json!({
        "target": target,
        "status": status,
        "error": error,
    });
    if let Some(url) = url {
        output["url"] = Value::String(url.to_string());
    }
    if let Some(content_type) = content_type {
        output["content_type"] = Value::String(content_type.to_string());
    }
    if let Some(title) = title {
        output["title"] = Value::String(title.to_string());
    }
    if let Some(content) = content {
        output["content"] = Value::String(content.to_string());
    }
    serde_json::to_string(&output).unwrap_or_else(|_| {
        "{\"target\":\"\",\"status\":\"failed\",\"error\":\"response serialization failed\"}"
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracao_basica() {
        let request = parse_request(r#"{"target":"local","url":"https://exemplo.test/"}"#).unwrap();
        assert_eq!(request.target, "local");
        assert_eq!(request.url, "https://exemplo.test/");
    }

    #[test]
    fn parse_exige_url_string() {
        assert!(parse_request(r#"{"target":"local"}"#).is_err());
        assert!(parse_request(r#"{"target":"local","url":42}"#).is_err());
    }

    #[test]
    fn resposta_em_linha_unica_com_contrato() {
        let line = response(
            "local",
            "success",
            "",
            Some("https://exemplo.test/"),
            Some("text/html"),
            Some("Titulo"),
            Some("corpo"),
        );
        assert!(!line.contains('\n'));
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["status"], "success");
        assert_eq!(value["url"], "https://exemplo.test/");
        assert_eq!(value["title"], "Titulo");
    }
}
