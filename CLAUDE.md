# nucleo-da-foto

O motor de foto que o PhotoKads e o VintageLightbox (RecordarFotos) compartilham. Público, versionado
por semver com tags, consumido por git com tag.

**Antes de mudar qualquer coisa — ou de consumir de um app —, use a skill `nucleo-da-foto`**
(`.claude/skills/nucleo-da-foto/SKILL.md`): o que pode morar aqui, como consumir, como desenvolver junto
com um app, o que é mudança que quebra e como lançá-la sem derrubar nenhum dos quatro consumidores.

## Comandos

```sh
make verificar   # o portão: fmt, clippy nativo e wasm32, testes e doc-tests, doc -D warnings, cargo deny
make cobertura   # cobertura por arquivo, com piso de 90 % (sem o vendor)
make semver      # cada crate contra a última tag — 🚨 não use --workspace: ele pula os crates
                 # publish = false (todos) e sai verde sem conferir nada
```

🚨 Tag publicada nunca se move nem se apaga: os balcões do RecordarFotos compilam o VintageLightbox do
GitHub, e uma tag que muda quebra a próxima compilação deles.
