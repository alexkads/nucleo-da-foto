//! O que os testes de mais de um módulo usam.

use image::RgbImage;

use crate::documento::{BaseRef, Documento};

/// Um documento que passa por todo caminho da composição: camada com
/// alfa parcial, máscara pintada e difusa, modo de mescla, opacidade, uma
/// recortada nela e uma camada de ajuste por cima.
pub(crate) fn documento_completo() -> (RgbImage, Documento) {
    documento_completo_de(700, 520)
}

/// [`documento_completo`] de `w × h` (a máscara e o alfa parcial ficam nos
/// tiles (0, 0) e (1, 1): `w` e `h` a partir de 512).
pub(crate) fn documento_completo_de(w: u32, h: u32) -> (RgbImage, Documento) {
    use crate::documento::{Camada, Mascara};
    use crate::tiles::CamadaDePixels;
    let base = RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([
            (x * 3 % 256) as u8,
            (y * 5 % 256) as u8,
            ((x ^ y) % 256) as u8,
        ])
    });
    let mut doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut a = Camada::nova("A", w, h);
    a.pixels = CamadaDePixels::da_imagem(&RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(y % 256) as u8, 200, (x % 256) as u8])
    }));
    for (k, p) in a
        .pixels
        .tile_mut((1, 1))
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        p[3] = (k % 256) as u8; // alfa parcial num tile
    }
    let mut mascara = Mascara::nova(255, w, h);
    for (k, p) in mascara
        .pixels
        .tile_mut((0, 0))
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        p.copy_from_slice(&[(k / 256 % 256) as u8, 0, 0, 255]);
    }
    mascara.difusao = 6.0;
    a.mascara = Some(mascara);
    a.opacidade = 0.8;
    a.modo = crate::mesclagem::Modo::Multiplicacao;
    doc.camadas.push(a);
    let mut b = Camada::nova("B", w, h);
    b.pixels = CamadaDePixels::da_imagem(&RgbImage::from_pixel(w, h, image::Rgb([0, 90, 255])));
    b.recortada = true;
    b.opacidade = 0.5;
    doc.camadas.push(b);
    doc.camadas.push(Camada::de_ajuste(
        "Níveis",
        crate::Ajuste::Niveis {
            preto: 12.0,
            gama: 1.4,
            branco: 235.0,
        },
        w,
        h,
    ));
    (base, doc)
}
