# Contribuir com o nucleo-da-foto

Antes de mudar qualquer coisa, leia a skill [`nucleo-da-foto`](.claude/skills/nucleo-da-foto/SKILL.md):
o que pode morar aqui, como consumir, o que é mudança que quebra e como lançar.

## O fluxo

1. `cargo xtask dev` verde antes de começar.
2. A mudança, com teste. Toda função pública nova tem doc (`///`) e, quando cabe, um exemplo que
   compila (doc-test).
3. `cargo xtask dev` enquanto trabalha, e `cargo xtask pr` verde antes de abrir o PR.
4. Entrada no `CHANGELOG.md` (Quebra, Comportamento, Novo, Correção).
5. Lançar é subir a versão no `Cargo.toml`, tag e push — ver a skill.

## Os comandos

As checagens são subcomandos do `cargo xtask`, um por finalidade. Nenhum recebe argumento de modo:
`cargo xtask pr --full` é recusado e diz qual usar. Os testes rodam no `cargo-nextest`
(`cargo install cargo-nextest --locked`, uma vez).

| Comando | Quando | O que faz |
|---|---|---|
| `cargo xtask dev` | enquanto se trabalha | fmt, e clippy `-D warnings` e testes (nextest e doctests) só dos pacotes alterados |
| `cargo xtask pr` | o gate do PR | clippy em todo pacote que a mudança alcança, testes dos alterados e dos dependentes diretos, e o clippy wasm32, o `cargo doc` sem aviso e a bancada quando a mudança os alcança; mudou o `Cargo.lock`, o workspace inteiro e o `cargo deny` |
| `cargo xtask release` | antes de lançar | o workspace inteiro: fmt, clippy nativo e wasm32, testes, doc, `cargo deny`, o Rust mínimo (`rust-version`) e a API pública de cada crate contra a última tag (`cargo semver-checks`) |
| `cargo xtask cobertura` | quando se quer o relatório | cobertura por arquivo (`cargo llvm-cov`), piso de 90 %, HTML em `target/cobertura` |
| `cargo xtask bancada` / `bancada-testar` | medir contra a última tag | a janela da bancada, e os testes dela |
| `cargo xtask faxina` | o `target/` cresceu | apaga o incremental e as árvores da cobertura e do semver |

O `dev` e o `pr` comparam com `origin/main`; outra base vai em `CHECK_BASE`. Cada checagem termina
com a tabela do tempo de cada etapa, gravada também em `target/check-timings/last.txt`.

## O estilo

- Português, como os dois produtos: nomes, docs e mensagens de erro.
- Commits contam o que foi **encontrado** e o que mudou.
- Nada de produto aqui (estúdio, plataforma, banco, rede, marca): ver a regra zero da skill.
