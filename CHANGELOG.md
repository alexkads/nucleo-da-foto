# Changelog

## Próxima (0.1.3) — ainda sem tag

Sem mudança na foto que sai nem na API (`make semver`: nada pede versão nova). Cada caminho novo tem
o teste que o compara byte a byte com o de antes.

### Desempenho
Na bancada (`make bancada`, contra a v0.1.2), foto de 12 MP, 4 núcleos:

| Caminho | v0.1.2 | Agora |
|---|---:|---:|
| Enquadrar: girar 90° e recortar | 159 ms | 33 ms |
| Enquadrar: endireitar 3° e recortar | 552 ms | 90 ms |
| Tela do Enquadrar: endireitar 3° (foto inteira) | 780 ms | 92 ms |
| Compor a foto inteira (salvar, varinha mágica) | 299 ms | 104 ms |
| Compor com a camada a 60 % e máscara difusa | 974 ms | 242 ms |
| Guia da Claridade, Textura e Remover névoa (1º uso numa foto) | 130 ms | 45 ms |
| Começar o ⌘T ou o Mover com seleção (o conteúdo de uma elipse) | 609 ms | 51 ms |
| ⌘T deslocando, por evento do ponteiro | 214 ms | 19 ms |
| ⌘T girando e ampliando, por evento do ponteiro | 298 ms | 130 ms |
| Deformar, por evento do ponteiro | 350 ms | 170 ms |
| Mover com seleção, por evento do ponteiro | 242 ms | 24 ms |
| Mover só o contorno da seleção, por evento | 161 ms | 5 ms |
| Transformar a seleção, por evento | 96 ms | 60 ms |
| Pintar: pincel padrão (raio 40), um traço de lado a lado | 84 ms | 49 ms |
| Pintar: pincel macio de raio 200 (o de máscara), um traço | 632 ms | 130 ms |
| Pintar: carimbo de raio 100, um traço | 263 ms | 112 ms |
| Pintar: Misturador de raio 100, um traço | 534 ms | 255 ms |

- `revelacao-core`: sem ângulo, espelhos + giro de 90° + recorte reto viram uma passada só, por
  índice, sem a imagem girada inteira no meio (RGBA8 e RGB8). Endireitar e a tela do Enquadrar
  amostram direto nos bytes, com as linhas divididas entre os núcleos (no wasm32, uma por vez).
- `editor-core`: compor uma região grande (a foto inteira) em faixas de linhas, uma por núcleo.
- `editor-core`, **⌘T e Mover com seleção**: o começo do gesto acha o tile da camada e o da
  seleção uma vez por tile (era uma busca por pixel), em faixas. O desenho de cada evento acha o
  tile pela coluna e lê `v / 255` de uma tabela; com deslocamento inteiro (o Mover com seleção, o
  colar) cada ponto cai no centro de um pixel e o desenho copia trechos de linha, sem a conta
  bilinear. Pôr o desenhado por cima do fundo vai um tile por núcleo.
- `editor-core`: andar a seleção (o contorno, e o fim do Mover com seleção) copia trechos de linha
  de tile para tile, em vez de ler o valor de cada pixel.
- `revelacao-core`: a guia da Claridade, da Textura e do Remover névoa reduz a foto em faixas de
  linhas (no wasm32, uma por vez), e as passadas verticais da caixa e do mínimo andam linha a
  linha, e não coluna a coluna — a mesma soma, na mesma ordem. Numa thread só (o site), 89 → 48 ms
  numa foto de 2560 × 1707.
- `editor-core` e `revelacao-core`, **a amostra bilinear e o arredondamento sem a libm**: no miolo
  da imagem (quase todo ponto) os quatro vizinhos são lidos direto, sem conferir a borda de cada
  um, com a mesma conta escrita canal a canal para o compilador fazer os quatro de uma vez; e o
  arredondamento para byte sai só com contas de `f32` (no x86-64 de base o `round` é uma chamada à
  libm) — o mesmo byte em todo `f32`, conferido nos 2³². Vale para o ⌘T com escala ou giro, o
  Deformar, o Transformar seleção, o Endireitar, a tela do Enquadrar e a perspectiva, a mescla de
  camada com opacidade ou alfa parcial e a máscara difusa. O Deformar e o Transformar seleção
  acham o tile pela coluna (era um `BTreeMap` por pixel), como o ⌘T.
- `editor-core`, **pintar** (pincel, borracha, carimbo, recuperação, tom, foco e Misturador): cada
  carimbo da ponta arredonda sem a libm (a tabela da ponta, a cobertura e cada canal) e lê
  `v / 255` de uma tabela; a fonte do carimbo acha o tile por uma grade (era um `BTreeMap` por
  pixel); o Misturador lê e grava por trecho de linha de cada tile. Com a ponta de 128 × 128 px
  ou mais, a conta dos pixels se divide em faixas de linhas entre os núcleos, e a fonte é lida
  por todas as threads, cada tile composto uma vez.

### Novo
- A bancada compara com a **v0.1.2** e ganhou os casos "Endireitar 3° na tela do Enquadrar", a
  guia, o começo do Mover com seleção, a composição com a camada a 60 % e máscara difusa, e por
  evento o ⌘T girando, o Deformar, o Mover com seleção, o contorno e o Transformar seleção; o
  arrasto passa a conferir também a seleção, tile a tile.
- A bancada ganhou o grupo **Pintar**: um traço de lado a lado com o pincel padrão, com o pincel
  macio de raio 200, com o carimbo e com o Misturador, conferindo a camada e o passo do desfazer.
- `cargo bench -p editor-core` e `-p revelacao-core`: o ⌘T (começar, desenhar com e sem
  deslocamento inteiro, pôr por cima), o Deformar, a seleção (andar, transformar), a composição
  com opacidade e com máscara difusa, o pincel (raio 40, e 200 macio), o carimbo, o Misturador e
  a guia.

## v0.1.2 — 19/out/2026

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
- `editor-core`: `as_chunks_mut` no lugar de `chunks_exact_mut` (o rubi da máscara e três testes),
  que o clippy do Rust 1.99 reprovava no `make verificar` do `main`.
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
