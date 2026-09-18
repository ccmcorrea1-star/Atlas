<p align="center">
  <img src="assets/brand/atlas-banner.png" alt="Atlas — inteligência residente para o seu ambiente digital" width="100%">
</p>

# Atlas

Atlas é uma inteligência pessoal residente e local-first criada para compreender e operar o ambiente digital do usuário.

A proposta é ir além de um chatbot ou copiloto: o Atlas busca funcionar como uma camada inteligente entre o usuário, seus computadores, aplicações, serviços, conhecimento e dispositivos.

O projeto prioriza contexto sob demanda, execução local, capacidades descobertas conforme a necessidade e uma experiência contínua entre diferentes interfaces.

## Estado

Atlas está em desenvolvimento ativo. A implementação atual já possui um Runtime local, execução de capabilities e um cliente de terminal.

A direção completa do projeto está descrita em [`docs/ATLAS_TECHNICAL_VISION.md`](docs/ATLAS_TECHNICAL_VISION.md).

## Executar

Requer Node.js 22+, CMake com suporte a C++23 e Rust/Cargo.

```bash
npm ci
cargo install --locked --path clients/tui
atlas server run
```

Em outro terminal, abra o cliente de terminal:

```bash
atlas
```

Para acompanhar, reiniciar ou desligar o servidor:

```bash
atlas server status
atlas server restart
atlas server stop
```

## Configuração

O Runtime lê uma configuração global única em JSON. O caminho é resolvido nesta
ordem: `ATLAS_CONFIG`, `$XDG_CONFIG_HOME/atlas/config.json`,
`~/.config/atlas/config.json`. Sem o arquivo, os defaults atuais são usados; o
arquivo nunca é criado automaticamente.

```json
{
  "version": 1,
  "provider": "opencode-go",
  "model": "gpt-5.6-luna",
  "providers": {
    "opencode-go": {
      "apiKey": "sua-chave"
    }
  }
}
```

`version`, `provider` e `model` são obrigatórios. A credencial pode ficar no
arquivo em `providers[`_provider_`].apiKey`; se `OPENCODE_GO_API_KEY` existir no
ambiente, ela sobrescreve o valor do arquivo. O contrato completo está em
[`protocol/config.schema.json`](protocol/config.schema.json).
