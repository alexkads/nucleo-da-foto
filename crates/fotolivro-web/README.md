# fotolivro-web

O `fotolivro` no navegador do cliente: o mesmo livro do balcão, gerado na galeria dele (só as fotos que já são suas, limpas).

wasm-bindgen sobre o `fotolivro` e o `foto-codec`. Gerado por `scripts/construir-fotolivro.sh`.

## Exemplo

```js
import init, { Fotolivro } from "/fotolivro/fotolivro_web.js";
await init();
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
