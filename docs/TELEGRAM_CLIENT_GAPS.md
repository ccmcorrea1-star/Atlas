# Lacunas do cliente Telegram do Atlas

## Objetivo

Este documento compara o cliente Telegram do Atlas com o comportamento já existente no cliente Telegram do Hermes e registra o que ainda precisa ser implementado no Atlas.

A referência do Hermes é comportamental. O Atlas não deve copiar a arquitetura interna do Hermes: o cliente deve permanecer um adapter fino entre a Telegram Bot API e o Atlas Runtime.

## Referências

- Hermes: `plugins/platforms/telegram/adapter.py`
- Hermes: `hermes_cli/commands.py`
- Hermes: `hermes_cli/commands_platforms.py`
- Atlas: `clients/telegram/adapter.ts`
- Atlas: `clients/telegram/api.ts`
- Atlas: `clients/telegram/routing.ts`
- Atlas: `clients/telegram/runtime.ts`
- Atlas: `clients/telegram/stream-consumer.ts`
- Atlas: `clients/telegram/text.ts`
- Atlas: `clients/telegram/types.ts`
- Atlas Runtime: `src/runtime/protocol.ts`
- Requisitos do produto: `docs/APPS.md`

## Comparação Hermes × Atlas

A comparação é comportamental; o Atlas continua mantendo um adapter fino e deixa sessão, Agent, memória, Tasks e execução no Runtime.

| Capacidade observada no Hermes          | Estado atual do Atlas                                                                                                   | Trabalho restante                                                                                                    |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Sessão persistente por chat/thread      | `conversation_id` determinístico e `MemorySession` no Runtime                                                           | Persistir estado `restart_interrupted` e retomar a partir do último turno confirmado após restart                    |
| Restart com execução em andamento       | Ledger marca requests em `executing` como `ambiguous` e impede replay silencioso                                        | Publicar aviso de restart e associar a sessão interrompida ao próximo input                                          |
| Próxima mensagem continua a sessão      | Novas mensagens podem executar outra tarefa; o Atlas não perde o agente inteiro                                         | Implementar marcador persistente e fluxo de continuação equivalente ao Hermes                                        |
| Mensagem chega enquanto há execução     | `interrupt`, `queue` e `steer` são políticas explícitas, com confirmação de busy e preservação do trabalho já concluído | Expor política por conversa, ack de busy e steer/queue no Runtime; hoje o adapter apenas serializa a fila            |
| Delivery ledger final                   | Atlas possui ledger de chunks/preview durante a entrega Telegram                                                        | Persistir resposta final, destino, thread, estado do envio, tentativas e expiração; redeliver sem reexecutar o Agent |
| Entrega potencialmente duplicada        | Atlas protege a execução ambígua, mas não rotula redelivery de resposta                                                 | Prefixo visível de recuperação e semântica at-least-once para respostas já produzidas                                |
| Retry de `/retry`                       | Não há comando equivalente no catálogo Telegram                                                                         | Adicionar somente depois de definir retry seguro no Runtime; não repetir Tools ambíguas automaticamente              |
| Liveness do gateway/polling             | Serviço pode reiniciar, mas não há watchdog de loop/heartbeat persistente equivalente                                   | Heartbeat, detecção de polling parado, motivo de degradação e recuperação observável                                 |
| Heartbeat de tarefa longa               | Typing e status intermediários existem                                                                                  | Bubble de progresso/heartbeat persistente e healthcheck do polling                                                   |
| Warnings/errors automáticos             | Erro ambíguo agora é traduzido para uma mensagem útil                                                                   | Canal persistente de lifecycle/recovery e deduplicação de avisos                                                     |
| Threads/fóruns                          | Parsing, roteamento e persistência básica implementados                                                                 | Handoff, bindings após restart e round-trip live em supergrupo Fórum                                                 |
| Sessões nomeadas e troca entre clientes | Ainda não há catálogo nem handlers                                                                                      | Contrato Runtime, comandos Telegram e retomada entre clientes                                                        |

**Referências verificadas do Hermes:**

- [Messaging Gateway](https://hermes-agent.nousresearch.com/docs/user-guide/messaging): sessão persistente, `restart_interrupted`, delivery ledger, redelivery com aviso e comandos `/retry`/`/resume`.
- [Fonte do Hermes](https://github.com/NousResearch/hermes-agent): comportamento do adapter Telegram e do gateway.
- Atlas: `src/runtime/request-ledger.ts`, `src/runtime/server.ts`, `clients/telegram/adapter.ts` e `clients/telegram/stream-consumer.ts`.

## Checklist de progresso

Atualizado após `079d752 fix: surface interrupted Telegram turns`.

### Concluído

- [x] Catálogo de comandos existente, `/help` e menus por escopo.
- [x] Mensagens de entrada: texto, voz, áudio, fotos, documentos, vídeos, GIFs, stickers, locations, venues, álbuns, posts de canais e mensagens editadas.
- [x] Contexto tipado de replies.
- [x] Reações tipadas de usuário e contagens agregadas.
- [x] Inline queries, incluindo validação pelo loop real de polling.
- [x] Notificações assíncronas básicas com assinatura persistente e entrega real na Bot API.
- [x] Tópicos/fóruns como eventos tipados, com persistência e restauração do estado local do tópico.
- [x] Pickers paginados, navegação, `Outro` e respostas tipadas.
- [x] Renderer MarkdownV2 semântico para listas, tabelas, headings e fences.
- [x] Streaming, chunking UTF-16, retomada por chunk e retry de flood control.
- [x] Validação de updates não textuais no polling.
- [x] Contratos Runtime e provas NDJSON para reações, notificações, tópicos e inline.
- [x] Proteção contra replay silencioso de requests ambíguos.
- [x] Erro `ambiguous_execution` tipado e mensagem Telegram explicando a interrupção.
- [x] Nova tarefa continua executável depois de uma execução ambígua.

### Parcial

- [ ] Recuperação estilo Hermes: Atlas informa a interrupção, mas ainda não persiste `restart_interrupted` por sessão nem retoma a partir do último turno confirmado no próximo input.
- [ ] Delivery ledger: Atlas persiste chunks/preview durante a entrega, mas ainda não persiste a resposta final como unidade durável para redelivery após crash sem reexecutar o Agent.
- [ ] Tópicos/fóruns: parsing, roteamento, persistência e restauração local concluídos; handoff, bindings entre processos e round-trip em supergrupo-fórum ainda não validados.
- [ ] Polling: backoff, IPv4, reconexão e refresh periódico de identidade concluídos; heartbeat, healthcheck avançado e recuperação de conflitos persistentes ainda pendentes.
- [ ] Mídia: caminhos principais concluídos; voice bubble Ogg/Opus, captions avançadas, fallback por URL e alguns casos de documentos ainda pendentes.
- [ ] Lifecycle público do Runtime: eventos existem/parcialmente são publicados, mas ainda não são consumidos pelo Telegram como avisos persistentes de restart/handoff.

### Pendente

- [ ] Retomada persistente de sessões interrompidas no padrão Hermes (`restart_interrupted` → aviso → próximo input continua a sessão).
- [ ] Delivery ledger final com redelivery at-least-once, prefixo de possível duplicata, tentativas limitadas e expiração.
- [ ] Comando `/retry` seguro e distinto de recuperação de uma operação ambígua.
- [ ] Lifecycle público do Runtime consumido pelo Telegram: `runtime.restarting`, `runtime.ready`, `operation.resuming` e `operation.resumed`.
- [ ] Tasks, Workers, progresso e cancelamento.
- [ ] Sessões nomeadas, troca e retomada entre clientes.
- [ ] Handoff avançado de fóruns e recuperação de tópicos apagados.
- [ ] Round-trip real de fóruns, dependente de um supergrupo com modo Fórum.

### TODO priorizado

#### P0 — não perder trabalho nem deixar o usuário sem resposta

1. **Recovery persistente de sessão:** no Runtime, registrar `restart_interrupted` por `conversation_id`, o último turno confirmado e a razão da interrupção; no boot, reidratar essa pendência e, no próximo input, continuar sem repetir Tools ambíguas.
2. **Delivery ledger final:** persistir resposta final, destino, thread, fase (`not_started`, `sending`, `delivered`, `abandoned`), tentativas e timestamps; redeliver resposta produzida sem executar o Agent novamente.
3. **Recovery Telegram:** consumir `runtime.restarting`, `runtime.ready`, `operation.resuming` e `operation.resumed`; enviar aviso idempotente de interrupção/retomada e prefixar redelivery potencialmente duplicada.
4. **Testes de crash boundary:** cobrir queda antes do envio, durante o envio, depois do envio sem confirmação e execução ambígua; provar que não há replay automático de efeito colateral.

#### P1 — disponibilidade e operação longa

5. **Busy-input policy:** definir no Runtime `queue`, `interrupt` e `steer`, com ack Telegram e preservação dos resultados já confirmados.
6. **Liveness:** heartbeat do polling, detecção de loop parado, conflito 409 persistente, shutdown com updates pendentes e motivo observável de degradação.
7. **Progresso:** heartbeat editável para tarefas longas e renovação de typing sem transformar o typing em prova de que o Agent está saudável.
8. **Fóruns:** validar handoff entre processos, bindings após restart e round-trip em supergrupo real com modo Fórum.
9. **Mídia:** voice bubble Ogg/Opus, captions avançadas, fallback de URL e recuperação de formatos rejeitados.

#### P2 — superfície de uso e integração avançada

10. **Tasks/Workers:** listar, consultar progresso, cancelar e receber conclusão fora do turno ativo.
11. **Sessões nomeadas:** listar, alternar e retomar sessão entre Telegram e outros clientes.
12. **Comandos:** `/retry`, `/resume`, `/sessions`, modelo/provider e diagnóstico apenas depois dos contratos Runtime correspondentes.

A ordem é deliberada: primeiro preservar execução e entrega; depois manter o gateway observável e utilizável em trabalhos longos; por fim ampliar a superfície de comandos. Não implementar `/retry` como alias de `turn.recover` enquanto a segurança de efeitos ambíguos não estiver definida.

Cada item concluído deve continuar obedecendo aos critérios de aceitação no final deste documento. Commits separados e gates completos são a fonte operacional de verificação; esta seção é o índice resumido.

## Estado atual do Atlas

O Atlas já possui:

- polling via `getUpdates`;
- allowlist por usuário e chat;
- autorização deny-by-default;
- ativação de grupos por menção ou reply;
- identidade determinística por chat e tópico;
- streaming editando uma mensagem de preview;
- divisão de respostas acima do limite do Telegram;
- persistência de offset, updates pendentes e mensagens processadas;
- deduplicação de mensagens;
- retomada de chunks confirmados durante a entrega;
- entrada de texto, voz, áudio, foto, documento, vídeo, GIF, sticker, location, venue e álbuns;
- entrega de imagem, voz, áudio, documento, vídeo, animação e álbuns;
- aprovações por botões inline;
- respostas de `input.requested`, incluindo escolhas e texto livre;
- comandos `/help`, `/new`, `/status` e `/stop`;
- testes focados do adapter Telegram.

Isso cobre o caminho básico de conversa, mas ainda não cobre a superfície completa esperada de um cliente Telegram do Atlas.

## Lacunas

> **Fonte de verdade:** consulte primeiro a seção [Checklist de progresso](#checklist-de-progresso). As listas desta seção são o catálogo histórico de capacidades e podem conter itens já concluídos. Não trate um item como pendente sem confirmar seu status na checklist.

### 1. Menu e catálogo de comandos

O Hermes deriva seus comandos de um registro central e publica o menu em escopos diferentes do Telegram. O Atlas já possui `/help`, `/new`, `/status` e `/stop` derivados de seus catálogos; ainda não possui os comandos Hermes de retry, sessões nomeadas, modelo/provider e retomada.

Implementar:

- corrigir e verificar o registro de `setMyCommands`;
- publicar comandos nos escopos default, privado e grupo;
- registrar comandos específicos para fóruns quando necessário;
- adicionar comandos somente quando existirem no catálogo público do Runtime;
- manter o parser e o menu derivados do mesmo catálogo;
- adicionar, conforme contratos do Runtime, comandos de:
  - sessões e retomada;
  - modelo/provider;
  - diagnóstico;
  - restart/update;
  - Tasks/Workers;
  - fila, steer e execução em background.

Não copiar automaticamente todos os comandos do Hermes. O Atlas deve expor somente operações que o Runtime realmente suporte.

### 2. Tipos de update e mensagens de entrada

O Atlas já trata texto, callback, location, venue, vídeo, sticker, animação, álbuns, mensagens editadas, posts de canais, reações, inline queries e contexto estruturado de replies. Esta seção permanece como catálogo histórico; novas famílias devem atualizar tipos, `allowed_updates`, validação e handlers. Os itens abaixo já estão cobertos e ficam como referência de contrato:

- `location` e `venue`;
- `video`;
- `sticker`;
- `animation`/GIF;
- `media_group`;
- mensagens editadas;
- posts de canais;
- reações;
- inline queries;
- contexto estruturado da mensagem respondida.

Também é necessário atualizar `allowed_updates`, os tipos de `clients/telegram/types.ts`, a validação de updates e os handlers do adapter.

### 3. Normalização e agrupamento de mídia recebida

O Hermes não trata cada arquivo de forma isolada. Ele normaliza e agrupa eventos antes de enviar ao Runtime.

Implementar:

- agrupamento de mensagens de texto divididas pelo cliente Telegram;
- agrupamento de fotos em álbuns;
- merge de captions;
- seleção da maior resolução de uma foto;
- identificação consistente de MIME e extensão;
- validação do tamanho antes do download;
- cache local com limpeza garantida;
- reaproveitamento da mídia da mensagem respondida;
- tratamento de documentos que são imagens ou vídeos;
- injeção controlada de conteúdo de documentos de texto;
- tratamento de stickers com contexto apropriado para visão.

Os anexos devem continuar chegando ao Runtime como anexos tipados. Não usar marcadores textuais como `MEDIA:/...`.

### 4. Mídia enviada pelo Atlas

O Atlas já envia vídeo, animação/GIF e álbuns com as APIs nativas. Restam:

- fallback de foto para documento quando o Telegram rejeitar as dimensões;
- imagem por URL com fallback seguro para download/upload;
- conversão para voice bubble Ogg/Opus;
- duração de áudio/voz;
- captions formatadas e limitadas a 1024 caracteres;
- fallback nativo quando o formato não for reproduzível pelo Telegram.

### 5. Formatação e entrega de texto

O Atlas já possui renderer MarkdownV2 semântico, fallback plain-text, tabelas, fences, chunking pós-formatação, UTF-16 e edição final. Restam apenas casos de rich messages dependentes de contratos futuros.

### 6. Streaming e entrega final

O Atlas já possui preview editável, throttling, status intermediário, deduplicação de edições, chunking UTF-16, retomada por chunk confirmado, tratamento de eventos terminais e retry de flood control. Restam:

- `sendMessageDraft`, quando suportado pelo contrato e pela Bot API;
- delivery ledger final após crash, separado do ledger de chunks;
- redelivery at-least-once com prefixo visível quando houver possível duplicata;
- tentativas limitadas, backoff e expiração para respostas não confirmadas;
- edição rica na finalização quando o Runtime fornecer o contrato.

O evento terminal deve continuar sendo a autoridade para o estado final do turno.

### 7. Pickers e callbacks interativos

O Atlas já suporta aprovação/rejeição, escolhas paginadas, `Outro`, navegação, autorização e respostas tipadas. Restam pickers de modelo/provider e prompts administrativos dependentes de contratos do Runtime:

- picker de modelos/providers;
- confirmação de comandos administrativos;
- expiração e limpeza de callbacks em casos não cobertos;
- edição da mensagem original após a escolha quando ainda não houver contrato;
- prompts interativos de atualização e operações administrativas.

A lógica de domínio deve permanecer no Runtime. O cliente apenas renderiza e devolve respostas tipadas.

### 8. Tópicos, fóruns e handoff

O Atlas inclui o `message_thread_id` no `conversation_id` e já persiste/restaura o estado local do tópico. Restam:

- validação de handoff entre processos;
- round-trip real em supergrupo com modo Fórum;
- comandos e menu específicos de fórum, se o Runtime os suportar;
- fallback para tópico apagado;
- limpeza de bindings obsoletos;
- recuperação do reply anchor após restart;
- isolamento correto por tópico e perfil.

A identidade da conversa deve continuar seguindo o formato determinístico:

```text
telegram:<encoded-chat-id>:thread:<encoded-thread-id-or-root>
```

### 9. Contexto de replies

O Atlas já preserva texto/caption, mídia tipada, reply anchor e replies dentro de tópicos. Restam fallback quando a mensagem original foi apagada e validação live de replies em fóruns.
Não transformar esse contexto em texto artificial sem contrato. Se o Runtime precisar de um novo campo, evoluir primeiro o protocolo.

### 10. Notificações assíncronas e lifecycle

O Atlas já possui `notification.subscribe`/`notification.publish`, assinatura persistente e entrega básica de notificações. O Runtime já declara eventos de lifecycle, mas o adapter Telegram ainda não os consome como avisos persistentes de restart/handoff.

Implementar primeiro o contrato público do Runtime para:

- `runtime.restarting`;
- `runtime.ready`;
- `operation.resuming`;
- `operation.resumed`;
- estado `restart_interrupted` por sessão;
- delivery ledger final e redelivery sem reexecutar o Agent;
- conclusão de Tasks;
- mudança de estado de Workers;
- notificações fora de um turno ativo.

Depois implementar no cliente:

- assinatura ou recebimento desses broadcasts;
- roteamento para `TELEGRAM_HOME_CHAT`;
- preservação de tópico quando houver destino específico;
- deduplicação de notificações e avisos de recovery;
- renderização de progresso, retomada, possível duplicata e conclusão.

Um método público sem consumidor não deve ser considerado suporte implementado.

### 11. Tasks, Workers e sessões

A documentação do Atlas Telegram promete acompanhamento de Tasks, Workers e continuidade entre clientes, mas o adapter não possui handlers específicos para isso.

Falta definir no Runtime e expor no Telegram:

- listar Tasks/Workers;
- consultar estado e progresso;
- cancelar trabalho persistente;
- abrir detalhes de uma execução;
- receber conclusão assíncrona;
- listar e alternar sessões;
- retomar uma sessão nomeada;
- manter a mesma `conversation_id` durante a troca de cliente.

Não armazenar estado de Tasks ou sessões como regra de domínio dentro do adapter.

### 12. Presença, reações e eventos de plataforma

O Atlas já cobre typing básico, reações de usuário/contagem, mensagens editadas e inline queries. Restam heartbeat de typing em turnos longos, status de progresso persistente e observação de mensagens não direcionadas em grupos quando o Runtime suportar esse modo.

### 13. Polling e resiliência operacional

O Atlas já possui backoff, timeout do long polling, preferência IPv4, reconexão, refresh periódico de identidade e tratamento básico de conflitos 409. Restam:

- heartbeat de polling;
- detecção de polling parado independente do timeout;
- probe seguro de updates pendentes;
- recuperação explícita de conflitos persistentes;
- reconstrução do cliente HTTP após falha de pool;
- controle de conexões `CLOSE_WAIT`;
- shutdown sem perder updates pendentes;
- observabilidade sem expor token ou dados sensíveis.

O adapter deve continuar resiliente a falhas transitórias sem mascarar erro de protocolo ou erro terminal do Runtime.

## Fronteira de implementação

### Pode ficar no cliente Telegram

- tipos e parsing da Bot API;
- autorização e roteamento Telegram;
- menu de comandos;
- formatação e chunking;
- upload/download de mídia;
- tópicos e reply routing;
- callbacks;
- presença e reações;
- retry e health do transporte Telegram.

### Precisa existir primeiro no Runtime/protocolo

- Tasks e Workers;
- notificações broadcast;
- lifecycle de restart/handoff e estado `restart_interrupted`;
- delivery ledger final e redelivery;
- lista e troca de sessões;
- picker de modelos;
- progresso assíncrono;
- comandos que ainda não existem no catálogo público.

O cliente não deve duplicar Agent, memória, sessão, autorização de tools, Tasks, Workers ou execução.

## Ordem inicial (histórica)

A ordem abaixo foi o plano original. Ela não representa o estado atual; use a checklist acima para decidir o próximo bloco.

### P0 — base utilizável

1. Corrigir e verificar `setMyCommands`.
2. Criar `/help` e derivar o menu do catálogo público.
3. Ampliar a matriz de tipos de mensagem e anexos.
4. Implementar mídia de saída: vídeo, animação e álbum.
5. Criar renderer MarkdownV2 seguro e chunking pós-formatação.
6. Adicionar testes NDJSON reais para comandos e eventos do Runtime.

### P1 — paridade de uso

7. Implementar contexto de replies e agrupamento de mídia.
8. Implementar tópicos DM, fóruns e handoff.
9. Implementar pickers e callbacks gerais.
10. Melhorar streaming, flood control e entrega final.
11. Adicionar presença, reações e eventos editados.

### P2 — integração completa com o Runtime

12. Definir broadcasts de lifecycle.
13. Conectar `notifyHome()` a um consumidor real.
14. Expor Tasks, Workers e progresso.
15. Implementar sessões, retomada e notificações assíncronas.
16. Completar healthcheck e recuperação avançada do polling.

## Critérios de aceitação

Uma lacuna só deve ser marcada como concluída quando houver:

- contrato tipado atualizado;
- parser e validação atualizados;
- adapter implementado;
- testes determinísticos do adapter;
- teste NDJSON real para cada nova família de request/evento;
- gates do repositório passando;
- `getMyCommands` confirmando o menu registrado;
- processo e socket do Runtime confirmados;
- round-trip real Telegram → adapter → Runtime → Telegram, quando a mudança afetar entrega;
- nenhuma credencial nos logs, testes, commits ou documentação.

Processo ativo, token válido, teste com fake e fila vazia não equivalem a uma integração Telegram verificada.
