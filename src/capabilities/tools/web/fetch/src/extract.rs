//! Extracao de conteudo util de HTML sem executar nada: titulo, texto
//! visivel e entidades basicas. Texto puro e JSON passam intactos.

/// Titulo da pagina, se houver <title> com conteudo nao vazio.
pub fn page_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start = lower.find("<title")?;
    let content_start = lower[start..].find('>')? + start + 1;
    let end = lower[content_start..].find("</title>")? + content_start;
    let title = decode_entities(html[content_start..end].trim());
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// Remove scripts, estilos, comentarios e tags; colapsa espacos.
pub fn html_to_text(html: &str) -> String {
    let without_blocks = strip_element(html, "script");
    let without_blocks = strip_element(&without_blocks, "style");
    let without_comments = strip_comments(&without_blocks);
    let mut text = String::with_capacity(without_comments.len());
    let mut inside_tag = false;
    for char in without_comments.chars() {
        match char {
            '<' => inside_tag = true,
            '>' => {
                inside_tag = false;
                text.push(' ');
            }
            _ if !inside_tag => text.push(char),
            _ => {}
        }
    }
    collapse_whitespace(&decode_entities(&text))
}

/// Remove <elemento>...</elemento> (case-insensitive, sem aninhamento).
fn strip_element(html: &str, element: &str) -> String {
    let lower = html.to_lowercase();
    let open_tag = format!("<{element}");
    let close_tag = format!("</{element}>");
    let mut result = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(open) = lower[cursor..].find(&open_tag) {
        let open = cursor + open;
        result.push_str(&html[cursor..open]);
        let after_open = match lower[open..].find('>') {
            Some(end) => open + end + 1,
            None => return result,
        };
        cursor = match lower[after_open..].find(&close_tag) {
            Some(close) => after_open + close + close_tag.len(),
            None => return result,
        };
    }
    result.push_str(&html[cursor..]);
    result
}

/// Remove <!-- ... --> preservando o restante.
fn strip_comments(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(open) = html[cursor..].find("<!--") {
        let open = cursor + open;
        result.push_str(&html[cursor..open]);
        cursor = match html[open..].find("-->") {
            Some(close) => open + close + 3,
            None => return result,
        };
    }
    result.push_str(&html[cursor..]);
    result
}

/// Colapsa espacos em branco e remove linhas vazias.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decodifica entidades comuns e numericas (&#65; e &#x41;).
pub fn decode_entities(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(start) = text[cursor..].find('&') {
        let start = cursor + start;
        result.push_str(&text[cursor..start]);
        let rest = &text[start..];
        let end = rest.find(';').unwrap_or(rest.len().min(10));
        let entity = &rest[..end.min(rest.len())];
        match decode_entity(entity) {
            Some(decoded) => {
                result.push_str(&decoded);
                cursor = start + entity.len() + usize::from(end < rest.len());
            }
            None => {
                result.push('&');
                cursor = start + 1;
            }
        }
    }
    result.push_str(&text[cursor..]);
    result
}

fn decode_entity(entity: &str) -> Option<String> {
    match entity {
        "&amp" => Some("&".to_string()),
        "&lt" => Some("<".to_string()),
        "&gt" => Some(">".to_string()),
        "&quot" => Some("\"".to_string()),
        "&#39" | "&apos" => Some("'".to_string()),
        "&nbsp" => Some(" ".to_string()),
        _ if entity.starts_with("&#x") || entity.starts_with("&#X") => {
            u32::from_str_radix(entity[3..].trim_start_matches('0'), 16)
                .ok()
                .and_then(char::from_u32)
                .map(|char| char.to_string())
        }
        _ if entity.starts_with("&#") => entity[2..]
            .trim_start_matches('0')
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|char| char.to_string()),
        _ => None,
    }
}

/// Tipo MIME do cabecalho Content-Type (sem parametros, minusculo).
pub fn mime_of(header: Option<&str>) -> String {
    header
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// HTML vai para extracao; texto, JSON e XML passam crus.
pub fn is_html(mime: &str) -> bool {
    mime == "text/html" || mime == "application/xhtml+xml"
}

/// Tipos entregues sem modificacao.
pub fn is_plain_text(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime.ends_with("+json")
        || mime == "application/xml"
        || mime.ends_with("+xml")
        || mime.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulo_extraido_e_decodificado() {
        let html = "<html><head><title>A &amp; B</title></head></html>";
        assert_eq!(page_title(html).as_deref(), Some("A & B"));
        assert_eq!(page_title("<p>sem titulo</p>"), None);
    }

    #[test]
    fn texto_remove_script_estilo_e_tags() {
        let html = "<html><head><style>.x{}</style><script>alert(1)</script></head>\
            <body><!-- oi --><h1>Titulo</h1><p>paragrafo   com <b>negrito</b></p></body></html>";
        assert_eq!(html_to_text(html), "Titulo paragrafo com negrito");
    }

    #[test]
    fn entidades_numericas() {
        assert_eq!(decode_entities("&#65;&#x42;"), "AB");
        assert_eq!(decode_entities("&desconhecida;"), "&desconhecida;");
    }

    #[test]
    fn mime_sem_parametros() {
        assert_eq!(mime_of(Some("text/html; charset=utf-8")), "text/html");
        assert_eq!(mime_of(None), "");
        assert!(is_html("text/html"));
        assert!(is_plain_text("application/json"));
        assert!(!is_plain_text("image/png"));
    }
}
