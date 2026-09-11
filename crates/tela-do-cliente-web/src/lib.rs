//! A **tela do cliente** do balcão, desenhada em Rust.
//!
//! É o segundo monitor, virado para quem paga: a foto em foco da galeria do
//! operador, inteira, sobre preto, sem preço nem controle nenhum. O modelo é o
//! da segunda tela do desktop (`ui-gpui/src/cliente.rs`).
//!
//! ## Por que esta tela é o canvas, e as outras não
//!
//! 🚨 A regra da casa é **wasm é motor, a tela é React**
//! (`docs/BIBLIOTECA_NO_NAVEGADOR.md`), e ela nasceu de um dia inteiro perdido
//! em 2026-09-05 pondo a galeria em egui. O motivo técnico: duas pilhas de
//! interface na mesma página nunca ficam iguais — outro select, outro campo,
//! outra fonte, outro foco de teclado.
//!
//! Aqui não há interface. A tela do cliente é uma foto sobre preto: não existe
//! select, campo, menu, diálogo ou toast para divergir de nada, e nem a página
//! do site em volta. O que ela faz é o que o canvas faz bem — imagem grande a
//! 60 fps, com revelação aplicada e transição entre uma foto e a seguinte. A
//! decisão de trazê-la para cá é do dono, em 2026-09-11, e está registrada em
//! STATUS §2.88.
//!
//! **O que ficou fora, e é o limite acordado**: o rodapé do `I` (nome e
//! estrelas) e o botão de tela cheia continuam em DOM. Texto rasterizado no
//! canvas pede fonte embutida — 250 KB de binário e um leitor de tela cego,
//! para escrever duas linhas.
//!
//! ## O que fica de cada lado
//!
//! | lado | faz |
//! |---|---|
//! | JavaScript | escuta o `BroadcastChannel`, baixa a cópia de trabalho, decodifica (`createImageBitmap`, 5–10× mais rápido que em wasm), chama `mostrar`/`ajustar` e roda o laço de quadros |
//! | aqui | sobe os pixels, revela com os 53 ajustes, enquadra, encaixa na janela, cruza as duas fotos e desenha |

//! # 🔑 Só existe no alvo `wasm32`
//!
//! `wgpu::SurfaceTarget::Canvas` e o `HtmlCanvasElement` não têm par em nativo,
//! e um crate que só compila num alvo quebraria `cargo test --workspace` e o
//! clippy do CI — que é exatamente o que aconteceu quando este nasceu sem a
//! guarda. É a mesma linha do `revelacao-web`, e pelo mesmo motivo: em nativo
//! ele é vazio de propósito, e o que se prova sem navegador é o
//! `revelacao-core`, onde a matemática mora.

#[cfg(target_arch = "wasm32")]
mod tela;
#[cfg(target_arch = "wasm32")]
pub use tela::{abrir, Tela};
