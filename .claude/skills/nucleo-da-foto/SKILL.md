---
name: nucleo-da-foto
description: Como consumir e como evoluir o nucleo-da-foto, o workspace Rust público com o motor de foto compartilhado pelo PhotoKads (photokads-desktop, photokads-plataforma) e pelo VintageLightbox (RecordarFotos) — revelacao-core, foto-codec, raw-codec, editor-core, recuperacao-core, fotolivro, visualizador-pdf, ia-local, preenchimento e os wrappers *-web. Use ao adicionar ou atualizar a dependência do núcleo num desses projetos, ao desenvolver o núcleo junto com um app ([patch] local), ao mudar qualquer API pública de um crate do núcleo (inclusive mudança que quebra compatibilidade), ao decidir a próxima versão ou lançar uma tag, e ao levar um crate novo de um app para o núcleo.
---

# nucleo-da-foto — consumir e evoluir

O `nucleo-da-foto` (`~/Projects/PhotoKads/nucleo-da-foto`, público em
`github.com/alexkads/nucleo-da-foto`) é o motor de foto que dois produtos compartilham. Toda mudança
aqui chega a **quatro consumidores**, e dois deles são de um produto em produção:

| Consumidor | Onde | Crates que usa |
|---|---|---|
| photokads-desktop | `~/Projects/PhotoKads/photokads-desktop` | quase todos |
| photokads-plataforma (backend) | `~/Projects/PhotoKads/photokads-plataforma/backend` | `fotolivro` |
| VintageLightbox (RecordarFotos) | `~/Projects/RecordarFotos/VintageLightbox-Rust` | quase todos |
| recordarfotos-e-commerce (backend) | `~/Projects/RecordarFotos/recordarfotos-e-commerce/backend` | `fotolivro` |

Os sites dos dois produtos também carregam os wasm gerados daqui (`scripts/construir-*.sh`).

🚨 **Os balcões do RecordarFotos compilam o `main` do VintageLightbox na máquina do cliente.** Por
isso este repositório é público, e por isso uma tag publicada é para sempre: se ela sumir ou mudar, a
próxima compilação de um balcão quebra.

Antes de agir, leia o `README.md` e o `CHANGELOG.md` deste repositório.

## Regra zero: o que pode morar aqui

Só código que **não sabe de produto**: nada de estúdio, tenant, plataforma, banco, rede, marca, API
de um backend, e-mail fixo, URL de produção. Se um crate precisa disso, ele fica no app — ou o pedaço
neutro sai para cá e o resto fica lá. Na dúvida, não traga.

## Consumir

### Declarar

Uma vez, no `[workspace.dependencies]` do consumidor, **por git com tag**:

```toml
[workspace.dependencies]
foto-codec     = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
revelacao-core = { git = "https://github.com/alexkads/nucleo-da-foto", tag = "v0.1.0" }
```

E em cada crate que usa: `revelacao-core = { workspace = true }` (com `features` ou
`default-features = false` ali, se precisar).

Regras:

- **Todos os crates do núcleo na mesma tag, no consumidor inteiro.** Duas tags no mesmo grafo viram
  dois `foto-codec` distintos: os tipos não conversam, e o erro é um `expected foto_codec::X, found
  foto_codec::X` confuso.
- **Tag, nunca `branch`.** `branch = "main"` faz cada `cargo update` puxar o que estiver lá — inclusive
  uma mudança que quebra. `rev` só como exceção temporária, e com comentário dizendo por quê.
- **Nunca `path` versionado para o núcleo.** O caminho só existe na máquina de quem desenvolve; no
  balcão e no CI a compilação quebra. Para desenvolver junto, use o `[patch]` local abaixo.
- O `Cargo.lock` do consumidor grava o commit da tag. Commite o lock junto com a troca de tag.

### Atualizar para uma versão nova

1. Leia a entrada do `CHANGELOG.md` de cada versão entre a atual e a nova — principalmente as seções
   **Quebra** e **Comportamento**.
2. Troque a tag em **todas** as linhas do núcleo no `Cargo.toml` do consumidor.
3. `cargo update -p foto-codec` (ou de cada crate do núcleo) e compile.
4. Faça as adaptações que o CHANGELOG pede.
5. Rode a bateria do consumidor (`make testar` e `make lint` no desktop; `make api-test` e
   `make api-lint` na plataforma).
6. Mudou o motor de revelação ou o codificador? Confira uma foto de verdade no app: o que o teste não
   pega, o olho pega.
7. Se o consumidor tem wasm do núcleo no site, regenere com os scripts da tag nova e commite os
   artefatos junto (o arquivo `VERSAO` diz de que commit eles saíram).

### Desenvolver o núcleo junto com um app

Sem lançar tag a cada tentativa: um `[patch]` **local e fora do git**, num `.cargo/config.toml` na
pasta do app (o `.cargo/` está no `.gitignore` dos consumidores):

```toml
# ~/Projects/PhotoKads/photokads-desktop/.cargo/config.toml
[patch."https://github.com/alexkads/nucleo-da-foto"]
foto-codec     = { path = "../nucleo-da-foto/crates/foto-codec" }
revelacao-core = { path = "../nucleo-da-foto/crates/revelacao-core" }
```

Caminhos: `../nucleo-da-foto` no photokads-desktop, `../../nucleo-da-foto` no backend da plataforma,
`../../PhotoKads/nucleo-da-foto` no VintageLightbox. Faça o `[patch]` de **todos** os crates do núcleo
que o app usa, não só do que mudou — senão o grafo mistura a cópia local e a da tag.

Pronto: lance a tag aqui, troque a tag no app, **apague o `[patch]` local** e compile de novo sem ele.
Compilar só com o `[patch]` ligado esconde que a tag não tem a mudança.

## Evoluir

### Antes de mudar

- A mudança serve aos dois produtos? Se só um precisa, avalie uma feature (desligada por padrão) ou
  deixe no app.
- Rode a bateria inteira antes de começar, para saber que o vermelho, se vier, é seu:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p conversao-web -p fotolivro-web -p raw-web -p revelacao-web -p tela-do-cliente-web
```

### O que é mudança que quebra (bump do menor enquanto for 0.x)

Quebra é tudo que faz um consumidor que compilava parar de compilar, ou passar a fazer outra coisa
sem ter pedido. Em Rust, entre outros:

- remover, renomear ou mover um item `pub` (função, tipo, módulo, constante, reexport);
- mudar a assinatura de uma função `pub` (parâmetros, retorno, genéricos, `async`, bounds);
- acrescentar campo `pub` a uma `struct` sem `#[non_exhaustive]` (quebra quem a constrói por
  literal), ou variante a um `enum` sem `#[non_exhaustive]` (quebra o `match` exaustivo);
- acrescentar método **sem default** a um trait `pub` que os consumidores implementam;
- remover uma feature, ou mudar o que vem nas `default` features;
- subir o MSRV (`rust-version`) ou trocar a edição de forma que exija Rust mais novo;
- mudar a API JavaScript de um `*-web` (nome ou forma do que o wasm-bindgen exporta), que o site
  chama;
- **mudar o resultado visível** sem mudar a assinatura: um ajuste da revelação que sai diferente, um
  JPEG com outra qualidade, um fotolivro com outra diagramação. A foto exportada é o que a tela
  mostra — se o mesmo parâmetro passa a dar outra foto, é quebra de comportamento, e vai na seção
  **Comportamento** do CHANGELOG mesmo que compile igual.

Não quebra (patch): correção que não muda a API nem o resultado esperado, item `pub` novo, campo novo
em struct `#[non_exhaustive]`, método novo com default, feature nova desligada por padrão,
desempenho.

Na dúvida entre patch e menor, é menor.

### Conferir com ferramenta, além da cabeça

`cargo-semver-checks` compara a API pública com a da última tag:

```sh
cargo install cargo-semver-checks --locked   # uma vez
cargo semver-checks --workspace --baseline-rev v0.1.0
```

Ele pega remoção, assinatura e `#[non_exhaustive]`; não pega mudança de comportamento nem a API
JavaScript dos `*-web`. Essas duas são revisão humana.

### Fazer uma mudança que quebra, sem derrubar ninguém

1. **Prefira o caminho de transição.** Acrescente o novo ao lado do velho, marque o velho com
   `#[deprecated(since = "0.X.Y", note = "use <novo>; sai na 0.(X+1)")]`, lance como patch ou menor
   compatível. Os consumidores migram no ritmo deles; o velho sai na próxima versão menor.
2. Quando a quebra é inevitável (ou o velho finalmente sai): suba o **menor** (`0.1.x` → `0.2.0`) e
   escreva a seção **Quebra** do CHANGELOG com o que muda e **como migrar**, com o trecho de código de
   antes e de depois.
3. Migre os consumidores **do PhotoKads** na mesma leva (photokads-desktop e, se usa o crate, o backend
   da plataforma), cada um no próprio commit.
4. Os consumidores **do RecordarFotos** são outro produto, em produção: a troca de tag lá só na branch
   `dev` deles, nunca no `main`, e só com o dono pedindo. Até migrarem, eles continuam na tag antiga —
   e isso é seguro justamente porque tag não se move.
5. Nunca reescreva, mova ou apague uma tag publicada. Errou? Lance outra (`v0.2.1`).

### Lançar uma versão

1. Bateria inteira verde (acima), mais o `cargo semver-checks` contra a tag anterior.
2. Suba `version` em `[workspace.package]` do `Cargo.toml` (uma versão para o workspace inteiro).
3. Escreva a entrada no topo do `CHANGELOG.md`:

```markdown
## v0.2.0 — 20/out/2026

### Quebra
- `revelacao_core::Ajustes::novo` agora recebe `&Perfil`. Antes: `Ajustes::novo()`. Depois:
  `Ajustes::novo(&Perfil::padrao())`.

### Comportamento
- O ajuste de nitidez saturava antes nos tons claros; a mesma foto sai mais suave nos realces.

### Novo
- `foto_codec::webp::codificar_com_qualidade`.

### Correção
- O DNG com perdas de 14 bits abria com as cores trocadas.
```

4. `git commit`, `git tag vX.Y.Z`, `git push origin main vX.Y.Z`.
5. Troque a tag nos consumidores do PhotoKads (o passo a passo de "Atualizar" acima).

### Levar um crate de um app para o núcleo

1. Ele passa na regra zero? Se ainda tem algo de produto, neutralize no app primeiro.
2. Traga **com o histórico**: `git filter-repo --path crates/<crate>/` numa cópia do app, e
   `git pull --allow-unrelated-histories` daquela cópia para cá (foi assim que o núcleo nasceu, do
   VintageLightbox `dc27aecb`).
3. Acrescente aos `members` do `Cargo.toml` e troque as dependências de caminho dele para os crates
   daqui (`path = "../foto-codec"`) ou do workspace (`serde.workspace = true`).
4. Lance como versão menor (crate novo é coisa nova, mas os consumidores precisam trocar o `path` pelo
   git).
5. No app, apague o crate e troque a dependência por `{ workspace = true }` apontando para a tag nova.

## O que não fazer

- Não trazer código de produto "só por enquanto": é o começo da divergência que este repositório
  existe para evitar.
- Não deixar `[patch]` local num commit de nenhum consumidor.
- Não usar `branch` nem `path` versionado para o núcleo.
- Não lançar sem a bateria inteira, inclusive o `wasm32` dos `*-web`.
- Não mexer em tag publicada.
- Não trocar a tag do núcleo nos projetos do RecordarFotos sem o dono pedir.
