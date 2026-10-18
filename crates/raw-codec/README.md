# raw-codec

O RAW da câmera e o DNG (inclusive o DNG com perdas do Lightroom), em Rust puro — no desktop e no navegador — e o XMP da revelação embutida.

Devolve os dados do sensor revelados sem efeito (balanço da câmera, matriz de cor, sRGB): a base neutra. Nunca usa a prévia JPEG embutida, que já vem revelada. Sobre o `rawler` vendorizado em `vendor/rawler`.

## Usar

```toml
[workspace.dependencies]
raw-codec = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
if raw_codec::eh_raw("IMG_0001.CR3") {
    let pixels = raw_codec::decodificar(&bytes)?;
    let xmp = raw_codec::xmp(&bytes);
}
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
