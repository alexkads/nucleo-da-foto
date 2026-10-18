# recuperacao-core

Recupera fotos de um cartão formatado: varre os setores atrás de JPEG e RAW e diz onde cada arquivo começa e termina — sem nunca gravar no cartão.

A formatação rápida grava uma tabela de arquivos nova e não toca nos dados; as fotos continuam nos setores até alguém gravar por cima. Este crate faz a varredura e a cópia para fora do cartão; ler o dispositivo de verdade é de quem chama.

## Usar

```toml
[workspace.dependencies]
recuperacao-core = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
use recuperacao_core::{varrer, Leitor};
// let resumo = varrer(&mut leitor, |andamento| { ... })?;
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
