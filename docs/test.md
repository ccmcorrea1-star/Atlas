# Testes

O Atlas usa o runner nativo de testes do Node.js através do `tsx`. Os testes são escritos em
TypeScript e ficam separados do código de produção.

## Estrutura

```text
tests/
├── support/
│   └── open-code-go-test-server.ts
└── unit/
    ├── atlas-conversations.test.ts
    └── opencode-go-provider.test.ts
```

`tests/support/` contém infraestrutura compartilhada pelos testes. O servidor simulado responde no
formato da Responses API e captura headers e payloads enviados pelo provider.

## Executar testes

```bash
npm test
```

Os testes são locais e não precisam de `OPENCODE_GO_API_KEY`. As requisições são direcionadas para
um servidor HTTP temporário em `127.0.0.1`.

## Cenários cobertos

`atlas-conversations.test.ts` verifica:

1. Turns da mesma conversa usam o mesmo `x-opencode-session`.
2. O histórico do primeiro turn é enviado no segundo turn.
3. Conversas diferentes usam IDs de sessão diferentes e não compartilham histórico.
4. O mesmo `Runner` e o mesmo provider são reutilizados.

`opencode-go-provider.test.ts` verifica:

1. Um Runner criado diretamente usa a sessão configurada no provider.
2. IDs de sessão vazios são rejeitados.

## Smoke test

O smoke test é uma verificação de integração local mais ampla:

```bash
npm run smoke
```

Ele exercita o fluxo público de conversas, incluindo endpoint, autenticação, User-Agent, headers de
sessão, preservação de contexto e reutilização do Runner.

Os testes nativos em `tests/native/` validam o Registry, o Loader, o Discovery, o Executor e a execução direta de
`process.exec` pelo runtime executável compartilhado definido em seu manifesto. `npm test` compila esses testes com C++23.

## CI

O GitHub Actions executa `npm test` e `npm run smoke` depois de format, lint, typecheck e build.

Para reproduzir localmente o mesmo fluxo:

```bash
npm run format:check
npm run lint
npm run typecheck
npm run build
npm test
npm run smoke
```
