//! O editor de fotos em camadas, sem janela.
//!
//! ```text
//! bruto → base neutra → [ este crate: camadas → composição ] → imagem editada → revelacao-core
//! ```
//!
//! A base entra pronta (quem decodifica é o `infrastructure::base_neutra`, C28),
//! e o que sai é a imagem editada e o projeto no disco. O contrato inteiro está em
//! `docs/editor-em-camadas/02-CONTRATO.md`; a gravação em `03-GRAVACAO-E-CATALOGO.md`.
//!
//! 🔑 **Tudo é RGB/RGBA de 8 bits em sRGB codificado, sem linearizar** (C29): é o
//! que o motor de revelação recebe, e pintar noutro espaço seria uma conversão a
//! mais entre o editor e a Revelação.

pub mod ajuste;
pub mod carimbo;
pub mod composicao;
pub mod contrato;
pub mod documento;
pub mod historico;
pub mod mesclagem;
pub mod operacoes;
pub mod pincel;
pub mod projeto;
pub mod retangulo;
pub mod selecao;
pub mod sessao;
pub mod tiles;
pub mod transformar;
pub mod vista;

#[cfg(test)]
mod testes_da_mascara;

pub use ajuste::Ajuste;
pub use contrato::VersaoEditada;
pub use documento::{BaseRef, Camada, Documento};
pub use historico::{Comando, Historico};
pub use mesclagem::Modo;
pub use pincel::{Ferramenta, Pincel};
pub use retangulo::Retangulo;
pub use selecao::{Forma, Operacao, Selecao};
pub use sessao::Sessao;
pub use tiles::{CamadaDePixels, LADO_DO_TILE};
pub use transformar::Transformacao;
