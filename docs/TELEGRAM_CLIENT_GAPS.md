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

## Checklist de progresso

Atualizado após o commit `9f21f89`.

### Concluído

- [x] Catálogo de comandos, `/help` e menus por escopo.
- [x] Mensagens de entrada: texto, voz, áudio, fotos, documentos, vídeos, GIFs, stickers, locations, venues, álbuns, posts de canais e mensagens editadas.
- [x] Contexto tipado de replies.
- [x] Reações tipadas de usuário e contagens agregadas.
- [x] Inline queries, incluindo validação pelo loop real de polling.
- [x] Notificações assíncronas com assinatura persistente e entrega real na Bot API.
- [x] Tópicos/fóruns como eventos tipados básicos.
- [x] Pickers paginados, navegação, `Outro` e respostas tipadas.
- [x] Renderer MarkdownV2 semântico para listas, tabelas, headings e fences.
- [x] Streaming, chunking UTF-16, retomada por chunk e retry de flood control.
- [x] Validação de updates não textuais no polling.
- [x] Contratos Runtime e provas NDJSON para reações, notificações, tópicos e inline.

### Parcial

- [ ] Tópicos/fóruns: parsing e roteamento básico concluídos; handoff, bindings persistentes e round-trip em supergrupo-fórum ainda não validados.
- [ ] Polling: backoff, IPv4 e reconexão concluídos; heartbeat, healthcheck avançado e recuperação de conflitos persistentes ainda pendentes.
- [ ] Mídia: caminhos principais concluídos; voice bubble, captions avançadas, fallback por URL e alguns casos de documentos ainda pendentes.

### Pendente

- [ ] Lifecycle público do Runtime: `runtime.restarting`, `runtime.ready`, `operation.resuming` e `operation.resumed` consumidos pelo Telegram.
- [ ] Tasks, Workers, progresso e cancelamento.
- [ ] Sessões nomeadas, troca e retomada entre clientes.
- [ ] Handoff avançado de fóruns e recuperação de tópicos apagados.
- [ ] Round-trip real de fóruns, dependente de um supergrupo com modo Fórum.

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
- retomada da entrega após restart;
- entrada de texto, voz, áudio, foto e documento;
- entrega de imagem, voz, áudio e documento;
- aprovações por botões inline;
- respostas de `input.requested`, incluindo escolhas e texto livre;
- comandos `/new`, `/status` e `/stop`;
- testes focados do adapter Telegram.

Isso cobre o caminho básico de conversa, mas ainda não cobre a superfície completa esperada de um cliente Telegram do Atlas.

## Lacunas

> **Fonte de verdade:** consulte primeiro a seção [Checklist de progresso](#checklist-de-progresso). As listas desta seção são o catálogo histórico de capacidades e podem conter itens já concluídos. Não trate um item como pendente sem confirmar seu status na checklist.

### 1. Menu e catálogo de comandos

O Hermes deriva seus comandos de um registro central e publica o menu em escopos diferentes do Telegram. O Atlas possui somente três comandos no catálogo do Runtime e atualmente não possui uma superfície de ajuda equivalente.

Implementar:

- corrigir e verificar o registro de `setMyCommands`;
- publicar comandos nos escopos default, privado e grupo;
- registrar comandos específicos para fóruns quando necessário;
- adicionar `/help`;
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

O Atlas atualmente trata `message` e `callback_query`. O Hermes trata uma superfície maior.

Implementar, conforme necessidade do Runtime:

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

O Atlas envia atualmente imagem, voz, áudio e documento. Falta cobertura equivalente à do Hermes para:

- vídeo nativo;
- animação/GIF;
- álbuns com `sendMediaGroup`;
- fallback de foto para documento quando o Telegram rejeitar as dimensões;
- imagem por URL com fallback seguro para download/upload;
- conversão para voice bubble Ogg/Opus;
- duração de áudio/voz;
- captions formatadas e limitadas a 1024 caracteres;
- fallback nativo quando o formato não for reproduzível pelo Telegram.

### 5. Formatação e entrega de texto

O Atlas envia MarkdownV2 e tenta texto simples quando o Telegram rejeita a mensagem. O Hermes possui uma camada de formatação mais completa.

Implementar:

- renderer MarkdownV2 seguro;
- fallback plain-text sem perder o conteúdo;
- conversão de tabelas para uma representação legível no Telegram;
- preservação de blocos de código;
- tratamento de links, títulos, blockquotes e listas;
- chunking depois da formatação, considerando UTF-16;
- separação correta de fences de código entre chunks;
- rich messages quando a Bot API e o Runtime permitirem;
- edição final formatada sem criar uma mensagem duplicada.

O módulo `clients/telegram/text.ts` atualmente cobre limite, UTF-16, truncamento e divisão, mas não faz renderização semântica.

### 6. Streaming e entrega final

O Atlas já possui preview editável, throttling e ledger de entrega. O Hermes possui controles adicionais para evitar flood e duplicação.

Avaliar e implementar:

- `sendMessageDraft`, quando suportado pelo contrato e pela Bot API;
- status progressivo de execução;
- renovação de typing após cada envio intermediário;
- preview saturado quando o acumulador passa de 4096 caracteres;
- deduplicação de edições idênticas;
- continuação de respostas longas sem regredir o preview;
- retomada por chunk confirmado;
- tratamento distinto de `delta`, `completed`, `cancelled` e `error`;
- retry controlado para flood control;
- fallback sem criar mensagens duplicadas;
- edição rica na finalização.

O evento terminal deve continuar sendo a autoridade para o estado final do turno.

### 7. Pickers e callbacks interativos

O Atlas suporta aprovação/rejeição e escolhas simples de input. O Hermes possui uma camada de callbacks mais ampla.

Implementar, conforme contratos do Runtime:

- picker de modelos/providers;
- picker de escolhas paginado;
- confirmação de comandos;
- opção `Outro` para respostas livres;
- paginação, voltar, cancelar e no-op;
- expiração e limpeza de callbacks;
- autorização por usuário e chat para cada callback;
- edição da mensagem original após a escolha;
- prompts interativos de atualização e operações administrativas.

A lógica de domínio deve permanecer no Runtime. O cliente apenas renderiza e devolve respostas tipadas.

### 8. Tópicos, fóruns e handoff

O Atlas inclui o `message_thread_id` no `conversation_id`, mas isso cobre somente o roteamento básico.

Implementar:

- criação de tópicos DM;
- renomeação de tópicos;
- persistência do vínculo tópico/sessão;
- recuperação de tópico após restart;
- handoff para novo tópico;
- comandos específicos de fórum;
- registro preguiçoso do menu por fórum;
- fallback para tópico apagado;
- limpeza de bindings obsoletos;
- recuperação do reply anchor;
- isolamento correto por tópico e perfil.

A identidade da conversa deve continuar seguindo o formato determinístico:

```text
telegram:<encoded-chat-id>:thread:<encoded-thread-id-or-root>
```

### 9. Contexto de replies

O Hermes preserva mais contexto ao processar uma resposta a outra mensagem.

Implementar:

- leitura do texto/caption da mensagem respondida;
- materialização da mídia respondida;
- envio desse contexto como anexo ou metadado tipado;
- preservação do reply anchor na resposta;
- fallback quando a mensagem original foi apagada;
- tratamento de replies dentro de tópicos.

Não transformar esse contexto em texto artificial sem contrato. Se o Runtime precisar de um novo campo, evoluir primeiro o protocolo.

### 10. Notificações assíncronas e lifecycle

O Atlas possui `notifyHome()`, mas não existe call site que o utilize. O Runtime também declara eventos de lifecycle que não são consumidos pelo adapter Telegram.

Implementar primeiro o contrato público do Runtime para:

- `runtime.restarting`;
- `runtime.ready`;
- `operation.resuming`;
- `operation.resumed`;
- conclusão de Tasks;
- mudança de estado de Workers;
- notificações fora de um turno ativo.

Depois implementar no cliente:

- assinatura ou recebimento desses broadcasts;
- roteamento para `TELEGRAM_HOME_CHAT`;
- preservação de tópico quando houver destino específico;
- deduplicação de notificações;
- renderização de progresso e conclusão.

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

O Hermes possui recursos adicionais de presença e eventos.

Avaliar e implementar:

- typing com cooldown e retry;
- renovação do typing em turnos longos;
- status online/offline quando aplicável;
- reações na mensagem;
- remoção de reações;
- mensagens editadas como eventos de plataforma;
- inline queries;
- observação de mensagens não direcionadas em grupos, caso o Runtime tenha suporte para esse modo.

### 13. Polling e resiliência operacional

O Atlas possui backoff básico no loop de polling. O Hermes possui uma camada mais completa de recuperação.

Implementar ou avaliar:

- fallback de IP quando DNS/IPv6 estiver instável;
- proxy configurável;
- heartbeat de polling;
- detecção de polling parado;
- probe seguro de updates pendentes;
- tratamento explícito de conflitos 409;
- reconstrução do cliente HTTP após falha de pool;
- controle de conexões `CLOSE_WAIT`;
- refresh periódico da identidade do bot;
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
- lifecycle de restart/handoff;
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
