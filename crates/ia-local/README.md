# ia-local

A IA local: o catálogo e a instalação dos modelos ONNX (download explícito com hash conferido), o backend (CPU, CoreML, DirectML, CUDA) e a sessão do ONNX Runtime — sem saber para que o modelo serve.

Nada sai da máquina: a única conexão é o download do modelo, pedido pelo operador. A feature `onnx` (padrão) liga o ONNX Runtime; `cuda` é opcional e não testada.

## Usar

```toml
[workspace.dependencies]
ia-local = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
use ia_local::{execucao, modelos};
// let sessao = execucao::sessao(&modelo, backend)?;
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
