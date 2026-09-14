# Atlas

Antes de alterar código, leia:

- [`ARCHITECTURE.md`](./ARCHITECTURE.md)
- [`CODE_STYLE.md`](./CODE_STYLE.md)

Preserve o mesmo `conversationId` durante a conversa.

Não implemente tools, tasks, MCP, discovery, memory avançada ou TUI sem solicitação explícita.

Antes de concluir qualquer alteração, execute nesta ordem:

```bash
npm run format:check
npm run lint
npm run typecheck
npm run build
npm test
npm run smoke
```

Consulte [`docs/build.md`](./docs/build.md) e [`docs/test.md`](./docs/test.md) para detalhes.