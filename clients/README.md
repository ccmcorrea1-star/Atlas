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
- `Shift+Enter` insere uma nova linha;
- `Esc` ou `Ctrl-C` encerra o cliente;
- `Backspace`, `Delete`, `Home`, `End` e as setas editam o campo de entrada;
- `PageUp`/`PageDown` e a roda do mouse navegam pelo transcript.

O transcript usa uma apresentação densa, com Markdown estilizado, células de execução agrupadas por
`execution_id` e previews de saída normalizados para leitura humana. O lifecycle de `process.exec` e
os limites entre componentes seguem o padrão do ExecCell do Codex CLI, com as adaptações registradas
em [`clients/tui/NOTICE`](tui/NOTICE).

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
