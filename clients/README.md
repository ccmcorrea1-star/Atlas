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

Durante a execução:

- `Enter` envia a mensagem;
- `Esc` ou `Ctrl-C` encerra o cliente;
- `Backspace` e as setas esquerda/direita editam o campo de entrada.

O `RuntimeClient` é a única fronteira entre o TUI e o Atlas. O transporte
público IPC/API ainda não foi definido, então a implementação inicial informa
essa indisponibilidade no próprio histórico sem simular respostas do Agent.

## Verificação

```bash
npm run build:client
npm run test:client
```

Novos clientes devem ganhar seus próprios diretórios dentro de `clients/`, sem
serem misturados ao Runtime ou criados como diretórios vazios antecipadamente.
