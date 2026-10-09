//! 🌫️ O menu Filtro do Photoshop: **Desfoque gaussiano** e **Máscara de
//! nitidez**, na camada escolhida (ou na máscara dela), dosados pela seleção.
//!
//! ```text
//! camada (RGBA, alfa reto)  →  pré-multiplicada em planos f32  →  3 caixas por eixo (≈ gaussiana de σ = raio)
//!                           →  de volta ao alfa reto            →  mistura com a original pela seleção
//! ```
//!
//! - **Pré-multiplicado**: o transparente em volta de um traço não escurece a
//!   borda do desfoque, e numa máscara (pixels sobre o `fundo`) o desfoque se
//!   mistura com o fundo — é a difusão que o Photoshop dá ao desfocar a máscara;
//! - o raio é o σ, em pixels da foto, como o "Raio" do Photoshop; as três
//!   caixas seguem o arranjo clássico de Kutskir (larguras ímpares w e w + 2);
//! - fora da foto a borda se repete (a foto não escurece nos cantos);
//! - só a região que pode mudar é calculada: o conteúdo (mais a margem de 3σ
//!   do desfoque), cortado pela caixa da seleção;
//! - a nitidez soma `quantidade × (original − desfocada)` em cada canal onde a
//!   diferença passa do limiar; o alfa fica.
//!
//! Funções puras: a janela calcula em segundo plano e a [`crate::Sessao`] só
//! troca os pixels (prévia) e grava um passo (OK).

use std::sync::Arc;

use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{indice, CamadaDePixels, BYTES_DO_TILE, LADO_DO_TILE};

/// O raio (σ) máximo, em pixels — o do Photoshop é 1000; 250 já desfaz uma
/// foto de 24 MP e mantém a conta em segundos.
pub const RAIO_MAXIMO: f32 = 250.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filtro {
    /// "Desfoque gaussiano…": o raio é o σ, em pixels.
    DesfoqueGaussiano { raio: f32 },
    /// "Máscara de nitidez…": quantidade (1 = 100%), raio (σ) e limiar (0–255).
    MascaraDeNitidez {
        quantidade: f32,
        raio: f32,
        limiar: u8,
    },
}

impl Filtro {
    /// O nome no Histórico e no menu.
    pub fn nome(&self) -> &'static str {
        match self {
            Filtro::DesfoqueGaussiano { .. } => "Desfoque gaussiano",
            Filtro::MascaraDeNitidez { .. } => "Máscara de nitidez",
        }
    }

    fn raio(&self) -> f32 {
        match *self {
            Filtro::DesfoqueGaussiano { raio } | Filtro::MascaraDeNitidez { raio, .. } => {
                raio.clamp(0.1, RAIO_MAXIMO)
            }
        }
    }

    /// Não muda nada (raio pequeno demais, nitidez sem quantidade).
    pub fn neutro(&self) -> bool {
        match *self {
            Filtro::DesfoqueGaussiano { raio } => raio < 0.1,
            Filtro::MascaraDeNitidez { quantidade, .. } => quantidade <= 0.0,
        }
    }
}

/// As larguras das três caixas que, passadas em sequência, dão a gaussiana
/// de `sigma` (ímpares; as primeiras `m` com `w`, as outras com `w + 2`).
pub fn caixas_da_gaussiana(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let wlf = wl as f32;
    let m = ((12.0 * sigma * sigma - n * wlf * wlf - 4.0 * n * wlf - 3.0 * n) / (-4.0 * wlf - 4.0))
        .round() as i64;
    [0, 1, 2].map(|i| if i < m { wl as usize } else { wu as usize })
}

/// A camada com o filtro: os tiles fora da região que muda são os mesmos
/// `Arc` da original (copiar é barato e a diferença sai pequena).
pub fn filtrada(
    original: &CamadaDePixels,
    filtro: Filtro,
    selecao: Option<&Selecao>,
) -> CamadaDePixels {
    let mut saida = original.clone();
    if filtro.neutro() {
        return saida;
    }
    let (largura, altura) = (original.largura(), original.altura());
    let sigma = filtro.raio();
    let margem = (sigma * 3.0).ceil() as u32 + 1;
    let conteudo = original
        .existentes()
        .fold(Retangulo::default(), |a, (p, _)| {
            a.uniao(&crate::tiles::retangulo_do_tile(*p, largura, altura))
        });
    if conteudo.vazio() {
        return saida;
    }
    // O que pode mudar: o desfoque espalha o conteúdo pela margem; a nitidez
    // não sai dele.
    let mut fora = match filtro {
        Filtro::DesfoqueGaussiano { .. } => expandido(&conteudo, margem, largura, altura),
        Filtro::MascaraDeNitidez { .. } => conteudo,
    };
    if let Some(s) = selecao {
        fora = intersecao(&fora, &s.caixa_justa());
    }
    if fora.vazio() {
        return saida;
    }
    let dentro = expandido(&fora, margem, largura, altura);
    let rgba = ler(original, &dentro);
    let resultado = match filtro {
        Filtro::DesfoqueGaussiano { .. } => {
            desfocar_rgba(&rgba, dentro.largura, dentro.altura, sigma)
        }
        Filtro::MascaraDeNitidez {
            quantidade, limiar, ..
        } => {
            let desfocada = desfocar_rgba(&rgba, dentro.largura, dentro.altura, sigma);
            nitidez(&rgba, &desfocada, quantidade.max(0.0), limiar)
        }
    };
    escrever(&mut saida, original, &dentro, &fora, &resultado, selecao);
    saida
}

fn expandido(r: &Retangulo, margem: u32, largura: u32, altura: u32) -> Retangulo {
    let x0 = r.x.saturating_sub(margem);
    let y0 = r.y.saturating_sub(margem);
    let x1 = (r.direita() + margem).min(largura);
    let y1 = (r.baixo() + margem).min(altura);
    Retangulo::novo(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

fn intersecao(a: &Retangulo, b: &Retangulo) -> Retangulo {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.direita().min(b.direita());
    let y1 = a.baixo().min(b.baixo());
    if x1 <= x0 || y1 <= y0 {
        return Retangulo::default();
    }
    Retangulo::novo(x0, y0, x1 - x0, y1 - y0)
}

/// Os pixels RGBA de `r`, linha a linha (o transparente onde não há tile).
fn ler(camada: &CamadaDePixels, r: &Retangulo) -> Vec<u8> {
    let (l, a) = (r.largura as usize, r.altura as usize);
    let mut saida = vec![0u8; l * a * 4];
    for posicao in camada.tiles_do_retangulo(r) {
        let Some(tile) = camada.tile(posicao) else {
            continue;
        };
        let (ox, oy) = crate::tiles::origem_do_tile(posicao);
        let x0 = (ox.max(r.x as i64)) as u32;
        let y0 = (oy.max(r.y as i64)) as u32;
        let x1 = ((ox + LADO_DO_TILE as i64).min(r.direita() as i64)) as u32;
        let y1 = ((oy + LADO_DO_TILE as i64).min(r.baixo() as i64)) as u32;
        for y in y0..y1 {
            let de = indice(x0 - ox as u32, y - oy as u32);
            let n = (x1 - x0) as usize * 4;
            let para = ((y - r.y) as usize * l + (x0 - r.x) as usize) * 4;
            saida[para..para + n].copy_from_slice(&tile[de..de + n]);
        }
    }
    saida
}

/// O desfoque de uma imagem RGBA de alfa reto, devolvido em alfa reto.
fn desfocar_rgba(rgba: &[u8], largura: u32, altura: u32, sigma: f32) -> Vec<u8> {
    let (l, a) = (largura as usize, altura as usize);
    let n = l * a;
    let caixas = caixas_da_gaussiana(sigma);
    let mut alfa: Vec<f32> = (0..n).map(|i| rgba[i * 4 + 3] as f32).collect();
    desfocar_plano(&mut alfa, l, a, &caixas);
    let mut saida = vec![0u8; n * 4];
    for c in 0..3 {
        let mut plano: Vec<f32> = (0..n)
            .map(|i| rgba[i * 4 + c] as f32 * rgba[i * 4 + 3] as f32 / 255.0)
            .collect();
        desfocar_plano(&mut plano, l, a, &caixas);
        for i in 0..n {
            let al = alfa[i];
            saida[i * 4 + c] = if al > 0.01 {
                (plano[i] * 255.0 / al).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
        }
    }
    for i in 0..n {
        saida[i * 4 + 3] = alfa[i].round().clamp(0.0, 255.0) as u8;
    }
    saida
}

/// Original + quantidade × (original − desfocada), canal a canal, onde a
/// diferença passa do limiar. O alfa é o da original.
fn nitidez(original: &[u8], desfocada: &[u8], quantidade: f32, limiar: u8) -> Vec<u8> {
    let mut saida = original.to_vec();
    for (px, (o, d)) in saida
        .chunks_exact_mut(4)
        .zip(original.chunks_exact(4).zip(desfocada.chunks_exact(4)))
    {
        if o[3] == 0 {
            continue;
        }
        for c in 0..3 {
            let dif = o[c] as f32 - d[c] as f32;
            if dif.abs() < limiar as f32 {
                continue;
            }
            px[c] = (o[c] as f32 + quantidade * dif).round().clamp(0.0, 255.0) as u8;
        }
    }
    saida
}

fn linhas_de_trabalho() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 16)
}

/// As três caixas, nas linhas e depois nas colunas, com a borda repetida.
fn desfocar_plano(plano: &mut [f32], l: usize, a: usize, caixas: &[usize; 3]) {
    if l == 0 || a == 0 {
        return;
    }
    let partes = linhas_de_trabalho();
    // Nas linhas: cada faixa de linhas numa thread, no lugar.
    let por_faixa = a.div_ceil(partes).max(1);
    std::thread::scope(|escopo| {
        for faixa in plano.chunks_mut(por_faixa * l) {
            escopo.spawn(move || {
                let mut aux = vec![0f32; l];
                for linha in faixa.chunks_exact_mut(l) {
                    for &w in caixas {
                        caixa_na_linha(linha, &mut aux, w / 2);
                        linha.copy_from_slice(&aux);
                    }
                }
            });
        }
    });
    // Nas colunas: cada faixa de colunas numa thread, com a soma corrida por
    // coluna descendo as linhas (lê o plano de forma contígua).
    let por_faixa = l.div_ceil(partes).max(1);
    let fonte: &[f32] = plano;
    let faixas: Vec<(usize, Vec<f32>)> = std::thread::scope(|escopo| {
        let tarefas: Vec<_> = (0..l)
            .step_by(por_faixa)
            .map(|x0| {
                let x1 = (x0 + por_faixa).min(l);
                escopo.spawn(move || {
                    let lf = x1 - x0;
                    let mut atual: Vec<f32> = (0..a)
                        .flat_map(|y| fonte[y * l + x0..y * l + x1].iter().copied())
                        .collect();
                    let mut aux = vec![0f32; lf * a];
                    for &w in caixas {
                        caixa_nas_colunas(&atual, &mut aux, lf, a, w / 2);
                        std::mem::swap(&mut atual, &mut aux);
                    }
                    (x0, atual)
                })
            })
            .collect();
        tarefas
            .into_iter()
            .map(|t| t.join().expect("faixa do desfoque"))
            .collect()
    });
    for (x0, faixa) in faixas {
        let lf = faixa.len() / a;
        for y in 0..a {
            plano[y * l + x0..y * l + x0 + lf].copy_from_slice(&faixa[y * lf..(y + 1) * lf]);
        }
    }
}

/// A média de 2r + 1 vizinhos em cada ponto, repetindo as pontas.
fn caixa_na_linha(linha: &[f32], saida: &mut [f32], r: usize) {
    let n = linha.len();
    if r == 0 {
        saida.copy_from_slice(linha);
        return;
    }
    let em = |i: i64| linha[i.clamp(0, n as i64 - 1) as usize];
    let mut soma: f64 = (-(r as i64)..=r as i64).map(|i| em(i) as f64).sum();
    let div = (2 * r + 1) as f64;
    for (i, s) in saida.iter_mut().enumerate() {
        *s = (soma / div) as f32;
        let i = i as i64;
        soma += em(i + r as i64 + 1) as f64 - em(i - r as i64) as f64;
    }
}

/// A mesma caixa, na vertical, para `lf` colunas lado a lado.
fn caixa_nas_colunas(fonte: &[f32], saida: &mut [f32], lf: usize, a: usize, r: usize) {
    if r == 0 {
        saida.copy_from_slice(fonte);
        return;
    }
    let linha = |y: i64| {
        let y = y.clamp(0, a as i64 - 1) as usize;
        &fonte[y * lf..(y + 1) * lf]
    };
    let mut soma = vec![0f64; lf];
    for y in -(r as i64)..=r as i64 {
        for (s, v) in soma.iter_mut().zip(linha(y)) {
            *s += *v as f64;
        }
    }
    let div = (2 * r + 1) as f64;
    for y in 0..a {
        for (o, s) in saida[y * lf..(y + 1) * lf].iter_mut().zip(&soma) {
            *o = (*s / div) as f32;
        }
        let (entra, sai) = (linha(y as i64 + r as i64 + 1), linha(y as i64 - r as i64));
        for ((s, e), t) in soma.iter_mut().zip(entra).zip(sai) {
            *s += *e as f64 - *t as f64;
        }
    }
}

/// O resultado (calculado em `dentro`) volta aos tiles só em `fora`,
/// misturado com a original pela seleção.
fn escrever(
    saida: &mut CamadaDePixels,
    original: &CamadaDePixels,
    dentro: &Retangulo,
    fora: &Retangulo,
    resultado: &[u8],
    selecao: Option<&Selecao>,
) {
    let l = dentro.largura as usize;
    for posicao in original.tiles_do_retangulo(fora) {
        let (ox, oy) = crate::tiles::origem_do_tile(posicao);
        let velho = original.tile(posicao).cloned();
        let mut novo: Vec<u8> = velho
            .as_ref()
            .map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        let x0 = (ox.max(fora.x as i64)) as u32;
        let y0 = (oy.max(fora.y as i64)) as u32;
        let x1 = ((ox + LADO_DO_TILE as i64).min(fora.direita() as i64)) as u32;
        let y1 = ((oy + LADO_DO_TILE as i64).min(fora.baixo() as i64)) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let (lx, ly) = (x - ox as u32, y - oy as u32);
                let v = match &m {
                    Ok(t) => t[(ly * LADO_DO_TILE + lx) as usize],
                    Err(v) => *v,
                } as u32;
                if v == 0 {
                    continue;
                }
                let i = indice(lx, ly);
                let j = ((y - dentro.y) as usize * l + (x - dentro.x) as usize) * 4;
                for c in 0..4 {
                    let (o, f) = (novo[i + c] as u32, resultado[j + c] as u32);
                    novo[i + c] = ((o * (255 - v) + f * v + 127) / 255) as u8;
                }
            }
        }
        let tem_algo = novo.iter().skip(3).step_by(4).any(|a| *a != 0);
        let tile = tem_algo.then(|| Arc::new(novo));
        if tile.as_ref() != velho.as_ref() {
            saida.definir(posicao, tile);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::selecao::Forma;

    fn camada_com_quadrado() -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(600, 400);
        for y in 100..300 {
            for x in 200..400 {
                let (ox, oy) = (x / LADO_DO_TILE, y / LADO_DO_TILE);
                let t = c.tile_mut((ox as i32, oy as i32));
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                t[i..i + 4].copy_from_slice(&[200, 40, 40, 255]);
            }
        }
        c
    }

    #[test]
    fn as_caixas_dao_a_variancia_da_gaussiana() {
        for sigma in [0.8f32, 2.0, 5.0, 17.3, 60.0] {
            let caixas = caixas_da_gaussiana(sigma);
            // A variância de uma caixa de largura w é (w² − 1)/12.
            let var: f32 = caixas.iter().map(|&w| ((w * w) as f32 - 1.0) / 12.0).sum();
            assert!(
                (var.sqrt() - sigma).abs() < 0.6,
                "σ {sigma}: caixas {caixas:?} dão {}",
                var.sqrt()
            );
            assert!(caixas.iter().all(|w| w % 2 == 1));
        }
    }

    #[test]
    fn o_desfoque_espalha_a_borda_e_conserva_o_meio_e_a_cor() {
        let c = camada_com_quadrado();
        let d = filtrada(&c, Filtro::DesfoqueGaussiano { raio: 6.0 }, None);
        // O meio do quadrado fica igual.
        assert_eq!(d.pixel(300, 200), [200, 40, 40, 255]);
        // Na borda, meio alfa, e a cor **não** escurece (pré-multiplicado).
        let borda = d.pixel(200, 200);
        assert!((110..=145).contains(&borda[3]), "alfa na borda {borda:?}");
        assert!(borda[0] >= 198 && borda[1] <= 42, "cor na borda {borda:?}");
        // Fora, a 10 px, já tem um pouco; a 40 px, nada.
        assert!(d.pixel(190, 200)[3] > 0);
        assert_eq!(d.pixel(150, 200)[3], 0);
        // A original não mudou.
        assert_eq!(c.pixel(200, 200)[3], 255);
    }

    #[test]
    fn a_selecao_dosa_o_desfoque() {
        let c = camada_com_quadrado();
        let s = Selecao::da_forma(600, 400, &Forma::Retangulo(Retangulo::novo(0, 0, 300, 400)));
        let d = filtrada(&c, Filtro::DesfoqueGaussiano { raio: 6.0 }, Some(&s));
        // A borda esquerda (dentro da seleção) desfoca; a direita fica dura.
        assert!(d.pixel(200, 200)[3] < 200);
        assert_eq!(d.pixel(399, 200), [200, 40, 40, 255]);
        assert_eq!(d.pixel(400, 200)[3], 0);
    }

    #[test]
    fn a_nitidez_realca_a_borda_e_respeita_o_limiar() {
        let mut c = CamadaDePixels::nova(300, 100);
        for y in 0..100 {
            for x in 0..300 {
                let v = if x < 150 { 100 } else { 150 };
                let t = c.tile_mut(((x / LADO_DO_TILE) as i32, (y / LADO_DO_TILE) as i32));
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                t[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let n = filtrada(
            &c,
            Filtro::MascaraDeNitidez {
                quantidade: 1.0,
                raio: 3.0,
                limiar: 0,
            },
            None,
        );
        // O lado escuro escurece junto da divisa, o claro clareia; longe dela
        // nada muda; o alfa fica.
        assert!(n.pixel(148, 50)[0] < 100);
        assert!(n.pixel(151, 50)[0] > 150);
        assert_eq!(n.pixel(20, 50), [100, 100, 100, 255]);
        // Com limiar acima da diferença, nada muda.
        let alto = filtrada(
            &c,
            Filtro::MascaraDeNitidez {
                quantidade: 1.0,
                raio: 3.0,
                limiar: 60,
            },
            None,
        );
        assert_eq!(alto.pixel(148, 50), [100, 100, 100, 255]);
    }

    #[test]
    fn camada_vazia_ou_filtro_neutro_nao_mudam() {
        let vazia = CamadaDePixels::nova(300, 200);
        let d = filtrada(&vazia, Filtro::DesfoqueGaussiano { raio: 10.0 }, None);
        assert!(d.vazia());
        let c = camada_com_quadrado();
        let n = filtrada(
            &c,
            Filtro::MascaraDeNitidez {
                quantidade: 0.0,
                raio: 3.0,
                limiar: 0,
            },
            None,
        );
        assert!(crate::operacoes::diferenca(&c, &n).is_none());
    }
}

/// Medida: `cargo test --release -p editor-core medir_os_filtros -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn medir_os_filtros_em_24_mp() {
    let foto = image::RgbImage::from_fn(6000, 4000, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 256) as u8])
    });
    let camada = CamadaDePixels::da_imagem(&foto);
    for filtro in [
        Filtro::DesfoqueGaussiano { raio: 4.0 },
        Filtro::DesfoqueGaussiano { raio: 60.0 },
        Filtro::MascaraDeNitidez {
            quantidade: 1.0,
            raio: 2.0,
            limiar: 0,
        },
    ] {
        let t = std::time::Instant::now();
        let f = filtrada(&camada, filtro, None);
        eprintln!("{filtro:?}: {:?} ({} tiles)", t.elapsed(), f.quantos());
    }
}
