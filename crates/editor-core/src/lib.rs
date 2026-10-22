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

#![deny(missing_docs)]

pub mod ajuste;
pub mod carimbo;
pub mod composicao;
pub mod contrato;
pub mod deformar;
pub mod difusao;
pub mod documento;
pub mod filtros;
pub mod frequencias;
pub mod historico;
pub mod liquidificar;
pub mod mesclagem;
pub mod misturador;
pub mod operacoes;
pub mod pincel;
pub mod projeto;
pub mod recuperacao;
pub mod retangulo;
pub mod selecao;
pub mod sessao;
pub mod tiles;
pub mod transformar;
pub mod vetor;
pub mod vista;

#[cfg(test)]
mod testes_comuns;
#[cfg(test)]
mod testes_da_caneta;
#[cfg(test)]
mod testes_da_etapa_16;
#[cfg(test)]
mod testes_da_mascara;
#[cfg(test)]
mod testes_das_ferramentas_de_selecao;
#[cfg(test)]
mod testes_do_arrasto;
#[cfg(test)]
mod testes_do_carimbo;
#[cfg(test)]
mod testes_do_retoque;
#[cfg(test)]
mod testes_do_tratamento_de_pele;
#[cfg(test)]
mod testes_dos_controles;
#[cfg(test)]
mod testes_fora_da_foto;

pub use ajuste::Ajuste;
pub use carimbo::AmostraDoCarimbo;
pub use composicao::Exibicao;
pub use contrato::VersaoEditada;
pub use deformar::Malha;
pub use documento::{BaseRef, Bloqueio, Camada, Documento, Mascara, Retoque};
pub use historico::{Comando, Historico};
pub use mesclagem::Modo;
pub use pincel::{Ferramenta, Pincel};
pub use retangulo::Retangulo;
pub use selecao::{Acabamento, Amostra, Estilo, Forma, Molde, Operacao, Selecao};
pub use sessao::{
    AmostraDaVarinha, Cadeado, Frequencia, OpcoesDaVarinha, OpcoesDoCarimbo, OrigemDaSeparacao,
    PedidoDeSeparacao, SaidaDoPreenchimento, Sessao, VarinhaRecusada, VistaDaSeparacao,
};
pub use tiles::{CamadaDePixels, LADO_DO_TILE};
pub use transformar::{Caixa, Transformacao};

/// `x.round().clamp(0.0, 255.0) as u8`, só com contas de `f32`: sem chamar
/// o `roundf` da libm (o x86-64 de base não tem a instrução) nem converter
/// para inteiro com saturação, e de um jeito que o compilador faz os quatro
/// canais de uma vez.
///
/// Limitar a `0..=255` antes de arredondar dá o mesmo byte (e o NaN vira 0,
/// como no `as u8`). Ali, `x + 2²³` cai num inteiro — o mais próximo, e no
/// empate o par —, que fica nos bits de baixo da soma. Só o meio que o par
/// levou para baixo ainda sobe, como no `round` (para longe do zero). O byte
/// é o mesmo do `round` em todo `f32` — conferido nos 2³² (nos testes do
/// `transformar`, `o_byte_arredondado_e_o_do_round` confere uma parte).
#[inline]
pub(crate) fn byte_arredondado(x: f32) -> u8 {
    const DOIS_A_23: f32 = 8_388_608.0;
    let x = if x > 0.0 { x } else { 0.0 };
    let x = if x < 255.0 { x } else { 255.0 };
    let soma = x + DOIS_A_23;
    let empate_para_baixo = x - (soma - DOIS_A_23) == 0.5;
    (soma.to_bits() as u8) + empate_para_baixo as u8
}

/// Quantos núcleos o sistema diz ter — perguntado uma vez por processo.
///
/// 🔑 No Linux, `available_parallelism` lê os arquivos do cgroup a cada
/// chamada; a vista e a difusão perguntavam a cada gesto. `None` quando o
/// sistema não sabe — quem chama escolhe o padrão.
pub(crate) fn nucleos() -> Option<usize> {
    static NUCLEOS: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *NUCLEOS.get_or_init(|| std::thread::available_parallelism().ok().map(|n| n.get()))
}
