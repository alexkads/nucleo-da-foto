# Changelog

## v0.1.1 — 18/out/2026

Sem mudança de API (`make semver`: nenhum crate pede versão nova) e sem mudança na foto que sai de
um arquivo bom. Troque a tag e compile.

### Correção
- 🐛 **RAW**: DNG descomprimido de 9 a 15 bits (fora 10 e 12) cuja linha não ocupa bytes inteiros
  — o rawler pulava o começo de cada linha com a bomba de bits vazia: pânico no debug, pixels errados
  no release. Corrigido no `vendor/rawler` (`unpack_generic_msb`).
- **RAW**: arquivo cortado ou torto dá erro, nunca pânico. 31 pânicos do rawler (fotometria
  desconhecida, largura 0, mais de 32 bits por amostra, matriz de cor sem D65, buffer esgotado,
  entrelaçamento desconhecido) viraram `Err`. No site o wasm roda com `panic = "abort"`: um arquivo
  estranho derrubava o módulo. Lista em `vendor/rawler/MUDANCAS.md`.
- `revelacao-web` e `tela-do-cliente-web`: a conferência de `largura × altura × 4` em 64 bits (no
  wasm32 o `usize` dava a volta e um buffer vazio passava na conta; o limite de textura barrava
  logo depois).
- **Segurança**: `zip` 8.6 → 9.0.5 no `ia-local` (RUSTSEC-2026-0367, -0368 e -0369: a extração
  respeitava links simbólicos e bits SUID fora da pasta de destino; o `ia-local` não usa o `extract`,
  mas a versão vulnerável não fica) e `lru` 0.16 → 0.18 no `revelacao-core` (RUSTSEC-2026-0253:
  possível uso-após-liberação em `LruCache::pop`). Nenhuma API pública muda.
- A documentação pública não aponta mais para itens privados nem tem URL solta (`cargo doc -D warnings`).

### Novo
- O portão `make verificar` (fmt, clippy nativo e wasm32, testes, doc, `cargo deny`), `make cobertura`,
  `make semver` e `make msrv`.
- Lints do workspace em todo crate (`unsafe_code = "forbid"`, sem `dbg!`/`todo!`/`unimplemented!`).
- `deny.toml`: licenças permitidas, RustSec e fontes; a exceção do `rawler` (LGPL-2.1-only) escrita.
- Metadados de todo crate: MSRV (**1.92**, conferido), `readme`, `keywords`, `categories`,
  `repository`, `publish = false`; um README por crate.
- `CONTRIBUTING.md` e a skill `nucleo-da-foto` (consumir e evoluir, mudança que quebra, lançamento).
- Todo item público documentado, com `#![deny(missing_docs)]` em todo crate, e exemplos que rodam
  (doc-tests) nas entradas principais. Os campos de `Ajustes` dizem unidade, faixa e neutro — e onde
  a escala não é a do Lightroom (contraste multiplicador, temperatura em ±10, matiz do HSL em graus).
- 758 testes (eram ~590), cobertura de linhas de 94 % sem o vendor e piso de 90 % no
  `make cobertura`. Os `*-web` têm a lógica num módulo testado em nativo; a API JavaScript é a mesma.

## v0.1.0 — 18/out/2026

A primeira versão: os crates do motor saíram do VintageLightbox (0.1.165, `dc27aecb`) para cá, sem
mudança de código e com o histórico preservado. Os mesmos que o VintageLightbox e o photokads-desktop
usavam, byte a byte.
