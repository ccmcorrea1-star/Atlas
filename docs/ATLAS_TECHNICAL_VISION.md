Atlas — Visão Técnica e Arquitetura

Versão 0.4 — Setembro de 2026

Este documento é a fonte de verdade para a visão, os princípios e as fronteiras arquiteturais do Atlas.

1. Resumo

Atlas é uma plataforma de inteligência pessoal residente e local-first, capaz de compreender, operar e evoluir o ambiente digital do usuário.

Atlas não é apenas um chatbot ou copiloto. Ele é uma camada operacional persistente entre o usuário, seus computadores, aplicações, serviços, conhecimento e dispositivos.

Atlas é uma inteligência residente para o ambiente digital do usuário.

O objetivo é evoluir de um agente reativo para uma inteligência residente: continuamente disponível, capaz de recuperar o contexto necessário, descobrir capacidades, agir por meio de tools, manter estado e delegar trabalho somente quando isso trouxer benefício.

2. Princípios centrais

Máxima capacidade, mínimo contexto necessário

Atlas pode ter acesso a muitas capacidades sem carregar todos os detalhes no contexto do modelo.

O custo de contexto deve crescer com a relevância para a tarefa atual, não com o tamanho total da plataforma.

Ação direta quando suficiente; delegação quando útil

O Agent interpreta objetivos, planeja, seleciona capacidades, chama tools e avalia resultados.

O caminho normal deve ser o mais simples possível:

Agent
  ↓ tool
Runtime
  ↓
Ambiente

Delegação não é obrigatória. O Agent pode criar uma Task e usar um Worker quando o trabalho for longo, paralelo, persistente, especializado ou quando for útil desacoplar a execução do turno principal.

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

Uma chamada direta de tool deve ser preferida quando resolver o objetivo de forma simples e eficiente.

Discovery cria conhecimento, não permissão

Discovery torna recursos conhecidos pelo modelo. Autorização e execução são dimensões separadas.

Divulgação progressiva

Atlas revela detalhes conforme necessário:

resumo
  ↓
candidatos
  ↓
materialização completa
  ↓
uso

Esse padrão se aplica a tools, skills, memória e conhecimento.

3. Disponibilidade e conhecimento

Uma capacidade pode existir sem estar materializada no contexto do modelo.

AVAILABLE / UNAVAILABLE
KNOWN / UNKNOWN

AVAILABLE indica que a capacidade existe e está operacional.

KNOWN indica que o modelo recebeu informação suficiente para saber que ela existe e para que serve.

Discovery transforma principalmente:

UNKNOWN → KNOWN

4. Recursos descobríveis

Nem tudo que pode ser descoberto é uma capacidade executável.

Discoverable Resource
├── Capability
│   ├── Tool
│   ├── Skill
│   └── MCP Capability
├── Memory
├── Knowledge
└── Resource

Discovery pesquisa recursos descobríveis. O Capability Registry administra capacidades executáveis e procedurais.

5. Discovery

Discovery deve selecionar o menor conjunto suficiente de recursos para a tarefa atual.

1. catálogo compacto
2. metadados dos candidatos
3. materialização completa

Uma capacidade pode permanecer KNOWN enquanto continuar relevante. Compaction pode remover detalhes materializados sem tornar a capacidade indisponível.

6. Tools

Tools representam ações executáveis e estruturadas.

O Agent não recebe todos os schemas antecipadamente.

Primeiro recebe um catálogo compacto:

files — ler, escrever e buscar arquivos
git — operar repositórios Git
docker — inspecionar e controlar containers

Quando necessário, recebe candidatas:

docker.list — lista containers
docker.inspect — inspeciona container
docker.logs — lê logs

Somente então recebe o schema completo da tool escolhida.

Descobrir de forma ampla, materializar de forma estreita e executar de forma determinística.

Depois que uma tool é escolhida, sua execução deve ser direta e determinística sempre que possível.

7. Skills

Skills representam procedimentos reutilizáveis para combinar capacidades.

Tools definem o que Atlas pode fazer. Skills definem como Atlas pode combinar essas capacidades para atingir um objetivo.

A divulgação também é progressiva:

1. resumo da skill
2. metadados: objetivo, quando usar, pré-condições e capacidades necessárias
3. procedimento completo

Uma skill pode declarar as tools que normalmente utiliza. Ao materializar a skill, Atlas pode materializar também os schemas dessas tools.

A skill não precisa se transformar em uma tool monolítica. O Agent segue o procedimento e continua chamando as tools adequadas.

8. Contexto, conversa, memória e conhecimento

Esses conceitos são distintos.

Estado da conversa é o histórico e o estado operacional da conversa atual.

Contexto de trabalho é a informação atualmente materializada para o modelo. É transitório e limitado por relevância e orçamento de contexto.

Memória de longo prazo contém fatos persistentes e recuperáveis sobre o usuário, o ambiente, preferências, sistemas, projetos e decisões anteriores.

Conhecimento vem de documentos, web, código, documentação, índices e sistemas externos.

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

Nada entra no contexto do modelo apenas porque existe.

9. Memória

A memória é recuperada sob demanda; não é despejada integralmente no prompt.

1. contexto basal / resumo de perfil
2. memórias candidatas
3. registros completos relevantes

O custo de acesso à memória deve crescer com a relevância para a tarefa, não com o volume total armazenado.

Cada memória deve possuir escopo, origem, atualidade, confiança, importância e provenance suficientes para permitir recuperação e atualização corretas.

Memórias podem ter origem explícita, observada ou inferida; essas origens não devem ter a mesma confiança.

Atlas não deve persistir automaticamente tudo que observa.

nova informação
    ↓
candidata a memória
    ↓
deduplicação / conflito
    ↓
avaliação de importância
    ↓
armazenar / atualizar / descartar

Quando uma informação nova contradiz uma memória existente, o sistema deve atualizar, versionar, invalidar ou representar explicitamente o conflito.

10. Capability Registry e Capability Factory

Toda capacidade conhecida pela plataforma deve entrar em um Registry unificado e independente da linguagem de implementação.

Atlas também pode criar novas capacidades quando uma operação recorrente se beneficia de uma implementação reutilizável e determinística.

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

Preferência:

código determinístico
>
tool estruturada
>
skill
>
improvisação do LLM

Capacidades geradas precisam de identidade, versão, origem, testes, dependências e provenance.

11. Tasks e Workers

Tasks existem para trabalho que se beneficia de execução desacoplada do turno principal. Elas não são o caminho obrigatório para toda ação.

Agent → tool → Runtime → resultado

Quando houver benefício:

Agent → Task → Worker → tools → Runtime

Uma Task representa estado persistente de trabalho. Um Worker é um executor temporário.

Invariantes:

Tasks persistem;

Workers podem ser descartados e recriados;

cancelamento é explícito;

efeitos relevantes devem ser identificáveis;

retries não podem duplicar silenciosamente efeitos;

recovery deve ser possível quando a operação permitir.

12. Eventos, provenance e estado

O Runtime deve produzir eventos estruturados para tornar o trabalho observável.

turn.started
tool.started
tool.completed
tool.failed
context.discovered
task.started
task.progress
task.completed

Eventos expõem atividade operacional, não chain-of-thought.

Atlas deve conseguir reconstruir a origem de ações relevantes:

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

O estado persistente do Atlas não deve depender estruturalmente de tipos internos de um provider ou Agent SDK específico.

13. MCP, dispositivos e clientes

MCP é uma fronteira de integração. Capacidades vindas por MCP continuam sujeitas a Discovery e observabilidade.

O servidor pode atuar como cérebro enquanto desktop, telefone, tablet e outros dispositivos tornam-se extensões operacionais.

Identidade do usuário e origem da interação são conceitos distintos.

Clientes não devem conter a inteligência principal do Agent.

Atlas Runtime
     ↓
eventos estruturados / API
     ↓
Clientes

A interface apresenta estado e coleta interação. O Runtime mantém execução e estado operacional. O Agent conduz o trabalho cognitivo.

14. Local-first e independência de infraestrutura

Atlas é local-first, não necessariamente local-only.

Execução local e estado local são caminhos de primeira classe. Serviços cloud podem existir, mas devem ser dependências substituíveis.

Modelos, providers e harnesses também são substituíveis.

Atlas Runtime API
        ↓
Agent SDK Adapter
        ↓
Agent SDK
        ↓
Model Provider

Atlas deve possuir contratos próprios. Tipos internos de um SDK não devem definir a API pública central do produto.

15. Workstation e trabalho observável

Atlas Workstation não deve ser apenas um chat maior. É um espaço visual compartilhado onde usuário e Agent trabalham sobre os mesmos objetos, aplicações e estado do mundo.

O usuário deve conseguir acompanhar o trabalho sem receber raciocínio privado.

16. Invariantes arquiteturais

O Agent age diretamente por meio de tools quando isso é suficiente.

Delegação para Tasks e Workers é opcional e usada quando houver benefício operacional.

Discovery cria conhecimento sobre recursos; não cria autoridade.

Somente o contexto necessário é materializado para o modelo.

Tools usam materialização progressiva de schema.

Skills usam divulgação progressiva de procedimento.

Memória é recuperada por relevância; nunca despejada integralmente no contexto.

Estado da conversa, contexto de trabalho, memória de longo prazo e conhecimento são conceitos distintos.

Trabalho persistente pertence a Tasks; Workers são executores temporários.

Toda ação relevante deve possuir provenance suficiente para ser rastreada.

Eventos visíveis expõem trabalho operacional, não raciocínio privado.

Capacidades geradas não são confiáveis apenas porque Atlas as gerou.

Clientes não possuem o runtime cognitivo principal.

Providers e Agent SDKs são substituíveis.

Atlas possui seus contratos públicos e seu estado persistente.

Execução e estado locais permanecem de primeira classe.

O custo de acessar tools, skills, memória e conhecimento cresce com a relevância atual, não com o tamanho total do sistema.

Depois da seleção de uma tool, a execução deve ser determinística sempre que possível.

Delegação nunca deve ser introduzida apenas por arquitetura quando a execução direta é suficiente.

17. Objetivo de longo prazo

Atlas deve evoluir de assistant para resident intelligence.

Uma inteligência residente compreende o ambiente, mantém estado, conhece suas capacidades, recupera contexto relevante, acompanha trabalho em andamento, age diretamente quando possível e delega quando necessário.

No estado final, o usuário trabalha normalmente enquanto Atlas permanece presente, operando o ambiente de forma contínua e observável.

Esse é o Atlas.

18. Referências da implementação atual

Repositório: https://github.com/ccmcorrea1-star/Atlas

OpenAI Agents SDK: https://openai.github.io/openai-agents-js/

Sessões do OpenAI Agents SDK: https://openai.github.io/openai-agents-js/guides/sessions/

Extensão AI SDK: https://openai.github.io/openai-agents-js/extensions/ai-sdk/

OpenCode Go: https://opencode.ai/docs/pt-br/go/