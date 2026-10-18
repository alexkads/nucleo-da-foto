# editor-core

O editor de fotos em camadas, sem janela: documento, tiles, pincel, histórico, composição, filtros, liquidificar, mesclagem e a gravação do projeto.

A base neutra entra pronta; o que sai é a imagem editada e o projeto no disco. A janela mora em cada app; aqui fica tudo o que se prova sem tela.

## Usar

```toml
[workspace.dependencies]
editor-core = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
use editor_core::documento;
// abrir um documento, aplicar operações, compor e gravar o projeto
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
