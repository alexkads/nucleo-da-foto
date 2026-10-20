//! Quanto custam os caminhos quentes do editor — `cargo bench -p editor-core`.
//!
//! Imprime a mediana de cada caminho, para comparar antes e depois de uma
//! mudança na mesma máquina. Não é teste: nada aqui falha.

#![allow(clippy::print_stdout)]

use std::hint::black_box;

use editor_core::composicao;
use editor_core::operacoes;
use editor_core::tiles::CamadaDePixels;
use editor_core::vista::Vista;
use editor_core::{Ajuste, BaseRef, Camada, Documento, Retangulo};
use image::RgbImage;

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
    // Uma foto de 12 MP com uma camada de pixels cheia e opaca e uma de
    // Níveis por cima — o arrasto do slider dos Níveis.
    let (w, h) = (4000u32, 3000u32);
    let base = RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x * 7 + y) as u8, (y * 3 ^ x) as u8, 90])
    });
    let mut doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut pintada = Camada::nova("Pintada", w, h);
    pintada.pixels = CamadaDePixels::da_imagem(&RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x / 3) as u8, (y / 5) as u8, 40])
    }));
    doc.camadas.push(pintada);
    doc.camadas.push(Camada::de_ajuste(
        "Níveis",
        Ajuste::Niveis {
            preto: 10.0,
            gama: 1.3,
            branco: 240.0,
        },
        w,
        h,
    ));
    let tudo = Retangulo::inteiro(w, h);

    let mut vista = Vista::nova(&base, &doc, 1600);
    medir(
        "vista 1600 px de 12 MP: rascunho (arrasto de slider)",
        10,
        || {
            vista.rascunhar(&base, &doc, &tudo);
        },
    );
    medir("vista 1600 px de 12 MP: refazer", 3, || {
        vista.refazer(&base, &doc, &tudo);
    });
    medir("compor 12 MP inteira (salvar, varinha)", 3, || {
        black_box(composicao::compor(&base, &doc));
    });
    let camada = &doc.camadas[1].pixels;
    medir("mover: deslocar a camada de 12 MP por (0, 0)", 5, || {
        black_box(operacoes::deslocada(camada, 0, 0));
    });
    medir("mover: deslocar a camada de 12 MP por (13, 7)", 5, || {
        black_box(operacoes::deslocada(camada, 13, 7));
    });
}
