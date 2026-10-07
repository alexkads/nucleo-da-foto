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

pub mod modelos;

#[cfg(feature = "onnx")]
pub mod execucao;
