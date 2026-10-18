# revelacao-web

O `revelacao-core` no navegador: o motor de revelação sobre um canvas, com WebGPU (ou WebGL2).

Só existe no alvo `wasm32`; em nativo é vazio de propósito (o que se prova em nativo é o `revelacao-core`). Gerado por `scripts/construir-web.sh`.

## Exemplo

```js
import init from "/revelacao/revelacao_web.js";
await init();
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
