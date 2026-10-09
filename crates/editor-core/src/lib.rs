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
pub mod deformar;
pub mod difusao;
pub mod documento;
pub mod filtros;
pub mod historico;
pub mod liquidificar;
pub mod mesclagem;
pub mod operacoes;
pub mod pincel;
pub mod projeto;
pub mod recuperacao;
pub mod retangulo;
pub mod selecao;
pub mod sessao;
pub mod tiles;
pub mod transformar;
pub mod vista;

#[cfg(test)]
mod testes_da_etapa_16;
#[cfg(test)]
mod testes_da_mascara;
#[cfg(test)]
mod testes_das_ferramentas_de_selecao;
#[cfg(test)]
mod testes_do_carimbo;
#[cfg(test)]
mod testes_do_retoque;
#[cfg(test)]
mod testes_dos_controles;
#[cfg(test)]
mod testes_fora_da_foto;

pub use ajuste::Ajuste;
pub use carimbo::AmostraDoCarimbo;
pub use composicao::Exibicao;
pub use contrato::VersaoEditada;
pub use deformar::Malha;
pub use documento::{BaseRef, Bloqueio, Camada, Documento, Mascara};
pub use historico::{Comando, Historico};
pub use mesclagem::Modo;
pub use pincel::{Ferramenta, Pincel};
pub use retangulo::Retangulo;
pub use selecao::{Acabamento, Amostra, Estilo, Forma, Molde, Operacao, Selecao};
pub use sessao::{
    AmostraDaVarinha, Cadeado, OpcoesDaVarinha, OpcoesDoCarimbo, SaidaDoPreenchimento, Sessao,
    VarinhaRecusada,
};
pub use tiles::{CamadaDePixels, LADO_DO_TILE};
pub use transformar::{Caixa, Transformacao};
