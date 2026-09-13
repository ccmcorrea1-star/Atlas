# Atlas

Agente de programação baseado no OpenAI Agents SDK e no OpenCode Go.

## Requisitos

- Node.js 22 ou superior
- Uma chave do OpenCode Go

## Instalação

```bash
npm ci
```

Configure a chave no ambiente:

```bash
export OPENCODE_GO_API_KEY="sua-chave"
```

## Uso

```ts
import { runAtlas } from './dist/index.js';

const result = await runAtlas('Explique este projeto.', {
  conversationId: 'conversation-123',
});

console.log(result.finalOutput);
```

Use o mesmo `conversationId` para preservar o contexto entre turns. Um ID diferente cria uma
conversa independente.

## Comandos

```bash
npm run dev
npm run build
npm test
npm run smoke
```

Para executar todos os checks de qualidade:

```bash
npm run format:check
npm run lint
npm run typecheck
npm test
npm run smoke
```

Consulte [`docs/build.md`](docs/build.md) para o fluxo completo de build, validação e CI.
