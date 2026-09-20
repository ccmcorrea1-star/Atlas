# Apps do Atlas

Os apps do Atlas são interfaces para a mesma inteligência e o mesmo Runtime.

Agent, memória, Tasks, Workers, Discovery e capabilities pertencem ao Runtime. Os apps apresentam essas capacidades conforme o contexto de uso e consomem contratos públicos definidos em [`protocol/`](../protocol/).

## Atlas Workstation

O **Atlas Workstation** é a principal interface visual do Atlas.

Ele funciona como um espaço de trabalho observável, controlado principalmente por voz. A interface reage ao contexto da conversa e ao trabalho executado pelo Atlas, em vez de depender de navegação tradicional por páginas e menus.

O Workstation pode apresentar conversas, Tasks, Workers, execuções, progresso, arquivos, artefatos, diffs, approvals e outras interações públicas do Runtime.

Voz é a forma principal de controle. Mouse e teclado complementam a interação quando seleção ou entrada precisa forem necessárias.

As animações devem representar mudanças reais de contexto e estado, como criação de Tasks, início de execuções, mudança de foco e conclusão de trabalho.

A primeira plataforma é Windows. A direção técnica atual é:

```text
C++23
C++/WinRT
Windows App SDK
WinUI 3
Microsoft.UI.Composition
Win2D
```

O Workstation é um cliente do Runtime e não contém a lógica do Agent.

## Atlas Code

O **Atlas Code** é o ambiente de desenvolvimento do ecossistema Atlas.

Ele ocupa o espaço de ferramentas como VS Code e Cursor, com integração direta ao contexto e às capacidades do Atlas.

Principais superfícies:

```text
Explorer
Editor
Terminal
Git
Diff
Problems
Agent
Tasks
```

O Atlas pode compreender o projeto, navegar código, alterar arquivos, executar ferramentas, consultar documentação e acompanhar trabalho sem depender de um painel de chat isolado.

Atlas Code deve ser um aplicativo desktop local. Uma versão web pode existir posteriormente como cliente complementar.

## Atlas Knowledge

O **Atlas Knowledge** é o espaço para criar, organizar, consultar e inspecionar conhecimento.

Ele combina notas, documentos e bases estruturadas com acesso ao conhecimento utilizado pelo próprio Atlas.

```text
Knowledge
├── Notes
├── Documents
├── Wiki
├── Databases
├── Memory
├── Sources
└── Graph
```

### Notes, Documents e Wiki

Permitem escrever e organizar notas, documentação, referências e páginas relacionadas.

### Databases

Permitem armazenar e visualizar informações estruturadas, como projetos, contatos, pesquisas, reuniões e outras coleções.

### Memory

Permite inspecionar a memória persistente utilizada pelo Atlas.

Memórias devem apresentar, quando disponível, conteúdo, origem, contexto, data, relações e provenance.

### Sources

Mostra as fontes que alimentam o conhecimento, como arquivos locais, repositórios, documentos, web, integrações e conteúdo criado pelo usuário.

### Graph

Apresenta relações entre entidades, documentos, projetos, pessoas, decisões e outras informações conhecidas pelo Atlas.

## Atlas Terminal

O **Atlas Terminal** é o cliente de terminal já existente em [`clients/tui/`](../clients/tui/).

O comando principal continua sendo:

```bash
atlas
```

Ele fornece uma interface rápida para conversar com o Atlas, acompanhar execuções e trabalhar diretamente a partir do terminal.

`TUI` descreve a implementação atual. **Atlas Terminal** é o nome do produto.

## Atlas Browser

O **Atlas Browser** é a superfície de navegação web integrada ao Atlas.

Seu objetivo é permitir que Atlas e usuário compartilhem o mesmo contexto de navegação, incluindo páginas abertas, pesquisas, conteúdo consultado e ações realizadas na web.

Ele utiliza o Atlas Runtime e suas capabilities, sem implementar um Agent separado.

## Atlas Torrent

O **Atlas Torrent** é o cliente BitTorrent do ecossistema Atlas.

Ele oferece as funções normais de um cliente torrent, incluindo downloads, uploads, fila, seleção de arquivos, peers, trackers, limites de velocidade e prioridades.

A integração com o Atlas permite controlar transferências por linguagem natural e conectá-las a outras capacidades do Runtime, como filesystem, automações e notificações. O usuário pode, por exemplo, alterar prioridades, definir limites temporários, acompanhar transferências ou organizar arquivos concluídos sem navegar manualmente pela interface.

A implementação deve reutilizar uma engine BitTorrent existente e manter protocolo, transferência e gerenciamento de peers fora do Agent. O Atlas Runtime orquestra ações e contexto; o cliente continua responsável pela experiência de gerenciamento das transferências.

## Atlas Media

O **Atlas Media** é o servidor e cliente de mídia do ecossistema Atlas.

Ele organiza bibliotecas de filmes, séries, músicas, fotos e outros conteúdos locais, mantendo histórico de reprodução, progresso, perfis, metadata, legendas e disponibilidade por dispositivo.

O Atlas Media deve permitir streaming para outros clientes e dispositivos, com direct play quando possível e transcoding quando necessário. A implementação pode reutilizar engines consolidadas para codecs e transcoding, mantendo essa responsabilidade fora do Agent.

A integração com o Atlas permite consultar e controlar a biblioteca por linguagem natural, continuar reproduções, organizar arquivos, encontrar conteúdo e automatizar ações relacionadas à mídia.

Atlas Torrent e Atlas Media podem trabalhar em conjunto por meio do Runtime: downloads concluídos podem ser organizados e adicionados à biblioteca quando solicitado ou configurado. Os dois apps permanecem desacoplados e podem funcionar independentemente.

A arquitetura deve separar a interface do serviço residente responsável por biblioteca, sessões, streaming e transcoding, permitindo que Workstation, Mobile, Web e futuros clientes de TV consumam o mesmo servidor de mídia.

## Atlas Music

O **Atlas Music** é o player e a experiência dedicada a música do ecossistema Atlas.

Ele apresenta artistas, álbuns, faixas, playlists, fila, letras, favoritos, histórico, recomendações e dispositivos de reprodução em uma interface própria para consumo musical.

Quando a origem for local, Atlas Music deve reutilizar biblioteca, metadata e streaming do Atlas Media em vez de manter uma base de mídia duplicada.

A integração com o Atlas permite controlar reprodução e fila por linguagem natural, criar playlists, consultar histórico, encontrar músicas e continuar a reprodução entre dispositivos.

A arquitetura deve permitir fontes adicionais no futuro, como serviços externos de música, sem acoplar o app a um único provider. Atlas Music continua responsável pela experiência de reprodução, enquanto Atlas Media permanece responsável pelo serviço de mídia local.

## Atlas Mobile

O **Atlas Mobile** é o companion móvel do Atlas.

Seu foco é interação por voz e texto, captura de fotos e arquivos, notificações, acompanhamento de Tasks, approvals e continuidade entre dispositivos.

Ele não precisa reproduzir toda a interface do Workstation.

## Atlas Telegram

O **Atlas Telegram** é o cliente remoto do Atlas para Telegram.

Ele deve oferecer uma experiência próxima à do Atlas nos demais clientes, usando mensagens, voz, imagens, documentos e controles interativos como interface para o mesmo Runtime.

A implementação fica em `clients/telegram/` e atua como adapter entre a Telegram Bot API e os contratos públicos do Atlas em [`protocol/runtime/v1/`](../protocol/runtime/v1/). Agent, sessões, Tasks, Workers, memória e execução continuam pertencendo ao Runtime.

O cliente deve suportar conversas persistentes, streaming por atualização de mensagens, anexos, notificações assíncronas, acompanhamento de Tasks e approvals. Comandos operacionais podem expor ações como iniciar uma nova conversa, consultar estado, cancelar uma execução e alternar sessões.

Controles que interrompem ou respondem a trabalho em andamento, como cancelamento e approvals, devem continuar disponíveis durante uma execução.

O acesso deve permitir restrição por usuário ou chat. Grupos e tópicos podem ser suportados quando o contexto de conversa puder ser associado de forma determinística a uma sessão do Atlas.

A criação e configuração da identidade do bot é feita pelo Telegram, enquanto o cliente continua sendo apenas uma interface do Atlas.

## Atlas Web

O **Atlas Web** é um cliente leve para acessar o Atlas pelo navegador.

Ele permite acesso remoto e continuidade quando um cliente local completo não estiver disponível.

Atlas Web não representa um Runtime diferente nem uma versão independente do Atlas. Ele continua consumindo os mesmos contratos públicos.

## Relação entre os apps

```text
                    Atlas Runtime
                         │
        ┌────────────────┼────────────────┐
        │                │                │
   Workstation          Code          Knowledge
        │                │                │
        ├──────── Terminal ───────────────┤
        │                │                │
      Mobile           Browser           Web
        │                │
     Telegram         Torrent
                           │
                         Media
                           │
                         Music
```

Os apps podem apresentar diferentes partes do mesmo estado, mas a fonte de verdade permanece no Runtime.

Uma conversa iniciada em um cliente pode continuar em outro quando o contrato e o estado persistente permitirem.

## Fronteiras

Apps são clientes do Atlas.

Tools, Skills, MCPs e integrações externas são capabilities ou fronteiras de integração, não apps.

```text
Atlas Knowledge     → app
Atlas Code          → app
Atlas Terminal      → app
Atlas Telegram      → app
Atlas Torrent       → app
Atlas Media         → app
Atlas Music         → app

GitHub              → integração
Google Drive        → integração
MCP                 → fronteira de integração
filesystem          → capability
shell               → capability
```

Clientes não devem duplicar regras do Runtime nem criar implementações próprias do Agent.

A arquitetura geral está em [`ARCHITECTURE.md`](./ARCHITECTURE.md) e a visão do produto em [`ATLAS_TECHNICAL_VISION.md`](./ATLAS_TECHNICAL_VISION.md).
