//! A fonte do carimbo: de onde o pincel copia.
//!
//! O carimbo do Photoshop amostra, por padrão nos retoques, **"a camada atual e
//! as de baixo"**: a foto como ela aparece até a camada escolhida. É o que deixa
//! retocar numa camada vazia por cima, sem tocar a base — e desfazer o retoque
//! apagando a camada.
//!
//! 🔑 **Um instantâneo do começo do traço.** O documento é copiado (barato: os
//! tiles são `Arc`) quando o ponteiro desce, e a fonte não enxerga o que o
//! próprio traço pinta — sem isso, passar por cima da origem copiaria a cópia.
//!
//! 🔑 **Composta por tile, sob demanda.** Compor a foto inteira a cada traço
//! seriam 65 ms numa foto de 24 MP; o carimbo só lê os tiles por onde a origem
//! passa, e cada um é composto uma vez por traço.

use std::collections::BTreeMap;
use std::sync::Arc;

use image::RgbImage;

use crate::composicao;
use crate::documento::Documento;
use crate::retangulo::Retangulo;
use crate::tiles::{retangulo_do_tile, Posicao, LADO_DO_TILE};

pub struct Fonte {
    base: Arc<RgbImage>,
    doc: Documento,
    /// Da posição pintada à posição copiada, em pixels da foto.
    deslocamento: (i64, i64),
    tiles: BTreeMap<Posicao, RgbImage>,
    /// Os mesmos tiles, desfocados — o desfoque e a nitidez leem daqui.
    suaves: BTreeMap<Posicao, RgbImage>,
}

/// O raio do desfoque da fonte: um binômio de 7 toques (≈ gaussiano de σ 1,2).
const RAIO_DO_DESFOQUE: u32 = 3;
const BINOMIO: [u32; 7] = [1, 6, 15, 20, 15, 6, 1];

impl Fonte {
    /// A foto composta da base até a camada `ate` (inclusive), deslocada de
    /// `deslocamento`: pintar em `(x, y)` copia `(x + dx, y + dy)`.
    pub fn nova(
        base: Arc<RgbImage>,
        doc: &Documento,
        ate: usize,
        deslocamento: (f32, f32),
    ) -> Self {
        let mut doc = doc.clone();
        doc.camadas.truncate(ate + 1);
        Self {
            base,
            doc,
            deslocamento: (deslocamento.0.round() as i64, deslocamento.1.round() as i64),
            tiles: BTreeMap::new(),
            suaves: BTreeMap::new(),
        }
    }

    /// A foto desfocada em `(x, y)` (sem deslocamento: o desfoque e a nitidez
    /// trabalham no lugar). `None` fora da foto.
    pub fn desfocada(&mut self, x: u32, y: u32) -> Option<[u8; 3]> {
        let (largura, altura) = (self.base.width(), self.base.height());
        if x >= largura || y >= altura {
            return None;
        }
        let posicao = (x / LADO_DO_TILE, y / LADO_DO_TILE);
        let tile = self.suaves.entry(posicao).or_insert_with(|| {
            // O tile com uma margem do raio, composto e desfocado em duas
            // passadas (linhas, depois colunas).
            let ret = retangulo_do_tile(posicao, largura, altura);
            let r = RAIO_DO_DESFOQUE;
            let x0 = ret.x.saturating_sub(r);
            let y0 = ret.y.saturating_sub(r);
            let x1 = (ret.direita() + r).min(largura);
            let y1 = (ret.baixo() + r).min(altura);
            let largo = Retangulo::novo(x0, y0, x1 - x0, y1 - y0);
            let foto = composicao::compor_recorte(&self.base, &self.doc, &largo);
            let (w, h) = (foto.width() as i64, foto.height() as i64);
            let passar = |img: &RgbImage, horizontal: bool| -> RgbImage {
                RgbImage::from_fn(img.width(), img.height(), |px, py| {
                    let mut soma = [0u32; 3];
                    let mut peso = 0u32;
                    for (k, b) in BINOMIO.iter().enumerate() {
                        let d = k as i64 - r as i64;
                        let (qx, qy) = if horizontal {
                            ((px as i64 + d).clamp(0, w - 1), py as i64)
                        } else {
                            (px as i64, (py as i64 + d).clamp(0, h - 1))
                        };
                        let p = img.get_pixel(qx as u32, qy as u32).0;
                        for i in 0..3 {
                            soma[i] += p[i] as u32 * b;
                        }
                        peso += b;
                    }
                    image::Rgb(soma.map(|v| ((v + peso / 2) / peso) as u8))
                })
            };
            let suave = passar(&passar(&foto, true), false);
            image::imageops::crop_imm(&suave, ret.x - x0, ret.y - y0, ret.largura, ret.altura)
                .to_image()
        });
        Some(tile.get_pixel(x % LADO_DO_TILE, y % LADO_DO_TILE).0)
    }

    /// A cor que vai para `(x, y)`; `None` quando a origem cai fora da foto.
    pub fn cor(&mut self, x: u32, y: u32) -> Option<[u8; 3]> {
        let (largura, altura) = (self.base.width() as i64, self.base.height() as i64);
        let (ox, oy) = (
            x as i64 + self.deslocamento.0,
            y as i64 + self.deslocamento.1,
        );
        if ox < 0 || oy < 0 || ox >= largura || oy >= altura {
            return None;
        }
        let (ox, oy) = (ox as u32, oy as u32);
        let posicao = (ox / LADO_DO_TILE, oy / LADO_DO_TILE);
        let tile = self.tiles.entry(posicao).or_insert_with(|| {
            let ret: Retangulo = retangulo_do_tile(posicao, self.base.width(), self.base.height());
            composicao::compor_recorte(&self.base, &self.doc, &ret)
        });
        Some(tile.get_pixel(ox % LADO_DO_TILE, oy % LADO_DO_TILE).0)
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::documento::{BaseRef, Camada};

    #[test]
    fn copia_a_foto_ate_a_camada_e_nada_de_fora() {
        let base = Arc::new(RgbImage::from_fn(600, 400, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 9])
        }));
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        doc.camadas[0].pixels.tile_mut((1, 0))[..4].copy_from_slice(&[1, 2, 3, 255]);
        let mut de_cima = Camada::nova("Camada 1", 600, 400);
        de_cima.pixels.tile_mut((0, 0))[..4].copy_from_slice(&[200, 200, 200, 255]);
        doc.camadas.push(de_cima);

        let mut f = Fonte::nova(base.clone(), &doc, 0, (256.0, 0.0));
        assert_eq!(f.cor(0, 0), Some([1, 2, 3]), "a camada 0 entra");
        assert_eq!(f.cor(10, 5), Some([10, 5, 9]));
        assert_eq!(f.cor(400, 0), None, "a origem passa da borda");

        // Até a camada 1, o pixel pintado nela aparece.
        let mut f = Fonte::nova(base, &doc, 1, (-100.0, 0.0));
        assert_eq!(f.cor(100, 0), Some([200, 200, 200]));
    }

    #[test]
    fn a_fonte_desfocada_suaviza_e_atravessa_a_emenda_dos_tiles() {
        // Listras de 1 px: desfocadas, viram cinza no meio.
        let base = Arc::new(RgbImage::from_fn(600, 300, |x, _| {
            if x % 2 == 0 {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([0, 0, 0])
            }
        }));
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        let mut f = Fonte::nova(base, &doc, 0, (0.0, 0.0));
        for x in [10, 255, 256, 257] {
            let p = f.desfocada(x, 100).unwrap();
            assert!((120..=135).contains(&p[0]), "x={x}: {p:?}");
        }
        assert_eq!(f.desfocada(600, 0), None);
    }
}
