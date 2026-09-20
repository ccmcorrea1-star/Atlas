# Testes

Os testes ficam em [`tests/`](../tests/) e não dependem de serviços externos.

## Executar

```bash
npm run test:runtime
npm run test:native
npm run test:lsp
npm run test:web-fetch
npm run test:web-search
npm run test:web-browser
npm run test:web-crawl
```

`npm test` executa os mesmos testes agrupados acima. O provider usa um servidor
HTTP local de teste, sem `OPENCODE_GO_API_KEY`.

Cada grupo pode ser executado após alterar somente a área correspondente. Os
builds nativos reutilizam `.native-cmake/` e os diretórios `target/` do Cargo;
um teste TypeScript do Runtime só constrói o bridge e o shell necessários.

## Smoke test

```bash
npm run smoke
```

Valida o fluxo público de conversa, sessão, contexto e provider.

## Cliente TUI

```bash
npm run test:tui
```

Para lint, build e teste da TUI em um único comando:

```bash
npm run check:tui
```

Para o fluxo completo de validação, consulte [`BUILD.md`](./BUILD.md).
