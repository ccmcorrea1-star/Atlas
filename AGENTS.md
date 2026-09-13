# AGENTS.md

## Projeto

- O Atlas é um agente de programação em TypeScript e ESM.
- Use o mesmo `conversationId` para preservar o contexto de uma conversa.
- Sempre comente o código novo ou alterado em `src/` de forma curta, clara e objetiva.
- Não implemente tools, tasks, MCP, discovery, memory avançada ou TUI sem solicitação explícita.

## Validação

Execute os checks em sequência antes de concluir uma alteração:

```bash
npm run format:check
npm run lint
npm run typecheck
npm run build
npm test
npm run smoke
```

Consulte `docs/build.md` e `docs/test.md` para detalhes.
