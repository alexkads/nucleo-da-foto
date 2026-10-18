# conversao-web

O `foto-codec` no navegador do cliente: converte a foto guardada (WebP) no JPEG que ele leva ao laboratório, sem GPU.

wasm-bindgen sobre o `foto-codec`. Gerado por `scripts/construir-conversao.sh`.

## Exemplo

```js
import init, { para_jpeg } from "/conversao/conversao_web.js";
await init();
const jpeg = para_jpeg(new Uint8Array(webp), 92);
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
