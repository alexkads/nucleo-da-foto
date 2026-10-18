# nucleo-da-foto

O motor de foto que o **VintageLightbox** (RecordarFotos) e o **PhotoKads** compartilham: revelação em
GPU, RAW de câmera, codificador, editor, recuperação, fotolivro, visualizador de PDF, IA local e os
wrappers wasm que os sites carregam.

Nada aqui sabe de estúdio, de plataforma, de banco ou de rede. O que é de um produto — o app, a API, a
marca, o atualizador — fica no repositório dele.

| Crate | O que é |
|---|---|
| `foto-codec` | O codificador de foto (JPEG/WebP), sem GPU — também vai para o navegador |
| `raw-codec` | O RAW da câmera e o DNG (inclusive com perdas), em Rust puro, sobre o `rawler` vendorizado |
| `revelacao-core` | O motor de revelação: os ajustes em WGSL, o enquadramento e o JPEG |
| `editor-core` | O editor em camadas |
| `recuperacao-core` | A recuperação de fotos apagadas do cartão |
| `fotolivro` | O fotolivro em PDF — o mesmo do balcão, da galeria do cliente e do backend |
| `visualizador-pdf` | As páginas de um PDF em pixels |
| `ia-local` | O runtime e os modelos de IA local (ONNX) |
| `preenchimento` | O preenchimento por IA (LaMa) |
| `*-web` | `conversao-web`, `fotolivro-web`, `raw-web`, `revelacao-web`, `tela-do-cliente-web`: os mesmos motores para o navegador (wasm-bindgen) |

Nasceu em 18/out/2026 do VintageLightbox (`crates/` e `vendor/rawler`), com o histórico de cada arquivo
preservado (`git filter-repo`).

## Consumir

Por **git com tag**, declarado uma vez no workspace de quem consome:

```toml
[workspace.dependencies]
revelacao-core = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
fotolivro      = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

e, em cada crate, `revelacao-core.workspace = true`. Todos os crates de um consumidor apontam para a
**mesma tag** — duas tags no mesmo grafo são dois `foto-codec` diferentes, e os tipos não conversam.

Subir de versão é trocar a tag, compilar e rodar os testes de quem consome.

## Desenvolver junto com o app

Para mexer no núcleo e ver no app sem lançar tag, um `[patch]` **local e fora do git** no app — um
`.cargo/config.toml` ao lado do `Cargo.toml` dele, que o `.gitignore` do app ignora:

```toml
# ~/Projects/PhotoKads/photokads-desktop/.cargo/config.toml
[patch."https://github.com/alexkads/nucleo-da-foto"]
foto-codec     = { path = "../nucleo-da-foto/crates/foto-codec" }
revelacao-core = { path = "../nucleo-da-foto/crates/revelacao-core" }
# ... os que o app usa
```

O caminho é relativo à pasta do app. Este repositório mora em `~/Projects/PhotoKads/nucleo-da-foto`
(decisão do dono, 18/out/2026: os crates comuns ficam num repositório separado **dentro** da pasta do
PhotoKads), então:

| App | Caminho até o núcleo |
|---|---|
| `~/Projects/PhotoKads/photokads-desktop` | `../nucleo-da-foto` |
| `~/Projects/PhotoKads/photokads-plataforma/backend` | `../../nucleo-da-foto` |
| `~/Projects/RecordarFotos/VintageLightbox-Rust` | `../../PhotoKads/nucleo-da-foto` |

Pronta a mudança, lança-se a tag aqui e o app troca a tag no `Cargo.toml` — e o `[patch]` local sai.

## Lançar uma versão

1. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace` e `cargo check --target wasm32-unknown-unknown -p conversao-web -p fotolivro-web -p raw-web -p revelacao-web -p tela-do-cliente-web`.
2. Subir `version` em `[workspace.package]` e escrever a entrada no `CHANGELOG.md`.
3. Commit, `git tag vX.Y.Z`, `git push origin main vX.Y.Z`.

**Semver.** Enquanto for `0.x`, mudança que quebra quem consome sobe o menor (`0.1` → `0.2`); correção e
coisa nova compatível sobem o patch. Tag publicada não se move nem se apaga: quem já a usa continua
compilando.

## Os wasm dos sites

`scripts/construir-*.sh` geram os `.wasm` e o glue do wasm-bindgen direto na pasta do frontend que
receber como argumento (`scripts/construir-fotolivro.sh ../photokads-plataforma/frontend`).

## Licença

MIT — ver `LICENSE`.
