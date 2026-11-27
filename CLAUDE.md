# nucleo-da-foto

O motor de foto que o PhotoKads e o VintageLightbox (RecordarFotos) compartilham. Público, versionado
por semver com tags, consumido por git com tag.

**Antes de mudar qualquer coisa — ou de consumir de um app —, use a skill `nucleo-da-foto`**
(`.claude/skills/nucleo-da-foto/SKILL.md`): o que pode morar aqui, como consumir, como desenvolver junto
com um app, o que é mudança que quebra e como lançá-la sem derrubar nenhum dos quatro consumidores.

## Comandos

As checagens são subcomandos do `cargo xtask` (crate `xtask/`), um por finalidade e sem argumentos.
Nada de `Makefile` nem de `.sh` para checar: eles não rodam no Windows.

```sh
cargo xtask dev        # enquanto se trabalha: fmt, e clippy e testes só dos pacotes alterados
cargo xtask pr         # o gate do PR: clippy no que a mudança alcança, testes dos alterados e dos
                       # dependentes diretos, wasm32, doc e bancada quando alcançados
cargo xtask release    # antes de lançar: o workspace inteiro, cargo deny, MSRV e semver crate a crate
cargo xtask cobertura  # cobertura por arquivo, com piso de 90 % (sem o vendor)
```

- O `dev` e o `pr` comparam com `origin/main` (ou `CHECK_BASE`). Mudança no `Cargo.toml` da raiz, no
  `Cargo.lock`, no `rustfmt.toml`, no `deny.toml` ou no `.cargo/` seleciona o workspace inteiro.
- Os testes rodam no `cargo-nextest`, que é obrigatório (`cargo install cargo-nextest --locked`).
- 🚨 O semver vai crate a crate: com `--workspace`, o `cargo-semver-checks` pula os crates
  `publish = false` (todos) e sai verde sem conferir nada.
- O pesado (o `pr` e o `release`) roda na macpro, não no Mac do dono (skill `rust-checagens-xtask`).

🚨 Tag publicada nunca se move nem se apaga: os balcões do RecordarFotos compilam o VintageLightbox do
GitHub, e uma tag que muda quebra a próxima compilação deles.
