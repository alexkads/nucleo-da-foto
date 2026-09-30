# rawler 0.8.0 — vendorizado

Origem: <https://crates.io/crates/rawler/0.8.0> (projeto dnglab), licença
LGPL-2.1 (ver `LICENSE`). Trazido para cá em 2026-09-30 para o VintageLightbox
e o site abrirem RAW e o DNG do Lightroom — inclusive no navegador.

## O que mudou em relação ao original

1. `std::time::Instant` → `web_time::Instant` em cinco arquivos
   (`imgop/sensor/bayer/ppg.rs`, `imgop/sensor/xtrans/{bilinear,markesteijn}.rs`,
   `decompressors/crx/decoder.rs`, `dng/writer.rs`). O relógio só serve ao log;
   no `wasm32-unknown-unknown` o do `std` entra em pânico, e era o demosaico de
   todo NEF/CR2 que parava. No nativo, `web_time::Instant` **é** o do `std`.
2. `uuid` com a feature `js` quando o alvo é wasm32.
3. Fora do pacote: `data/testdata`, `tests/`, `benches/` e `src/bin/` (32 MB que
   só servem ao desenvolvimento do próprio rawler).

## O que **não** está aqui, e o VintageLightbox faz por fora

A `OpcodeList2` do DNG (o `MapPolynomial` que o DNG com perdas do Lightroom usa
para linearizar os 8 bits). O rawler 0.8.0 ignora os opcodes e devolve a foto
rosada; a linearização é aplicada em `crates/raw-codec`.

Para atualizar: copiar a versão nova do crates.io, reaplicar os itens 1–3 e
rodar `cargo test -p raw-codec`.
