# tela-do-cliente-web

A tela do cliente do balcão em wgpu: a foto revelada sobre preto no segundo monitor, e o cruzamento entre uma foto e a seguinte.

wasm-bindgen sobre o `revelacao-core`. Gerado por `scripts/construir-tela-do-cliente.sh`.

## Exemplo

```js
import init, { abrir } from "/tela-do-cliente/tela_do_cliente_web.js";
await init();
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
