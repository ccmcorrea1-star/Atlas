# Atlas

Antes de alterar código, leia:

- [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md)
- [`docs/CODE_STYLE.md`](./docs/CODE_STYLE.md)

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
npm run sync:client
```

Consulte [`docs/BUILD.md`](./docs/BUILD.md) e [`docs/TEST.md`](./docs/TEST.md) para detalhes.
