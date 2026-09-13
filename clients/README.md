# Atlas Clients

`clients/` é a fronteira permanente para interfaces do Atlas. Cada cliente deve
consumir uma API ou IPC público do Runtime e permanecer independente da
implementação interna em `src/`.

## TUI

O primeiro cliente oficial é o TUI em Rust, construído com Ratatui,
Crossterm, Tokio e Clap.

```bash
cargo run --manifest-path clients/tui/Cargo.toml -- --conversation-id minha-conversa
```

Em outro terminal, inicie o Runtime local primeiro:

```bash
npm run runtime
```

Durante a execução:

- `Enter` envia a mensagem;
- `Esc` ou `Ctrl-C` encerra o cliente;
- `Backspace` e as setas esquerda/direita editam o campo de entrada.

O `RuntimeClient` é a única fronteira entre o TUI e o Atlas. Ele usa o contrato
público em [`protocol/runtime/v1`](../protocol/runtime/v1/) sobre o Unix Socket
`/tmp/atlas-runtime.sock`; o caminho pode ser alterado com
`ATLAS_RUNTIME_SOCKET`.

## Verificação

```bash
npm run build:client
npm run test:client
```

Novos clientes devem ganhar seus próprios diretórios dentro de `clients/`, sem
serem misturados ao Runtime ou criados como diretórios vazios antecipadamente.
