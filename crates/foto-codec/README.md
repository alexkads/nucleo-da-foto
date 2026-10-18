# foto-codec

O codificador de foto: JPEG e WebP, decodificar e codificar, espaço de cor e orientação EXIF — um codificador só para todo destino (desktop, navegador, servidor).

Uma foto revelada no desktop e a mesma foto convertida no navegador do cliente saem byte a byte do mesmo código. É por isso que o navegador não usa `canvas.toBlob` para o arquivo final, e por isso este crate não depende de GPU: vai ao wasm leve da galeria.

## Usar

```toml
[workspace.dependencies]
foto-codec = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
let imagem = foto_codec::decodificar(&bytes)?;
let jpeg = foto_codec::codificar(&imagem, 92)?;
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
