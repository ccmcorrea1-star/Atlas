# Build

Este documento descreve como instalar, validar e gerar o build do Atlas localmente.

## Requisitos

- Node.js 22 ou superior
- npm
- CMake 3.20 ou superior e um compilador C++23 para as capabilities nativas

As dependências do projeto utilizam o lockfile `package-lock.json`. Use `npm ci` para obter uma instalação
reproduzível.

## Instalação

```bash
npm ci
```

## Validação local

Execute os comandos em sequência:

```bash
npm run format:check
npm run lint
npm run typecheck
npm test
npm run smoke
```

O smoke test usa um servidor HTTP local simulado. Ele não precisa de `OPENCODE_GO_API_KEY` nem faz requisições
para o OpenCode Go.

Não execute essas verificações em paralelo em máquinas com poucos recursos. O TypeScript, o ESLint e os testes
iniciam processos Node separados e podem consumir memória e swap simultaneamente.

## Build

```bash
npm run build
```

O TypeScript compila os arquivos de `src/` para `dist/`. O diretório `dist/` é artefato gerado e não deve ser
versionado.

## Build das capabilities nativas

```bash
cmake -S . -B .native-cmake
cmake --build .native-cmake
ctest --test-dir .native-cmake --output-on-failure
```

O build instala a implementação executável de `process.exec` junto do manifesto quando usado com `cmake --install`.

Para executar a saída compilada:

```bash
npm start
```

## Comandos disponíveis

| Comando                | Finalidade                                               |
| ---------------------- | -------------------------------------------------------- |
| `npm run dev`          | Executa o TypeScript em modo watch                       |
| `npm run format`       | Formata os arquivos com Prettier                         |
| `npm run format:check` | Verifica a formatação sem alterar arquivos               |
| `npm run lint`         | Executa o ESLint                                         |
| `npm run lint:fix`     | Corrige automaticamente problemas do ESLint              |
| `npm run typecheck`    | Verifica o código de produção e os testes com TypeScript |
| `npm test`             | Executa os testes automatizados em `tests/unit/`         |
| `npm run smoke`        | Executa o smoke test do provider e das conversas         |
| `npm run build`        | Gera os arquivos JavaScript em `dist/`                   |

## CI

O workflow `.github/workflows/quality.yml` executa `npm ci` e todos os checks de qualidade em um job do GitHub
Actions:

1. Formatação
2. Lint
3. Typecheck
4. Build
5. Testes automatizados
6. Smoke test

Uma mudança só deve ser considerada pronta quando todos esses comandos passarem localmente.
