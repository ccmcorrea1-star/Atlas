# Paleta visual do Atlas

Fonte: `assets/brand/atlas-logo.svg` e `assets/brand/atlas-banner.svg`.

A identidade visual do Atlas usa uma base escura e fria, off-white e acentos discretos em ciano e azul-violeta. A estética é minimalista e retrofuturista; efeitos CRT/VHS pertencem às aplicações da marca, não à geometria do símbolo.

## Cores

| Nome             | Hex       | Uso                                        |
| ---------------- | --------- | ------------------------------------------ |
| Fundo profundo   | `#050607` | Fundo principal do banner                  |
| Fundo Atlas      | `#090C10` | Fundo principal do logo                    |
| Fundo elevado    | `#0A0B0D` | Superfícies discretamente elevadas         |
| Fundo painel     | `#111316` | Painéis e áreas de conteúdo                |
| Fundo agente     | `#181B1F` | Background das mensagens do Atlas na TUI   |
| Fundo usuário    | `#34383D` | Background das mensagens do usuário na TUI |
| Grafite claro    | `#34383D` | Destaques, planeta e seleção               |
| Cinza orbital    | `#858B92` | Texto auxiliar e metadados                 |
| Cinza descritivo | `#8F969E` | Elementos gráficos secundários             |
| Cinza texto      | `#C4C8CD` | Texto secundário com boa leitura           |
| Off-white Atlas  | `#F1EEE3` | Wordmark e realces principais              |
| Ciano orbital    | `#78D9E6` | Órbita e acentos tecnológicos              |
| Azul-violeta     | `#7775C9` | Aberração cromática e detalhe secundário   |

## Gradientes dos assets

- Fundo do logo: `#090C10`.
- `bg` do banner: `#050607` → `#0A0B0D` → `#141619`.
- `planet`: `#F1EEE3` → `#4D8998` → `#0B1117`.
- `orbit`: `#78D9E6` → `#728BD8`.
- Wordmark: `#F1EEE3`, com aberração cromática discreta em ciano e azul-violeta.

## Aplicação na TUI

A TUI usa fundos sólidos derivados da paleta para separar semanticamente as mensagens:

- **Usuário:** `#34383D`, o grafite de maior contraste da paleta.
- **Atlas:** `#181B1F`, um grafite escuro intermediário.
- **Superfícies:** `#0A0B0D` no estado normal e `#111316` no painel focado, sem bordas externas.
- **Seleção/foco:** ciano pode ser usado de forma econômica em interação, estado e detalhes alinhados à identidade orbital.
- **Erro:** vermelho continua reservado para falhas operacionais.

Os backgrounds devem preencher a largura útil da linha, manter contraste suficiente com o texto e permanecer discretos em terminais escuros.
