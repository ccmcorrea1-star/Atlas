# Build

## Requisitos

Node.js 22+, npm, CMake 3.20+ com C++23 e Rust/Cargo para o TUI.

## Instalação

```bash
npm ci
```

## Verificação

```bash
npm run format:check
npm run lint
npm run lint:client
npm run typecheck
npm run build
npm test
npm run smoke
```

O smoke test é local e não precisa de `OPENCODE_GO_API_KEY`.

## Cliente TUI

Para disponibilizar o comando `atlas` no PATH:

```bash
cargo install --locked --path clients/tui
```

```bash
npm run build:client
npm run test:client
```

## Build nativo

`npm run build` compila o TypeScript e as capabilities C++.

Para executar somente o build nativo:

```bash
cmake -S . -B .native-cmake
cmake --build .native-cmake
ctest --test-dir .native-cmake --output-on-failure
```

`dist/` e `.native-cmake/` são artefatos gerados e não devem ser versionados.

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

| Comando                | Finalidade                                               |
| ---------------------- | -------------------------------------------------------- |
| `npm run dev`          | Executa o TypeScript em modo watch                       |
| `atlas server run`     | Inicia o servidor local do Atlas Runtime                 |
| `atlas server status`  | Consulta o estado do servidor sem alterá-lo              |
| `atlas server restart` | Reinicia o servidor local do Atlas Runtime               |
| `atlas server stop`    | Desliga o servidor local do Atlas Runtime                |
| `npm run format`       | Formata os arquivos com Prettier                         |
| `npm run format:check` | Verifica a formatação sem alterar arquivos               |
| `npm run lint`         | Executa o ESLint                                         |
| `npm run lint:client`  | Executa o Clippy no cliente TUI                          |
| `npm run lint:fix`     | Corrige automaticamente problemas do ESLint              |
| `npm run typecheck`    | Verifica o código de produção e os testes com TypeScript |
| `npm test`             | Executa os testes automatizados em `tests/unit/`         |
| `npm run smoke`        | Executa o smoke test do provider e das conversas         |
| `npm run build`        | Gera os arquivos JavaScript em `dist/`                   |
| `npm run build:client` | Compila o cliente TUI em Rust                            |
| `npm run test:client`  | Executa os testes unitários do cliente TUI               |

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
