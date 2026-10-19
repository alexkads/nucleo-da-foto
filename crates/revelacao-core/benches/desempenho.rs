//! Quanto custam os caminhos quentes da revelação na CPU —
//! `cargo bench -p revelacao-core`.
//!
//! Imprime a mediana de cada caminho, para comparar antes e depois de uma
//! mudança na mesma máquina. Não é teste: nada aqui falha. O que roda na GPU
//! fica de fora (depende da placa, não do código).

#![allow(clippy::print_stdout)]

use std::hint::black_box;

use image::{DynamicImage, RgbaImage};
use revelacao_core::preenchimento::preencher_buraco;
use revelacao_core::transformacao::{self, Corte};

/// Roda `f` uma vez para aquecer, depois `vezes` vezes, e imprime a mediana.
///
/// Sem dependência de propósito (o `criterion` traria dezenas de crates ao
/// `cargo deny`): a mediana de poucas rodadas basta para comparar antes e
/// depois de uma mudança na mesma máquina.
fn medir(nome: &str, vezes: usize, mut f: impl FnMut()) {
    f();
    let mut tempos: Vec<std::time::Duration> = (0..vezes)
        .map(|_| {
            let comeco = std::time::Instant::now();
            f();
            comeco.elapsed()
        })
        .collect();
    tempos.sort();
    let mediana = tempos[tempos.len() / 2].as_secs_f64() * 1000.0;
    println!("{nome:<56} {mediana:>9.2} ms  (mediana de {vezes})");
}

fn main() {
    // A revelada de um arrasto de slider: 2560², RGBA, como vem da GPU.
    let revelada = DynamicImage::ImageRgba8(RgbaImage::from_fn(2560, 2560, |x, y| {
        image::Rgba([(x * 7 + y) as u8, (y * 3 ^ x) as u8, ((x + y) / 5) as u8, 255])
    }));
    let sem_corte = Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false);
    let reto = Corte::novo(0.1, 0.1, 0.8, 0.8, 0, 0.0, false, false);
    let girado = Corte::novo(0.1, 0.1, 0.8, 0.8, 1, 0.0, false, false);
    let inclinado = Corte::novo(0.1, 0.1, 0.8, 0.8, 0, 3.0, false, false);

    medir("enquadrar 2560²: sem corte, tomando a foto", 20, || {
        black_box(transformacao::aplicar_dono(revelada.clone(), &sem_corte, true));
    });
    medir("  (o clone que a linha acima paga para ter o que tomar)", 20, || {
        black_box(revelada.clone());
    });
    medir("enquadrar 2560²: sem corte, emprestada", 20, || {
        black_box(transformacao::aplicar(&revelada, &sem_corte, true));
    });
    medir("enquadrar 2560²: recorte reto", 20, || {
        black_box(transformacao::aplicar(&revelada, &reto, true));
    });
    medir("enquadrar 2560²: giro de 90° e recorte", 10, || {
        black_box(transformacao::aplicar(&revelada, &girado, true));
    });
    medir("enquadrar 2560²: endireitar 3° e recortar", 5, || {
        black_box(transformacao::aplicar(&revelada, &inclinado, true));
    });

    // Content-Aware: um buraco de 80² numa estampa de 400×300.
    let (w, h) = (400u32, 300u32);
    let estampa: Vec<u8> = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| {
                let v = ((x * 5 + y * 7 + (x / 9) * 40) % 251) as u8;
                [v, v.wrapping_mul(3), 255 - v, 255]
            })
        })
        .collect();
    let no_buraco = |x: f32, y: f32| (160.0..240.0).contains(&x) && (110.0..190.0).contains(&y);
    medir("preencher: buraco de 80² em 400×300", 5, || {
        black_box(preencher_buraco(&estampa, w, h, (160, 110, 240, 190), &no_buraco, 7));
    });
}
