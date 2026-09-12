//! Quanto custa refazer as grades bilaterais do estágio darktable, etapa a etapa.
//!
//! É o custo que o editor paga **por quadro** enquanto o operador arrasta um
//! slider de exposição, sombras e realces ou monocromático.
//!
//! ```text
//! cargo run --release -p revelacao-core --example darktable-grades-tempo -- <foto.jpg> [lado da cópia]
//! ```
use std::time::Instant;

use image::imageops::FilterType;
use revelacao_core::darktable::{
    exposure, grades_do_estagio, shadhi, Bilateral, Exposure, Monochrome, ShadowsHighlights,
    Tubulacao,
};
use revelacao_core::Ajustes;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let foto = args.first().expect("uso: <foto.jpg> [lado]");
    let lado: u32 = args.get(1).map(|l| l.parse().unwrap()).unwrap_or(2048);
    let original = image::open(foto).expect("abrir").to_rgba8();
    let escala = lado as f32 / original.width().max(original.height()) as f32;
    let (w, h) = (
        (original.width() as f32 * escala).round() as u32,
        (original.height() as f32 * escala).round() as u32,
    );
    let copia = image::imageops::resize(&original, w, h, FilterType::Triangle);
    let (w, h) = (w as usize, h as usize);
    println!("cópia {w}×{h}, escala {escala:.3}");

    let a = Ajustes {
        dt_exposure_ativo: 1.0,
        dt_exposure_black: -0.0019,
        dt_exposure_exposure: 0.163,
        dt_shadhi_ativo: 1.0,
        dt_shadhi_shadows: 65.38,
        dt_shadhi_highlights: -20.51,
        dt_monochrome_ativo: 1.0,
        ..Default::default()
    };

    let medir = |nome: &str, t: Instant| {
        println!("{nome:34} {:>8.1} ms", t.elapsed().as_secs_f64() * 1000.0)
    };
    let tub = Tubulacao::nova();

    let t = Instant::now();
    let mut px: Vec<_> = copia
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| tub.entrar([c[0], c[1], c[2]]))
        .collect();
    medir("entrar (sRGB → trabalho)", t);

    let t = Instant::now();
    exposure(&mut px, Exposure::de(&a));
    medir("exposure", t);

    let t = Instant::now();
    let l: Vec<f32> = px.iter().map(|c| tub.para_lab(*c)[0]).collect();
    medir("L (para_lab inteiro)", t);

    let t = Instant::now();
    let mut g = Bilateral::nova(w, h, 100.0 * escala, 100.0);
    g.espalhar(&l);
    medir("grade shadhi: espalhar", t);
    let t = Instant::now();
    g.borrar();
    medir("grade shadhi: borrar", t);

    let t = Instant::now();
    shadhi(&mut px, w, h, &tub, ShadowsHighlights::de(&a), escala);
    medir("shadhi inteiro em CPU (p/ monochrome)", t);

    let t = Instant::now();
    let p = Monochrome::de(&a);
    let sigma2 = 2.0 * (p.size * 128.0) * (p.size * 128.0);
    let filtro: Vec<f32> = px
        .iter()
        .map(|c| {
            let lab = tub.para_lab(*c);
            let d = ((lab[1] - p.a).powi(2) + (lab[2] - p.b).powi(2)) / sigma2;
            100.0 * (-d.clamp(0.0, 1.0)).exp()
        })
        .collect();
    medir("filtro do monochrome (para_lab)", t);

    let t = Instant::now();
    let mut g = Bilateral::nova(w, h, 20.0 / (1.0 / escala).max(1.0), 250.0);
    g.espalhar(&filtro);
    g.borrar();
    medir("grade monochrome: espalhar+borrar", t);

    for _ in 0..3 {
        let t = Instant::now();
        let _ = grades_do_estagio(copia.as_raw(), w, h, &a, escala);
        medir("TOTAL grades_do_estagio", t);
    }
}
