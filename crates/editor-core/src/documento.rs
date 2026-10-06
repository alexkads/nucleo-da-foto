//! O documento: a base de que partiu e as camadas por cima dela.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mesclagem::Modo;
use crate::retangulo::Retangulo;
use crate::tiles::{retangulo_do_tile, CamadaDePixels};

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

/// A máscara da camada, como no Photoshop: branco revela, preto esconde, os
/// cinzas deixam passar em parte.
///
/// 🔑 **É uma camada de pixels como as outras**, pintada sobre o `fundo`: o
/// valor de um pixel é a cor pintada (em cinza) sobre o fundo, na proporção do
/// alfa. Assim o pincel, a borracha (que devolve ao fundo), o Delete, o
/// preenchimento, o degradê e a lata de tinta funcionam nela sem código à
/// parte — e um tile que nunca foi pintado não existe, como na camada.
#[derive(Clone, Debug, PartialEq)]
pub struct Mascara {
    /// O valor de onde não se pintou: 255 revela tudo (o botão), 0 esconde
    /// tudo (⌥ + o botão).
    pub fundo: u8,
    /// Desligada (⇧ + clique na miniatura), a camada aparece inteira.
    pub ativa: bool,
    pub pixels: CamadaDePixels,
}

impl Mascara {
    pub fn nova(fundo: u8, largura: u32, altura: u32) -> Self {
        Self {
            fundo,
            ativa: true,
            pixels: CamadaDePixels::nova(largura, altura),
        }
    }

    /// O valor (0 esconde, 255 revela) de um pixel RGBA da máscara.
    #[inline]
    pub fn valor_do_pixel(fundo: u8, p: [u8; 4]) -> u8 {
        if p[3] == 0 {
            return fundo;
        }
        let cinza = ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32 + 128) >> 8) as i32;
        let (f, a) = (fundo as i32, p[3] as i32);
        let d = (cinza - f) * a;
        (f + (d + 127 * d.signum()) / 255) as u8
    }

    /// O valor em `(x, y)`.
    pub fn valor(&self, x: u32, y: u32) -> u8 {
        Self::valor_do_pixel(self.fundo, self.pixels.pixel(x, y))
    }

    /// A máscara esconde a camada inteira.
    pub fn esconde_tudo(&self) -> bool {
        self.ativa && self.fundo == 0 && self.pixels.vazia()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camada {
    pub nome: String,
    pub visivel: bool,
    /// `0..=1`.
    pub opacidade: f32,
    pub modo: Modo,
    pub pixels: CamadaDePixels,
    pub mascara: Option<Mascara>,
}

impl Camada {
    /// Transparente e visível, do tamanho da base.
    pub fn nova(nome: &str, largura: u32, altura: u32) -> Self {
        Self {
            nome: nome.into(),
            visivel: true,
            opacidade: 1.0,
            modo: Modo::Normal,
            pixels: CamadaDePixels::nova(largura, altura),
            mascara: None,
        }
    }

    /// A máscara que vale na composição — `None` sem máscara ou com ela
    /// desligada.
    pub fn mascara_ativa(&self) -> Option<&Mascara> {
        self.mascara.as_ref().filter(|m| m.ativa)
    }

    /// Os pixels onde se pinta: os da máscara ou os da camada.
    pub fn alvo(&self, na_mascara: bool) -> &CamadaDePixels {
        match (&self.mascara, na_mascara) {
            (Some(m), true) => &m.pixels,
            _ => &self.pixels,
        }
    }

    pub fn alvo_mut(&mut self, na_mascara: bool) -> &mut CamadaDePixels {
        match (&mut self.mascara, na_mascara) {
            (Some(m), true) => &mut m.pixels,
            _ => &mut self.pixels,
        }
    }

    /// O retângulo que os tiles pintados cobrem — onde a camada pode mudar a
    /// foto. Esconder, mexer na opacidade ou trocar de lugar só recompõe aqui.
    pub fn area(&self) -> Retangulo {
        let (largura, altura) = (self.pixels.largura(), self.pixels.altura());
        self.pixels
            .existentes()
            .fold(Retangulo::default(), |area, (posicao, _)| {
                area.uniao(&retangulo_do_tile(*posicao, largura, altura))
            })
    }

    /// A camada não muda nenhum pixel da base (C30).
    pub fn sem_efeito(&self) -> bool {
        !self.visivel
            || self.opacidade <= 0.0
            || self.pixels.vazia()
            || self.mascara.as_ref().is_some_and(Mascara::esconde_tudo)
    }
}

/// O nome da camada que nasce com o documento.
pub const NOME_DA_PRIMEIRA: &str = "Pintura";

#[derive(Clone, Debug, PartialEq)]
pub struct Documento {
    pub base: BaseRef,
    /// De baixo para cima — a de índice 0 fica logo acima da base.
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

    /// O nome de uma camada nova: "Camada N", com o N seguinte ao maior que já
    /// existe — como no Photoshop, que não reaproveita o número de quem saiu.
    pub fn proximo_nome(&self) -> String {
        let maior = self
            .camadas
            .iter()
            .filter_map(|c| c.nome.strip_prefix("Camada ")?.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("Camada {}", maior + 1)
    }

    /// Nenhuma camada muda a base (C30): a imagem editada seria a base byte a
    /// byte, e não se publica versão.
    pub fn neutro(&self) -> bool {
        self.camadas.iter().all(Camada::sem_efeito)
    }
}
