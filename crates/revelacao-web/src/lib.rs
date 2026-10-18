//! O motor de revelação no navegador — ver `web`.
//!
//! 🔑 **A API do JavaScript só existe no alvo `wasm32`.** `wgpu::SurfaceTarget::Canvas`
//! e o `HtmlCanvasElement` não têm par em nativo, e um crate que só compila num
//! alvo quebraria `cargo test --workspace` e o clippy do CI. Por isso `web`
//! (o motor sobre o canvas e as funções `#[wasm_bindgen]`) fica atrás de
//! `cfg(target_arch = "wasm32")`, e em nativo este crate não exporta nada.
//!
//! O que não precisa de GPU nem de canvas mora em `contrato`, que compila em
//! todo alvo: a forma dos vetores que o site manda, as conferências de
//! tamanho e as mensagens de erro, as matrizes do enquadramento, os
//! codificadores e o decodificador de arquivo. As funções de `web` são cascas
//! finas sobre ele, e os testes de `contrato` provam o contrato com o site em
//! `cargo test`, sem navegador. A matemática da revelação continua provada no
//! `revelacao-core`, que é onde ela mora.

// Em nativo só os testes chamam o contrato; quem o usa de verdade é `web`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod contrato;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;
