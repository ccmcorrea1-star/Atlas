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

Todos os eventos mantêm o mesmo `request_id` e `conversation_id`.

- `turn.started`: o Runtime aceitou o turno.
- `context.updated`: snapshot do uso de contexto, com `data.used_tokens` e `data.context_window`.
- `message.delta`: fragmento de texto transmitido, com `data.message_id` e `data.delta`.
- `message.completed`: mensagem pública completa, com `data.message_id` e `data.content`.
- `tool.started`: tool executada pelo Runtime iniciou, com `data.tool_id` e `data.name`.
- `tool.completed`: tool terminou, com `data.tool_id`, `data.name` e `data.output` opcional.
- `execution.started`: execução de `process.exec` iniciou, com `data.execution_id`, `data.capability`, `data.program`, `data.args` e `data.cwd`/`data.target` opcionais.
- `execution.output.delta`: fragmento ordenado de saída de `process.exec`, com `data.execution_id`, `data.capability`, `data.channel` (`stdout` ou `stderr`) e `data.delta`. Clientes acrescentam os fragmentos na ordem recebida.
- `execution.completed`: execução de `process.exec` terminou, com `data.execution_id`, `data.capability`, `data.stdout`, `data.stderr`, `data.exit_code`, `data.duration_ms` e `data.status`.
- `turn.completed`: turno terminou, com `data.content`, `data.message_id` opcional e o snapshot de contexto opcional:
  `{ "used_tokens": 6600, "context_window": 256000 }`.
- `error`: o turno falhou, com `data.code` e `data.message`.

`process.exec` usa o `call_id` da tool como `execution_id`, permitindo que clientes atualizem a mesma execução do início ao fim.

Clientes devem aceitar `message.delta`, `message.completed` ou ambos. Um Runtime só emite um evento quando a execução subjacente fornece aquela informação. Clientes devem ignorar tipos adicionados em versões posteriores quando puderem continuar com segurança.

Dados genéricos de tools não contêm argumentos, requisições de Discovery ou objetos de SDK. `process.exec` é a exceção explícita: seu lifecycle público contém apenas os campos estruturados necessários para renderizar e correlacionar uma célula de execução. O Runtime continua responsável por Discovery, capabilities, autorização e execução.

Quando disponível, `context` contém:

```json
{
  "used_tokens": 6600,
  "context_window": 256000
}
```

## Versionamento

`version` é um inteiro que representa a versão major do contrato. Dados adicionais e novos tipos de evento são compatíveis com v1. Mudanças incompatíveis exigem um novo diretório e valor de versão; clientes v1 nunca devem precisar conhecer mudanças internas do Runtime.
