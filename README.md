<p align="center">
  <img src="assets/brand/atlas-banner.png" alt="Atlas — inteligência residente para o seu ambiente digital" width="100%">
</p>

# Atlas

Atlas é uma inteligência pessoal residente e local-first para compreender e operar o ambiente digital do usuário.

A visão do projeto está em [`docs/ATLAS_TECHNICAL_VISION.md`](docs/ATLAS_TECHNICAL_VISION.md).

Documentação: [`ARCHITECTURE.md`](ARCHITECTURE.md) · [`CODE_STYLE.md`](CODE_STYLE.md) · [`docs/build.md`](docs/build.md) · [`docs/test.md`](docs/test.md) · [`clients/README.md`](clients/README.md) · [`protocol/runtime/v1`](protocol/runtime/v1/README.md)

## Estado atual

O Runtime usa TypeScript/ESM, [OpenAI Agents SDK](https://openai.github.io/openai-agents-js/) e [OpenCode Go](https://opencode.ai/docs/pt-br/go/). Capabilities nativas usam C++ e o cliente TUI usa Rust.

## Instalação

Requer Node.js 22+, npm, CMake 3.20+, compilador C++23 e Rust/Cargo para o TUI.

```bash
npm ci
export OPENCODE_GO_API_KEY="sua-chave"
```

## Execução

```bash
npm run runtime
cargo run --manifest-path clients/tui/Cargo.toml -- --conversation-id minha-conversa
```

O caminho do Unix Socket pode ser alterado com `ATLAS_RUNTIME_SOCKET`.

## Verificação

```bash
npm run format:check
npm run lint
npm run typecheck
npm run build
npm test
npm run smoke
npm run build:client
npm run test:client
```

Detalhes em [`docs/build.md`](docs/build.md) e [`docs/test.md`](docs/test.md).
