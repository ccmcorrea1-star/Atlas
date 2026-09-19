# Atlas — Visão Técnica

**Versão 0.8 — Setembro de 2026**

Atlas é uma inteligência pessoal residente e local-first para compreender, operar e evoluir o ambiente digital do usuário.

Este documento define a visão do produto. A organização do código está em [`ARCHITECTURE.md`](./ARCHITECTURE.md).

## Objetivo

Atlas deve funcionar como uma camada operacional persistente entre o usuário, seus computadores, aplicações, serviços, conhecimento e dispositivos.

O Agent interpreta objetivos e decide o que fazer. O Runtime executa, mantém estado e expõe eventos públicos. Clientes apresentam o trabalho e coletam interação.

## Princípios

- Máxima capacidade com o mínimo de contexto necessário.
- Ação direta quando suficiente; delegação somente quando útil.
- Discovery cria conhecimento sobre recursos, não permissão para executá-los.
- Detalhes são materializados progressivamente conforme a tarefa exige.

Atlas é local-first, não local-only. Providers, SDKs e serviços externos devem permanecer substituíveis.

## Fluxo

O caminho principal deve ser simples:

```text
Usuário
  ↓
Agent
  ↓
Discovery
  ↓
Capability Registry
  ↓
Runtime
  ↓
Ambiente
```

Tasks e Workers entram nesse fluxo apenas quando o trabalho precisa ser persistente, paralelo, longo ou desacoplado do turno atual.

## Discovery, Tools e Skills

O Agent consulta o catálogo com `list_tools` e busca por intenção usando Discovery:

```text
list_tools()
list_tools({ group: "shell" })
discover({ query: "logs de container" })
```

`list_tools` lista grupos e Tools registrados, ou somente as Tools de um grupo.
Discovery retorna somente Tools e Skills utilizáveis encontradas por intenção.
Grupos organizam o Registry e não são resultados de Discovery. Somente a
capacidade relevante deve ser materializada por completo.

Uma **Tool** representa uma ação executável. Depois de descoberta, deve ser chamada diretamente e executada de forma determinística sempre que possível.

Uma **Skill** representa um procedimento reutilizável. Ela orienta o Agent a combinar Tools sem precisar se tornar uma Tool monolítica.

Discovery encontra Tools e Skills. O [`Capability Registry`](#capability-registry) administra capacidades executáveis e procedurais.

## Contexto e memória

Estado da conversa, contexto de trabalho, memória de longo prazo e conhecimento são conceitos diferentes.

O contexto de trabalho contém apenas o necessário para a tarefa atual. Memória e conhecimento são recuperados por relevância, não carregados integralmente no prompt.

A compactação reconstrói o contexto ativo, preservando objetivo, estado, decisões, referências importantes e mensagens recentes. O estado persistente continua sendo a fonte de verdade.

> Nada entra no contexto do modelo apenas porque existe.

## Capability Registry

Toda capacidade conhecida pelo Atlas deve entrar em um Registry independente da linguagem de implementação.

O Registry pode receber capacidades nativas, integrações externas ou MCP sem expor seus detalhes ao restante do sistema.

Capacidades geradas precisam de identidade, versão, origem, dependências, testes e provenance. Código criado pelo Agent não se torna confiável apenas por ter sido gerado pelo próprio Atlas.

## Tasks e Workers

Uma Task representa estado persistente de trabalho. Um Worker é um executor temporário.

Cancelamento deve ser explícito. Retries não podem duplicar efeitos silenciosamente e o trabalho deve permitir recuperação quando a operação suportar isso.

## Estado e observabilidade

O Runtime deve produzir eventos estruturados para tornar o trabalho observável sem expor raciocínio privado.

Toda ação relevante deve possuir provenance suficiente para identificar origem, execução e efeito produzido.

O estado persistente do Atlas não deve depender de tipos internos de um provider ou Agent SDK.

O contrato público entre Runtime e clientes está em [`protocol/runtime/v1`](../protocol/runtime/v1/README.md).

## Integrações e clientes

MCP é uma fronteira de integração, não a implementação interna obrigatória de Discovery.

Clientes permanecem desacoplados do Runtime e usam contratos públicos. A documentação dos clientes está em [`clients/`](../clients/README.md).

Atlas Workstation deve funcionar como um espaço de trabalho observável, não apenas como uma interface de chat maior.

## Referências

Estado atual e execução: [`README.md`](../README.md) · Arquitetura: [`ARCHITECTURE.md`](./ARCHITECTURE.md) · Estilo: [`CODE_STYLE.md`](./CODE_STYLE.md)

Dependências principais: [OpenAI Agents SDK](https://openai.github.io/openai-agents-js/) · [Integração com AI SDK](https://openai.github.io/openai-agents-js/extensions/ai-sdk/)
