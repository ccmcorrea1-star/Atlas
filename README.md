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
npm run sync:client
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
`config/atlas/config.json` **dentro do projeto**. Sem o arquivo, os defaults
atuais são usados; o arquivo nunca é criado automaticamente e `config/atlas/`
está fora do controle de versão.

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

Os adapters web também ficam nessa configuração global. O default local é
SearXNG para `web.search`, o extractor nativo para `web.fetch`, Camoufox +
Playwright para `web.browser` e Crawl4AI é opcional para `web.crawl`:

```json
{
  "web": {
    "search": {
      "provider": "searxng",
      "endpoint": "http://searxng.home",
      "fallbackProviders": []
    },
    "fetch": { "extractor": "native" },
    "browser": {
      "provider": "camoufox",
      "executablePath": "/opt/camoufox/camoufox",
      "headless": true
    },
    "crawl": {
      "provider": "crawl4ai",
      "endpoint": "http://127.0.0.1:11235/crawl"
    }
  }
}
```

Use a menor capability suficiente na ordem `web.search → web.fetch →
web.browser → web.crawl`. Browser Use não é o backend principal do browser; é
uma integração opcional para Workers/Tasks longos.
