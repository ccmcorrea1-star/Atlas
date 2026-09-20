# Arquitetura

Este documento define onde cada responsabilidade do Atlas deve ficar.

## Estrutura

- [`src/`](../src/) — Runtime e API principal.
- [`src/prompts/`](../src/prompts/) — prompt padrão do Agent.
- [`src/capabilities/`](../src/capabilities/) — capabilities nativas.
- [`clients/`](../clients/) — clientes desacoplados do Runtime.
- [`protocol/`](../protocol/) — contratos compartilhados.
- [`tests/`](../tests/) — testes.
- [`docs/`](./) — documentação técnica.

## Regras

Cada módulo deve ter uma responsabilidade clara.

Clientes não devem conter regras do Runtime.

O gerenciamento local do processo do Runtime é uma fronteira operacional separada
do cliente de conversa. Ele pode iniciar, acompanhar e encerrar um comando de
Runtime configurado, mas não deve interpretar eventos, estado de sessão ou regras
de domínio.

Integrações externas devem ficar isoladas de regras internas.

Código compartilhado entre linguagens deve depender de contratos definidos em [`protocol/`](../protocol/).

A entrada comum de execução fica em [`src/capabilities/core/executor.cpp`](../src/capabilities/core/executor.cpp):
ela valida `arguments` contra o schema antes de escolher `implementation.kind`. A fronteira
`implementation.kind` continua permitindo runtimes executáveis independentes em C++, Rust e
TypeScript. Runtimes C++ usam [`runtime/executable`](../src/capabilities/runtime/executable/)
como adaptador de protocolo e conectam o `run()` ao dispatch C++ da capability.

[`src/capabilities/core/spawn.cpp`](../src/capabilities/core/spawn.cpp) permanece uma primitive
de processo. [`command_runner.cpp`](../src/capabilities/core/command_runner.cpp) fornece a
camada comum de comandos, incluindo captura de saída, timeout, código de saída e a distinção
explícita de executável ausente.

O bridge de capabilities em [`src/capabilities/runtime/bridge/`](../src/capabilities/runtime/bridge/)
é um processo residente iniciado sob demanda por [`NativeCapabilityRuntime`](../src/capability-runtime.ts).
Ele mantém `Registry` e `Executor` vivos e atende múltiplas requisições NDJSON pelo mesmo stdin,
correlacionadas por `request_id`. O cliente TypeScript reutiliza o processo enquanto o runtime
existir, reinicia após crash/EOF e preserva o streaming de `execution.output.delta`. Os runtimes
individuais das capabilities continuam sendo processos por execução.

A extensão transversal de execução fica em [`src/capability-hooks.ts`](../src/capability-hooks.ts):
[`HookableCapabilityRuntime`](../src/capability-hooks.ts) decora qualquer `CapabilityRuntime` e
aplica os pontos `before_execute` e `after_execute` sem alterar o contrato das capabilities.
[`RetryGuard`](../src/capability-hooks.ts) usa esses hooks para bloquear, dentro do mesmo turno,
uma chamada idêntica (mesmo id, target e argumentos normalizados) depois de uma falha. O guard é
criado por turno em [`runAtlas`](../src/atlas.ts) e não deve ser implementado dentro das tools.

## Tools e Skills

O modelo público separa definições executáveis e procedurais:

- **Tool** possui `schema` e `implementation`, pode ser materializada como Function Tool
  e é a única capacidade aceita por `execute`.
- **Skill** possui apenas identidade, resumo e instruções procedurais. Seu conteúdo vem de
  `SKILL.md` e não é executável pelo Runtime.

[`discover`](../src/atlas.ts) retorna somente `id`, `type` e `summary` para ambos os tipos.
[`list_tools`](../src/atlas.ts) retorna somente Tools. A Function Tool `skill({ id })`
materializa as instruções completas somente quando o Agent escolhe uma Skill; assim o
conteúdo não entra no contexto por existir no disco.

Skills são carregadas pelo bridge a partir destes locais, nesta ordem de precedência:

- `.atlas/skills/<name>/SKILL.md`
- `~/.config/atlas/skills/<name>/SKILL.md`
- `.agents/skills/<name>/SKILL.md` (compatibilidade)

O `SKILL.md` exige frontmatter com `name` e `description`; `name` é o id da Skill,
`description` é o resumo de discovery e o restante do arquivo são suas instruções.
O fluxo esperado é `discover -> skill -> execute` para combinar a Skill carregada com as
Tools necessárias.

Antes de criar um novo módulo, verifique se a responsabilidade pertence a um módulo existente.

## Fronteira web

As capabilities públicas da web são independentes do provider e usam a menor
capacidade suficiente para a tarefa:

```text
web.search → web.fetch → web.browser → web.crawl
```

- [`web.search`](../src/capabilities/tools/web/search/) usa SearXNG local como
  default. Providers hospedados entram somente como adapters substituíveis,
  selecionados na configuração global.
- [`web.fetch`](../src/capabilities/tools/web/fetch/) permanece o fast path HTTP
  nativo, sem JavaScript. Trafilatura é apenas um extractor interno opcional;
  não é uma capability pública.
- [`web.browser`](../src/capabilities/tools/web/browser/) mantém uma sessão local
  residente quando possível, usando Camoufox + Playwright e operações
  estruturadas. O Agent decide e planeja; o browser não contém planejamento.
- [`web.crawl`](../src/capabilities/tools/web/crawl/) é separado e opcional,
  usando um adapter local Crawl4AI para múltiplas páginas. Ele não substitui
  fetch ou browser.

Browser Use não faz parte do caminho principal de `web.browser`; quando adotado,
fica como integração opcional para Workers/Tasks de automação longa e complexa.
Endpoints, credenciais e seleção de adapters pertencem à configuração global do
Atlas, nunca às regras específicas do Agent.

Mudanças na estrutura ou nas fronteiras do projeto devem atualizar este documento.
