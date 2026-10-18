# Changelog

## Não lançado

### Correção
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

## v0.1.0 — 18/out/2026

A primeira versão: os crates do motor saíram do VintageLightbox (0.1.165, `dc27aecb`) para cá, sem
mudança de código e com o histórico preservado. Os mesmos que o VintageLightbox e o photokads-desktop
usavam, byte a byte.
