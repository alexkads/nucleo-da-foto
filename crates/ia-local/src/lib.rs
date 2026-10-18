//! 🧠 A IA local do VintageLightbox: tudo o que roda um modelo no computador
//! do balcão, sem saber para que ele serve.
//!
//! - [`modelos`] — o catálogo e a instalação: download explícito, hash
//!   conferido, importação de arquivo, remoção, a pasta de dados do app.
//! - [`execucao`] — o **backend** (CPU, CoreML, DirectML, CUDA), escolhido
//!   automaticamente ou à mão, com volta à CPU avisada; a **sessão** do ONNX
//!   Runtime carregada uma vez e reaproveitada; a inferência fora da thread da
//!   tela, com cancelamento real.
//!
//! O primeiro uso é o Preenchimento sensível ao conteúdo (a LaMa, no crate
//! `preenchimento`). Uma tarefa nova declara o seu [`modelos::Modelo`] e usa
//! [`execucao::sessao`] — o resto é dela (pré e pós-processamento).
//!
//! 🔒 **Nada sai da máquina**: a única conexão é o download do modelo, pedido
//! pelo operador. Fotos, máscaras e resultados ficam aqui.
//!
//! # Exemplo
//!
//! Uma tarefa declara o seu modelo e pergunta à pasta se ele está lá — sem
//! rede: só o [`modelos::baixar`] fala com a internet, e só quando o operador
//! pede.
//!
//! ```rust
//! use ia_local::modelos::{self, Estado, Modelo};
//!
//! const EXEMPLO: Modelo = Modelo {
//!     id: "exemplo",
//!     nome: "Modelo de exemplo",
//!     arquivo: "exemplo.onnx",
//!     versao: "exemplo/repo @ 0000000",
//!     url: "https://exemplo.invalid/exemplo.onnx",
//!     bytes: 1024,
//!     sha256: "0000000000000000000000000000000000000000000000000000000000000000",
//!     licenca: "MIT",
//!     origem: "exemplo",
//! };
//!
//! let pasta = tempfile::tempdir().unwrap();
//! assert_eq!(modelos::estado(pasta.path(), &EXEMPLO), Estado::Ausente);
//! ```

#![deny(missing_docs)]

pub mod modelos;

#[cfg(feature = "onnx")]
pub mod execucao;
#[cfg(feature = "onnx")]
pub mod runtime;
