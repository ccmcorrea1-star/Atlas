# Testes

Os testes ficam em [`tests/`](../tests/) e não dependem de serviços externos.

## Executar

```bash
npm test
```

`npm test` executa os testes TypeScript e nativos. O provider usa um servidor HTTP local de teste, sem `OPENCODE_GO_API_KEY`.

## Smoke test

```bash
npm run smoke
```

Valida o fluxo público de conversa, sessão, contexto e provider.

## Cliente TUI

```bash
npm run test:client
```

Para o fluxo completo de validação, consulte [`BUILD.md`](./BUILD.md).
