//! 🧴 A separação de frequências do tratamento de pele: a foto `I` vira a
//! **baixa** `L` (cor e tom, um desfoque) e a **alta** `H` (textura, o que o
//! desfoque tirou), em duas camadas que recompõem `I` — e cada uma se retoca
//! sem mexer na outra.
//!
//! ```text
//! L  = desfoque gaussiano de I (σ = raio, em pixels da foto)
//! D  = I − L                          o detalhe, em −255..=255
//! H  = 0,5 + D / 2                    em 8 bits: h = (I − L + 255) / 2
//! R  = L + 2H − 1  (Luz Linear)       em 8 bits: r = L + 2h − 255 = I
//! ```
//!
//! A extração divide por dois e a Luz Linear multiplica por dois — a mesma
//! escala nos dois lados. O filtro "Alta frequência" (`I − L + 128`, sem a
//! divisão) **não** recompõe pela Luz Linear: dobra o detalhe (o teste
//! `o_filtro_alta_frequencia_nao_recompoe_pela_luz_linear` mede).
//!
//! ## Precisão em 8 bits
//!
//! Os tiles são RGBA de 8 bits (C29). Com `L` só arredondado, `I − L` par dá
//! `h` com meio nível, e a recomposição erra 1 nível em cerca de metade dos
//! pixels (medido em `o_arredondamento_simples_erra_um_nivel_em_metade_dos_pixels`).
//! 🔑 **Aqui a baixa é arredondada para o inteiro mais perto do desfoque com
//! `I − L` ímpar**: `h` sai inteiro e `L + 2h − 255 = I` exato, em todo pixel
//! (provado para os 65 536 pares em `mesclagem.rs`). O preço é a baixa poder
//! diferir do desfoque em até **1 nível** (em vez de meio) — 0,4% da escala,
//! abaixo do que o olho separa e do que qualquer retoque da baixa muda. Nada
//! de buffer de 16 bits ou de ponto flutuante: a recomposição já é exata, e o
//! resto do editor (pincel, carimbo, máscara, projeto) continua igual.
//!
//! Depois do retoque, a conta segue em 8 bits: desfocar a baixa arredonda
//! cada pixel a meio nível, e a composição arredonda uma vez por camada.
//!
//! ## Espaço de cor
//!
//! O do documento: **sRGB codificado, sem linearizar** (C29), canal a canal —
//! o mesmo do fluxo clássico do Photoshop num documento RGB/8, e o mesmo em
//! que a Luz Linear compõe. Nenhuma conversão no caminho.
//!
//! ## Transparência e bordas
//!
//! O desfoque é pré-multiplicado (o transparente não escurece a borda) e
//! repete a beira da foto (sem halo nos cantos). A baixa tem o alfa da origem;
//! a alta é opaca onde a origem existe e entra **recortada** na baixa (máscara
//! de corte), em Luz Linear: o conjunto compõe `I` com o alfa de `I`, numa
//! conta só — a mesma de uma camada com os pixels de `I`.
//!
//! O desfoque roda na região inteira de uma vez (o conteúdo mais a margem de
//! 3σ), e não tile a tile: não há emenda entre tiles para esconder.
//!
//! Funções puras: a janela calcula em segundo plano.

use std::sync::Arc;

use crate::documento::{Camada, Retoque};
use crate::filtros::{caixas_da_gaussiana, desfocar_plano, expandido, ler};
use crate::mesclagem::Modo;
use crate::retangulo::Retangulo;
use crate::tiles::{indice, retangulo_do_tile, CamadaDePixels, BYTES_DO_TILE, LADO_DO_TILE};

/// O menor e o maior raio da separação, em pixels da foto. Num retrato de
/// 24 MP a textura da pele (poros) mora abaixo de ~4–8 px; manchas e
/// transições de cor, acima.
pub const RAIO_MINIMO: f32 = 0.5;
pub const RAIO_MAXIMO: f32 = 60.0;

/// Os nomes das camadas que a separação cria.
pub const NOME_DA_BAIXA: &str = "Baixa frequência";
pub const NOME_DA_ALTA: &str = "Alta frequência";

/// O raio sugerido para uma foto: proporcional ao lado maior (≈ 5 px numa
/// foto de 6000 px), dentro dos limites. É só o começo do controle — o certo
/// depende do tamanho do rosto na foto, que o operador vê.
pub fn raio_sugerido(largura: u32, altura: u32) -> f32 {
    let lado = largura.max(altura) as f32;
    ((lado / 1200.0).round()).clamp(2.0, 12.0)
}

/// A baixa e a alta de um canal: `i` é o valor da foto e `lf` o do desfoque
/// (em ponto flutuante). A baixa é o inteiro mais perto de `lf` com `i − l`
/// ímpar (empate: o de baixo), e a alta é `(i − l + 255) / 2`, exata.
#[inline]
pub fn baixa_e_alta(i: u8, lf: f32) -> (u8, u8) {
    let i = i as i32;
    let r = lf.round().clamp(0.0, 255.0) as i32;
    let l = if (i - r) & 1 == 1 {
        r
    } else {
        let (abaixo, acima) = (r - 1, r + 1);
        if abaixo < 0 {
            acima
        } else if acima > 255 || lf - abaixo as f32 <= acima as f32 - lf {
            abaixo
        } else {
            acima
        }
    };
    (l as u8, ((i - l + 255) / 2) as u8)
}

/// A recomposição de um canal: `L + 2H − 1`, a Luz Linear em 8 bits.
#[inline]
pub fn recompor(l: u8, h: u8) -> u8 {
    (l as i32 + 2 * h as i32 - 255).clamp(0, 255) as u8
}

/// A baixa e a alta de uma origem.
#[derive(Clone, Debug, PartialEq)]
pub struct Separacao {
    pub baixa: CamadaDePixels,
    pub alta: CamadaDePixels,
}

/// Separa `fonte` (uma camada de pixels: a foto como aparece, ou uma camada)
/// no raio `raio` (σ, em pixels da foto).
pub fn separar(fonte: &CamadaDePixels, raio: f32) -> Separacao {
    let (largura, altura) = (fonte.largura(), fonte.altura());
    let mut baixa = CamadaDePixels::nova(largura, altura);
    let mut alta = CamadaDePixels::nova(largura, altura);
    let conteudo = fonte
        .existentes()
        .fold(Retangulo::default(), |a, (p, _)| {
            a.uniao(&retangulo_do_tile(*p, largura, altura))
        })
        .limitado(largura, altura);
    if conteudo.vazio() {
        return Separacao { baixa, alta };
    }
    let sigma = raio.clamp(RAIO_MINIMO, RAIO_MAXIMO);
    let caixas = caixas_da_gaussiana(sigma);
    // O alcance das três caixas: o que um pixel do conteúdo enxerga.
    let margem = caixas.iter().map(|w| w / 2).sum::<usize>() as u32 + 1;
    let regiao = expandido(&conteudo, margem, largura, altura);
    let (l, a) = (regiao.largura as usize, regiao.altura as usize);
    let n = l * a;
    let rgba = ler(fonte, &regiao);
    let mut alfa: Vec<f32> = (0..n).map(|k| rgba[k * 4 + 3] as f32).collect();
    desfocar_plano(&mut alfa, l, a, &caixas);
    let mut saida_baixa = vec![0u8; n * 4];
    let mut saida_alta = vec![0u8; n * 4];
    let mut plano = vec![0f32; n];
    for c in 0..3 {
        for (k, p) in plano.iter_mut().enumerate() {
            *p = rgba[k * 4 + c] as f32 * rgba[k * 4 + 3] as f32 / 255.0;
        }
        desfocar_plano(&mut plano, l, a, &caixas);
        for k in 0..n {
            if rgba[k * 4 + 3] == 0 {
                continue;
            }
            let i = rgba[k * 4 + c];
            let lf = if alfa[k] > 0.01 {
                (plano[k] * 255.0 / alfa[k]).clamp(0.0, 255.0)
            } else {
                i as f32
            };
            let (lb, h) = baixa_e_alta(i, lf);
            saida_baixa[k * 4 + c] = lb;
            saida_alta[k * 4 + c] = h;
        }
    }
    for k in 0..n {
        let a = rgba[k * 4 + 3];
        if a > 0 {
            saida_baixa[k * 4 + 3] = a;
            saida_alta[k * 4 + 3] = 255;
        }
    }
    escrever(&mut baixa, &regiao, &saida_baixa);
    escrever(&mut alta, &regiao, &saida_alta);
    Separacao { baixa, alta }
}

/// Os pixels RGBA de `regiao` (linha a linha) viram tiles de `camada` — só
/// os tiles com algum pixel visível.
fn escrever(camada: &mut CamadaDePixels, regiao: &Retangulo, rgba: &[u8]) {
    let l = regiao.largura as usize;
    for posicao in camada.tiles_do_retangulo(regiao) {
        let (ox, oy) = crate::tiles::origem_do_tile(posicao);
        let mut tile = vec![0u8; BYTES_DO_TILE];
        let x0 = ox.max(regiao.x as i64) as u32;
        let y0 = oy.max(regiao.y as i64) as u32;
        let x1 = ((ox + LADO_DO_TILE as i64).min(regiao.direita() as i64)) as u32;
        let y1 = ((oy + LADO_DO_TILE as i64).min(regiao.baixo() as i64)) as u32;
        let mut algo = false;
        for y in y0..y1 {
            let de = ((y - regiao.y) as usize * l + (x0 - regiao.x) as usize) * 4;
            let m = (x1 - x0) as usize * 4;
            let para = indice(x0 - ox as u32, y - oy as u32);
            let linha = &rgba[de..de + m];
            algo |= linha.iter().skip(3).step_by(4).any(|a| *a != 0);
            tile[para..para + m].copy_from_slice(linha);
        }
        if algo {
            camada.definir(posicao, Some(Arc::new(tile)));
        }
    }
}

/// As duas camadas da separação, prontas para entrar no documento nesta
/// ordem (a baixa embaixo): a baixa em Normal, a alta recortada nela em Luz
/// Linear — o conjunto compõe a origem.
pub fn camadas(separacao: Separacao, raio: f32) -> [Camada; 2] {
    let (largura, altura) = (separacao.baixa.largura(), separacao.baixa.altura());
    let mut baixa = Camada::nova(NOME_DA_BAIXA, largura, altura);
    baixa.pixels = separacao.baixa;
    baixa.retoque = Some(Retoque::Baixa { raio });
    let mut alta = Camada::nova(NOME_DA_ALTA, largura, altura);
    alta.pixels = separacao.alta;
    alta.modo = Modo::LuzLinear;
    alta.recortada = true;
    alta.retoque = Some(Retoque::Alta { raio });
    [baixa, alta]
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::composicao::compor;
    use crate::documento::{BaseRef, Documento};
    use image::RgbImage;

    /// Um pouco de tudo: degradê, grão, bordas duras pretas e brancas e
    /// cores saturadas, em mais de um tile em cada eixo.
    fn foto(largura: u32, altura: u32) -> RgbImage {
        RgbImage::from_fn(largura, altura, |x, y| {
            let grao = ((x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 7) % 23;
            if (300..340).contains(&x) {
                // Uma faixa preta e branca: a borda mais forte que existe.
                let v = if y % 40 < 20 { 0 } else { 255 };
                return image::Rgb([v, v, v]);
            }
            if y > altura - 60 {
                // Saturadas puras.
                return match x / 50 % 3 {
                    0 => image::Rgb([255, 0, 0]),
                    1 => image::Rgb([0, 255, 0]),
                    _ => image::Rgb([0, 0, 255]),
                };
            }
            image::Rgb([
                ((x * 255 / largura) as u8).saturating_add(grao as u8),
                ((y * 200 / altura) as u8).saturating_add((grao / 2) as u8),
                (120 + grao) as u8,
            ])
        })
    }

    fn documento_com(base: &RgbImage, camadas: impl IntoIterator<Item = Camada>) -> Documento {
        let mut doc = Documento::novo(BaseRef::da_imagem(base));
        doc.camadas.extend(camadas);
        doc
    }

    #[test]
    fn a_composicao_recompoe_a_foto_byte_a_byte() {
        let base = foto(600, 540);
        for raio in [0.5, 2.0, 6.0, 25.0] {
            let sep = separar(&CamadaDePixels::da_imagem(&base), raio);
            let doc = documento_com(&base, camadas(sep, raio));
            assert!(!doc.neutro());
            let r = compor(&base, &doc);
            assert_eq!(r.as_raw(), base.as_raw(), "raio {raio}");
        }
    }

    #[test]
    fn cinza_uniforme_da_baixa_a_um_nivel_e_alta_neutra() {
        let base = RgbImage::from_pixel(300, 300, image::Rgb([128, 128, 128]));
        let sep = separar(&CamadaDePixels::da_imagem(&base), 4.0);
        // Empate entre 127 e 129: o de baixo; a alta fica no cinza neutro 128.
        assert_eq!(sep.baixa.pixel(150, 150), [127, 127, 127, 255]);
        assert_eq!(sep.alta.pixel(150, 150), [128, 128, 128, 255]);
        assert_eq!(sep.alta.pixel(0, 299), [128, 128, 128, 255]);
        let doc = documento_com(&base, camadas(sep, 4.0));
        assert_eq!(compor(&base, &doc).as_raw(), base.as_raw());
    }

    #[test]
    fn a_baixa_fica_a_um_nivel_do_desfoque_sem_emenda_e_sem_halo_na_borda() {
        // Referência independente: as três caixas pixel a pixel, em f64, com
        // a borda da foto repetida — sem tiles, sem somas corridas.
        let (w, h) = (560u32, 530u32);
        let base = foto(w, h);
        let raio = 5.0;
        let sep = separar(&CamadaDePixels::da_imagem(&base), raio);
        let caixas = caixas_da_gaussiana(raio);
        for c in 0..3 {
            let mut plano: Vec<f64> = base.pixels().map(|p| p.0[c] as f64).collect();
            for &cx in &caixas {
                let r = (cx / 2) as i64;
                let mut aux = plano.clone();
                for y in 0..h as i64 {
                    for x in 0..w as i64 {
                        let mut s = 0.0;
                        for d in -r..=r {
                            s += plano[(y * w as i64 + (x + d).clamp(0, w as i64 - 1)) as usize];
                        }
                        aux[(y * w as i64 + x) as usize] = s / (2 * r + 1) as f64;
                    }
                }
                for y in 0..h as i64 {
                    for x in 0..w as i64 {
                        let mut s = 0.0;
                        for d in -r..=r {
                            s += aux[((y + d).clamp(0, h as i64 - 1) * w as i64 + x) as usize];
                        }
                        plano[(y * w as i64 + x) as usize] = s / (2 * r + 1) as f64;
                    }
                }
            }
            let mut pior = 0.0f64;
            for y in 0..h {
                for x in 0..w {
                    let l = sep.baixa.pixel(x, y)[c] as f64;
                    pior = pior.max((l - plano[(y * w + x) as usize]).abs());
                }
            }
            // ≤ 1 nível da paridade, mais a folga do f32.
            assert!(pior <= 1.0 + 1e-3, "canal {c}: {pior}");
        }
        // Nas emendas dos tiles (x = 256, y = 256) o degradê da baixa anda
        // como no resto: nenhum degrau.
        for y in [100, 255, 256, 257, 400] {
            for x in [254u32, 255, 256, 257, 511, 512] {
                let d = sep.baixa.pixel(x + 1, y)[0] as i32 - sep.baixa.pixel(x, y)[0] as i32;
                assert!(d.abs() <= 2, "degrau em ({x},{y}): {d}");
            }
        }
    }

    #[test]
    fn a_camada_com_transparencia_recompoe_igual_a_ela() {
        let base = foto(400, 300);
        let mut camada = Camada::nova("Retoque", 400, 300);
        let mut pixels = CamadaDePixels::nova(400, 300);
        for y in 20..280u32 {
            for x in 30..390u32 {
                // Um disco opaco com borda que esmaece, e furos.
                let d = ((x as f32 - 200.0).powi(2) + (y as f32 - 150.0).powi(2)).sqrt();
                let a = (255.0 * (1.0 - (d - 90.0) / 40.0)).clamp(0.0, 255.0) as u8;
                if a == 0 || (x / 7 + y / 7) % 9 == 0 {
                    continue;
                }
                let p = crate::tiles::tile_de(x, y);
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                pixels.tile_mut(p)[i..i + 4].copy_from_slice(&[
                    (x % 256) as u8,
                    (255 - y % 256) as u8,
                    ((x * y) % 251) as u8,
                    a,
                ]);
            }
        }
        camada.pixels = pixels;
        let com_a_camada = compor(&base, &documento_com(&base, [camada.clone()]));
        let sep = separar(&camada.pixels, 6.0);
        // Onde a camada é transparente, as duas também são.
        assert_eq!(sep.baixa.pixel(5, 5)[3], 0);
        assert_eq!(sep.alta.pixel(5, 5)[3], 0);
        let com_o_conjunto = compor(&base, &documento_com(&base, camadas(sep, 6.0)));
        assert_eq!(com_o_conjunto.as_raw(), com_a_camada.as_raw());
    }

    #[test]
    fn o_arredondamento_simples_erra_um_nivel_em_metade_dos_pixels() {
        // A separação sem a paridade: L = o desfoque arredondado (o mesmo
        // desfoque, do filtro) e h = arredondar(127,5 + D/2). A medida que
        // justifica a escolha da paridade.
        let base = foto(500, 400);
        let fonte = CamadaDePixels::da_imagem(&base);
        let simples = crate::filtros::filtrada(
            &fonte,
            crate::filtros::Filtro::DesfoqueGaussiano { raio: 4.0 },
            None,
        );
        let (mut errados, mut pior, mut total) = (0usize, 0i32, 0usize);
        for (x, y, p) in base.enumerate_pixels() {
            let l = simples.pixel(x, y);
            for c in 0..3 {
                let (l, i) = (l[c] as i32, p.0[c] as i32);
                let h = ((127.5 + (i - l) as f32 / 2.0).round() as i32).clamp(0, 255);
                let e = ((l + 2 * h - 255).clamp(0, 255) - i).abs();
                total += 1;
                errados += (e > 0) as usize;
                pior = pior.max(e);
            }
        }
        let fracao = errados as f64 / total as f64;
        eprintln!(
            "recomposição em 8 bits sem a paridade: {:.1}% dos canais erram, pior erro {pior} nível; com a paridade: 0",
            fracao * 100.0
        );
        assert_eq!(pior, 1);
        assert!((0.3..0.7).contains(&fracao), "{fracao}");
    }

    #[test]
    fn o_filtro_alta_frequencia_nao_recompoe_pela_luz_linear() {
        // O "I − L + 128" sem a divisão, em Luz Linear sobre a baixa, dobra o
        // detalhe — a mistura de escalas que a separação evita.
        let base = foto(300, 280);
        let fonte = CamadaDePixels::da_imagem(&base);
        let velho = crate::filtros::filtrada(
            &fonte,
            crate::filtros::Filtro::AltaFrequencia { raio: 4.0 },
            None,
        );
        let sep = separar(&fonte, 4.0);
        let [baixa, mut alta] = camadas(sep, 4.0);
        alta.pixels = velho;
        let r = compor(&base, &documento_com(&base, [baixa, alta]));
        let diferentes = r
            .as_raw()
            .iter()
            .zip(base.as_raw())
            .filter(|(a, b)| (**a as i32 - **b as i32).abs() > 2)
            .count();
        assert!(diferentes > base.as_raw().len() / 10, "{diferentes}");
    }

    #[test]
    fn as_camadas_tem_os_papeis_o_modo_e_o_recorte() {
        let base = foto(260, 260);
        let [baixa, alta] = camadas(separar(&CamadaDePixels::da_imagem(&base), 3.0), 3.0);
        assert_eq!(baixa.nome, NOME_DA_BAIXA);
        assert_eq!(baixa.modo, Modo::Normal);
        assert!(!baixa.recortada);
        assert_eq!(baixa.retoque, Some(Retoque::Baixa { raio: 3.0 }));
        assert_eq!(alta.modo, Modo::LuzLinear);
        assert!(alta.recortada);
        assert_eq!(alta.retoque, Some(Retoque::Alta { raio: 3.0 }));
        assert_eq!(raio_sugerido(6000, 4000), 5.0);
        assert_eq!(raio_sugerido(800, 600), 2.0);
    }

    #[test]
    fn baixa_e_alta_respeitam_os_extremos() {
        for i in 0..=255u8 {
            for lf in [0.0f32, 0.3, 0.5, 1.7, 127.5, 200.49, 254.6, 255.0] {
                let (l, h) = baixa_e_alta(i, lf);
                assert!((l as f32 - lf).abs() <= 1.0 + 1e-4, "i {i} lf {lf} l {l}");
                assert_eq!(recompor(l, h), i);
            }
        }
    }
}
