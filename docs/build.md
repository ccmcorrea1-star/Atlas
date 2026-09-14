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
npm run typecheck
npm run build
npm test
npm run smoke
```

O smoke test é local e não precisa de `OPENCODE_GO_API_KEY`.

## Cliente TUI

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

A CI está em [`.github/workflows/quality.yml`](../.github/workflows/quality.yml). Testes são descritos em [`test.md`](./test.md).
