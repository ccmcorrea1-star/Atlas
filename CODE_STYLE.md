# Code Style

Este documento define o padrão de escrita do código do Atlas.

A organização do projeto está em [`ARCHITECTURE.md`](./ARCHITECTURE.md).

## Código

O código deve ser escrito em inglês.

Prefira código pequeno, simples, organizado e legível. Funções e arquivos devem ter responsabilidades claras.

Evite funções grandes, muitos níveis de indentação, duplicação e abstrações sem necessidade real.

Use as convenções idiomáticas da linguagem e os formatters e linters definidos pelo projeto.

## Nomes

Use nomes claros e específicos em inglês para funções, variáveis, tipos, módulos e arquivos.

Evite abreviações e nomes genéricos quando existir uma opção mais descritiva.

## Comentários

Comentários devem ser curtos, diretos e escritos em português do Brasil.

Use comentários para explicar rapidamente o que uma parte relevante do código faz.

```ts
// Carrega a configuração do agente.
const config = loadConfig();
```

Evite comentários longos ou desnecessários.

## Organização

Mantenha código relacionado próximo. Separe responsabilidades diferentes e remova código morto.

Não crie arquivos, funções, tipos ou abstrações sem necessidade.

## Documentação

A documentação deve ser escrita em português do Brasil, de forma limpa, curta e direta.

Ao mencionar outro arquivo, módulo, documentação ou referência, use um link sempre que possível.

```md
Veja [`ARCHITECTURE.md`](./ARCHITECTURE.md).
```

Evite repetir na documentação o que o código já deixa claro.