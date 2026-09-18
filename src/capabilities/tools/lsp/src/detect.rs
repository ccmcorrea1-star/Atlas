//! Deteccao do language server adequado a extensao, projeto e PATH.
//! Tabela pequena e extensivel: novas linguagens entram em SERVER_TABLE.

use std::path::{Path, PathBuf};

/// Como falar com um language server: comando, argumentos, languageId do
/// didOpen e marcadores de raiz do projeto (do mais especifico ao mais vago).
pub struct ServerSpec {
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub language_id: &'static str,
    pub markers: &'static [&'static str],
}

const RUST: ServerSpec = ServerSpec {
    command: "rust-analyzer",
    args: &[],
    language_id: "rust",
    markers: &["Cargo.toml"],
};

const TYPESCRIPT: ServerSpec = ServerSpec {
    command: "typescript-language-server",
    args: &["--stdio"],
    language_id: "typescript",
    markers: &["tsconfig.json", "package.json"],
};

const JAVASCRIPT: ServerSpec = ServerSpec {
    command: "typescript-language-server",
    args: &["--stdio"],
    language_id: "javascript",
    markers: &["package.json"],
};

const CLANGD_C: ServerSpec = ServerSpec {
    command: "clangd",
    args: &[],
    language_id: "c",
    markers: &["compile_commands.json"],
};

const CLANGD_CPP: ServerSpec = ServerSpec {
    command: "clangd",
    args: &[],
    language_id: "cpp",
    markers: &["compile_commands.json"],
};

/// (extensao, spec): comece pelas linguagens do proprio Atlas.
const SERVER_TABLE: &[(&str, &ServerSpec)] = &[
    ("rs", &RUST),
    ("ts", &TYPESCRIPT),
    ("tsx", &TYPESCRIPT),
    ("mts", &TYPESCRIPT),
    ("cts", &TYPESCRIPT),
    ("js", &JAVASCRIPT),
    ("jsx", &JAVASCRIPT),
    ("mjs", &JAVASCRIPT),
    ("c", &CLANGD_C),
    ("h", &CLANGD_C),
    ("cpp", &CLANGD_CPP),
    ("hpp", &CLANGD_CPP),
    ("cc", &CLANGD_CPP),
    ("cxx", &CLANGD_CPP),
];

/// Devolve o spec da extensao (sem ponto, minuscula) ou None.
pub fn spec_for_extension(extension: &str) -> Option<&'static ServerSpec> {
    SERVER_TABLE
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map(|(_, spec)| *spec)
}

/// Procura o binario no PATH; None significa servidor indisponivel.
pub fn find_in_path(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let candidate = directory.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Sobe do diretorio do arquivo ate achar um marcador; sem marcador, usa o
/// proprio diretorio (servidores como clangd operam sem raiz).
pub fn project_root(start: &Path, markers: &[&str]) -> PathBuf {
    let mut current = Some(start);
    while let Some(directory) = current {
        if markers.iter().any(|marker| directory.join(marker).exists()) {
            return directory.to_path_buf();
        }
        current = directory.parent();
    }
    start.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn tabela_cobre_linguagens_do_atlas() {
        assert_eq!(spec_for_extension("rs").unwrap().command, "rust-analyzer");
        let ts = spec_for_extension("tsx").unwrap();
        assert_eq!(ts.command, "typescript-language-server");
        assert_eq!(ts.args, &["--stdio"]);
        assert_eq!(spec_for_extension("jsx").unwrap().language_id, "javascript");
        assert_eq!(spec_for_extension("cpp").unwrap().command, "clangd");
        assert!(spec_for_extension("xyz").is_none());
        assert!(spec_for_extension("").is_none());
    }

    #[test]
    fn path_encontra_binario_existente() {
        assert!(find_in_path("sh").is_some());
        assert!(find_in_path("atlas-lsp-servidor-que-nao-existe").is_none());
    }

    #[test]
    fn raiz_sobe_ate_marcador() {
        let base = std::env::temp_dir().join("atlas-lsp-detect-test");
        let nested = base.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(base.join("Cargo.toml"), "[package]\n").unwrap();
        assert_eq!(project_root(&nested, &["Cargo.toml"]), base);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn raiz_cai_para_diretorio_do_arquivo() {
        let base = std::env::temp_dir().join("atlas-lsp-detect-fallback");
        fs::create_dir_all(&base).unwrap();
        assert_eq!(project_root(&base, &["compile_commands.json"]), base);
        fs::remove_dir_all(&base).unwrap();
    }
}
