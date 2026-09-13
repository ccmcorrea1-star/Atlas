# Atlas

- Projeto em TypeScript e ESM.
- Preserve o mesmo `conversationId` durante a conversa.
- Comente alterações em `src/` de forma curta, clara e objetiva.
- Não implemente tools, tasks, MCP, discovery, memory avançada ou TUI sem solicitação explícita.

## Estrutura

```text
.
|-- src/                         # produção em TypeScript
|   |-- index.ts                 # API pública
|   |-- atlas.ts                 # Agent, Runner e conversas
|   |-- opencode-go.ts           # provider OpenCode Go
|   |-- smoke.ts                 # smoke test local
|   `-- capabilities/            # capabilities nativas
|-- clients/                     # clientes oficiais desacoplados do Runtime
|   `-- tui/                     # cliente de terminal em Rust
|-- tests/                       # testes TypeScript e C++
|-- docs/                        # visão técnica, build e testes
|-- scripts/                     # automações auxiliares
|-- config/                      # configuração do ESLint
|-- assets/                      # identidade visual
|-- .github/                     # CI
|-- package.json                 # scripts e dependências npm
|-- tsconfig*.json               # configuração do TypeScript
|-- CMakeLists.txt               # build nativo
|-- dist/                        # saída gerada, não versionar
`-- .native-cmake/               # artefatos CMake, não versionar
```

Antes de concluir qualquer alteração, execute nesta ordem:

```bash
npm run format:check
npm run lint
npm run typecheck
npm run build
npm test
npm run smoke
```

Consulte `docs/build.md` e `docs/test.md` para detalhes.
