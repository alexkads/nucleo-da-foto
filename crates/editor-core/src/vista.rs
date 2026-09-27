//! A vista: a imagem editada reduzida para a tela, refeita só onde sujou.
//!
//! 🔑 **O traço pinta em resolução cheia e a tela vê uma redução.** Numa foto de
//! 24 MP, recompor e reenviar a foto inteira a cada movimento do ponteiro seria
//! refazer 24 milhões de pixels por evento. Aqui cada gesto devolve o retângulo
//! que sujou; a vista recompõe **só os pixels da tela** que caem nele, e marca
//! os ladrilhos da tela (de [`LADO_DO_LADRILHO`] px) que precisam subir de novo
//! para a GPU. É o `dirty_rect` + `set_partial` do PaintFE, em ladrilhos.
//!
//! A redução é por um fator inteiro, pela média da caixa `fator × fator` da
//! **imagem composta** — o que a tela mostra é a imagem editada reduzida, e não
//! uma aproximação dela.

use std::collections::BTreeSet;

use image::RgbImage;

use crate::composicao;
use crate::documento::Documento;
use crate::retangulo::Retangulo;

/// O lado dos ladrilhos da tela, em pixels da vista.
pub const LADO_DO_LADRILHO: u32 = 256;

pub struct Vista {
    fator: u32,
    imagem: RgbImage,
    sujos: BTreeSet<(u32, u32)>,
}

impl Vista {
    /// A vista inteira, com o maior lado até `lado_maximo`.
    pub fn nova(base: &RgbImage, doc: &Documento, lado_maximo: u32) -> Self {
        let maior = base.width().max(base.height()).max(1);
        let fator = maior.div_ceil(lado_maximo.max(1)).max(1);
        let largura = base.width().div_ceil(fator).max(1);
        let altura = base.height().div_ceil(fator).max(1);
        let mut vista = Self {
            fator,
            imagem: RgbImage::new(largura, altura),
            sujos: BTreeSet::new(),
        };
        vista.refazer(base, doc, &Retangulo::inteiro(base.width(), base.height()));
        vista
    }

    /// Quantos pixels da foto cada pixel da vista cobre, por lado.
    pub fn fator(&self) -> u32 {
        self.fator
    }

    pub fn imagem(&self) -> &RgbImage {
        &self.imagem
    }

    pub fn colunas(&self) -> u32 {
        self.imagem.width().div_ceil(LADO_DO_LADRILHO)
    }

    pub fn linhas(&self) -> u32 {
        self.imagem.height().div_ceil(LADO_DO_LADRILHO)
    }

    /// Refaz a vista onde a foto mudou em `ret` (pixels da foto).
    pub fn refazer(&mut self, base: &RgbImage, doc: &Documento, ret: &Retangulo) {
        let (largura, altura) = (base.width(), base.height());
        let ret = ret.limitado(largura, altura);
        if ret.vazio() {
            return;
        }
        let f = self.fator;
        // Os pixels da vista que tocam o retângulo, e a região da foto que eles
        // cobrem inteira (alinhada ao fator).
        let (vx0, vy0) = (ret.x / f, ret.y / f);
        let (vx1, vy1) = (
            ret.direita().div_ceil(f).min(self.imagem.width()),
            ret.baixo().div_ceil(f).min(self.imagem.height()),
        );
        let regiao = Retangulo::novo(vx0 * f, vy0 * f, (vx1 - vx0) * f, (vy1 - vy0) * f)
            .limitado(largura, altura);
        let composta = composicao::compor_recorte(base, doc, &regiao);
        for vy in vy0..vy1 {
            for vx in vx0..vx1 {
                let mut soma = [0u32; 3];
                let mut n = 0u32;
                for y in (vy * f)..((vy + 1) * f).min(altura) {
                    for x in (vx * f)..((vx + 1) * f).min(largura) {
                        let p = composta.get_pixel(x - regiao.x, y - regiao.y).0;
                        soma[0] += p[0] as u32;
                        soma[1] += p[1] as u32;
                        soma[2] += p[2] as u32;
                        n += 1;
                    }
                }
                let n = n.max(1);
                self.imagem.put_pixel(
                    vx,
                    vy,
                    image::Rgb([
                        ((soma[0] + n / 2) / n) as u8,
                        ((soma[1] + n / 2) / n) as u8,
                        ((soma[2] + n / 2) / n) as u8,
                    ]),
                );
            }
        }
        for ly in (vy0 / LADO_DO_LADRILHO)..=((vy1 - 1) / LADO_DO_LADRILHO) {
            for lx in (vx0 / LADO_DO_LADRILHO)..=((vx1 - 1) / LADO_DO_LADRILHO) {
                self.sujos.insert((lx, ly));
            }
        }
    }

    /// Os ladrilhos que mudaram desde a última pergunta — e esquece-os.
    pub fn levar_os_sujos(&mut self) -> Vec<(u32, u32)> {
        std::mem::take(&mut self.sujos).into_iter().collect()
    }

    /// Todos os ladrilhos sujos de novo (a janela recriou as imagens).
    pub fn sujar_tudo(&mut self) {
        for ly in 0..self.linhas() {
            for lx in 0..self.colunas() {
                self.sujos.insert((lx, ly));
            }
        }
    }

    /// O retângulo (em pixels da vista) de um ladrilho.
    pub fn retangulo_do_ladrilho(&self, ladrilho: (u32, u32)) -> Retangulo {
        Retangulo::novo(
            ladrilho.0 * LADO_DO_LADRILHO,
            ladrilho.1 * LADO_DO_LADRILHO,
            LADO_DO_LADRILHO,
            LADO_DO_LADRILHO,
        )
        .limitado(self.imagem.width(), self.imagem.height())
    }

    /// Os pixels de um ladrilho em **BGRA** — a ordem que o GPUI espera
    /// (`ui-gpui/src/imagem.rs`) — e o tamanho dele.
    pub fn ladrilho_bgra(&self, ladrilho: (u32, u32)) -> (u32, u32, Vec<u8>) {
        let ret = self.retangulo_do_ladrilho(ladrilho);
        let mut bytes = Vec::with_capacity((ret.largura * ret.altura * 4) as usize);
        for y in ret.y..ret.baixo() {
            for x in ret.x..ret.direita() {
                let p = self.imagem.get_pixel(x, y).0;
                bytes.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        (ret.largura, ret.altura, bytes)
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::documento::BaseRef;
    use crate::pincel::{Pincel, Traco};

    #[test]
    fn a_vista_e_a_imagem_editada_reduzida_e_so_suja_onde_mudou() {
        let base = RgbImage::from_fn(1200, 900, |x, y| {
            image::Rgb([(x / 5) as u8, (y / 4) as u8, 90])
        });
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let mut vista = Vista::nova(&base, &doc, 400);
        assert_eq!(vista.fator(), 3);
        assert_eq!(
            (vista.imagem().width(), vista.imagem().height()),
            (400, 300)
        );
        vista.levar_os_sujos();

        let mut traco = Traco::novo(Pincel {
            raio: 30.0,
            dureza: 1.0,
            cor: [255, 255, 255],
            ..Pincel::default()
        });
        let sujo = traco.ate(&mut doc.camadas[0].pixels, 900.0, 150.0);
        traco.terminar(&mut doc.camadas[0].pixels);
        vista.refazer(&base, &doc, &sujo);
        assert_eq!(
            vista.levar_os_sujos(),
            vec![(1, 0)],
            "só o ladrilho do traço"
        );

        // A vista inteira refeita do zero é igual à refeita aos pedaços.
        let do_zero = Vista::nova(&base, &doc, 400);
        assert_eq!(do_zero.imagem().as_raw(), vista.imagem().as_raw());
        assert_eq!(vista.imagem().get_pixel(300, 50).0, [255, 255, 255]);
    }

    #[test]
    fn o_ladrilho_sai_em_bgra() {
        let base = RgbImage::from_pixel(10, 10, image::Rgb([1, 2, 3]));
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        let vista = Vista::nova(&base, &doc, 100);
        let (l, a, bytes) = vista.ladrilho_bgra((0, 0));
        assert_eq!((l, a), (10, 10));
        assert_eq!(&bytes[..4], &[3, 2, 1, 255]);
    }
}
