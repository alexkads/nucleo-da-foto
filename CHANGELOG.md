# Changelog

## Próxima (0.1.2) — ainda sem tag

Sem mudança na foto que sai: cada caminho novo tem o teste que o compara byte a byte com o antigo.
`make semver`: nenhum crate pede versão nova (só itens `pub` novos).

### Desempenho
Medido com `cargo bench -p <crate>` (novo, sem dependência), melhor de duas rodadas, mesma máquina
(4 núcleos), antes → depois:

| Caminho | Antes | Depois |
|---|---:|---:|
| Vista de 1600 px de uma foto de 12 MP, rascunho do slider (Níveis) | 62 ms | 15 ms |
| A mesma vista, refazer | 202 ms | 76 ms |
| Compor 12 MP inteira (salvar, varinha mágica) | 667 ms | 174 ms |
| Mover: deslocar uma camada de 12 MP por tiles inteiros (o 1º evento do arrasto) | 16 ms | 0,01 ms |
| Exportar no navegador sem corte (2560², o resultado da GPU tomado) | 36 ms | 2,4 ms |
| Enquadrar 2560²: recorte reto / giro de 90° / endireitar 3° | 7 / 123 / 265 ms | 5 / 94 / 211 ms |
| JPEG de 12 MP direto dos pixels do JavaScript | 433 ms | 335 ms |
| Procurar o XMP num RAW de 40 MB sem XMP | 28 ms | 2,1 ms |
| Adobe RGB → sRGB numa miniatura de 256² | 1,6 ms | 1,0 ms |
| Content-Aware, buraco de 80² | 122 ms | 113 ms |

- `editor-core`: a composição é preparada (papéis, tabelas dos ajustes, leitores das máscaras) uma
  vez por faixa da vista — o rascunho preparava uma vez por linha. O Normal opaco sai sem conta.
  Deslocar por tiles inteiros reaproveita os tiles. Os núcleos são perguntados uma vez.
- `revelacao-core`: espelho, giro e endireitar não copiam mais a origem antes de começar; a leitura
  da GPU é uma cópia só quando a linha não tem enchimento; o Content-Aware não varre o nível inteiro
  a cada anel da casca de cebola e não aloca por pixel.
- `foto-codec`: `codificar` não copia para RGB8 o que já é RGB8 ou RGBA8 (72 MB a 24 MP); a tabela de
  volta ao sRGB é feita uma vez por processo.
- `revelacao-web`: o JPEG sai dos pixels emprestados do JavaScript, a exportação não clona a
  revelada e `decodificar_imagem` monta o RGBA uma vez — menos pico de memória no celular.
- `raw-codec`: desce a 8 bits antes de girar (metade dos bytes, e sem a cópia do fim); XMP por
  `memchr::memmem`.
- `editor-core`, **o arrasto no tamanho da tela** (como o Lightroom): Mover, ⌘T, Deformar, Mover
  com seleção, prévia de filtro, opacidade e densidade/difusão da máscara vetorial passam a mostrar
  a vista em **rascunho** durante o gesto (um pixel da foto por pixel da tela), como o slider de
  ajuste já fazia, e a vista exata ao soltar, aplicar, cancelar ou fechar o gesto. O rascunho
  compõe só as colunas que usa (custo W·H/f², era W·H/f). Na bancada, por evento, foto de 12 MP
  e vista de 1600 px: opacidade 94 → 12 ms, slider dos Níveis 57 → 10 ms, Mover 167 → 40 ms,
  prévia de filtro 147 → 53 ms, ⌘T 339 → 199 ms.

### Comportamento (só na tela)
- Durante esses arrastos a vista é o rascunho; ao fim do gesto, a exata. O documento, a
  composição (`compor`), o histórico e a vista depois do gesto são os mesmos byte a byte
  (`testes_do_arrasto`, `o_rascunho_por_amostras_e_o_de_compor_a_linha`).

### Correção
- Os `cargo bench` da 0.1.2 passam no `cargo fmt --check` e no `clippy -D warnings`
  (`--all-targets`), que reprovavam no `main`.

### Novo
- `revelacao_core::transformacao::aplicar_dono`: o `aplicar` que toma a foto — sem corte, devolve
  a mesma, sem cópia. Para quem já é dono do resultado da GPU (o desktop, a cada quadro).
- `foto_codec::codificar_rgba`: o mesmo JPEG de `codificar`, de pixels RGBA emprestados.
- `cargo bench -p foto-codec | raw-codec | revelacao-core | editor-core`.
- **A bancada** (`ferramentas/bancada`, `make bancada`): uma janela (GPUI Kit) que abre uma foto
  sua (JPEG, PNG, WebP, TIFF ou RAW), roda cada caminho na última tag e no código da pasta,
  alternados, e mostra antes, depois, ganho e se a foto saiu igual byte a byte. Também no
  terminal: `cargo run --release -- --sem-janela foto.cr3`. Ela tem o próprio workspace, então o
  GPUI não entra no grafo de quem consome o núcleo; `make bancada-testar` roda os testes dela,
  inclusive o da janela sem tela.

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
