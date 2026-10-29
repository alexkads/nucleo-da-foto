//! Quanto custam os caminhos quentes do codificador — `cargo bench -p foto-codec`.
//!
//! Imprime a mediana de cada caminho, para comparar antes e depois de uma
//! mudança na mesma máquina. Não é teste: nada aqui falha.

#![allow(clippy::print_stdout)]

use std::hint::black_box;

use foto_codec::espaco_de_cor::{rgba_para_srgb, EspacoDeCor};
use image::{DynamicImage, RgbaImage};

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

/// Uma foto de 12 MP com conteúdo que o JPEG não comprime de graça.
fn foto(largura: u32, altura: u32) -> RgbaImage {
    RgbaImage::from_fn(largura, altura, |x, y| {
        image::Rgba([
            (x * 7 + y) as u8,
            ((y * 3) ^ x) as u8,
            ((x + y) / 5) as u8,
            255,
        ])
    })
}

fn main() {
    let rgba = DynamicImage::ImageRgba8(foto(4000, 3000));
    let rgb = DynamicImage::ImageRgb8(rgba.to_rgb8());
    let crus = rgba.to_rgba8().into_raw();

    medir("jpeg: codificar 12 MP RGBA, q90", 5, || {
        black_box(foto_codec::codificar(&rgba, 90).unwrap());
    });
    medir("jpeg: codificar 12 MP RGB, q90", 5, || {
        black_box(foto_codec::codificar(&rgb, 90).unwrap());
    });
    medir("jpeg: codificar_rgba 12 MP emprestado, q90", 5, || {
        black_box(foto_codec::codificar_rgba(&crus, 4000, 3000, 90).unwrap());
    });

    let mut miniatura = foto(256, 256).into_raw();
    medir("cor: Adobe RGB → sRGB, miniatura 256²", 20, || {
        rgba_para_srgb(black_box(&mut miniatura), EspacoDeCor::AdobeRgb);
    });
    let mut inteira = crus.clone();
    medir("cor: Adobe RGB → sRGB, 12 MP", 5, || {
        rgba_para_srgb(black_box(&mut inteira), EspacoDeCor::AdobeRgb);
    });

    medir("orientação: girar 90° uma foto de 12 MP RGB", 5, || {
        let mut deitada = rgb.clone();
        foto_codec::orientacao::aplicar(&mut deitada, image::metadata::Orientation::Rotate90);
        black_box(deitada);
    });
}
