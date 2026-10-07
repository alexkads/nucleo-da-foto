//! 🧪 A bancada do preenchimento sensível ao conteúdo: cenas com **a verdade
//! conhecida** (um pedaço de textura da foto, ou uma cena sintética, com um
//! buraco no meio) e cada variante do motor medida contra ela.
//!
//! ```text
//! cargo run --release -p revelacao-core --example bancada_do_preenchimento -- foto.jpg saida/
//! ```
//!
//! Para cada cena e variante: o erro no buraco (RMSE da luminância), a nitidez
//! (energia do gradiente no buraco ÷ a da verdade: < 1 é borrado), a costura
//! (o salto na borda do buraco ÷ o da verdade: > 1 é emenda visível) e o
//! tempo. Grava `cena-variante.ppm` para olhar.

use std::time::Instant;

use revelacao_core::preenchimento::{sintetizar, Controle, Pedido, Qualidade};

struct Cena {
    nome: &'static str,
    w: u32,
    h: u32,
    rgb: Vec<u8>,
}

fn recorte(foto: &image::RgbImage, nome: &'static str, x: u32, y: u32, w: u32, h: u32) -> Cena {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for yy in y..y + h {
        for xx in x..x + w {
            rgb.extend_from_slice(&foto.get_pixel(xx, yy).0);
        }
    }
    Cena { nome, w, h, rgb }
}

fn sintetica(nome: &'static str, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Cena {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            rgb.extend_from_slice(&f(x, y));
        }
    }
    Cena { nome, w, h, rgb }
}

fn luma(p: &[u8]) -> f64 {
    0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64
}

struct Medida {
    rmse: f64,
    nitidez: f64,
    costura: f64,
    ms: u128,
}

fn medir(c: &Cena, destino: &[bool], resultado: &[u8]) -> (f64, f64, f64) {
    let (w, h) = (c.w as usize, c.h as usize);
    let l = |img: &[u8], x: usize, y: usize| luma(&img[(y * w + x) * 3..(y * w + x) * 3 + 3]);
    let (mut erro, mut n) = (0.0, 0.0);
    let (mut g_res, mut g_ver) = (0.0, 0.0);
    let (mut s_res, mut s_ver, mut ns) = (0.0, 0.0, 0.0);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            if destino[i] {
                let e = l(resultado, x, y) - l(&c.rgb, x, y);
                erro += e * e;
                n += 1.0;
                let grad = |img: &[u8]| {
                    (l(img, x + 1, y) - l(img, x - 1, y)).abs()
                        + (l(img, x, y + 1) - l(img, x, y - 1)).abs()
                };
                g_res += grad(resultado);
                g_ver += grad(&c.rgb);
                // A costura: o salto entre um pixel do buraco e o vizinho de fora.
                for (vx, vy) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                    if !destino[vy * w + vx] {
                        s_res += (l(resultado, x, y) - l(resultado, vx, vy)).abs();
                        s_ver += (l(&c.rgb, x, y) - l(&c.rgb, vx, vy)).abs();
                        ns += 1.0;
                    }
                }
            }
        }
    }
    let _ = ns;
    (
        (erro / n).sqrt(),
        g_res / g_ver.max(1e-9),
        s_res / s_ver.max(1e-9),
    )
}

fn gravar_ppm(caminho: &std::path::Path, w: u32, h: u32, rgb: &[u8]) {
    let mut bytes = format!("P6\n{w} {h}\n255\n").into_bytes();
    bytes.extend_from_slice(rgb);
    std::fs::write(caminho, bytes).expect("grava");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let foto = image::open(&args[1]).expect("foto").to_rgb8();
    let saida = std::path::PathBuf::from(&args[2]);
    std::fs::create_dir_all(&saida).unwrap();

    let cenas = vec![
        recorte(&foto, "papel", 3700, 450, 600, 600),
        recorte(&foto, "madeira", 3800, 2300, 600, 300),
        recorte(&foto, "tapete", 3400, 2700, 600, 350),
        recorte(&foto, "persiana", 2650, 270, 600, 600),
        sintetica("liso", 600, 400, |x, y| {
            let r = (90.0 + x as f32 * 0.12 + ((x * 7 + y * 13) % 5) as f32) as u8;
            let g = (130.0 + y as f32 * 0.1 + ((x * 3 + y * 11) % 5) as f32) as u8;
            [r, g, 200]
        }),
        sintetica("linhas", 500, 400, |x, y| {
            let fundo = 150 + ((x * 7 + y * 13) % 11) as u8;
            let horizontal = (y % 60) < 4;
            let diagonal = ((x as i32 - y as i32).rem_euclid(90)) < 5;
            if horizontal {
                [40, 40, 120]
            } else if diagonal {
                [180, 60, 40]
            } else {
                [fundo, fundo, fundo - 10]
            }
        }),
    ];
    let variantes: Vec<(&str, Qualidade)> = vec![
        ("antes", Qualidade::default()),
        ("recomendada", Qualidade::recomendada()),
        (
            "so-patch9-grosso",
            Qualidade {
                meio_lado_grosso: 4,
                ..Qualidade::default()
            },
        ),
        (
            "so-gradiente",
            Qualidade {
                peso_do_gradiente: 1,
                ..Qualidade::default()
            },
        ),
        (
            "so-membrana",
            Qualidade {
                harmonizar: true,
                ..Qualidade::default()
            },
        ),
    ];
    println!("cena\tvariante\trmse\tnitidez\tcostura\tms");
    for c in &cenas {
        let (w, h) = (c.w as usize, c.h as usize);
        // O buraco: uma elipse no meio, um terço da cena.
        let destino: Vec<bool> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let dx = (x as f64 - w as f64 / 2.0) / (w as f64 * 0.17);
                let dy = (y as f64 - h as f64 / 2.0) / (h as f64 * 0.17);
                dx * dx + dy * dy < 1.0
            })
            .collect();
        let rgba: Vec<u8> = c
            .rgb
            .chunks(3)
            .zip(&destino)
            .flat_map(|(p, d)| {
                if *d {
                    [255, 0, 255, 255]
                } else {
                    [p[0], p[1], p[2], 255]
                }
            })
            .collect();
        gravar_ppm(
            &saida.join(format!("{}-verdade.ppm", c.nome)),
            c.w,
            c.h,
            &c.rgb,
        );
        for (nome, q) in &variantes {
            let pedido = Pedido {
                rgba: &rgba,
                largura: c.w,
                altura: c.h,
                destino: &destino,
                amostragem: None,
                qualidade: *q,
                semente: 77,
            };
            let inicio = Instant::now();
            let r = sintetizar(&pedido, &Controle::sem_controle()).expect("remendo");
            let ms = inicio.elapsed().as_millis();
            let mut resultado = c.rgb.clone();
            for y in 0..r.altura {
                for x in 0..r.largura {
                    let (gx, gy) = ((r.x0 + x) as usize, (r.y0 + y) as usize);
                    if destino[gy * w + gx] {
                        let k = ((y * r.largura + x) * 4) as usize;
                        resultado[(gy * w + gx) * 3..(gy * w + gx) * 3 + 3]
                            .copy_from_slice(&r.rgba[k..k + 3]);
                    }
                }
            }
            let (rmse, nitidez, costura) = medir(c, &destino, &resultado);
            let m = Medida {
                rmse,
                nitidez,
                costura,
                ms,
            };
            println!(
                "{}\t{}\t{:.1}\t{:.2}\t{:.2}\t{}",
                c.nome, nome, m.rmse, m.nitidez, m.costura, m.ms
            );
            gravar_ppm(
                &saida.join(format!("{}-{}.ppm", c.nome, nome)),
                c.w,
                c.h,
                &resultado,
            );
        }
    }
}
