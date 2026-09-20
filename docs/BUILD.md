# Build

## Requisitos

Node.js 22+, npm, CMake 3.20+ com C++23 e Rust/Cargo para o TUI.

## Instalação

```bash
npm ci
```

## Verificação

```bash
npm run check:full
npm run sync:client
```

`check` valida rapidamente o código TypeScript. Para uma área específica, use
`test:tui`, `test:runtime`, `test:native`, `test:lsp`, `test:web-fetch`,
`test:web-search`, `test:web-browser` ou `test:web-crawl`. `check:tui` inclui lint,
build incremental e testes da TUI.
`check:full` combina os checks estáticos, todos os builds e toda a suíte.

O smoke test é local e não precisa de `OPENCODE_GO_API_KEY`.

## Cliente TUI

Para disponibilizar o comando `atlas` no PATH:

```bash
npm run sync:client
```

```bash
npm run build:client
npm run test:client
```

`sync:client` reaproveita `clients/tui/target/debug/atlas` e o copia para o
diretório de binários do Cargo. O `cargo build` usado para garantir o artefato
é incremental; não há um novo `cargo install --force` a cada alteração.

## Build nativo

`npm run build` compila o TypeScript e as capabilities C++.

Para executar somente o build nativo:

```bash
cmake -S . -B .native-cmake
cmake --build .native-cmake
ctest --test-dir .native-cmake --output-on-failure
```

Os comandos de teste por área constroem apenas os alvos necessários e
reaproveitam `.native-cmake/` e os diretórios `target/` do Cargo. `dist/`,
`.native-cmake/`, `.web-search-build/` e `.web-tools-build/` são artefatos
gerados e não devem ser versionados.

## Execução

Depois de compilar o cliente TUI, inicie o Runtime com:

```bash
atlas server run
```

O comando usa `npm run runtime` por padrão. Para usar outro launcher, defina
`ATLAS_RUNTIME_PROGRAM` e, opcionalmente, `ATLAS_RUNTIME_ARGS` e
`ATLAS_RUNTIME_CWD` para o diretório de trabalho do Runtime.

Para reiniciar uma instância existente:

```bash
atlas server restart
```

Para desligar o servidor:

```bash
atlas server stop
```

## Comandos disponíveis

| Comando                    | Finalidade                                               |
| -------------------------- | -------------------------------------------------------- |
| `npm run dev`              | Executa o TypeScript em modo watch                       |
| `atlas server run`         | Inicia o servidor local do Atlas Runtime                 |
| `atlas server status`      | Consulta o estado do servidor sem alterá-lo              |
| `atlas server restart`     | Reinicia o servidor local do Atlas Runtime               |
| `atlas server stop`        | Desliga o servidor local do Atlas Runtime                |
| `npm run format`           | Formata os arquivos com Prettier                         |
| `npm run format:check`     | Verifica a formatação sem alterar arquivos               |
| `npm run lint`             | Executa o ESLint                                         |
| `npm run lint:client`      | Executa o Clippy no cliente TUI                          |
| `npm run lint:fix`         | Corrige automaticamente problemas do ESLint              |
| `npm run typecheck`        | Verifica o código de produção e os testes com TypeScript |
| `npm test`                 | Executa os testes automatizados por domínio              |
| `npm run smoke`            | Executa o smoke test do provider e das conversas         |
| `npm run build`            | Gera TypeScript e runtimes de todas as capabilities      |
| `npm run build:client`     | Compila o cliente TUI em Rust                            |
| `npm run test:client`      | Executa os testes unitários do cliente TUI               |
| `npm run test:tui`         | Executa somente os testes da TUI                         |
| `npm run test:runtime`     | Executa somente os testes TypeScript do Runtime          |
| `npm run test:native`      | Executa somente os testes nativos C++                    |
| `npm run test:lsp`         | Executa somente os testes do runtime LSP                 |
| `npm run test:web-fetch`   | Executa somente os testes do runtime web.fetch           |
| `npm run test:web-search`  | Executa somente os testes do runtime web.search          |
| `npm run test:web-browser` | Executa somente os testes do runtime web.browser         |
| `npm run test:web-crawl`   | Executa somente os testes do runtime web.crawl           |
| `npm run check`            | Executa checks estáticos rápidos do Runtime              |
| `npm run check:tui`        | Executa lint, build e testes da TUI                      |
| `npm run check:full`       | Executa a validação completa de todos os domínios        |
| `npm run sync:client`      | Sincroniza o binário incremental da TUI no PATH          |

## CI

A CI está em [`.github/workflows/quality.yml`](../.github/workflows/quality.yml).
O workflow executa `npm ci` e todos os checks de qualidade em um job do GitHub
Actions:

1. Formatação
2. Lint
3. Lint do cliente TUI
4. Typecheck
5. Build
6. Testes automatizados
7. Smoke test
8. Build e testes do cliente TUI

Uma mudança só deve ser considerada pronta quando todos esses comandos passarem localmente. Testes adicionais são descritos em [`TEST.md`](./TEST.md).
