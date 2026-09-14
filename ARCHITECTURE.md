# Arquitetura

Este documento define onde cada responsabilidade do Atlas deve ficar.

## Estrutura

- [`src/`](./src/) — Runtime e API principal.
- [`src/capabilities/`](./src/capabilities/) — capabilities nativas.
- [`clients/`](./clients/) — clientes desacoplados do Runtime.
- [`protocol/`](./protocol/) — contratos compartilhados.
- [`tests/`](./tests/) — testes.
- [`docs/`](./docs/) — documentação técnica.

## Regras

Cada módulo deve ter uma responsabilidade clara.

Clientes não devem conter regras do Runtime.

Integrações externas devem ficar isoladas de regras internas.

Código compartilhado entre linguagens deve depender de contratos definidos em [`protocol/`](./protocol/).

Antes de criar um novo módulo, verifique se a responsabilidade pertence a um módulo existente.

Mudanças na estrutura ou nas fronteiras do projeto devem atualizar este documento.