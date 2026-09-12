//! A referência de CPU do que a tela do cliente **deve** desenhar.
//!
//! # Por que existe
//!
//! 🚨 **Em 2026-09-12 o dono ficou duas horas travado num defeito que não
//! existia no código.** Ele girava a foto no editor, a tela do cliente girava
//! para o outro lado, e me avisou cinco vezes; cinco vezes eu respondi provando
//! que a conta estava certa — e estava. A janela dele executava o motor de
//! `31d14a5`, três commits atrás, cujo código girava por `+ângulo`.
//!
//! Este exemplo é a metade em Rust da conferência que fechou o caso: ele
//! desenha o enquadramento pelas UVs de [`uvs_do_enquadramento`] nas **duas**
//! hipóteses de sinal, e o
//! `scripts/conferir-enquadramento-da-tela.mjs` do e-commerce fotografa o wasm
//! que o servidor entrega e diz com qual das duas ele se parece. Um print
//! contra outro print não decide isso; isto decide.
//!
//! ```text
//! cargo run -q -p revelacao-core --example referencia-da-tela
//! node scripts/conferir-enquadramento-da-tela.mjs   # no outro repositório
//! ```
use image::{Rgba, RgbaImage};
use revelacao_core::transformacao::{uvs_do_enquadramento, Corte};
use std::path::PathBuf;

const L: u32 = 600;
const A: u32 = 400;

fn grade() -> RgbaImage {
    let mut i = RgbaImage::new(L, A);
    for y in 0..A {
        for x in 0..L {
            let mut p = [30u8, 30, 34];
            if y % 40 < 4 {
                p = [60, 120, 255]
            }
            if x % 40 < 4 {
                p = [255, 70, 60]
            }
            if (x as i64 - (L as i64 * 2 / 5)).abs() < 30 && (y as i64 - (A as i64 / 4)).abs() < 30
            {
                p = [80, 230, 90]
            }
            i.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
        }
    }
    i
}

/// O mesmo canvas do navegador: 600×400, com o quad preenchendo tudo.
fn desenhar(o: &RgbaImage, corte: &Corte) -> RgbaImage {
    let (ux, uy, uoff) = uvs_do_enquadramento(L, A, corte);
    let mut d = RgbaImage::new(L, A);
    for j in 0..A {
        for i in 0..L {
            let (s, t) = ((i as f32 + 0.5) / L as f32, (j as f32 + 0.5) / A as f32);
            let u = uoff[0] + ux[0] * s + uy[0] * t;
            let v = uoff[1] + ux[1] * s + uy[1] * t;
            d.put_pixel(
                i,
                j,
                if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
                    *o.get_pixel((u * L as f32) as u32, (v * A as f32) as u32)
                } else {
                    Rgba([0, 0, 0, 255])
                },
            );
        }
    }
    d
}

/// A mesma pasta que o script do e-commerce lê (`PASTA_DA_CONFERENCIA`).
fn pasta() -> PathBuf {
    let padrao = std::env::temp_dir().join("enquadramento-da-tela");
    let p = std::env::var("PASTA_DA_CONFERENCIA")
        .map(PathBuf::from)
        .unwrap_or(padrao);
    std::fs::create_dir_all(&p).expect("criar a pasta da conferência");
    p
}

fn main() {
    let pasta = pasta();
    let o = grade();
    for angulo in [0.0f32, 45.0, -45.0] {
        let nome = if angulo == 0.0 {
            "0".to_string()
        } else {
            format!("{angulo}")
        };
        desenhar(
            &o,
            &Corte::novo(0.3, 0.3, 0.3, 0.3, 0, angulo, false, false),
        )
        .save(pasta.join(format!("ref-atual-{nome}.png")))
        .unwrap();
        // 🔑 O sinal trocado, que é a hipótese a descartar: em 0° as duas são a
        // mesma conta, e é só com ângulo que elas se separam.
        desenhar(
            &o,
            &Corte::novo(0.3, 0.3, 0.3, 0.3, 0, -angulo, false, false),
        )
        .save(pasta.join(format!("ref-invertido-{nome}.png")))
        .unwrap();
    }
    println!("referências em {}", pasta.display());
}
