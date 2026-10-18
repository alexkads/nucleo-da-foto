# fotolivro

O fotolivro virtual do ensaio em PDF: diagramado, com moldura de álbum antigo em cada foto, navegável e tocável — o mesmo livro no balcão, na galeria do cliente e no servidor.

Sem tela e sem GPU. A foto levada sai limpa; a disponível, com a marca d'água do sistema. A ordem do livro: capa, uma foto por página em fundo preto, a galeria, "leve o seu ensaio" e o encerramento.

## Usar

```toml
[workspace.dependencies]
fotolivro = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
use fotolivro::{Plano, FotoDaFolha};
// montar o Plano com a capa e as fotos e gerar o PDF
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
