//! Cliente HTTP/HTTPS com redirects limitados, timeout e teto de tamanho.
//! Sem JavaScript, sem browser: so o corpo da resposta.

use std::env;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::extract;

/// Timeout total de conexao e leitura.
const TIMEOUT: Duration = Duration::from_secs(15);
/// Maximo de redirects seguidos.
const MAX_REDIRECTS: u32 = 5;
/// Teto do corpo aceito (1 MiB).
const MAX_BYTES: u64 = 1024 * 1024;

/// Conteudo buscado e normalizado.
#[derive(Debug)]
pub struct Fetched {
    pub url: String,
    pub content_type: String,
    pub title: Option<String>,
    pub content: String,
}

/// Busca a URL; HTML vira texto util, texto e JSON passam crus.
pub fn fetch(url: &str) -> Result<Fetched, String> {
    require_http_scheme(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .redirects(MAX_REDIRECTS)
        .build();
    let response = agent.get(url).call().map_err(|error| match error {
        ureq::Error::Status(code, _) => format!("unexpected HTTP status {code}"),
        ureq::Error::Transport(transport) => format!("request failed: {transport}"),
    })?;
    let final_url = response.get_url().to_string();
    let content_type = extract::mime_of(response.header("content-type"));
    let body = read_capped(response.into_reader())?;
    let text = String::from_utf8_lossy(&body).into_owned();
    if extract::is_html(&content_type) {
        return Ok(Fetched {
            url: final_url,
            content_type,
            title: extract::page_title(&text),
            content: extract_html(&text),
        });
    }
    if extract::is_plain_text(&content_type) {
        return Ok(Fetched {
            url: final_url,
            content_type,
            title: None,
            content: text,
        });
    }
    Err(format!("unsupported content type '{content_type}'"))
}

fn extract_html(html: &str) -> String {
    if configured_extractor() != "trafilatura" {
        return extract::html_to_text(html);
    }

    let child = Command::new("trafilatura")
        .args(["--stdin", "--output-format", "txt", "--no-comments"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return extract::html_to_text(html);
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(html.as_bytes()).is_err() {
            return extract::html_to_text(html);
        }
    }
    let Ok(output) = child.wait_with_output() else {
        return extract::html_to_text(html);
    };
    if !output.status.success() {
        return extract::html_to_text(html);
    }
    let content = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if content.is_empty() {
        extract::html_to_text(html)
    } else {
        content
    }
}

fn configured_extractor() -> String {
    let Some(raw) = env::var_os("ATLAS_WEB_CONFIG_JSON") else {
        return "native".to_string();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(raw.to_string_lossy().as_bytes()) else {
        return "native".to_string();
    };
    value
        .get("fetch")
        .and_then(|fetch| fetch.get("extractor"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("native")
        .to_string()
}

/// So HTTP e HTTPS; qualquer outro esquema e rejeitado antes de discar.
fn require_http_scheme(url: &str) -> Result<(), String> {
    let lower = url.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(());
    }
    Err("unsupported URL scheme (expected http or https)".to_string())
}

/// Le o corpo ate o teto; acima disso, falha em vez de truncar calado.
fn read_capped(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    reader
        .take(MAX_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| format!("failed to read response body: {error}"))?;
    if body.len() as u64 > MAX_BYTES {
        return Err("response body exceeds 1 MiB limit".to_string());
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    /// Servidor HTTP minimo: responde cada caminho uma vez e encerra.
    fn stub(responses: Vec<(&'static str, &'static str, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for (path, headers, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = std::io::Read::read(&mut stream, &mut request);
                let request = String::from_utf8_lossy(&request);
                let status = if request.starts_with(&format!("GET {path} ")) {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.0 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                );
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{address}")
    }

    #[test]
    fn html_vira_texto_com_titulo() {
        let base = stub(vec![(
            "/",
            "Content-Type: text/html; charset=utf-8\r\n",
            b"<html><head><title>Oi</title><script>x()</script></head><body><p>corpo <b>legal</b></p></body></html>".to_vec(),
        )]);
        let fetched = fetch(&format!("{base}/")).unwrap();
        assert_eq!(fetched.title.as_deref(), Some("Oi"));
        assert_eq!(fetched.content, "Oi corpo legal");
        assert_eq!(fetched.content_type, "text/html");
    }

    #[test]
    fn json_passa_cru() {
        let base = stub(vec![(
            "/dados",
            "Content-Type: application/json\r\n",
            br#"{"a":1}"#.to_vec(),
        )]);
        let fetched = fetch(&format!("{base}/dados")).unwrap();
        assert_eq!(fetched.content, r#"{"a":1}"#);
        assert_eq!(fetched.content_type, "application/json");
        assert!(fetched.title.is_none());
    }

    #[test]
    fn redirect_e_seguido_ate_o_destino() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = std::io::Read::read(&mut stream, &mut request);
                let text = String::from_utf8_lossy(&request).into_owned();
                if text.starts_with("GET /antigo ") {
                    let _ = stream.write_all(
                        b"HTTP/1.0 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                } else {
                    let body = b"chegou";
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    );
                    let _ = stream.write_all(body);
                }
            }
        });
        let fetched = fetch(&format!("http://{address}/antigo")).unwrap();
        assert_eq!(fetched.content, "chegou");
        assert!(fetched.url.ends_with("/final"));
    }

    #[test]
    fn status_inesperado_e_esquema_invalido_falham() {
        let base = stub(vec![(
            "/falta",
            "Content-Type: text/plain\r\n",
            b"".to_vec(),
        )]);
        let error = fetch(&format!("{base}/inexistente")).unwrap_err();
        assert!(error.contains("404"), "{error}");
        for url in [
            "ftp://x/y",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "gopher://x",
        ] {
            assert!(fetch(url).unwrap_err().contains("scheme"), "{url}");
        }
    }

    #[test]
    fn corpo_acima_do_teto_falha() {
        let base = stub(vec![(
            "/grande",
            "Content-Type: text/plain\r\n",
            vec![b'x'; (MAX_BYTES + 8) as usize],
        )]);
        let error = fetch(&format!("{base}/grande")).unwrap_err();
        assert!(error.contains("exceeds"), "{error}");
    }
}
