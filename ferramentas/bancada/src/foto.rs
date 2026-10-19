//! A foto da bancada: o arquivo que o usuário abriu, ou uma de exemplo.

use std::path::Path;

use image::{DynamicImage, RgbaImage};

/// Uma foto pronta para os casos: os bytes do arquivo (para os casos de RAW)
/// e os pixels já de pé, em RGBA8 — o que a GPU devolve ao revelar.
pub struct Foto {
    /// O nome do arquivo, ou "exemplo".
    pub nome: String,
    /// Os bytes do arquivo; `None` na foto de exemplo.
    pub bytes: Option<Vec<u8>>,
    /// O arquivo é RAW (pela extensão).
    pub raw: bool,
    /// Os pixels, de pé, em RGBA8.
    pub rgba: RgbaImage,
}

impl Foto {
    /// Abre JPEG, PNG, WebP, TIFF ou RAW, de pé — pelo núcleo **desta pasta**.
    pub fn abrir(caminho: &Path) -> Result<Foto, String> {
        let nome = caminho
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| caminho.display().to_string());
        let bytes = std::fs::read(caminho).map_err(|e| format!("não abriu {nome}: {e}"))?;
        let raw = raw_codec::eh_raw(&nome);
        let rgba = if raw {
            DynamicImage::ImageRgb8(raw_codec::decodificar(&bytes)?).into_rgba8()
        } else {
            foto_codec::orientacao::decodificar_de_pe(&bytes)
                .map_err(|e| format!("{nome} não é uma foto que o núcleo abre: {e}"))?
                .into_rgba8()
        };
        Ok(Foto {
            nome,
            bytes: Some(bytes),
            raw,
            rgba,
        })
    }

    /// Uma foto sintética de `largura × altura`, com textura que o JPEG não
    /// comprime de graça — para rodar sem arquivo nenhum.
    pub fn de_exemplo(largura: u32, altura: u32) -> Foto {
        let rgba = RgbaImage::from_fn(largura, altura, |x, y| {
            image::Rgba([
                (x.wrapping_mul(7) + y) as u8,
                (y.wrapping_mul(3) ^ x) as u8,
                ((x + y) / 5) as u8,
                255,
            ])
        });
        Foto {
            nome: "exemplo".into(),
            bytes: None,
            raw: false,
            rgba,
        }
    }

    /// "IMG_0001.CR3 — 6000 × 4000 (24,0 MP), RAW".
    pub fn descricao(&self) -> String {
        let (w, h) = self.rgba.dimensions();
        let mp = w as f64 * h as f64 / 1e6;
        let tipo = if self.raw { ", RAW" } else { "" };
        let mp = format!("{mp:.1}").replace('.', ",");
        format!("{} — {w} × {h} ({mp} MP){tipo}", self.nome)
    }
}
