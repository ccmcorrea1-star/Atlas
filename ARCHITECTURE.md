# Arquitetura

Este documento define como o código do Atlas deve ser organizado.

Antes de alterar código, leia também [`CODE_STYLE.md`](./CODE_STYLE.md).

## Princípios

Cada módulo deve ter uma responsabilidade clara. Código relacionado deve permanecer próximo e responsabilidades diferentes devem permanecer separadas.

Prefira estruturas pequenas e simples. Não crie novas camadas, módulos ou abstrações sem necessidade real.

Dependências devem ser explícitas. Evite dependências circulares e acoplamento desnecessário.

## Estrutura

- [`src/`](./src/) contém o Runtime e a API principal do Atlas.
- [`src/capabilities/`](./src/capabilities/) contém capabilities nativas.
- [`clients/`](./clients/) contém clientes desacoplados do Runtime.
- [`protocol/`](./protocol/) contém contratos compartilhados.
- [`tests/`](./tests/) contém testes.
- [`docs/`](./docs/) contém documentação técnica.

Antes de criar um novo arquivo ou módulo, verifique se a responsabilidade já pertence a uma estrutura existente.

## Fronteiras

Clientes não devem concentrar regras do Runtime.

Integrações externas devem permanecer isoladas para que detalhes de APIs, SDKs e bibliotecas não se espalhem pelo projeto.

Código compartilhado entre linguagens deve depender de contratos claros, não de detalhes internos de outra implementação.

## Linguagens

A arquitetura não depende de uma linguagem específica. Cada parte deve seguir as convenções da linguagem utilizada e as regras de [`CODE_STYLE.md`](./CODE_STYLE.md).

## Mudanças

Antes de alterar a arquitetura existente, entenda a implementação atual e preserve seus limites sempre que possível.

Mudanças estruturais relevantes devem atualizar este documento.