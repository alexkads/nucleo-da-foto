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

## 2026-10-18 — o identificador da licença

`license = "LGPL-2.1"` → `"LGPL-2.1-only"` no `Cargo.toml`: o primeiro é um identificador SPDX
obsoleto, e o `cargo deny` não o lê. A licença é a mesma (`LICENSE`); só o nome mudou.

## 2026-10-18 — arquivo estranho vira erro, não pânico

Cinco `todo!()`/`unimplemented!()` no caminho da decodificação viraram
`Err(RawlerError::DecoderFailed(..))`, cada um marcado com `// nucleo-da-foto:`:

- `decoders/dng.rs`: DNG com uma `PhotometricInterpretation` fora de 1, 32803 e 34892;
- `decoders/mod.rs`, `plain_image_from_ifd`: `Row/ColumnInterleaveFactor` fora de 1×1 e 2×2;
- `decoders/mod.rs`, `dynamic_image_from_ifd`: amostras por pixel ou bits por amostra fora do
  previsto (três braços).

O `raw-codec` chama o `raw_image` do decodificador direto, sem o `catch_unwind` do
`RawLoader::decode` — e no wasm (`panic = "abort"`) nem ele salvaria: um DNG com a fotometria
errada derrubava o módulo do site. Achado pelo teste
`dng_com_fotometria_que_o_rawler_nao_conhece_da_erro_e_nao_panico` do `raw-codec`.

Na mesma leva, o teste que troca cada byte do cabeçalho de um DNG sintético
(`dng_com_um_byte_trocado_em_qualquer_ponto_da_erro_e_nao_panico`) achou mais 30 pânicos em
~900 arquivos tortos, em quatro lugares:

- `decoders/dng.rs`: largura ou altura 0 (`par_chunks_mut(0)`), mais de 32 bits por amostra
  (`WhiteLevel::new_bits`) — o cabeçalho agora é conferido antes (largura e altura > 0, 1 a 4
  amostras por pixel, 1 a 32 bits) e o inválido vira erro;
- `imgop/develop.rs`: `ColorMatrix` sem D65 e sem 9 valores (`unimplemented!`), e o `assert_eq!`
  do tamanho da matriz — viraram erro;
- `pumps.rs`: o `refill` com o buffer esgotado era `panic!` (nas três bombas) — além do fim agora
  vêm zeros; e a `BitPumpMSB::peek_bits` completa com zeros quando o último refill lento traz
  menos bits que o pedido (o `nbits - num` estourava);
- 🐛 `decompressors/packed.rs`, `unpack_generic_msb`: **defeito do rawler, não só de arquivo
  torto.** Cada linha é decodificada em paralelo e pula os `bias` bits do começo com
  `consume_bits` na bomba ainda vazia: o `nbits -= bias` estoura. Acontece em **todo** DNG
  descomprimido de 9 a 15 bits (fora 10 e 12, que têm decodificador próprio) cuja linha não
  ocupa bytes inteiros — pânico no debug, e no release o contador dá a volta. Agora é
  `get_bits(bias)`. Teste: `decodifica_um_dng_de_14_bits_com_linhas_que_nao_comecam_num_byte`
  (falha sem a correção).

## 2026-10-28 — revelar tomando o RawImage, sem copiar o sensor

`imgop/develop.rs`: `RawDevelop::develop_intermediate_dono(RawImage)` ao lado do
`develop_intermediate(&RawImage)`, que passa a chamá-lo com um `clone()`. O original clonava o
`RawImage` inteiro no começo (os 24–42 MP do sensor em `u16`) e depois copiava de novo os valores
em `f32` (`as_f32().into_owned()`); o `_dono` move o `data` para fora do `RawImage` e o converte
uma vez. A conta é a mesma, na mesma ordem — só somem duas cópias da foto. Marcado com
`// nucleo-da-foto:`; quem chama é o `raw-codec`.
