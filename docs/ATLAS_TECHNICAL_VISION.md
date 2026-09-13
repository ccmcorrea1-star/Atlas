# Atlas — Visão Técnica e Arquitetura

**Versão 0.8 — Setembro de 2026**

> Este documento é a fonte de verdade para a visão, os princípios e as fronteiras arquiteturais do Atlas.

## 1. Resumo

Atlas é uma plataforma de inteligência pessoal residente e local-first, capaz de compreender, operar e evoluir o ambiente digital do usuário.

Atlas não é apenas um chatbot ou copiloto. Ele é uma camada operacional persistente entre o usuário, seus computadores, aplicações, serviços, conhecimento e dispositivos.

> **Atlas é uma inteligência residente para o ambiente digital do usuário.**

O objetivo é evoluir de um agente reativo para uma inteligência residente: continuamente disponível, capaz de recuperar o contexto necessário, descobrir capacidades, agir por meio de tools, manter estado e delegar trabalho somente quando isso trouxer benefício.

## 2. Princípios centrais

### Máxima capacidade, mínimo contexto necessário

Atlas pode ter acesso a muitas capacidades sem carregar todos os detalhes no contexto do modelo.

O custo de contexto deve crescer com a relevância para a tarefa atual, não com o tamanho total da plataforma.

### Ação direta quando suficiente; delegação quando útil

O Agent interpreta objetivos, planeja, seleciona capacidades, chama tools e avalia resultados.

O caminho normal deve ser o mais simples possível:

```text
Agent
  ↓ tool
Runtime
  ↓
Ambiente
```

Delegação não é obrigatória. O Agent pode criar uma Task e usar um Worker quando o trabalho for longo, paralelo, persistente, especializado ou quando for útil desacoplar a execução do turno principal.

```text
Agent
  ├── tool → Runtime → Ambiente
  │
  └── quando houver benefício
      ↓
     Task
      ↓
    Worker
      ↓
     tools
      ↓
    Runtime
```

Uma chamada direta de tool deve ser preferida quando resolver o objetivo de forma simples e eficiente.

### Discovery cria conhecimento, não permissão

Discovery torna recursos conhecidos pelo modelo. Autorização e execução são dimensões separadas.

### Divulgação progressiva

Atlas revela detalhes conforme necessário:

```text
resumo
  ↓
candidatos
  ↓
materialização completa
  ↓
uso
```

Esse padrão se aplica a tools, skills, memória e conhecimento.

## 3. Recursos descobríveis

Nem tudo que pode ser descoberto é uma capacidade executável.

```text
Discoverable Resource
├── Capability
│   ├── Tool
│   ├── Skill
│   └── MCP Capability
├── Memory
├── Knowledge
└── Resource
```

Discovery pesquisa recursos descobríveis. O Capability Registry administra capacidades executáveis e procedurais.

## 4. Discovery

Discovery permite que o Agent conheça capacidades progressivamente, sem receber todos os schemas e instruções antecipadamente.

Antes de navegar, o Agent recebe apenas um catálogo compacto dos grupos de tools disponíveis. Esse catálogo fornece o mapa de alto nível do que Atlas pode fazer, sem expor as tools individuais nem seus schemas.

```text
process — executar e gerenciar processos
filesystem — operar arquivos e diretórios
docker — operar containers, imagens, redes e volumes
git — operar repositórios Git
system — consultar e operar o sistema
```

A partir desse mapa, o modelo deve suportar dois caminhos complementares:

1. navegação hierárquica;
2. busca direta.

### Navegação hierárquica

As capacidades podem ser organizadas em níveis previsíveis, por exemplo:

```text
docker
├── container
│   ├── list
│   ├── inspect
│   ├── logs
│   ├── restart
│   └── stats
├── image
│   ├── list
│   ├── inspect
│   └── pull
├── network
├── volume
└── system
    ├── info
    └── status
```

O Agent pode se aprofundar somente quando necessário:

```text
docker
  ↓
docker.container
  ↓
docker.container.logs
  ↓
schema completo
```

O primeiro nível conhecido pelo Agent é o grupo. Ao consultar `docker`, recebe apenas os subgrupos ou capacidades imediatamente relevantes daquela área. Ao consultar `docker.container`, recebe as tools daquele grupo. Ao chegar a uma capacidade executável, pode solicitar ou receber sua materialização completa.

A hierarquia organiza o espaço de capacidades; ela não exige que todas as tools tenham exatamente três níveis. O nome deve refletir o domínio de forma clara, sem criar árvores artificialmente profundas.

### Busca direta

Quando a intenção já é específica, o Agent não precisa navegar por toda a hierarquia.

Exemplo:

```text
"logs de container docker"
  ↓
docker.container.logs
```

Assim, Discovery oferece exploração quando o Agent ainda não sabe exatamente o que precisa e acesso rápido quando a intenção já está clara.

### Como o Agent busca e executa

O Agent mantém uma capability mínima de Discovery disponível:

```text
discover({ path?, query? })
```

A navegação hierárquica usa `path`:

```text
discover({ path: "docker" })
  ↓
docker.container
docker.image
docker.network
docker.volume
docker.system
```

O aprofundamento continua somente quando necessário:

```text
discover({ path: "docker.container" })
  ↓
docker.container.list
docker.container.inspect
docker.container.logs
docker.container.restart
```

Quando a intenção já é específica, a busca direta usa `query`:

```text
discover({ query: "logs de container docker" })
  ↓
docker.container.logs
```

O Discovery consulta o Capability Registry. Ao encontrar uma tool relevante, o Runtime carrega sua definição completa e disponibiliza o schema dessa tool ao modelo. O Agent então chama a tool diretamente.

```text
Agent
  ↓ discover(...)
Capability Registry
  ↓
docker.container.logs
  ↓ materialização
schema da tool
  ↓
Agent chama docker.container.logs(...)
  ↓
Runtime resolve a implementação
  ↓
execução
  ↓
resultado
```

Discovery não executa a capability. Ele apenas permite que o Agent a encontre e obtenha o nível de detalhe necessário.

Para skills, o fluxo é semelhante até a materialização: Discovery encontra a skill, o Runtime materializa o procedimento e o Agent segue esse procedimento chamando as tools necessárias.

### Materialização progressiva

O fluxo conceitual para tools é:

```text
grupos de tools
  ↓
subgrupos ou candidatas
  ↓
detalhes do recurso relevante
  ↓
schema completo
```

Compaction pode remover detalhes já materializados do contexto. Se a capacidade voltar a ser necessária, o Agent pode aprofundar o Discovery novamente sem carregar todo o catálogo antecipadamente.

O paper não fixa neste momento a implementação interna de índices, ranking, embeddings ou armazenamento do estado de Discovery. Esses detalhes pertencem ao design técnico da implementação.

## 5. Tools

Tools representam ações executáveis e estruturadas.

O Agent não recebe todos os schemas nem a lista completa de tools antecipadamente.

Inicialmente, recebe apenas os grupos de tools e uma descrição curta de cada grupo:

```text
process — executar e gerenciar processos
filesystem — operar arquivos e diretórios
docker — operar containers, imagens, redes e volumes
git — operar repositórios Git
system — consultar e operar o sistema
```

Ao se aprofundar em Docker:

```text
docker.container
docker.image
docker.network
docker.volume
docker.system
```

Ao se aprofundar em `docker.container`:

```text
docker.container.list — lista containers
docker.container.inspect — inspeciona um container
docker.container.logs — lê logs de um container
docker.container.restart — reinicia um container
```

Somente a tool necessária precisa ter seu schema completo materializado.

### Chamada de tool

Depois que Discovery encontra a capacidade, o Runtime deve materializá-la como uma tool disponível para o modelo. O Agent então chama essa tool diretamente.

```text
Agent
  ↓ Discovery
capacidade encontrada
  ↓ materialização
schema da tool disponível
  ↓
Agent chama a tool
  ↓
Runtime executa
  ↓
resultado volta ao Agent
```

Exemplo:

```text
discovery → docker.container.logs
materialização → docker.container.logs({ container, tail? })
chamada → docker.container.logs({ container: "atlas-db", tail: 100 })
```

O caminho principal não deve depender de uma tool genérica do tipo `call_tool(name, args)`, porque isso esconderia os schemas reais atrás de uma interface genérica. Uma invocação genérica pode existir como compatibilidade ou fallback, mas não deve ser o modelo principal.

Depois que uma tool é escolhida, sua execução deve ser direta e determinística sempre que possível.

> **Descobrir de forma ampla, materializar de forma estreita e executar de forma determinística.**

## 6. Skills

Skills representam procedimentos reutilizáveis para combinar capacidades.

> **Tools definem o que Atlas pode fazer. Skills definem como Atlas pode combinar essas capacidades para atingir um objetivo.**

A divulgação de uma skill também é progressiva:

```text
1. resumo da skill
2. metadados: objetivo, quando usar, pré-condições e capacidades necessárias
3. procedimento completo
```

### Chamada de skill

Uma skill não precisa ser uma operação executável como uma tool.

Quando Discovery encontra uma skill relevante, o procedimento é materializado no contexto do Agent. O Agent passa a seguir esse procedimento e chama as tools necessárias normalmente.

```text
Agent
  ↓ Discovery
deploy-service
  ↓ materialização
procedimento da skill
  ↓
Agent segue o procedimento
  ↓
chama tools necessárias
```

Exemplo:

```text
Skill: deploy-service
1. verificar estado do repositório
2. executar testes
3. gerar build
4. fazer deploy
5. validar serviço
```

O Agent pode então usar:

```text
git.status
tests.run
docker.build
service.restart
service.health
```

Uma skill pode declarar as tools que normalmente utiliza. Ao materializar a skill, Atlas pode materializar também os schemas das tools necessárias quando isso for útil.

A skill não precisa se transformar em uma tool monolítica.

Em resumo:

```text
Tool = fazer alguma coisa
Skill = saber como fazer alguma coisa
```

## 7. Contexto, conversa, memória e conhecimento

Esses conceitos são distintos.

**Estado da conversa** é o histórico e o estado operacional da conversa atual.

**Contexto de trabalho** é a informação atualmente materializada para o modelo. É transitório e limitado por relevância e orçamento de contexto.

**Memória de longo prazo** contém fatos persistentes e recuperáveis sobre o usuário, o ambiente, preferências, sistemas, projetos e decisões anteriores.

**Conhecimento** vem de documentos, web, código, documentação, índices e sistemas externos.

```text
Entrada do usuário
    ↓
Estado da conversa
    ↓
Discovery
    ↓
Recuperação de memória
    ↓
Recuperação de conhecimento
    ↓
Materialização de capacidades
    ↓
Orçamento de contexto
    ↓
Modelo
```

> **Nada entra no contexto do modelo apenas porque existe.**

## 8. Compactação de contexto

Compactação não é apenas resumir a conversa. Ela reconstrói o contexto de trabalho para manter somente o que continua relevante ao assunto atual.

O `system prompt` permanece intacto e não é resumido pela compactação.

Ao compactar, Atlas deve identificar o assunto ativo, o objetivo atual e o estado do trabalho. Conteúdo de assuntos anteriores que não seja mais relevante sai do contexto ativo, mas continua preservado no estado persistente da conversa.

O contexto compactado deve ser organizado aproximadamente nesta ordem:

```text
system prompt
assunto atual
objetivo atual
estado do trabalho
skills relevantes
tools relevantes
resultados e referências importantes
decisões e restrições
últimas mensagens
```

### Assunto e objetivo

A compactação deve identificar explicitamente sobre o que o Agent está trabalhando e qual resultado está tentando alcançar.

```text
Assunto:
Diagnóstico do container atlas-db.

Objetivo:
Descobrir por que atlas-db está reiniciando e corrigir o problema.
```

Quando o assunto muda, informações antigas que não contribuem mais para o objetivo atual deixam de ocupar o contexto ativo.

### Skills e tools

Skills ainda relevantes permanecem organizadas por nome e finalidade. O procedimento completo pode ser removido quando não for necessário naquele momento e materializado novamente pelo Discovery quando necessário.

Tools relevantes ou em uso permanecem identificadas no contexto. Schemas completos podem ser removidos após o uso e materializados novamente antes de uma nova chamada.

```text
Skills:
- docker-diagnostics — diagnóstico de problemas Docker

Tools:
- docker.container.inspect
- docker.container.logs
- docker.container.restart
```

### Estado, resultados e referências

Resultados grandes de tools, logs, documentos e outras saídas devem ser reduzidos ao estado relevante e, quando possível, manter referência ao conteúdo original persistido pelo Runtime.

```text
Estado:
- atlas-db está em restart loop;
- exit code 1;
- logs indicam falha de conexão com PostgreSQL.

Resultados:
- inspect: exit code 1 → referência ao resultado original
- logs: connection refused em postgres:5432 → referência ao resultado original
```

Compactação não deve ser a única cópia de informação importante. Resultados completos, artefatos, Tasks e demais estados persistentes continuam fora do contexto do modelo.

### Decisões, restrições e últimas mensagens

Decisões tomadas e restrições ainda válidas devem permanecer explícitas.

As mensagens mais recentes permanecem literalmente no contexto, sem serem substituídas imediatamente por resumo, para preservar nuance, intenção e continuidade da conversa.

O fluxo conceitual é:

```text
contexto cresce
  ↓
identificar assunto ativo
  ↓
identificar objetivo atual
  ↓
remover conteúdo fora do assunto
  ↓
consolidar estado relevante
  ↓
organizar skills e tools relevantes
  ↓
resumir resultados grandes e manter referências
  ↓
preservar decisões e restrições
  ↓
manter últimas mensagens
  ↓
novo contexto de trabalho
```

> **O contexto ativo é um cache de trabalho do Agent; o estado persistente do Atlas é a fonte de verdade.**

## 9. Memória

A memória é recuperada sob demanda; não é despejada integralmente no prompt.

```text
1. contexto basal / resumo de perfil
2. memórias candidatas
3. registros completos relevantes
```

O custo de acesso à memória deve crescer com a relevância para a tarefa, não com o volume total armazenado.

Cada memória deve possuir escopo, origem, atualidade, confiança, importância e provenance suficientes para permitir recuperação e atualização corretas.

Memórias podem ter origem explícita, observada ou inferida; essas origens não devem ter a mesma confiança.

Atlas não deve persistir automaticamente tudo que observa.

```text
nova informação
    ↓
candidata a memória
    ↓
deduplicação / conflito
    ↓
avaliação de importância
    ↓
armazenar / atualizar / descartar
```

Quando uma informação nova contradiz uma memória existente, o sistema deve atualizar, versionar, invalidar ou representar explicitamente o conflito.

## 10. Capability Registry e Capability Factory

Toda capacidade conhecida pela plataforma deve entrar em um Registry unificado e independente da linguagem de implementação.

O Registry pode reunir capacidades vindas de diferentes implementações:

```text
tool local
MCP
API
CLI
dispositivo
capacidade gerada
```

O Agent não precisa conhecer a origem de implementação para usar a capacidade de forma consistente.

Atlas também pode criar novas capacidades quando uma operação recorrente se beneficia de uma implementação reutilizável e determinística.

```text
uso recorrente
  ↓
padrão identificado
  ↓
proposta
  ↓
geração
  ↓
validação
  ↓
testes
  ↓
aprovação
  ↓
registro
```

Preferência:

```text
código determinístico
>
tool estruturada
>
skill
>
improvisação do LLM
```

Capacidades geradas precisam de identidade, versão, origem, testes, dependências e provenance.

## 11. Tasks e Workers

Tasks existem para trabalho que se beneficia de execução desacoplada do turno principal. Elas não são o caminho obrigatório para toda ação.

```text
Agent → tool → Runtime → resultado
```

Quando houver benefício:

```text
Agent → Task → Worker → tools → Runtime
```

Uma Task representa estado persistente de trabalho. Um Worker é um executor temporário.

Invariantes:

- Tasks persistem;
- Workers podem ser descartados e recriados;
- cancelamento é explícito;
- efeitos relevantes devem ser identificáveis;
- retries não podem duplicar silenciosamente efeitos;
- recovery deve ser possível quando a operação permitir.

## 12. Eventos, provenance e estado

O Runtime deve produzir eventos estruturados para tornar o trabalho observável.

```text
turn.started
tool.started
tool.completed
execution.started
execution.completed
tool.failed
context.discovered
task.started
task.progress
task.completed
```

Eventos expõem atividade operacional, não chain-of-thought.

Atlas deve conseguir reconstruir a origem de ações relevantes:

```text
usuário
  ↓
sessão
  ↓
turno
  ↓
chamada direta de tool
  ou
Task → Worker → tool
  ↓
artefato / efeito
```

O estado persistente do Atlas não deve depender estruturalmente de tipos internos de um provider ou Agent SDK específico.

## 13. MCP, dispositivos e clientes

MCP é uma fronteira de integração, não o mecanismo interno obrigatório de Discovery.

```text
MCP Server
   ↓
MCP Adapter
   ↓
Capability Registry
   ↓
Discovery
   ↓
Agent
```

Uma capability pode vir de MCP e ainda ser apresentada ao Agent sob a mesma taxonomia usada por capacidades locais.

O Agent não precisa saber se uma capacidade é implementada por função local, MCP, API, CLI ou dispositivo.

O servidor pode atuar como cérebro enquanto desktop, telefone, tablet e outros dispositivos tornam-se extensões operacionais.

Identidade do usuário e origem da interação são conceitos distintos.

Clientes não devem conter a inteligência principal do Agent.

```text
Atlas Runtime
     ↓
eventos estruturados / API
     ↓
Clientes
```

A interface apresenta estado e coleta interação. O Runtime mantém execução e estado operacional. O Agent conduz o trabalho cognitivo.

## 14. Local-first e independência de infraestrutura

Atlas é local-first, não necessariamente local-only.

Execução local e estado local são caminhos de primeira classe. Serviços cloud podem existir, mas devem ser dependências substituíveis.

Modelos, providers e harnesses também são substituíveis.

```text
Atlas Runtime API
        ↓
Agent SDK Adapter
        ↓
Agent SDK
        ↓
Model Provider
```

Atlas deve possuir contratos próprios. Tipos internos de um SDK não devem definir a API pública central do produto.

## 15. Workstation e trabalho observável

Atlas Workstation não deve ser apenas um chat maior. É um espaço visual compartilhado onde usuário e Agent trabalham sobre os mesmos objetos, aplicações e estado do mundo.

O usuário deve conseguir acompanhar o trabalho sem receber raciocínio privado.

## 16. Invariantes arquiteturais

1. O Agent age diretamente por meio de tools quando isso é suficiente.
2. Delegação para Tasks e Workers é opcional e usada quando houver benefício operacional.
3. O Agent começa conhecendo apenas os grupos de tools; Discovery permite aprofundamento hierárquico e busca direta.
4. Discovery cria conhecimento sobre recursos; não cria autoridade.
5. Somente o contexto necessário é materializado para o modelo.
6. Tools usam materialização progressiva de schema.
7. Depois de descoberta e materializada, uma tool é chamada diretamente pelo Agent.
8. Uma skill materializa procedimento; o Agent continua chamando as tools necessárias.
9. O `system prompt` não é resumido pela compactação.
10. Compactação identifica assunto, objetivo e estado do trabalho e remove do contexto ativo o que deixou de ser relevante.
11. Skills e tools relevantes permanecem organizadas no contexto, com rematerialização de detalhes quando necessário.
12. As últimas mensagens permanecem literalmente no contexto após compactação.
13. Compactação não substitui o estado persistente nem é a única cópia de informação importante.
14. Memória é recuperada por relevância; nunca despejada integralmente no contexto.
15. Estado da conversa, contexto de trabalho, memória de longo prazo e conhecimento são conceitos distintos.
16. Trabalho persistente pertence a Tasks; Workers são executores temporários.
17. Toda ação relevante deve possuir provenance suficiente para ser rastreada.
18. Eventos visíveis expõem trabalho operacional, não raciocínio privado.
19. Capacidades geradas não são confiáveis apenas porque Atlas as gerou.
20. Clientes não possuem o runtime cognitivo principal.
21. Providers e Agent SDKs são substituíveis.
22. Atlas possui seus contratos públicos e seu estado persistente.
23. Execução e estado locais permanecem de primeira classe.
24. O custo de acessar tools, skills, memória e conhecimento cresce com a relevância atual, não com o tamanho total do sistema.
25. Depois da seleção de uma tool, a execução deve ser determinística sempre que possível.
26. Delegação nunca deve ser introduzida apenas por arquitetura quando a execução direta é suficiente.
27. MCP é uma fonte de capacidades, não uma dependência estrutural do núcleo do Discovery.

## 17. Objetivo de longo prazo

Atlas deve evoluir de `assistant` para `resident intelligence`.

Uma inteligência residente compreende o ambiente, mantém estado, conhece suas capacidades, recupera contexto relevante, acompanha trabalho em andamento, age diretamente quando possível e delega quando necessário.

No estado final, o usuário trabalha normalmente enquanto Atlas permanece presente, operando o ambiente de forma contínua e observável.

**Esse é o Atlas.**

## 18. Referências da implementação atual

- Repositório: https://github.com/ccmcorrea1-star/Atlas
- OpenAI Agents SDK: https://openai.github.io/openai-agents-js/
- Sessões do OpenAI Agents SDK: https://openai.github.io/openai-agents-js/guides/sessions/
- Extensão AI SDK: https://openai.github.io/openai-agents-js/extensions/ai-sdk/
- OpenCode Go: https://opencode.ai/docs/pt-br/go/
