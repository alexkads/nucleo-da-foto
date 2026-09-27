//! O documento: a base de que partiu e as camadas por cima dela.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::tiles::CamadaDePixels;

/// O perfil dos pixels da base e da imagem editada (C29): sRGB codificado, 8
/// bits por canal. Um formato novo (16 bits) terá outro nome.
pub const PERFIL: &str = "sRGB-8";

/// A base neutra de que o documento partiu — **nunca** os pixels, só quem ela
/// é. Os pixels vêm do bruto a cada abertura (C28).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseRef {
    pub largura: u32,
    pub altura: u32,
    /// O SHA-256 dos pixels RGB da base, precedido das dimensões. É o que a
    /// reabertura confere: pintar sobre outra base desalinharia tudo.
    pub sha256: String,
    pub perfil: String,
}

impl BaseRef {
    pub fn da_imagem(base: &image::RgbImage) -> Self {
        Self {
            largura: base.width(),
            altura: base.height(),
            sha256: impressao_da_base(base),
            perfil: PERFIL.into(),
        }
    }
}

/// A impressão digital da base: dimensões e pixels.
pub fn impressao_da_base(base: &image::RgbImage) -> String {
    let mut hash = Sha256::new();
    hash.update(base.width().to_le_bytes());
    hash.update(base.height().to_le_bytes());
    hash.update(base.as_raw());
    hex(&hash.finalize())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camada {
    pub nome: String,
    pub visivel: bool,
    /// `0..=1`.
    pub opacidade: f32,
    pub pixels: CamadaDePixels,
}

impl Camada {
    /// Transparente e visível, do tamanho da base.
    pub fn nova(nome: &str, largura: u32, altura: u32) -> Self {
        Self {
            nome: nome.into(),
            visivel: true,
            opacidade: 1.0,
            pixels: CamadaDePixels::nova(largura, altura),
        }
    }

    /// A camada não muda nenhum pixel da base (C30).
    pub fn sem_efeito(&self) -> bool {
        !self.visivel || self.opacidade <= 0.0 || self.pixels.vazia()
    }
}

/// O nome da camada que nasce com o documento.
pub const NOME_DA_PRIMEIRA: &str = "Pintura";

#[derive(Clone, Debug, PartialEq)]
pub struct Documento {
    pub base: BaseRef,
    /// De baixo para cima. 🔑 A etapa 1 tem uma camada; o `Vec` já é o formato
    /// das próximas.
    pub camadas: Vec<Camada>,
}

impl Documento {
    /// Um documento novo: a base e uma camada transparente por cima.
    pub fn novo(base: BaseRef) -> Self {
        let camada = Camada::nova(NOME_DA_PRIMEIRA, base.largura, base.altura);
        Self {
            base,
            camadas: vec![camada],
        }
    }

    pub fn largura(&self) -> u32 {
        self.base.largura
    }

    pub fn altura(&self) -> u32 {
        self.base.altura
    }

    /// Nenhuma camada muda a base (C30): a imagem editada seria a base byte a
    /// byte, e não se publica versão.
    pub fn neutro(&self) -> bool {
        self.camadas.iter().all(Camada::sem_efeito)
    }
}
