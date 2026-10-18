//! Os formatos que a câmera grava, e onde cada arquivo termina.
//!
//! Cada formato responde a duas perguntas:
//!
//! 1. **a assinatura**: os primeiros bytes de um setor parecem o começo dele?
//!    Barato, porque roda em todo setor do cartão;
//! 2. **a medida**: lendo a estrutura a partir dali, onde o arquivo termina?
//!    Só roda quando a assinatura bateu, e devolve `None` quando a estrutura não
//!    fecha. É o que separa uma foto de um `FF D8 FF` perdido no meio de outra.

use std::io::{self, Read, Seek};

use crate::leitor::Leitor;

pub mod cr3;
pub mod jpeg;
pub mod raf;
pub mod tiff;

/// Nenhuma foto passa disto. Um RAW de médio formato sem compressão fica perto
/// de 250 MB; o limite existe para que um campo corrompido não faça a varredura
/// copiar meio cartão como se fosse uma foto só.
pub const MAIOR_FOTO: u64 = 1 << 30;

/// O formato de uma foto achada — o que decide a extensão do arquivo
/// recuperado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Formato {
    /// JPEG, com as prévias MPF anexadas quando a câmera as grava.
    Jpeg,
    /// Canon antes de 2018 (TIFF com `CR` no byte 8, ou `Make` Canon).
    Cr2,
    /// Canon desde 2018 (caixas ISO BMFF, `ftyp` `crx `).
    Cr3,
    /// Nikon (TIFF com `Make` Nikon).
    Nef,
    /// Sony (TIFF com `Make` Sony).
    Arw,
    /// Adobe DNG, de qualquer marca (TIFF com a tag `DNGVersion`).
    Dng,
    /// Olympus e OM System (assinatura `IIRO`/`IIRS`/`MMOR`, ou `Make`).
    Orf,
    /// Panasonic e Leica (mágica TIFF `0x55`, ou `Make`).
    Rw2,
    /// Fujifilm (cabeçalho `FUJIFILMCCD-RAW `).
    Raf,
    /// Um TIFF que não é de nenhuma das marcas acima.
    Tiff,
}

impl Formato {
    /// A extensão do arquivo recuperado. São as que a importação reconhece
    /// (`infrastructure::file_system::SUPPORTED_EXTENSIONS`).
    pub fn extensao(self) -> &'static str {
        match self {
            Formato::Jpeg => "jpg",
            Formato::Cr2 => "cr2",
            Formato::Cr3 => "cr3",
            Formato::Nef => "nef",
            Formato::Arw => "arw",
            Formato::Dng => "dng",
            Formato::Orf => "orf",
            Formato::Rw2 => "rw2",
            Formato::Raf => "raf",
            Formato::Tiff => "tif",
        }
    }

    /// Se é o arquivo cru do sensor. Só [`Jpeg`](Self::Jpeg) e
    /// [`Tiff`](Self::Tiff) não são.
    pub fn e_raw(self) -> bool {
        !matches!(self, Formato::Jpeg | Formato::Tiff)
    }
}

/// Qual família a assinatura sugere. A medida é quem confirma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Familia {
    /// `FF D8 FF` e um marcador de começo de arquivo.
    Jpeg,
    /// Um cabeçalho TIFF: o TIFF puro e os RAW que moram nele.
    Tiff,
    /// Uma caixa `ftyp` da Canon.
    Cr3,
    /// O cabeçalho do RAF.
    Raf,
}

/// Quantos bytes do começo do setor a [`assinatura`] olha.
pub const CABECA: usize = 16;

/// Os primeiros bytes de um setor parecem o começo de uma foto?
///
/// Olha só os [`CABECA`] primeiros bytes; com menos, `None`. Barato de
/// propósito: roda em todo setor do cartão.
pub fn assinatura(cabeca: &[u8]) -> Option<Familia> {
    if cabeca.len() < CABECA {
        return None;
    }
    if jpeg::parece(cabeca) {
        Some(Familia::Jpeg)
    } else if tiff::parece(cabeca) {
        Some(Familia::Tiff)
    } else if cr3::parece(cabeca) {
        Some(Familia::Cr3)
    } else if raf::parece(cabeca) {
        Some(Familia::Raf)
    } else {
        None
    }
}

/// Onde termina a foto que começa em `inicio`, e de que formato ela é.
/// `None` quando a estrutura não fecha.
///
/// O `u64` devolvido é o **tamanho** em bytes (o fim menos `inicio`), já
/// conferido: maior que zero, até [`MAIOR_FOTO`] e, quando o leitor sabe o
/// total, dentro do cartão. Chame-a depois que [`assinatura`] deu a família.
///
/// # Erros
///
/// O erro de E/S do dispositivo, como veio. Estrutura estragada não é erro: é
/// `Ok(None)`.
pub fn medir<R: Read + Seek>(
    leitor: &mut Leitor<R>,
    inicio: u64,
    familia: Familia,
) -> io::Result<Option<(u64, Formato)>> {
    let medida = match familia {
        Familia::Jpeg => jpeg::medir(leitor, inicio)?.map(|t| (t, Formato::Jpeg)),
        Familia::Tiff => tiff::medir(leitor, inicio)?,
        Familia::Cr3 => cr3::medir(leitor, inicio)?.map(|t| (t, Formato::Cr3)),
        Familia::Raf => raf::medir(leitor, inicio)?.map(|t| (t, Formato::Raf)),
    };
    // A última conferência vale para todos: o arquivo inteiro tem de estar
    // dentro do cartão. Um fim além do último setor é estrutura corrompida, ou
    // uma foto de que só sobrou o começo.
    Ok(medida.filter(|(tamanho, _)| {
        *tamanho > 0
            && *tamanho <= MAIOR_FOTO
            && leitor.total().is_none_or(|total| inicio + tamanho <= total)
    }))
}
