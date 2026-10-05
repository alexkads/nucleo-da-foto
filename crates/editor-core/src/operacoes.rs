//! O que se faz de uma vez numa camada: apagar e preencher a seleção, mesclar
//! uma camada na de baixo e a miniatura do painel.
//!
//! 🔑 **Todas devolvem uma [`Mudanca`]** — o mesmo passo do desfazer que o
//! traço do pincel produz, com o tile de antes e o de depois. O histórico, a
//! gravação e a coleta já sabem lidar com ele.

use std::sync::Arc;

use crate::documento::Camada;
use crate::mesclagem::mesclar_em_camada;
use crate::pincel::Mudanca;
use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{
    indice, retangulo_do_tile, CamadaDePixels, Posicao, Tile, BYTES_DO_TILE, LADO_DO_TILE,
};

/// Refaz cada tile de `posicoes` com `fazer(posicao, tile_de_antes)` e devolve
/// o que mudou. `fazer` devolve o tile novo (ou `None` para "não muda").
fn refazer_tiles(
    camada: &mut CamadaDePixels,
    posicoes: impl IntoIterator<Item = Posicao>,
    mut fazer: impl FnMut(Posicao, Option<&Tile>) -> Option<Vec<u8>>,
) -> Option<Mudanca> {
    let mut antes = Vec::new();
    let mut depois = Vec::new();
    for posicao in posicoes {
        let velho = camada.tile(posicao).cloned();
        let Some(novo) = fazer(posicao, velho.as_ref()) else {
            continue;
        };
        let novo: Option<Tile> = novo
            .iter()
            .skip(3)
            .step_by(4)
            .any(|a| *a != 0)
            .then(|| Arc::new(novo));
        let igual = match (&velho, &novo) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if igual {
            continue;
        }
        camada.definir(posicao, novo.clone());
        antes.push((posicao, velho));
        depois.push((posicao, novo));
    }
    (!antes.is_empty()).then_some(Mudanca { antes, depois })
}

/// A máscara do pixel `(lx, ly)` de um tile.
#[inline]
fn mascara(m: &Result<&[u8], u8>, lx: u32, ly: u32) -> u8 {
    match m {
        Ok(t) => t[(ly * LADO_DO_TILE + lx) as usize],
        Err(v) => *v,
    }
}

/// Apaga a camada onde a seleção está (Delete): o alfa cai na proporção da
/// máscara, a cor fica — como a borracha.
pub fn apagar(camada: &mut CamadaDePixels, selecao: &Selecao) -> Option<Mudanca> {
    let posicoes: Vec<Posicao> = camada.existentes().map(|(p, _)| *p).collect();
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let velho = velho?;
        let m = selecao.do_tile(posicao);
        if let Err(0) = m {
            return None;
        }
        let mut novo = velho.as_ref().clone();
        for ly in 0..LADO_DO_TILE {
            for lx in 0..LADO_DO_TILE {
                let v = mascara(&m, lx, ly) as u32;
                if v == 0 {
                    continue;
                }
                let i = indice(lx, ly) + 3;
                novo[i] = ((novo[i] as u32 * (255 - v) + 127) / 255) as u8;
            }
        }
        Some(novo)
    })
}

/// Preenche a seleção com uma cor (⌥Delete), por cima do que a camada tem —
/// sem seleção, a camada inteira.
pub fn preencher(
    camada: &mut CamadaDePixels,
    selecao: Option<&Selecao>,
    cor: [u8; 3],
) -> Option<Mudanca> {
    let (largura, altura) = (camada.largura(), camada.altura());
    let area = selecao.map_or(Retangulo::inteiro(largura, altura), Selecao::limites);
    let posicoes = camada.tiles_do_retangulo(&area);
    let pincel = crate::pincel::Pincel {
        cor,
        opacidade: 1.0,
        ..Default::default()
    };
    refazer_tiles(camada, posicoes, |posicao, velho| {
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        if let Err(0) = m {
            return None;
        }
        let pedaco = retangulo_do_tile(posicao, largura, altura);
        let mut novo = velho.map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        for ly in 0..pedaco.altura {
            for lx in 0..pedaco.largura {
                let v = mascara(&m, lx, ly);
                if v == 0 {
                    continue;
                }
                let i = indice(lx, ly);
                let antes = [novo[i], novo[i + 1], novo[i + 2], novo[i + 3]];
                novo[i..i + 4].copy_from_slice(&crate::pincel::aplicar(&pincel, antes, v));
            }
        }
        Some(novo)
    })
}

/// Mescla `cima` em `baixo` (⌘E), com o modo e a opacidade de `cima`. A de
/// baixo guarda o modo e a opacidade dela, como no Photoshop.
pub fn mesclar_na_de_baixo(baixo: &mut CamadaDePixels, cima: &Camada) -> Option<Mudanca> {
    let posicoes: Vec<Posicao> = cima.pixels.existentes().map(|(p, _)| *p).collect();
    refazer_tiles(baixo, posicoes, |posicao, velho| {
        let de_cima = cima.pixels.tile(posicao)?;
        let mut novo = velho.map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        for k in (0..BYTES_DO_TILE).step_by(4) {
            let c = [de_cima[k], de_cima[k + 1], de_cima[k + 2], de_cima[k + 3]];
            if c[3] == 0 {
                continue;
            }
            let b = [novo[k], novo[k + 1], novo[k + 2], novo[k + 3]];
            novo[k..k + 4].copy_from_slice(&mesclar_em_camada(b, c, cima.opacidade, cima.modo));
        }
        Some(novo)
    })
}

/// A miniatura da camada para o painel, em RGBA de `largura × altura`, pelo
/// pixel mais próximo (são umas mil leituras). O transparente fica
/// transparente: quem desenha põe o xadrez por baixo.
pub fn miniatura(camada: &CamadaDePixels, largura: u32, altura: u32) -> Vec<u8> {
    let (lf, af) = (camada.largura().max(1), camada.altura().max(1));
    let mut saida = Vec::with_capacity((largura * altura * 4) as usize);
    for y in 0..altura {
        for x in 0..largura {
            let fx = ((x as u64 * 2 + 1) * lf as u64 / (largura as u64 * 2)) as u32;
            let fy = ((y as u64 * 2 + 1) * af as u64 / (altura as u64 * 2)) as u32;
            saida.extend_from_slice(&camada.pixel(fx.min(lf - 1), fy.min(af - 1)));
        }
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::mesclagem::Modo;
    use crate::selecao::Forma;

    fn cheia(largura: u32, altura: u32, cor: [u8; 4]) -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(largura, altura);
        for p in c.tiles_do_retangulo(&Retangulo::inteiro(largura, altura)) {
            let t = c.tile_mut(p);
            for k in (0..t.len()).step_by(4) {
                t[k..k + 4].copy_from_slice(&cor);
            }
        }
        c
    }

    #[test]
    fn apagar_tira_so_o_que_esta_selecionado() {
        let mut c = cheia(600, 400, [10, 20, 30, 255]);
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(100, 100, 50, 50)),
        );
        let m = apagar(&mut c, &s).unwrap();
        assert_eq!(c.pixel(120, 120)[3], 0);
        assert_eq!(c.pixel(99, 120), [10, 20, 30, 255]);
        assert_eq!(m.antes.len(), 1, "um tile mexido");
        // Apagar de novo não muda nada.
        assert!(apagar(&mut c, &s).is_none());
    }

    #[test]
    fn preencher_pinta_a_selecao_e_sem_selecao_a_camada_toda() {
        let mut c = CamadaDePixels::nova(600, 400);
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Elipse(Retangulo::novo(300, 100, 200, 200)),
        );
        preencher(&mut c, Some(&s), [255, 0, 0]).unwrap();
        assert_eq!(c.pixel(400, 200), [255, 0, 0, 255]);
        assert_eq!(c.pixel(305, 105)[3], 0, "fora da elipse");
        preencher(&mut c, None, [0, 0, 255]).unwrap();
        assert_eq!(c.pixel(5, 5), [0, 0, 255, 255]);
        assert_eq!(c.pixel(599, 399), [0, 0, 255, 255]);
    }

    #[test]
    fn mesclar_na_de_baixo_compoe_igual_as_duas_separadas() {
        use crate::composicao::compor;
        use crate::documento::{BaseRef, Documento};
        let base = image::RgbImage::from_fn(300, 300, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 77])
        });
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        doc.camadas[0].pixels = cheia(300, 300, [200, 100, 50, 255]);
        let mut cima = Camada::nova("Camada 1", 300, 300);
        cima.pixels = cheia(300, 300, [40, 200, 90, 180]);
        cima.modo = Modo::Multiplicacao;
        cima.opacidade = 0.6;
        doc.camadas.push(cima.clone());
        let separadas = compor(&base, &doc);

        let mut juntas = doc.clone();
        let de_cima = juntas.camadas.pop().unwrap();
        mesclar_na_de_baixo(&mut juntas.camadas[0].pixels, &de_cima).unwrap();
        let mescladas = compor(&base, &juntas);
        for (a, b) in separadas.as_raw().iter().zip(mescladas.as_raw()) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "{a} × {b}");
        }
    }

    #[test]
    fn a_miniatura_pega_o_pixel_mais_proximo() {
        let mut c = CamadaDePixels::nova(400, 200);
        c.tile_mut((1, 0))[..4].copy_from_slice(&[1, 2, 3, 4]);
        let m = miniatura(&c, 4, 2);
        assert_eq!(m.len(), 4 * 2 * 4);
        assert_eq!(&m[..4], &[0, 0, 0, 0]);
    }
}
