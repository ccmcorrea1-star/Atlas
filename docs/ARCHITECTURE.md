# Arquitetura

Este documento define onde cada responsabilidade do Atlas deve ficar.

## Estrutura

- [`src/`](../src/) — Runtime e API principal.
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

Antes de criar um novo módulo, verifique se a responsabilidade pertence a um módulo existente.

Mudanças na estrutura ou nas fronteiras do projeto devem atualizar este documento.
