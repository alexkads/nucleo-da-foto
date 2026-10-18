# nucleo-da-foto

O motor de foto que o PhotoKads e o VintageLightbox (RecordarFotos) compartilham. Público, versionado
por semver com tags, consumido por git com tag.

**Antes de mudar qualquer coisa — ou de consumir de um app —, use a skill `nucleo-da-foto`**
(`.claude/skills/nucleo-da-foto/SKILL.md`): o que pode morar aqui, como consumir, como desenvolver junto
com um app, o que é mudança que quebra e como lançá-la sem derrubar nenhum dos quatro consumidores.

## Comandos

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p conversao-web -p fotolivro-web -p raw-web -p revelacao-web -p tela-do-cliente-web
cargo semver-checks --workspace --baseline-rev <tag anterior>
```

🚨 Tag publicada nunca se move nem se apaga: os balcões do RecordarFotos compilam o VintageLightbox do
GitHub, e uma tag que muda quebra a próxima compilação deles.
