//! Quanto custa enquadrar com perspectiva, comparado ao endireitar de sempre.
//!
//! `cargo run --release -p revelacao-core --example medir-perspectiva`
//!
//! Mede `transformacao::aplicar` em três tamanhos: a prévia reduzida do arrasto
//! (1024 px), a cópia de trabalho (2560 px) e o arquivo (6000×4000, 24 MP).

use std::time::Instant;

use image::{DynamicImage, Rgba, RgbaImage};
use revelacao_core::perspectiva::Perspectiva;
use revelacao_core::transformacao::aplicar;
use revelacao_core::Corte;

fn foto(l: u32, a: u32) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(l, a, |x, y| {
        Rgba([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8, 255])
    }))
}

fn medir(rotulo: &str, imagem: &DynamicImage, corte: &Corte, recortar: bool, vezes: u32) {
    let _ = aplicar(imagem, corte, recortar);
    let inicio = Instant::now();
    for _ in 0..vezes {
        std::hint::black_box(aplicar(imagem, corte, recortar));
    }
    let ms = inicio.elapsed().as_secs_f64() * 1000.0 / vezes as f64;
    println!("{rotulo:<58} {ms:>8.2} ms");
}

fn main() {
    let angulo = Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 4.0, false, false);
    let persp = Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false)
        .com_perspectiva(Perspectiva::nova([9.0, -3.0, 1.0], 1.6, 0.0, 0.0));
    let persp_recorte = Corte::novo(0.1, 0.1, 0.8, 0.8, 1, 2.0, false, false)
        .com_perspectiva(Perspectiva::nova([9.0, -3.0, 1.0], 1.6, 0.0, 0.0));
    for (l, a, vezes, nome) in [
        (1024, 683, 30, "prévia do arrasto 1024×683"),
        (2560, 1707, 10, "cópia de trabalho 2560×1707"),
        (6000, 4000, 2, "arquivo 6000×4000"),
    ] {
        let imagem = foto(l, a);
        println!("— {nome}");
        medir(
            "  antes: endireitar 4° (Enquadrar, foto inteira)",
            &imagem,
            &angulo,
            false,
            vezes,
        );
        medir(
            "  antes: endireitar 4° (recortado)",
            &imagem,
            &angulo,
            true,
            vezes,
        );
        medir(
            "  perspectiva (Enquadrar, foto inteira)",
            &imagem,
            &persp,
            false,
            vezes,
        );
        medir(
            "  perspectiva + giro + 2° + recorte (arquivo)",
            &imagem,
            &persp_recorte,
            true,
            vezes,
        );
    }
}
