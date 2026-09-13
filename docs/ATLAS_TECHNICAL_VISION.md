# Atlas — Technical Vision & Architecture

**Version 0.3 — September 2026**

> Este documento é a fonte de verdade para a visão, os princípios e as fronteiras arquiteturais do Atlas.
>
> Ele descreve o produto-alvo e os invariantes que devem permanecer verdadeiros enquanto a implementação evolui.

## 1. Abstract

Atlas é uma plataforma de inteligência pessoal residente e local-first, capaz de compreender, operar e evoluir o ambiente digital do usuário.

Atlas não é apenas um chatbot, copiloto ou interface para modelos de linguagem. Ele é uma camada operacional persistente entre o usuário, seus computadores, aplicações, serviços, conhecimento e dispositivos.

Versão curta:

> **Atlas é uma inteligência residente para o ambiente digital do usuário.**

O objetivo é evoluir de um agente reativo para uma inteligência residente: continuamente disponível, consciente do contexto necessário, capaz de descobrir capacidades, executar ações, manter estado, delegar trabalho e operar o ambiente do usuário de forma observável e controlada.

---

## 2. Vision

No estado final, o usuário continua trabalhando normalmente enquanto Atlas permanece presente no ambiente.

Atlas deve ser capaz de:

- compreender objetivos e contexto;
- conhecer o ambiente digital;
- recuperar memória relevante;
- consultar conhecimento;
- descobrir capacidades sob demanda;
- operar aplicações e serviços;
- controlar dispositivos;
- executar ações;
- delegar trabalhos longos;
- acompanhar resultados;
- criar novas capacidades quando necessário;
- apresentar o trabalho em andamento sem expor raciocínio privado;
- permitir intervenção humana quando necessário.

O usuário não deveria precisar transformar cada intenção em uma sequência manual de comandos.

---

## 3. Current State vs Target State

### Current state

A implementação atual é o bootstrap do runtime do Agent.

Hoje o projeto utiliza:

- TypeScript;
- OpenAI Agents SDK como harness;
- OpenCode Go como provider;
- `gpt-5.6-luna` como modelo inicial;
- conversas persistentes em memória durante a vida do processo;
- `x-opencode-session` estável por conversa;
- testes locais, smoke test e CI.

### Target state

A visão do Atlas é maior que o runtime atual.

O produto-alvo inclui progressivamente:

```text
Agent Runtime
Discovery
Capability Registry
Tools
Skills
Memory
Knowledge
Tasks
Workers
Events
Policy
Execution
MCP
Device Fabric
Workstation
Applications
```

A implementação atual não deve limitar a arquitetura futura.

---

## 4. Scope and Non-goals

Atlas deve ser uma plataforma agentic geral, não um agente preso a uma única finalidade, provider, interface ou linguagem.

Não é objetivo arquitetural:

- acoplar Atlas permanentemente ao OpenAI Agents SDK;
- definir Atlas como um agente de programação;
- carregar todas as tools, skills ou memórias no contexto do modelo;
- transformar Discovery em sistema de permissões;
- usar o modelo como executor direto de autoridade;
- expor chain-of-thought ao usuário;
- tornar cloud obrigatória para toda operação;
- persistir toda informação observada como memória.

---

## 5. Architectural Principles

### 5.1 Maximum capability, minimum necessary context

```text
maximum capability
+
minimum necessary context
```

Atlas pode ter acesso a milhares de capabilities sem enviar milhares de schemas, instruções ou memórias ao modelo em cada turno.

O custo de contexto deve crescer com o que é relevante para a tarefa atual, não com o tamanho total da plataforma.

### 5.2 Agent decides; Runtime executes

O Agent interpreta objetivos, planeja, seleciona capacidades, solicita ações, delega trabalho e avalia resultados.

O Runtime executa efeitos no mundo.

```text
Agent
  ↓ decision / intent
Runtime
  ↓ execution
Environment
```

O Agent nunca é a autoridade executora final.

### 5.3 Discovery is knowledge, not permission

Discovery cria awareness.

Policy cria autoridade.

Execution cria efeitos.

Essas dimensões devem permanecer separadas.

### 5.4 Progressive disclosure

Atlas revela detalhes ao modelo progressivamente.

```text
summary
  ↓
candidates
  ↓
full materialization
  ↓
execution / use
```

Esse padrão se aplica a tools, skills, memory e knowledge.

### 5.5 Content is data, not authority

Arquivos, páginas web, emails, documentos, resultados de tools, MCP e qualquer conteúdo externo podem fornecer informação.

Eles não podem, por si mesmos:

- conceder permissões;
- alterar policy;
- aprovar execução;
- revelar secrets;
- criar autoridade.

---

## 6. System Model

```text
                    Atlas
                      │
                Main Agent
                      │
               Agent Runtime
                      │
        ┌─────────────┼─────────────┐
        │             │             │
    Discovery       Memory        Tasks
        │             │             │
        └──────┬──────┴──────┬──────┘
               │             │
         Capability Registry │
               │             │
      ┌────────┼────────┐    │
      │        │        │    │
    Tools    Skills    MCP  Workers
      │                 │
      └────────┬────────┘
               │
            Policy
               │
          Execution
               │
       Digital Environment
```

---

## 7. Awareness, Availability, Authorization and Execution

Uma capability possui dimensões independentes.

### Availability

```text
AVAILABLE
UNAVAILABLE
```

Indica se a capability existe e está operacional.

### Awareness

```text
KNOWN
UNKNOWN
```

Indica se o modelo recebeu informação suficiente para saber que a capability existe e para que serve.

### Authorization

```text
ALLOW
CONFIRM
DENY
```

Indica se aquela ação pode ser executada naquele contexto.

### Execution

```text
IDLE
RUNNING
COMPLETED
FAILED
```

Indica o estado operacional da execução.

Discovery transforma principalmente:

```text
UNKNOWN → KNOWN
```

Discovery não concede autorização.

---

## 8. Discoverable Resources

Nem tudo que pode ser descoberto é uma capability executável.

Atlas deve utilizar o conceito mais amplo de `Discoverable Resource`:

```text
Discoverable Resource
├── Capability
│   ├── Tool
│   ├── Skill
│   └── MCP Capability
│
├── Memory
├── Knowledge
└── Resource
```

Discovery pesquisa recursos descobríveis.

Capability Registry administra capacidades executáveis e procedurais.

---

## 9. Discovery

Discovery deve selecionar o menor conjunto suficiente de recursos para a tarefa atual.

Exemplo:

```text
User intent
  ↓
Agent: "preciso inspecionar containers"
  ↓
Discovery
  ↓
container / docker candidates
  ↓
materialize relevant capabilities
  ↓
Agent selects tool
```

Discovery pode trabalhar em níveis:

```text
1. compact catalog
2. candidate metadata
3. full materialization
```

Uma capability pode continuar `KNOWN` durante uma conversa enquanto permanecer relevante.

Compaction ou mudança de contexto pode remover detalhes materializados sem tornar a capability indisponível.

---

## 10. Tools

Tools representam ações executáveis e estruturadas.

Exemplos:

```text
files.read
files.write
files.edit
process.exec
shell.exec
git.status
docker.list
memory.search
tasks.create
```

### Progressive tool materialization

O Agent não recebe todos os schemas antecipadamente.

Primeiro recebe um catálogo compacto:

```text
files — ler, escrever e buscar arquivos
git — operar repositórios Git
docker — inspecionar e controlar containers
system — consultar e operar o sistema local
```

Quando necessário, Discovery retorna candidatas:

```text
docker.list — lista containers
docker.inspect — inspeciona container
docker.logs — lê logs
```

Somente depois o schema completo é materializado:

```ts
docker.inspect({
  container: string
})
```

Princípio:

> **Discover broadly, materialize narrowly, execute deterministically.**

### Tool contract

Uma tool pode possuir:

```text
tool metadata
→ discovery

schema
→ executable contract

TOOL.md
→ procedural documentation
```

`TOOL.md` não define policy nem autorização.

### Execution

Depois que uma tool é escolhida, sua execução deve ser determinística sempre que possível:

```text
LLM
→ structured tool call
→ Runtime
→ implementation
```

Evitar routers LLM adicionais quando um dispatch determinístico é suficiente.

---

## 11. Skills

Skills representam procedimentos reutilizáveis para combinar capabilities.

> **Tools define what Atlas can do. Skills define how Atlas should combine capabilities to accomplish a goal.**

Exemplo:

```text
deploy-service — deploy seguro com validação e rollback
debug-container — diagnostica problemas em containers
review-repository — analisa um repositório
```

### Progressive skill disclosure

```text
1. Skill summary
   nome + descrição curta

2. Skill metadata
   objetivo
   quando usar
   pré-condições
   capabilities necessárias

3. Full skill instructions
   procedimento completo
```

Ao materializar uma skill, Atlas pode materializar também as tools normalmente necessárias.

Exemplo:

```yaml
name: deploy-service
requires:
  - git.status
  - docker.build
  - docker.restart
  - http.health
```

A skill não precisa ser uma tool monolítica. O Agent segue o procedimento e continua fazendo native structured tool calls.

---

## 12. Context, Conversation State, Memory and Knowledge

Esses conceitos são distintos.

### Conversation State

Histórico e estado operacional da conversa atual.

Conversation State não é Long-term Memory.

### Working Context

Informação atualmente materializada para o modelo.

É transitória e limitada por relevância e orçamento de contexto.

### Long-term Memory

Fatos persistentes e recuperáveis sobre:

```text
user
environment
preferences
systems
projects
relationships
procedures
past decisions
```

### Knowledge

Informação recuperável de fontes como:

```text
documents
web
code
documentation
indexes
external systems
```

### Context pipeline

```text
User Input
    ↓
Conversation State
    ↓
Discovery
    ↓
Memory Retrieval
    ↓
Knowledge Retrieval
    ↓
Capability Materialization
    ↓
Context Budget
    ↓
Model
```

Princípio:

> **Nothing enters model context merely because it exists.**

---

## 13. Memory

> **Memory is retrieved, not dumped.**

Atlas não deve carregar toda a memória do usuário no prompt.

### Progressive memory retrieval

```text
1. Memory summary / profile context
2. Candidate memories
3. Full relevant records
```

O custo de acesso à memória deve crescer com a relevância para a tarefa, não com o volume total armazenado.

### Suggested memory record

```ts
Memory {
  id
  type
  content
  scope
  source
  createdAt
  updatedAt
  confidence
  freshness
  importance
  provenance
}
```

Scopes podem incluir:

```text
user
project
device
environment
organization
conversation
```

### Memory origin

Memórias podem ser:

```text
explicit
observed
inferred
```

Essas origens não devem ter a mesma confiança.

### Memory writes

Atlas não deve persistir automaticamente tudo que observa.

```text
new information
    ↓
memory candidate
    ↓
dedupe / conflict check
    ↓
importance + persistence decision
    ↓
store / update / discard
```

### Memory and authority

Memory fornece informação.

Memory não concede autorização.

Uma memória dizendo que o usuário costuma aprovar uma ação não substitui Policy.

---

## 14. Knowledge

Knowledge deve considerar origem, versão e freshness.

```text
stable
versioned
volatile
```

Exemplos:

- uma especificação pode ser relativamente estável;
- documentação de API pode depender de versão;
- estado de serviço pode ser altamente volátil;
- notícias dependem de freshness atual.

Knowledge retrieval deve seguir o mesmo princípio de materialização progressiva aplicado à memória.

---

## 15. Capability Registry

Toda capability conhecida pela plataforma deve entrar em um Registry unificado.

```text
implementation
      ↓
registry
      ↓
discovery
      ↓
KNOWN
```

O Registry pode representar:

```text
first-party tools
skills
MCP tools
generated capabilities
future capability types
```

O Registry não deve ficar acoplado a uma linguagem específica.

---

## 16. Capability Factory

Atlas pode criar novas capabilities quando uma operação recorrente se beneficia de uma implementação reutilizável e determinística.

```text
usage
  ↓
pattern detected
  ↓
capability proposal
  ↓
generation
  ↓
validation
  ↓
tests
  ↓
approval
  ↓
registration
```

Lifecycle sugerido:

```text
DRAFT
GENERATED
VALIDATED
TESTED
APPROVED
REGISTERED
ACTIVE
DEPRECATED
REVOKED
```

Princípio:

```text
deterministic code
>
structured tool
>
skill
>
LLM improvisation
```

Toda capability gerada deve possuir identidade e provenance suficientes para sabermos exatamente o que foi aprovado.

Metadata mínima desejável:

```text
version
origin
generator
tests
required permissions
dependencies
provenance
checksum / immutable identity
```

Uma capability não se torna confiável apenas porque Atlas a gerou.

---

## 17. Execution

Atlas deve operar o ambiente real através de executores controlados.

Exemplos:

```text
filesystem
processes
shell
Git
Docker
services
APIs
MCP
devices
applications
```

### process.exec vs shell.exec

`process.exec` executa processos com argv estruturado.

`shell.exec` é reservado para quando recursos de shell forem necessários:

```text
|
&&
>
$VAR
glob
loops
multi-line scripts
```

Execução estruturada deve ser preferida quando suficiente.

---

## 18. Policy and Confirmation

Policy existe para evitar acidentes e controlar autoridade, não para tornar Atlas artificialmente incapaz.

> **Maximum capability, controlled execution.**

Policy pode produzir:

```text
ALLOW
CONFIRM
DENY
```

Fluxo:

```text
Agent
  ↓ tool intent
Policy
  ├── ALLOW
  ├── CONFIRM → user decision
  └── DENY
  ↓
Executor
```

Confirmation pertence ao Runtime, não à interface.

Clientes apenas apresentam e coletam a decisão.

---

## 19. Security and Trust Model

Atlas terá potencialmente acesso a filesystem, shell, serviços, APIs, devices e credentials. Segurança precisa ser uma fronteira arquitetural de primeira classe.

Princípios:

```text
content is not authority
least necessary disclosure
explicit provenance
controlled execution
revocable capability
observable side effects
```

### Prompt injection and untrusted content

Dados externos podem conter instruções hostis.

Atlas deve tratar:

```text
web pages
emails
files
documents
tool output
MCP output
external messages
```

como conteúdo não confiável por padrão.

Eles não podem alterar policy nem criar autoridade.

### Secrets

Secrets não devem ser embutidos, logados ou revelados ao modelo sem necessidade.

Modelo preferido:

```text
Agent
→ requests credential capability
→ Secret Broker / Runtime
→ injects or performs credential use
→ raw secret preferably never reaches the model
```

O Agent pode conhecer handles como:

```text
github.personal
opencode-go
homelab.ssh
```

sem conhecer necessariamente o valor bruto do secret.

---

## 20. Tasks and Workers

Trabalhos longos não devem bloquear o Main Agent.

```text
User
  ↓
Main Agent
├── interactive work
└── Task
      ↓
    Worker
```

### Task

Unidade persistente de trabalho.

```text
queued
running
blocked
needs_input
completed
failed
cancelled
interrupted
```

### Worker

Executor temporário de uma Task.

```text
Task
= persistent work state

Worker
= disposable execution process

Agent
= cognitive decision-maker
```

Invariantes:

```text
Tasks are durable.
Workers are disposable.
Cancellation is explicit.
Side effects are identifiable.
Retries must not silently duplicate unsafe effects.
Recovery should be possible where the underlying operation allows it.
```

---

## 21. Events and Provenance

O Runtime deve produzir eventos estruturados para tornar o trabalho observável.

Exemplos:

```text
turn.started
tool.started
tool.completed
tool.failed
context.discovered
confirmation.requested
confirmation.resolved
file.changed
file.diff
task.started
task.progress
task.completed
```

Events expõem atividade operacional, não chain-of-thought.

### Event envelope

Um contrato mínimo pode incluir:

```ts
Event {
  id
  type
  timestamp
  version
  source
  sessionId?
  turnId?
  taskId?
  workerId?
  causationId?
  correlationId?
  payload
}
```

### Provenance

Atlas deve conseguir reconstruir a origem de ações relevantes:

```text
user
  ↓
session
  ↓
turn
  ↓
task
  ↓
worker
  ↓
tool call
  ↓
artifact / side effect
```

---

## 22. State

Atlas deve possuir estado persistente próprio.

Exemplos:

```text
sessions
events
tasks
discovery state
KNOWN capabilities
memory
provenance
interaction state
```

Restart do processo não deve significar perda completa do mundo operacional.

O estado persistente do Atlas não deve depender estruturalmente de tipos internos de um provider ou Agent SDK específico.

---

## 23. MCP

MCP é uma fronteira importante de integração.

```text
Atlas
  ↓
MCP Client
  ↓
MCP Server
  ↓
External System
```

Atlas pode usar:

```text
first-party MCP servers
third-party MCP servers
remote services
```

MCP servers devem continuar independentes do Main Agent.

Capabilities vindas por MCP continuam sujeitas a Discovery, Policy, provenance e observabilidade.

---

## 24. Device Fabric

O servidor pode atuar como cérebro enquanto dispositivos tornam-se extensões operacionais.

```text
Atlas
  ├── Server
  ├── Desktop
  ├── Phone
  ├── Tablet
  └── future devices
```

Capabilities possíveis:

```text
open_url
share
files.receive
clipboard
camera
screen
media
notifications
```

Atlas deve compreender onde o usuário está trabalhando e qual device originou uma interação.

---

## 25. Identity and Interaction Origin

Identidade do usuário e origem da interação são conceitos distintos.

### User Profile

Contexto basal relativamente estável:

```text
identity
language
timezone
locale
expertise
communication preferences
devices
broad permissions
```

Não deve conter secrets ou histórico completo.

### Interaction Origin

```text
TUI
Web
Desktop
Phone
SSH
Voice
API
Automation
```

Pode conter:

```text
device
interface
transport
session
active app
active object
selection
```

---

## 26. Background Intelligence

Parte do trabalho pode ocorrer fora do turno interativo.

Exemplos:

```text
scheduled tasks
background analysis
monitoring
maintenance
knowledge updates
capability suggestions
```

Toda atividade autônoma persistente deve possuir origem de autoridade explícita:

```text
initiating principal
authorization scope
reason
schedule / trigger
provenance
```

Exemplo:

```text
Task: verificar backups diariamente
Created by: user
Authorized scope: read backup status
Schedule: daily
```

---

## 27. Applications and Workstation

Atlas Workstation não deve ser apenas um chat maior.

> **É um espaço visual compartilhado onde usuário e agente trabalham sobre os mesmos objetos, aplicações e estado do mundo.**

Pode representar:

```text
article
map
media
code
file
chart
infrastructure
knowledge
memory
task
```

Aplicações Atlas devem compartilhar estado:

```text
human action
      ↕
Application State
      ↕
Atlas action
```

---

## 28. Clients

Clientes não devem conter a inteligência principal do Agent.

```text
Atlas Runtime
     ↓
structured events / API
     ↓
Clients
```

Clientes possíveis:

```text
TUI
Web
Desktop
Mobile
Voice
```

A interface apresenta estado e coleta interação.

O Runtime controla execução.

O Agent decide.

---

## 29. Local-first

Atlas é local-first, não necessariamente local-only.

Local-first significa:

```text
Local execution is a first-class path.
Local state remains authoritative where possible.
The system remains useful during partial cloud failure.
Cloud services are replaceable dependencies.
User data is not moved remotely unless required by a capability.
```

Cloud models e serviços podem existir, mas não devem definir a identidade arquitetural do produto.

---

## 30. Provider Independence and Agent Harness

Modelos e harnesses são infraestrutura substituível.

```text
Atlas Runtime API
        ↓
Agent SDK Adapter
        ↓
Agent SDK
        ↓
Model Provider
```

O harness pode fornecer primitivas como:

```text
agent loop
model calls
tool calling
sessions
handoffs
events
```

Mas Atlas deve possuir contratos próprios.

Tipos internos como `Agent`, `Runner`, `RunResult` ou `MemorySession` não devem definir a API pública central do Atlas.

Tipos conceituais próprios devem emergir, por exemplo:

```text
AtlasRuntime
AtlasConversation
AtlasTurn
AtlasEvent
AtlasResult
```

Princípio:

> **Models are replaceable. Harnesses are replaceable. Atlas is the product.**

---

## 31. Observable Work

O usuário deve conseguir ver o que Atlas está fazendo sem receber raciocínio privado.

Exemplos:

```text
Analyzing project
Reading config
Running tests
Waiting for confirmation
Created artifact
Task completed
```

Events e state devem ser suficientes para explicar o trabalho operacional.

---

## 32. Voice and Multimodal

Voice não é um produto separado.

É outro canal de interação, junto com:

```text
text
screen
camera
files
selection
pointing
```

Todos podem alimentar o mesmo Runtime, identidade, contexto e estado.

---

## 33. Architectural Invariants

Estes princípios devem ser tratados como invariantes do Atlas:

1. **Agent decides; Runtime executes.**
2. **Discovery creates awareness, never authority.**
3. **Availability, awareness, authorization and execution are independent dimensions.**
4. **Only necessary context is materialized for the model.**
5. **Tools use progressive schema materialization.**
6. **Skills use progressive procedural disclosure.**
7. **Memory is retrieved, scoped and provenance-aware, never dumped wholesale.**
8. **Conversation state, working context, long-term memory and knowledge are distinct.**
9. **Untrusted content cannot create authority.**
10. **Durable work belongs to Tasks; Workers are disposable executors.**
11. **Every consequential action should have provenance.**
12. **User-visible events expose operational work, not private reasoning.**
13. **Generated capabilities are not trusted merely because Atlas generated them.**
14. **Clients present and interact; they do not own the cognitive runtime.**
15. **Provider and Agent SDK implementations are replaceable.**
16. **Atlas owns its public contracts and persistent state.**
17. **Local execution and local state remain first-class.**
18. **Memory and Discovery provide information, not permission.**
19. **Execution should become deterministic whenever practical.**
20. **The cost of accessing capabilities, memory and knowledge should scale with current relevance, not total system size.**

---

## 34. Core Principles

```text
Maximum capability.
Minimum necessary context.

Discover broadly.
Materialize narrowly.
Execute deterministically.

Discovery is knowledge, not permission.
Memory is information, not authority.

Agent decides.
Runtime executes.

Tasks persist.
Workers execute.

Events expose work, not chain-of-thought.

Memory is retrieved, not dumped.

Content is data, not authority.

Clients present.
Runtime controls.
Agent reasons.

Local-first, not local-only.

Models are replaceable.
Harnesses are replaceable.
Atlas is the product.
```

---

## 35. Long-term Goal

Atlas deve evoluir de:

```text
assistant
```

para:

```text
resident intelligence
```

Um assistant espera comandos.

Uma resident intelligence compreende o ambiente, mantém estado, conhece suas capacidades, recupera contexto relevante, acompanha tarefas, opera de forma contínua e permanece sob autoridade do usuário.

No estado final, o usuário trabalha normalmente.

Atlas está presente.

Ele conhece o ambiente, entende o contexto, lembra o que importa, acessa conhecimento, controla aplicações e dispositivos, executa ações, delega tarefas, cria capabilities quando necessário e apresenta seu trabalho de forma observável.

O usuário intervém quando quiser.

**Esse é o Atlas.**

---

## 36. Current Implementation References

- Repository: https://github.com/ccmcorrea1-star/Atlas
- OpenAI Agents SDK: https://openai.github.io/openai-agents-js/
- OpenAI Agents SDK sessions: https://openai.github.io/openai-agents-js/guides/sessions/
- OpenAI Agents SDK AI SDK extension: https://openai.github.io/openai-agents-js/extensions/ai-sdk/
- OpenCode Go: https://opencode.ai/docs/pt-br/go/
