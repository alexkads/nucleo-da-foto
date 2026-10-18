# preenchimento

Os motores do preenchimento sensível ao conteúdo do editor: PatchMatch (síntese por patches) e LaMa (rede neural local, ONNX), atrás de um contrato só.

Um motor recebe a região da foto e a máscara de destino e devolve o remendo. A feature `lama` (padrão) liga o modelo; sem ela, só o PatchMatch.

## Usar

```toml
[workspace.dependencies]
preenchimento = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

Todos os crates do núcleo na mesma tag — ver o README da raiz.

## Exemplo

```rust
use preenchimento::Motor;
// let remendo = motor.preencher(&regiao, &mascara_de_destino)?;
```

Parte do [nucleo-da-foto](../../README.md) — o motor de foto compartilhado pelo PhotoKads e pelo
VintageLightbox. Licença MIT.
