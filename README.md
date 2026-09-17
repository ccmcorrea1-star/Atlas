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
cargo install --locked --path clients/tui
export OPENCODE_GO_API_KEY="sua-chave"
atlas server run
```

Para reiniciar ou desligar o servidor:

```bash
atlas server restart
atlas server stop
```

Em outro terminal:

```bash
cargo run --manifest-path clients/tui/Cargo.toml -- --conversation-id minha-conversa
```
