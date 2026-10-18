# raw-web

O `raw-codec` no navegador: abre o RAW da câmera e o DNG na importação do site, num wasm à parte que só desce quando a leva tem um RAW.

wasm-bindgen sobre o `raw-codec`. Gerado por `scripts/construir-raw.sh`.

## Exemplo

```js
import init, { eh_raw, decodificar_raw } from "/raw/raw_web.js";
await init();
if (eh_raw(nome)) { const rgb = decodificar_raw(new Uint8Array(bytes)); }
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
