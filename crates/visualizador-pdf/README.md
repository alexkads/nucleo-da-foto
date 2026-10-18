# visualizador-pdf

As páginas de um PDF desenhadas em pixels, sem tela e sem GPU, com os links de cada página e uma fila que só guarda o trabalho que ainda interessa.

Para o app mostrar o fotolivro que acabou de gerar sem abrir outro programa. O desenho é do `hayro` (Rust puro, CPU); os tetos e a fila vêm do PdfCraft (MIT OU Apache-2.0).

## Usar

```toml
[workspace.dependencies]
visualizador-pdf = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
use visualizador_pdf::Documento;
// let doc = Documento::abrir(bytes)?; desenhar a folha na escala pedida
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
