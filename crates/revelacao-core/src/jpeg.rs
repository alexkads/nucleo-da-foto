//! O JPEG do estúdio — **mora em `foto-codec`**, e este módulo aponta para lá.
//!
//! 🔑 Ele saiu daqui em 2026-09-12 para poder ser compilado para wasm sem o
//! `wgpu` junto: o download do cliente converte WebP em JPEG no navegador dele,
//! e um crate que traz um motor de renderização a reboque não cabe num celular.
//! O caminho antigo continua valendo — `revelacao_core::jpeg::codificar` é a
//! mesma função de sempre.
pub use foto_codec::{codificar, decodificar};
