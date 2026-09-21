# Atlas Runtime Protocol v1

Contrato público entre o Runtime e clientes como TUI, desktop, web e mobile.

O protocolo expõe intenção do usuário e eventos públicos de execução. Detalhes internos de SDK, Discovery e capabilities não fazem parte do contrato.

## Transporte

Mensagens são objetos JSON UTF-8 separados por nova linha (`JSON Lines`). O transporte atual usa Unix Domain Socket.

Toda mensagem contém:

```json
{
  "protocol": "atlas-runtime",
  "version": 1
}
```

O schema normativo está em [`schema.json`](./schema.json).

## Requisição

O cliente envia um `turn.request`:

```json
{
  "protocol": "atlas-runtime",
  "version": 1,
  "type": "turn.request",
  "request_id": "request-1",
  "conversation_id": "minha-conversa",
  "input": "execute node --version"
}
```

`request_id` identifica a requisição. Reutilizar `conversation_id` preserva a conversa.

### Cancelamento

O cliente pode cancelar um turno ativo enviando uma mensagem de controle com o mesmo `request_id` e `conversation_id`:

```json
{
  "protocol": "atlas-runtime",
  "version": 1,
  "type": "turn.cancel",
  "request_id": "request-1",
  "conversation_id": "minha-conversa"
}
```

O Runtime aborta a requisição real do modelo quando o provider suporta `AbortSignal` e emite um evento `error` terminal com a mensagem de cancelamento. Um cancelamento para turno desconhecido ou já concluído retorna um evento `error`.

## Eventos

Eventos associados a um turno mantêm o mesmo `request_id` e `conversation_id`. Eventos
broadcast do Host (`runtime.restarting`/`runtime.ready`) podem omitir essas identidades.

- `turn.started`: o Runtime aceitou o turno.
- `session.updated`: metadados públicos da sessão, com `data.model` e `data.provider`.
- `context.updated`: snapshot do uso de contexto, com `data.used_tokens` e `data.context_window`.
- `message.delta`: fragmento de texto transmitido, com `data.message_id` e `data.delta`.
- `message.completed`: mensagem pública completa, com `data.message_id` e `data.content`.
- `reasoning-start`: iniciou um bloco de reasoning, com `data.reasoning_id`.
- `reasoning-delta`: fragmento do reasoning, com `data.reasoning_id` e `data.delta`.
- `reasoning-end`: terminou um bloco de reasoning, com `data.reasoning_id`.
- `tool.started`: tool executada pelo Runtime iniciou, com `data.tool_id` e `data.name`.
- `tool.completed`: tool terminou, com `data.tool_id`, `data.name` e `data.output` opcional.
- `execution.started`: execução de `shell.exec` iniciou, com `data.execution_id`, `data.capability`, `data.program`, `data.args` e `data.cwd`/`data.target` opcionais. O comando interpretado é exposto como a invocação de shell equivalente (`program`/`args`).
- `execution.output.delta`: fragmento ordenado de saída de `shell.exec`, com `data.execution_id`, `data.capability`, `data.channel` (`stdout` ou `stderr`) e `data.delta`. Clientes acrescentam os fragmentos na ordem recebida.
- `execution.completed`: execução de `shell.exec` terminou, com `data.execution_id`, `data.capability`, `data.stdout`, `data.stderr`, `data.exit_code`, `data.duration_ms` e `data.status`.
- `turn.completed`: turno terminou, com `data.content`, `data.message_id` opcional e o snapshot de contexto opcional:
  `{ "used_tokens": 6600, "context_window": 256000 }`.
- `runtime.restarting`: o Host iniciou a troca do Runtime, sem expor detalhes de build.
- `runtime.ready`: o novo Runtime está saudável e aceitando conexões.
- `operation.resuming`: uma operação persistida está sendo retomada na mesma conversa, com `data.operation_id`.
- `operation.resumed`: a retomada foi aceita pelo Runtime, com `data.operation_id`.
- `error`: o turno falhou, com `data.code` e `data.message`.

`shell.exec` usa o `call_id` da tool como `execution_id`, permitindo que clientes atualizem a mesma execução do início ao fim.

Clientes devem aceitar `message.delta`, `message.completed` ou ambos. Um Runtime só emite um evento quando a execução subjacente fornece aquela informação. Clientes devem ignorar tipos adicionados em versões posteriores quando puderem continuar com segurança.

Dados genéricos de tools não contêm argumentos, requisições de Discovery ou objetos de SDK. `shell.exec` é a exceção explícita: seu lifecycle público contém apenas os campos estruturados necessários para renderizar e correlacionar uma célula de execução. O Runtime continua responsável por Discovery, capabilities, autorização e execução.

### Anexos

`turn.request` aceita `attachments` como uma lista tipada. Cada item possui `type` (`voice`, `audio`, `image` ou `document`), `uri`, `media_type` e metadados opcionais. A origem do arquivo é transportada em `source`, sem marcadores inseridos no texto do usuário ou da resposta.

### Sessões e comandos

O Runtime mantém a sessão e a expõe tipada em `command.completed`. O status `restart_interrupted` indica que uma execução anterior foi interrompida durante restart; `interrupted_request_id` identifica a requisição protegida. O primeiro `turn.request` novo na mesma conversa associa essa interrupção ao novo request, sem repetir automaticamente a execução anterior. Clientes enviam `command.request` com um comando do catálogo público `new`, `status` ou `stop`. Esses comandos podem ser enviados enquanto outro turno está ativo. `session.updated` continua carregando apenas o modelo e o provider da sessão. O Runtime é responsável por resetar sessões e cancelar turnos.

### Approvals

`approval.requested` e `approval.resolved` são eventos públicos tipados. Um cliente responde com `approval.respond`; a implementação atual rejeita a resposta quando nenhum approval está aguardando, sem criar estado de approval no cliente.

## Superfícies disponíveis no cliente TUI

O cliente TUI mapeia os eventos públicos para as superfícies canônicas existentes:

- `message.*`, `tool.*` e `execution.*` alimentam as history cells e o transcript pager;
- `turn.started`, `context.updated`, `turn.completed` e `error` alimentam o status do turno, o footer e o contexto visível;
- `turn.cancel` fecha o ciclo de cancelamento real do turno ativo.

O v1 ainda não oferece eventos ou requisições para `request_user_input`, elicitation MCP ou keymap configurável. Essas superfícies não devem aparecer como atalhos ou views no cliente até que exista semântica equivalente no Runtime. Clientes v1 ignoram eventos aditivos desconhecidos quando puderem continuar com segurança.

Quando disponível, `context` contém:

```json
{
  "used_tokens": 6600,
  "context_window": 256000
}
```

## Versionamento

`version` é um inteiro que representa a versão major do contrato. Dados adicionais e novos tipos de evento são compatíveis com v1. Mudanças incompatíveis exigem um novo diretório e valor de versão; clientes v1 nunca devem precisar conhecer mudanças internas do Runtime.
