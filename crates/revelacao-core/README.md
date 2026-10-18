# revelacao-core

O motor de revelação: os 46 ajustes em WGSL executados na GPU (wgpu), o enquadramento e a perspectiva em CPU, o histograma e o JPEG — sem janela, sem banco, sem rede.

O mesmo motor roda no app desktop e no navegador (pelo `revelacao-web`), e a foto exportada é o que a tela mostra: mesmo `.wgsl`, mesmo enquadramento. Compila para `wasm32` (com a feature `webgl` para navegador sem WebGPU).

## Usar

```toml
[workspace.dependencies]
revelacao-core = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
use revelacao_core::{Ajustes, Motor};

let mut ajustes = Ajustes::default();
// ... preencher os controles e revelar com o Motor
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
