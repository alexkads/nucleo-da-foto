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
}

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
        }
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
}
