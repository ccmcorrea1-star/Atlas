# Paleta visual do Atlas

Fonte: `assets/brand/atlas-logo.svg` e `assets/brand/atlas-banner.svg`.

A identidade visual do Atlas usa uma base escura, neutra e fria, com contraste em branco/cinza. A paleta deve ser usada de forma econômica em interfaces de terminal e aplicações gráficas.

## Cores

| Nome             | Hex       | Uso                                        |
| ---------------- | --------- | ------------------------------------------ |
| Fundo profundo   | `#050607` | Fundo principal do banner                  |
| Fundo Atlas      | `#070809` | Fundo principal do logo                    |
| Fundo elevado    | `#0A0B0D` | Superfícies discretamente elevadas         |
| Fundo painel     | `#111316` | Painéis e áreas de conteúdo                |
| Fundo agente     | `#181B1F` | Background das mensagens do Atlas na TUI   |
| Fundo usuário    | `#34383D` | Background das mensagens do usuário na TUI |
| Grafite claro    | `#34383D` | Destaques, planeta e seleção               |
| Cinza orbital    | `#858B92` | Texto auxiliar e metadados                 |
| Cinza descritivo | `#8F969E` | Elementos gráficos secundários             |
| Cinza texto      | `#C4C8CD` | Texto secundário com boa leitura           |
| Cinza claro      | `#CBD1D7` | Linhas, bordas e realces suaves            |
| Branco Atlas     | `#FFFFFF` | Marca e texto de maior destaque            |

## Gradientes dos assets

- `bg` do logo: `#070809` → `#111316`.
- `bg` do banner: `#050607` → `#0A0B0D` → `#141619`.
- `planet`: `#34383D` → `#181B1F` → `#070809`.
- Texto principal (`white`/`mark`): `#FFFFFF` → `#D7DBE0`.

## Aplicação na TUI

A TUI usa fundos sólidos derivados da paleta para separar semanticamente as mensagens:

- **Usuário:** `#34383D`, o grafite de maior contraste da paleta.
- **Atlas:** `#181B1F`, um grafite escuro intermediário.
- **Superfícies:** `#0A0B0D` no estado normal e `#111316` no painel focado, sem bordas externas.
- **Seleção/foco:** realces ciano existentes podem continuar sendo usados apenas para interação e estado, não como identidade de fundo.
- **Erro:** vermelho continua reservado para falhas operacionais.

Os backgrounds devem preencher a largura útil da linha, manter contraste suficiente com o texto e permanecer discretos em terminais escuros.
