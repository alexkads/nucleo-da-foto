# Contribuir com o nucleo-da-foto

Antes de mudar qualquer coisa, leia a skill [`nucleo-da-foto`](.claude/skills/nucleo-da-foto/SKILL.md):
o que pode morar aqui, como consumir, o que é mudança que quebra e como lançar.

## O fluxo

1. `make verificar` verde antes de começar.
2. A mudança, com teste. Toda função pública nova tem doc (`///`) e, quando cabe, um exemplo que
   compila (doc-test).
3. `make verificar` verde de novo, e `make semver` contra a última tag.
4. Entrada no `CHANGELOG.md` (Quebra, Comportamento, Novo, Correção).
5. Lançar é subir a versão no `Cargo.toml`, tag e push — ver a skill.

## Os comandos

| Comando | O que faz |
|---|---|
| `make verificar` | fmt, clippy `-D warnings`, testes, wasm32 dos `*-web`, `cargo doc` sem aviso e `cargo deny` |
| `make testar` | só os testes |
| `make cobertura` | cobertura por crate (`cargo llvm-cov`), com relatório HTML em `target/cobertura` |
| `make semver` | a API pública comparada com a última tag (`cargo semver-checks`) |
| `make msrv` | compila com o Rust mínimo declarado (`rust-version`) |

## O estilo

- Português, como os dois produtos: nomes, docs e mensagens de erro.
- Commits contam o que foi **encontrado** e o que mudou.
- Nada de produto aqui (estúdio, plataforma, banco, rede, marca): ver a regra zero da skill.
