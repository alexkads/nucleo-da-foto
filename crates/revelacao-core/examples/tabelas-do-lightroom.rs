//! Gera `src/tabelas_lightroom.bin` — o processo 1 do motor — a partir das
//! exportações da régua do Lightroom (`ferramentas/lightroom/`).
//!
//! ```text
//! cargo run --release -p revelacao-core --example tabelas-do-lightroom -- \
//!     "<pasta Comparar Presets>"
//! ```
//!
//! A pasta tem as três réguas, cada uma pedida ao plug-in com o seu `casos=`:
//!
//! - `regua-tom/rampa-cor/` (`casos=tom`, na `rampa-cor.jpg`): a faixa cinza de
//!   cima dá a curva de cada valor de cada slider;
//! - `regua-forca2/quad-{a,b,c}/` (`casos=vinheta-forca-fina`): o canto de cada
//!   quadrante, a vinheta inteira sobre 12 níveis de cinza;
//! - `regua-grade/cinza-128/` (`casos=vinheta-grade`): o perfil da vinheta do
//!   centro ao canto, para cada ponto médio × difusão.
//!
//! O desenho das linhas está em `src/lightroom.rs`.

use std::{collections::BTreeMap, path::Path};

use image::RgbImage;
use revelacao_core::lightroom::*;

fn luma(p: &image::Rgb<u8>) -> f64 {
    0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
}

fn abrir(caminho: &Path) -> RgbImage {
    image::open(caminho)
        .unwrap_or_else(|e| panic!("{}: {e}", caminho.display()))
        .to_rgb8()
}

/// Os `.jpg` de uma pasta, pelo nome do caso (sem o número da frente).
fn casos(pasta: &Path) -> BTreeMap<String, std::path::PathBuf> {
    let mut m = BTreeMap::new();
    for e in std::fs::read_dir(pasta).unwrap_or_else(|e| panic!("{}: {e}", pasta.display())) {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "jpg") {
            let nome = p.file_stem().unwrap().to_string_lossy().to_string();
            let caso = nome
                .split_once('-')
                .map(|(_, r)| r.to_string())
                .unwrap_or(nome);
            m.insert(caso, p);
        }
    }
    m
}

/// A faixa cinza (a de cima) da rampa: a luma média do miolo de cada degrau.
fn curva_da_rampa(img: &RgbImage) -> [f64; 256] {
    let (w, h) = (img.width() as f64, img.height() as f64);
    let (y0, y1) = ((0.3 * h / 4.0) as u32, (0.7 * h / 4.0) as u32);
    let mut c = [0.0; 256];
    for (i, v) in c.iter_mut().enumerate() {
        let x0 = ((i as f64 + 0.3) * w / 256.0) as u32;
        let x1 = (((i as f64 + 0.7) * w / 256.0) as u32).max(x0 + 1);
        let (mut s, mut n) = (0.0, 0.0);
        for y in y0..y1 {
            for x in x0..x1 {
                s += luma(img.get_pixel(x, y));
                n += 1.0;
            }
        }
        *v = s / n;
    }
    c
}

/// A faixa cinza da rampa, um canal só (0 R, 1 G, 2 B).
fn canal_da_rampa(img: &RgbImage, canal: usize) -> [f64; 256] {
    let (w, h) = (img.width() as f64, img.height() as f64);
    let (y0, y1) = ((0.3 * h / 4.0) as u32, (0.7 * h / 4.0) as u32);
    let mut c = [0.0; 256];
    for (i, v) in c.iter_mut().enumerate() {
        let x0 = ((i as f64 + 0.3) * w / 256.0) as u32;
        let x1 = (((i as f64 + 0.7) * w / 256.0) as u32).max(x0 + 1);
        let (mut s, mut n) = (0.0, 0.0);
        for y in y0..y1 {
            for x in x0..x1 {
                s += img.get_pixel(x, y)[canal] as f64;
                n += 1.0;
            }
        }
        *v = s / n;
    }
    c
}

/// O balanço: para Temperatura e Matiz, de −100 a +100, as três curvas.
fn balanco(raiz: &Path) -> Vec<[f64; 256]> {
    let m = casos(&raiz.join("regua-balanco-rampa").join("wb-rampa-cor"));
    let neutro = abrir(&m["00-neutro"]);
    let neutros: [[f64; 256]; 3] = std::array::from_fn(|c| canal_da_rampa(&neutro, c));
    let mut linhas = Vec::new();
    for chave in ["Temperature", "Tint"] {
        for passo in 0..21 {
            let v = -100 + passo * 10;
            if v == 0 {
                linhas.extend([identidade(); 3]);
                continue;
            }
            let img = abrir(&m[&format!("{chave}{v:+04}")]);
            for (c, neutro_c) in neutros.iter().enumerate() {
                linhas.push(relativa(&canal_da_rampa(&img, c), neutro_c));
            }
        }
    }
    linhas
}

/// Sobe sempre, entre 0 e 255.
fn monotona(c: &mut [f64]) {
    let mut maior = 0.0f64;
    for v in c.iter_mut() {
        *v = v.clamp(0.0, 255.0).max(maior);
        maior = *v;
    }
}

fn identidade() -> [f64; 256] {
    std::array::from_fn(|i| i as f64)
}

/// A curva medida, corrigida pelo neutro do Lightroom: `f(i) = caso(i) + (i −
/// neutro(i))`, para o zero sair identidade e não o desvio do JPEG.
fn relativa(caso: &[f64; 256], neutro: &[f64; 256]) -> [f64; 256] {
    let mut c: [f64; 256] = std::array::from_fn(|i| caso[i] + (i as f64 - neutro[i]));
    monotona(&mut c);
    c
}

fn misturar(a: &[f64; 256], b: &[f64; 256], t: f64) -> [f64; 256] {
    std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t)
}

/// Interpolação linear por pontos (x crescente), constante fora deles.
fn por_pontos(pontos: &[(f64, f64)], x: f64) -> f64 {
    if x <= pontos[0].0 {
        return pontos[0].1;
    }
    for j in 1..pontos.len() {
        if x <= pontos[j].0 {
            let ((x0, y0), (x1, y1)) = (pontos[j - 1], pontos[j]);
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    pontos.last().unwrap().1
}

fn tom(raiz: &Path, linhas: &mut Vec<[f64; 256]>) {
    let m = casos(&raiz.join("regua-tom").join("rampa-cor"));
    let neutro = curva_da_rampa(&abrir(&m["00-neutro"]));
    let medida = |chave: &str| relativa(&curva_da_rampa(&abrir(&m[chave])), &neutro);

    // Exposição: de −2 a +2 na régua de tom (com ±0,75), e o resto do alcance
    // do slider — ±2,5, ±3, ±4, ±5 — na régua `casos=exposicao`, na mesma
    // rampa. A grade é de 0,25, de −5 a +5.
    let medidos = [
        -2.0, -1.5, -1.0, -0.75, -0.5, -0.25, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0,
    ];
    let mut por_ev: Vec<(f64, [f64; 256])> = medidos
        .iter()
        .map(|ev| (*ev, medida(&format!("tom-Exposure2012{ev:+.2}"))))
        .collect();
    let m_exp = casos(&raiz.join("regua-exposicao").join("rampa-cor"));
    let neutro_exp = curva_da_rampa(&abrir(&m_exp["00-neutro"]));
    for ev in [-5.0, -4.0, -3.0, -2.5, 2.5, 3.0, 4.0, 5.0] {
        let caso = curva_da_rampa(&abrir(&m_exp[&format!("exposicao{ev:+.2}")]));
        por_ev.push((ev, relativa(&caso, &neutro_exp)));
    }
    por_ev.push((0.0, identidade()));
    por_ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for i in 0..LINHAS_DE_EXPOSICAO {
        let ev = -5.0 + i as f64 * 0.25;
        let j = por_ev.iter().position(|(e, _)| *e >= ev - 1e-9).unwrap();
        let curva = if (por_ev[j].0 - ev).abs() < 1e-9 {
            por_ev[j].1
        } else {
            let (e0, c0) = &por_ev[j - 1];
            let (e1, c1) = &por_ev[j];
            misturar(c0, c1, (ev - e0) / (e1 - e0))
        };
        linhas.push(curva);
    }
    for chave in [
        "Contrast2012",
        "Highlights2012",
        "Shadows2012",
        "Whites2012",
        "Blacks2012",
    ] {
        for passo in 0..21 {
            let v = -100 + passo * 10;
            linhas.push(if v == 0 {
                identidade()
            } else {
                medida(&format!("tom-{chave}{v:+04}"))
            });
        }
    }
}

/// Os 12 níveis de cinza dos quadrantes: a, b, c × (sup. esq., sup. dir.,
/// inf. esq., inf. dir.).
const NIVEIS: [(&str, [f64; 4]); 3] = [
    ("quad-a", [0.0, 16.0, 32.0, 48.0]),
    ("quad-b", [64.0, 96.0, 128.0, 160.0]),
    ("quad-c", [192.0, 224.0, 240.0, 255.0]),
];

fn cantos(img: &RgbImage) -> [f64; 4] {
    let (w, h) = (img.width(), img.height());
    [(1, 1), (w - 5, 1), (1, h - 5), (w - 5, h - 5)].map(|(x0, y0)| {
        let mut s = 0.0;
        for y in y0..y0 + 4 {
            for x in x0..x0 + 4 {
                s += luma(img.get_pixel(x, y)) / 16.0;
            }
        }
        s
    })
}

/// A força da vinheta: para cada estilo e quantidade, a curva valor → valor
/// no canto (onde a máscara é inteira).
fn forca(raiz: &Path, estilo: u32) -> Vec<[f64; 256]> {
    let pastas: Vec<_> = NIVEIS
        .iter()
        .map(|(q, n)| (casos(&raiz.join("regua-forca2").join(q)), *n))
        .collect();
    (0..21)
        .map(|passo| {
            let a = -100 + passo * 10;
            if a == 0 {
                return identidade();
            }
            let mut pontos: Vec<(f64, f64)> = Vec::new();
            for (m, niveis) in &pastas {
                let neutro = cantos(&abrir(&m["00-neutro"]));
                let caso = cantos(&abrir(&m[&format!("forca-s{estilo}-a{a:+04}")]));
                for q in 0..4 {
                    // corrigido pelo neutro, como o tom
                    pontos.push((niveis[q], caso[q] + (niveis[q] - neutro[q])));
                }
            }
            pontos.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let mut c: [f64; 256] = std::array::from_fn(|i| por_pontos(&pontos, i as f64));
            monotona(&mut c);
            c
        })
        .collect()
}

/// O perfil da vinheta em 29 pontos de d (0 a 1,4 de 0,05 em 0,05): pela
/// horizontal até 1 e pela diagonal dali em diante.
fn perfil(img: &RgbImage) -> Vec<(f64, f64)> {
    let (w, h) = (img.width() as f64, img.height() as f64);
    let media = |cx: f64, cy: f64| {
        let (mut s, mut n) = (0.0, 0.0);
        for dy in -2i64..=2 {
            for dx in -2i64..=2 {
                let x = (cx as i64 + dx).clamp(0, w as i64 - 1) as u32;
                let y = (cy as i64 + dy).clamp(0, h as i64 - 1) as u32;
                s += luma(img.get_pixel(x, y));
                n += 1.0;
            }
        }
        s / n
    };
    let (cx, cy) = (w / 2.0, h / 2.0);
    (0..=28)
        .map(|i| {
            let d = i as f64 * 0.05;
            let v = if d <= 1.0 {
                media(cx + d * (cx - 1.0), cy)
            } else {
                let t = (d / std::f64::consts::SQRT_2).min(1.0);
                media(cx + t * (cx - 1.0), cy + t * (cy - 1.0))
            };
            (d, v)
        })
        .collect()
}

/// A máscara m(d) de cada ponto médio × difusão: a quantidade efetiva que a
/// tabela de força precisa para dar o valor medido em 128, dividida por −61.
fn mascara(raiz: &Path, forca1: &[[f64; 256]]) -> Vec<[f64; 256]> {
    let m = casos(&raiz.join("regua-grade").join("cinza-128"));
    // F(128, a) para a de −100 a 0, crescente em a.
    let f128: Vec<(f64, f64)> = (0..=10)
        .map(|p| (-100.0 + p as f64 * 10.0, forca1[p][128]))
        .collect();
    let inverter = |v: f64| {
        // a busca é no valor (que cresce com a)
        let pontos: Vec<(f64, f64)> = f128.iter().map(|(a, f)| (*f, *a)).collect();
        por_pontos(&pontos, v)
    };
    let passos = [0, 13, 25, 38, 50, 63, 75, 88, 100];
    let mut saida = Vec::new();
    for meio in passos {
        for dif in passos {
            let img = abrir(&m[&format!("grade-m{meio:03}-f{dif:03}")]);
            let pontos: Vec<(f64, f64)> = perfil(&img)
                .into_iter()
                .map(|(d, v)| (d, (inverter(v) / -61.0).clamp(0.0, 1.0)))
                .collect();
            let mut c: [f64; 256] = std::array::from_fn(|i| {
                por_pontos(&pontos, i as f64 / 255.0 * DISTANCIA_MAXIMA as f64)
            });
            // a máscara não desce do centro para fora
            let mut maior = 0.0f64;
            for v in c.iter_mut() {
                *v = v.max(maior);
                maior = *v;
            }
            saida.push(c);
        }
    }
    saida
}

fn main() {
    let raiz = std::env::args().nth(1).expect("a pasta Comparar Presets");
    let raiz = Path::new(&raiz);
    let mut linhas: Vec<[f64; 256]> = Vec::new();
    tom(raiz, &mut linhas);
    assert_eq!(linhas.len() as u32, LINHA_VINHETA);

    let forca1 = forca(raiz, 1);
    let forca2 = forca(raiz, 2);
    let diferenca = forca1
        .iter()
        .zip(&forca2)
        .flat_map(|(a, b)| a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()))
        .fold(0.0f64, f64::max);
    println!("estilos 1 e 2: maior diferença {diferenca:.2} (iguais no Lightroom)");
    linhas.extend(forca1.iter().copied());
    linhas.extend(forca(raiz, 3));
    assert_eq!(linhas.len() as u32, LINHA_MASCARA);
    linhas.extend(mascara(raiz, &forca1));
    assert_eq!(linhas.len() as u32, LINHA_TEMPERATURA);
    linhas.extend(balanco(raiz));
    assert_eq!(linhas.len() as u32, ALTURA);

    let bytes: Vec<u8> = linhas
        .iter()
        .flat_map(|l| l.iter().flat_map(|v| (*v as f32).to_le_bytes()))
        .collect();
    let destino = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("tabelas_lightroom.bin");
    std::fs::write(&destino, &bytes).unwrap();
    println!(
        "{} linhas × {LARGURA} → {}",
        linhas.len(),
        destino.display()
    );

    let mostrar = |rotulo: &str, l: u32| {
        let c = &linhas[l as usize];
        println!(
            "{rotulo:24} {}",
            [0, 32, 64, 128, 192, 255]
                .iter()
                .map(|i| format!("{:5.1}", c[*i]))
                .collect::<Vec<_>>()
                .join(" ")
        );
    };
    mostrar("exposição −1", LINHA_EXPOSICAO + 16);
    mostrar("exposição +4", LINHA_EXPOSICAO + 36);
    mostrar("contraste −57 ≈ −60", LINHA_CONTRASTE + 4);
    mostrar("pretos −100", LINHA_PRETOS);
    mostrar("vinheta −60, estilo 1", LINHA_VINHETA + 4);
    mostrar("vinheta −60, estilo 3", LINHA_SOBREPOSICAO + 4);
    mostrar("máscara m50 f50 (d×5,7)", LINHA_MASCARA + 4 * 9 + 4);
    // Temperatura +30 (passo 13): R, G, B
    for (c, rotulo) in ["R", "G", "B"].iter().enumerate() {
        mostrar(
            &format!("temperatura +30, {rotulo}"),
            LINHA_TEMPERATURA + 13 * 3 + c as u32,
        );
    }
}
