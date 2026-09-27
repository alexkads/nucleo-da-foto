//! A composição: a base com as camadas por cima.
//!
//! ```text
//! a   = alfa_do_pixel / 255 · opacidade_da_camada
//! out = base + (cor − base) · a
//! ```
//!
//! 🔑 **Com `a = 0` a conta devolve a base exata** — não "quase": é o C30, uma
//! camada vazia compõe a base byte a byte, e a Revelação da imagem editada é a
//! mesma do bruto. Por isso o caminho de `a = 0` sai antes da conta em ponto
//! flutuante.

use image::RgbImage;

use crate::documento::{Camada, Documento};
use crate::retangulo::Retangulo;
use crate::tiles::{indice, retangulo_do_tile, CamadaDePixels, LADO_DO_TILE};

/// Um pixel da camada sobre um pixel de baixo.
#[inline]
pub fn sobre(baixo: [u8; 3], cima: [u8; 4], opacidade: f32) -> [u8; 3] {
    if cima[3] == 0 || opacidade <= 0.0 {
        return baixo;
    }
    let a = cima[3] as f32 / 255.0 * opacidade.min(1.0);
    let mistura = |b: u8, c: u8| -> u8 {
        let b = b as f32;
        (b + (c as f32 - b) * a).round().clamp(0.0, 255.0) as u8
    };
    [
        mistura(baixo[0], cima[0]),
        mistura(baixo[1], cima[1]),
        mistura(baixo[2], cima[2]),
    ]
}

/// A imagem editada em resolução cheia.
pub fn compor(base: &RgbImage, doc: &Documento) -> RgbImage {
    let mut saida = base.clone();
    compor_regiao(
        base,
        doc,
        &Retangulo::inteiro(base.width(), base.height()),
        &mut saida,
    );
    saida
}

/// Recompõe só `ret` dentro de `saida` (do tamanho da base).
///
/// 🔑 **Anda por tile, e não por pixel**: a camada é um mapa de tiles, e
/// perguntar por cada um dos 24 milhões de pixels de uma foto grande seriam 24
/// milhões de buscas no mapa. Aqui é uma busca por tile e por camada.
pub fn compor_regiao(base: &RgbImage, doc: &Documento, ret: &Retangulo, saida: &mut RgbImage) {
    let largura_da_saida = saida.width();
    compor_deslocado(base, doc, ret, saida, largura_da_saida, (0, 0));
}

/// Só o recorte `ret`, numa imagem do tamanho dele — a vista usa isto para não
/// alocar a foto inteira a cada gesto.
pub fn compor_recorte(base: &RgbImage, doc: &Documento, ret: &Retangulo) -> RgbImage {
    let ret = ret.limitado(base.width(), base.height());
    let mut saida = RgbImage::new(ret.largura, ret.altura);
    compor_deslocado(base, doc, &ret, &mut saida, ret.largura, (ret.x, ret.y));
    saida
}

/// Compõe `ret` e escreve em `destino`, onde o pixel `(x, y)` da foto mora em
/// `(x − origem.0, y − origem.1)`.
fn compor_deslocado(
    base: &RgbImage,
    doc: &Documento,
    ret: &Retangulo,
    destino: &mut [u8],
    largura_do_destino: u32,
    origem: (u32, u32),
) {
    let (largura, altura) = (base.width(), base.height());
    let ret = ret.limitado(largura, altura);
    if ret.vazio() {
        return;
    }
    let destino_em_bytes = largura_do_destino as usize * 3;
    let camadas: Vec<&Camada> = doc.camadas.iter().filter(|c| !c.sem_efeito()).collect();
    let referencia = CamadaDePixels::nova(largura, altura);
    let fonte = base.as_raw();
    let largura_em_bytes = largura as usize * 3;
    for posicao in referencia.tiles_do_retangulo(&ret) {
        let pedaco = retangulo_do_tile(posicao, largura, altura).limitado(largura, altura);
        let pedaco = interseccao(&pedaco, &ret);
        let tiles: Vec<(&[u8], f32)> = camadas
            .iter()
            .filter_map(|c| c.pixels.tile(posicao).map(|t| (t.as_slice(), c.opacidade)))
            .collect();
        for y in pedaco.y..pedaco.baixo() {
            let inicio = y as usize * largura_em_bytes + pedaco.x as usize * 3;
            let fim = inicio + pedaco.largura as usize * 3;
            let d_inicio =
                (y - origem.1) as usize * destino_em_bytes + (pedaco.x - origem.0) as usize * 3;
            if tiles.is_empty() {
                destino[d_inicio..d_inicio + (fim - inicio)].copy_from_slice(&fonte[inicio..fim]);
                continue;
            }
            let ty = y % LADO_DO_TILE;
            for x in pedaco.x..pedaco.direita() {
                let i = y as usize * largura_em_bytes + x as usize * 3;
                let mut pixel = [fonte[i], fonte[i + 1], fonte[i + 2]];
                let j = indice(x % LADO_DO_TILE, ty);
                for (tile, opacidade) in &tiles {
                    pixel = sobre(
                        pixel,
                        [tile[j], tile[j + 1], tile[j + 2], tile[j + 3]],
                        *opacidade,
                    );
                }
                let d = d_inicio + (x - pedaco.x) as usize * 3;
                destino[d..d + 3].copy_from_slice(&pixel);
            }
        }
    }
}

/// A parte comum de dois retângulos.
pub fn interseccao(a: &Retangulo, b: &Retangulo) -> Retangulo {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Retangulo::novo(
        x,
        y,
        a.direita().min(b.direita()).saturating_sub(x),
        a.baixo().min(b.baixo()).saturating_sub(y),
    )
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::documento::BaseRef;

    fn base() -> RgbImage {
        RgbImage::from_fn(300, 280, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 7) as u8])
        })
    }

    #[test]
    fn camada_vazia_compoe_a_base_byte_a_byte() {
        let base = base();
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        assert!(doc.neutro());
        assert_eq!(compor(&base, &doc).as_raw(), base.as_raw());
    }

    #[test]
    fn visibilidade_e_opacidade_entram_na_composicao() {
        let base = base();
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let tile = doc.camadas[0].pixels.tile_mut((0, 0));
        tile[..4].copy_from_slice(&[255, 0, 0, 255]);
        let cheia = compor(&base, &doc);
        assert_eq!(cheia.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(cheia.get_pixel(1, 0).0, base.get_pixel(1, 0).0);

        doc.camadas[0].opacidade = 0.5;
        let meia = compor(&base, &doc).get_pixel(0, 0).0;
        assert_eq!(meia, [128, 0, 0], "metade do vermelho sobre (0,0,0)");

        doc.camadas[0].visivel = false;
        assert!(doc.neutro());
        assert_eq!(compor(&base, &doc).as_raw(), base.as_raw());

        doc.camadas[0].visivel = true;
        doc.camadas[0].opacidade = 0.0;
        assert_eq!(compor(&base, &doc).as_raw(), base.as_raw());
    }

    #[test]
    fn a_composicao_de_uma_regiao_e_a_mesma_da_foto_inteira() {
        let base = base();
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let tile = doc.camadas[0].pixels.tile_mut((1, 1));
        for i in 0..tile.len() / 4 {
            tile[i * 4..i * 4 + 4].copy_from_slice(&[(i % 256) as u8, 40, 200, (i % 200) as u8]);
        }
        let inteira = compor(&base, &doc);
        let mut parcial = base.clone();
        compor_regiao(
            &base,
            &doc,
            &Retangulo::novo(250, 250, 50, 30),
            &mut parcial,
        );
        let recorte = compor_recorte(&base, &doc, &Retangulo::novo(240, 245, 60, 35));
        for y in 250..280 {
            for x in 250..300 {
                assert_eq!(parcial.get_pixel(x, y), inteira.get_pixel(x, y));
                assert_eq!(recorte.get_pixel(x - 240, y - 245), inteira.get_pixel(x, y));
            }
        }
    }
}
