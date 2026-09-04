//! O motor de revelação no navegador — ver [`web`].
//!
//! 🔑 **Só existe no alvo `wasm32`.** `wgpu::SurfaceTarget::Canvas` e o
//! `HtmlCanvasElement` não têm par em nativo, e um crate que só compila num
//! alvo quebraria `cargo test --workspace` e o clippy do CI. Em nativo este
//! crate é vazio de propósito: o que se prova dele em nativo é o
//! `revelacao-core`, que é onde a matemática mora.

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;
