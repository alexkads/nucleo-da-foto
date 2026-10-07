//! A composição: a base com as camadas por cima, de baixo para cima, cada uma
//! no seu modo de mesclagem (`mesclagem.rs`).
//!
//! ```text
//! a   = alfa_do_pixel / 255 · opacidade_da_camada
//! out = baixo + (B(baixo, cor) − baixo) · a        (Normal: B = cor)
//! ```
//!
//! 🔑 **Com `a = 0` a conta devolve a base exata** — não "quase": é o C30, uma
//! camada vazia compõe a base byte a byte, e a Revelação da imagem editada é a
//! mesma do bruto. Por isso o caminho de `a = 0` sai antes da conta em ponto
//! flutuante.

use image::RgbImage;

use crate::ajuste::{Ajuste, Preparado};
use crate::documento::{Camada, Documento, Mascara};
use crate::mesclagem::{mesclar, Modo};
use crate::retangulo::Retangulo;
use crate::tiles::{indice, retangulo_do_tile, CamadaDePixels, LADO_DO_TILE};

/// Um pixel da camada sobre um pixel de baixo, no modo Normal.
#[inline]
pub fn sobre(baixo: [u8; 3], cima: [u8; 4], opacidade: f32) -> [u8; 3] {
    mesclar(baixo, cima, opacidade, Modo::Normal)
}

/// Um tile de camada na composição: os pixels, a opacidade (já com a máscara
/// quando ela é lisa no tile), o modo, e a máscara do tile com o fundo dela.
///
/// `None` nos pixels = uma camada de ajuste: a cor de cima é a de baixo
/// ajustada.
type TileNaComposicao<'a> = (
    Option<&'a [u8]>,
    Option<&'a Preparado>,
    f32,
    Modo,
    Option<(&'a [u8], u8)>,
);

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
    // A conta de cada ajuste, montada uma vez (as tabelas de 256).
    let preparados: Vec<Option<Preparado>> = camadas
        .iter()
        .map(|c| c.ajuste.as_ref().map(Ajuste::preparar))
        .collect();
    let referencia = CamadaDePixels::nova(largura, altura);
    let fonte = base.as_raw();
    let largura_em_bytes = largura as usize * 3;
    for posicao in referencia.tiles_do_retangulo(&ret) {
        let pedaco = retangulo_do_tile(posicao, largura, altura).limitado(largura, altura);
        let pedaco = interseccao(&pedaco, &ret);
        // A máscara de cada camada neste tile: sem tile pintado nela, o tile
        // inteiro vale o fundo — e entra na opacidade de uma vez.
        let tiles: Vec<TileNaComposicao> = camadas
            .iter()
            .zip(&preparados)
            .filter_map(|(c, preparado)| {
                let t = match preparado {
                    Some(_) => None,
                    None => Some(c.pixels.tile(posicao)?.as_slice()),
                };
                let p = preparado.as_ref();
                match c.mascara_ativa() {
                    None => Some((t, p, c.opacidade, c.modo, None)),
                    Some(m) => match m.pixels.tile(posicao) {
                        Some(mt) => {
                            Some((t, p, c.opacidade, c.modo, Some((mt.as_slice(), m.fundo))))
                        }
                        None if m.fundo == 0 => None,
                        None => Some((t, p, c.opacidade * m.fundo as f32 / 255.0, c.modo, None)),
                    },
                }
            })
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
                for (tile, preparado, opacidade, modo, mascara) in &tiles {
                    let opacidade = match mascara {
                        None => *opacidade,
                        Some((m, fundo)) => {
                            let v = Mascara::valor_do_pixel(
                                *fundo,
                                [m[j], m[j + 1], m[j + 2], m[j + 3]],
                            );
                            if v == 0 {
                                continue;
                            }
                            *opacidade * v as f32 / 255.0
                        }
                    };
                    let cima = match (tile, preparado) {
                        (Some(t), _) => [t[j], t[j + 1], t[j + 2], t[j + 3]],
                        (None, Some(p)) => {
                            let [r, g, b] = p.aplicar(pixel);
                            [r, g, b, 255]
                        }
                        (None, None) => continue,
                    };
                    pixel = mesclar(pixel, cima, opacidade, *modo);
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
    fn as_camadas_compoem_de_baixo_para_cima_cada_uma_no_seu_modo() {
        let base = RgbImage::from_pixel(10, 10, image::Rgb([200, 100, 50]));
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        doc.camadas[0].pixels.tile_mut((0, 0))[..4].copy_from_slice(&[0, 0, 255, 255]);
        let mut de_cima = Camada::nova("Camada 1", 10, 10);
        de_cima.pixels.tile_mut((0, 0))[..4].copy_from_slice(&[255, 255, 255, 255]);
        de_cima.modo = Modo::Multiplicacao;
        doc.camadas.push(de_cima);
        // Branco em Multiplicação não muda o azul de baixo.
        assert_eq!(compor(&base, &doc).get_pixel(0, 0).0, [0, 0, 255]);
        // Em Normal, a de cima vence.
        doc.camadas[1].modo = Modo::Normal;
        assert_eq!(compor(&base, &doc).get_pixel(0, 0).0, [255, 255, 255]);
        // Trocadas de lugar, o azul vence.
        doc.camadas.swap(0, 1);
        assert_eq!(compor(&base, &doc).get_pixel(0, 0).0, [0, 0, 255]);
        assert_eq!(compor(&base, &doc).get_pixel(1, 0).0, [200, 100, 50]);
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
