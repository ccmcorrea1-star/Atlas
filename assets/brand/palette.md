# Paleta visual do Atlas

Fonte: [Vint-HS, por michdev](https://lospec.com/palette-list/vint-hs).

A identidade visual do Atlas usa uma seleção reduzida da Vint-HS. A base deve permanecer escura, com texto claro e um único acento azul/índigo. O efeito retrô vem do tratamento CRT/VHS, não de usar muitas cores.

## Paleta principal

| Nome | Hex | Uso |
| --- | --- | --- |
| Fundo | `#141414` | Fundo principal |
| Superfície | `#202125` | Composer, cards e painéis |
| Texto | `#E9F6E1` | Respostas, títulos e conteúdo principal |
| Secundário | `#827D7D` | Thought, footer, duração e metadados |
| Azul Atlas | `#3C53CE` | Foco, links, tools e identidade |
| Azul ativo | `#797DDE` | Thinking, cursor e estados ativos |
| Erro | `#BE173B` | Falhas e estados destrutivos |

## Aplicação

A interface deve ser majoritariamente fundo, superfície e texto. O Azul Atlas aparece apenas onde existe ação, foco ou identidade.

Na TUI e nos apps:

- mensagem e resposta: `#E9F6E1`
- prompt `›`, links e tools: `#3C53CE`
- Thinking ativo: `#797DDE`
- Thought concluído, footer e metadados: `#827D7D`
- composer e superfícies: `#202125`
- erros: `#BE173B`

Evite criar cores específicas para cada tool ou estado. O símbolo e o texto devem comunicar o estado sem transformar a interface em uma paleta multicolorida.

Scanlines, glow, ghosting e aberração cromática podem usar cores adicionais da Vint-HS apenas como efeito visual. Essas cores não fazem parte da hierarquia funcional da interface.
