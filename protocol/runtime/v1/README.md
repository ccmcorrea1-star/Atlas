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

## Eventos

Todos os eventos mantêm o mesmo `request_id` e `conversation_id`.

| Evento | Dados |
| --- | --- |
| `turn.started` | turno aceito |
| `message.delta` | `message_id`, `delta` |
| `message.completed` | `message_id`, `content` |
| `tool.started` | `tool_id`, `name` |
| `tool.completed` | `tool_id`, `name`, `output?` |
| `execution.started` | `execution_id`, `capability`, `program`, `args`, `cwd?`, `target?` |
| `execution.completed` | `execution_id`, `capability`, `stdout`, `stderr`, `exit_code`, `duration_ms`, `status` |
| `turn.completed` | `content`, `message_id?`, `context?` |
| `error` | `code`, `message` |

`process.exec` usa o `call_id` da tool como `execution_id`, permitindo que clientes atualizem a mesma execução do início ao fim.

`context`, quando disponível, contém:

```json
{
  "used_tokens": 6600,
  "context_window": 256000
}
```

Clientes devem aceitar `message.delta`, `message.completed` ou ambos e ignorar novos tipos de evento quando isso puder ser feito com segurança.

Eventos genéricos de tools não expõem argumentos, Discovery ou objetos de SDK. O Runtime continua responsável por capabilities, autorização e execução.

## Versionamento

`version` representa a versão major do contrato. Novos campos opcionais e eventos são compatíveis com v1. Mudanças incompatíveis exigem uma nova versão.
