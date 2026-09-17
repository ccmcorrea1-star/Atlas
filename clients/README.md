# Clientes

`clients/` contém interfaces desacopladas do Runtime. Clientes consomem apenas contratos públicos.

## TUI

O cliente atual fica em [`clients/tui/`](./tui/) e usa Rust.

Instale o comando `atlas` a partir da raiz do projeto:

```bash
cargo install --locked --path clients/tui
```

Inicie o Runtime:

```bash
atlas server run
```

Para reiniciar ou desligar o servidor:

```bash
atlas server restart
atlas server stop
```

Depois execute o TUI:

```bash
atlas --conversation-id minha-conversa
```

Atalhos principais:

- `Enter` envia a mensagem;
- `Shift+Enter` insere uma nova linha;
- `Esc` ou `Ctrl-C` encerra o cliente; `Esc` pede confirmação durante um turn ativo;
- `Backspace`, `Delete`, `Home`, `End` e as setas editam o campo de entrada;
- `PageUp`/`PageDown` e a roda do mouse navegam pelo transcript;
- `Ctrl-R` pesquisa o histórico reversamente;
- `@` abre a busca local de caminhos.

O `RuntimeClient` é a única fronteira entre o TUI e o Atlas. Ele usa o contrato
público em [`protocol/runtime/v1`](../protocol/runtime/v1/) sobre um Unix Socket
no diretório `XDG_RUNTIME_DIR` do usuário; sem essa variável, usa
`/tmp/atlas-runtime.sock`. O caminho pode ser alterado com `ATLAS_RUNTIME_SOCKET`.

Os subcomandos `atlas server` são apenas uma fronteira operacional para o processo
local. Por padrão, executam `npm run runtime`; um launcher externo pode ser usado
com `ATLAS_RUNTIME_PROGRAM` e `ATLAS_RUNTIME_ARGS`. `ATLAS_RUNTIME_CWD` define o
diretório de trabalho quando o comando é executado fora da raiz do projeto.

## Verificação

```bash
npm run build:client
npm run test:client
```
