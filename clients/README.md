# Clientes

`clients/` contém interfaces desacopladas do Runtime. Clientes consomem apenas contratos públicos.

## TUI

O cliente atual fica em [`clients/tui/`](./tui/) e usa Rust.

Inicie o Runtime:

```bash
npm run runtime
```

Depois execute o TUI:

```bash
cargo run --manifest-path clients/tui/Cargo.toml -- --conversation-id minha-conversa
```

`Enter` envia, `Shift+Enter` quebra linha e `Esc` ou `Ctrl-C` encerra.

A comunicação usa o [`Atlas Runtime Protocol v1`](../protocol/runtime/v1/README.md) pelo Unix Socket `/tmp/atlas-runtime.sock`. Use `ATLAS_RUNTIME_SOCKET` para alterar o caminho.

## Verificação

```bash
npm run build:client
npm run test:client
```
