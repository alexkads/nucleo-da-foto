//! Revela uma foto pelo MOTOR (GPU) com o estilo do darktable e mede contra o
//! render do `darktable-cli` 5.6.1.
//!
//! É a conferência de ponta a ponta: o `darktable-estilo` prova o gabarito de
//! CPU, e os testes de `motor.rs` provam a GPU contra o gabarito numa carta
//! pequena. Este passa a foto real inteira pelo mesmo caminho da exportação —
//! grades bilaterais em CPU, shader na GPU — e compara com o que o darktable
//! escreveu.
//!
//! ```text
//! cargo run --release -p revelacao-core --example darktable-motor -- \
//!     <foto.jpg> <parametros.json> <referencia-do-darktable.png> <pasta de saída> [lado da cópia]
//! ```
//!
//! Com `lado da cópia`, revela também uma cópia reduzida (como a do navegador)
//! com a escala certa, e a compara com a referência reduzida do mesmo jeito.
use std::{fs, path::Path, sync::Arc, time::Instant};

use image::{imageops::FilterType, RgbImage};
use revelacao_core::{Ajustes, Motor};
use serde_json::Value;

fn f(v: &Value, k: &str) -> f32 {
    v[k].as_f64()
        .unwrap_or_else(|| panic!("falta o campo `{k}`")) as f32
}

/// Os parâmetros decodificados do `.dtstyle` (o mesmo JSON do `darktable-estilo`)
/// nos campos `dt_*` do [`Ajustes`].
fn ajustes_do_json(p: &Value) -> Ajustes {
    let (pe, ps, pm, pv, pc) = (
        &p["exposure"],
        &p["shadhi"],
        &p["monochrome"],
        &p["vignette"],
        &p["colorbalancergb"],
    );
    assert_eq!(
        ps["shadhi_algo"].as_i64(),
        Some(1),
        "só o algoritmo bilateral está portado"
    );
    assert_eq!(
        pc["saturation_formula"].as_i64(),
        Some(1),
        "só a fórmula dt UCS está portada"
    );
    Ajustes {
        dt_exposure_ativo: 1.0,
        dt_exposure_black: f(pe, "black"),
        dt_exposure_exposure: f(pe, "exposure"),
        dt_shadhi_ativo: 1.0,
        dt_shadhi_radius: f(ps, "radius"),
        dt_shadhi_shadows: f(ps, "shadows"),
        dt_shadhi_whitepoint: f(ps, "whitepoint"),
        dt_shadhi_highlights: f(ps, "highlights"),
        dt_shadhi_compress: f(ps, "compress"),
        dt_shadhi_shadows_ccorrect: f(ps, "shadows_ccorrect"),
        dt_shadhi_highlights_ccorrect: f(ps, "highlights_ccorrect"),
        dt_shadhi_flags: ps["flags"].as_u64().unwrap() as f32,
        dt_monochrome_ativo: 1.0,
        dt_monochrome_a: f(pm, "a"),
        dt_monochrome_b: f(pm, "b"),
        dt_monochrome_size: f(pm, "size"),
        dt_monochrome_highlights: f(pm, "highlights"),
        dt_vignette_ativo: 1.0,
        dt_vignette_scale: f(pv, "scale"),
        dt_vignette_falloff_scale: f(pv, "falloff_scale"),
        dt_vignette_brightness: f(pv, "brightness"),
        dt_vignette_saturation: f(pv, "saturation"),
        dt_vignette_center_x: f(pv, "center_x"),
        dt_vignette_center_y: f(pv, "center_y"),
        dt_vignette_autoratio: pv["autoratio"].as_i64().unwrap() as f32,
        dt_vignette_whratio: f(pv, "whratio"),
        dt_vignette_shape: f(pv, "shape"),
        dt_vignette_unbound: pv["unbound"].as_i64().unwrap() as f32,
        dt_cb_ativo: 1.0,
        dt_cb_shadows_y: f(pc, "shadows_Y"),
        dt_cb_shadows_c: f(pc, "shadows_C"),
        dt_cb_shadows_h: f(pc, "shadows_H"),
        dt_cb_midtones_y: f(pc, "midtones_Y"),
        dt_cb_midtones_c: f(pc, "midtones_C"),
        dt_cb_midtones_h: f(pc, "midtones_H"),
        dt_cb_highlights_y: f(pc, "highlights_Y"),
        dt_cb_highlights_c: f(pc, "highlights_C"),
        dt_cb_highlights_h: f(pc, "highlights_H"),
        dt_cb_global_y: f(pc, "global_Y"),
        dt_cb_global_c: f(pc, "global_C"),
        dt_cb_global_h: f(pc, "global_H"),
        dt_cb_shadows_weight: f(pc, "shadows_weight"),
        dt_cb_white_fulcrum: f(pc, "white_fulcrum"),
        dt_cb_highlights_weight: f(pc, "highlights_weight"),
        dt_cb_chroma_shadows: f(pc, "chroma_shadows"),
        dt_cb_chroma_highlights: f(pc, "chroma_highlights"),
        dt_cb_chroma_global: f(pc, "chroma_global"),
        dt_cb_chroma_midtones: f(pc, "chroma_midtones"),
        dt_cb_saturation_global: f(pc, "saturation_global"),
        dt_cb_saturation_highlights: f(pc, "saturation_highlights"),
        dt_cb_saturation_midtones: f(pc, "saturation_midtones"),
        dt_cb_saturation_shadows: f(pc, "saturation_shadows"),
        dt_cb_hue_angle: f(pc, "hue_angle"),
        dt_cb_brilliance_global: f(pc, "brilliance_global"),
        dt_cb_brilliance_highlights: f(pc, "brilliance_highlights"),
        dt_cb_brilliance_midtones: f(pc, "brilliance_midtones"),
        dt_cb_brilliance_shadows: f(pc, "brilliance_shadows"),
        dt_cb_mask_grey_fulcrum: f(pc, "mask_grey_fulcrum"),
        dt_cb_vibrance: f(pc, "vibrance"),
        dt_cb_grey_fulcrum: f(pc, "grey_fulcrum"),
        dt_cb_contrast: f(pc, "contrast"),
        ..Default::default()
    }
}

/// Média, p99, máxima e fração ≤ 1 e ≤ 2 níveis, sobre todos os canais.
fn medir(nome: &str, nosso: &RgbImage, referencia: &RgbImage, saida: &Path) {
    assert_eq!(
        nosso.dimensions(),
        referencia.dimensions(),
        "{nome}: tamanhos diferentes"
    );
    let mut histograma = [0u64; 256];
    let mut soma = 0u64;
    let mut mapa = RgbImage::new(nosso.width(), nosso.height());
    for ((a, b), m) in nosso
        .pixels()
        .zip(referencia.pixels())
        .zip(mapa.pixels_mut())
    {
        let mut pior = 0u8;
        for c in 0..3 {
            let d = a[c].abs_diff(b[c]);
            histograma[d as usize] += 1;
            soma += d as u64;
            pior = pior.max(d);
        }
        let v = pior.saturating_mul(16);
        *m = image::Rgb([v, v, v]);
    }
    let total: u64 = histograma.iter().sum();
    let ate = |n: usize| histograma[..=n].iter().sum::<u64>() as f64 / total as f64 * 100.0;
    let quantil = |q: f64| {
        let mut acumulado = 0u64;
        histograma
            .iter()
            .position(|h| {
                acumulado += h;
                acumulado as f64 >= q * total as f64
            })
            .unwrap()
    };
    let maxima = histograma.iter().rposition(|h| *h > 0).unwrap();
    println!(
        "{nome:10} {}×{}  média {:.2}  p99 {}  máx {maxima}  ≤1: {:.1}%  ≤2: {:.1}%",
        nosso.width(),
        nosso.height(),
        soma as f64 / total as f64,
        quantil(0.99),
        ate(1),
        ate(2),
    );
    mapa.save(saida.join(format!("{nome}-diferenca-x16.png")))
        .unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [foto, json, referencia, saida, resto @ ..] = args.as_slice() else {
        panic!("uso: <foto.jpg> <parametros.json> <referencia.png> <saída> [lado da cópia]")
    };
    let saida = Path::new(saida);
    fs::create_dir_all(saida).unwrap();
    let ajustes =
        ajustes_do_json(&serde_json::from_str(&fs::read_to_string(json).unwrap()).unwrap());
    let original = image::open(foto).expect("abrir a foto").to_rgba8();
    let referencia = image::open(referencia)
        .expect("abrir a referência")
        .to_rgb8();
    let mut motor = Motor::abrir().expect("sem GPU");
    println!("motor: {} ({:?})", motor.backend(), motor.entrada());

    let revelar = |motor: &mut Motor, img: &image::RgbaImage, escala: f32| {
        motor.definir_escala_do_original(escala);
        let pixels = Arc::new(img.as_raw().clone());
        let t = Instant::now();
        let saida = motor
            .revelar(&pixels, img.width(), img.height(), &ajustes)
            .expect("o motor não revelou")
            .to_rgb8();
        println!("revelado em {:.2?}", t.elapsed());
        saida
    };

    let cheia = revelar(&mut motor, &original, 1.0);
    cheia.save(saida.join("motor-cheia.jpg")).unwrap();
    medir("cheia", &cheia, &referencia, saida);

    if let Some(lado) = resto.first() {
        let lado: u32 = lado.parse().expect("lado da cópia em pixels");
        let (w, h) = original.dimensions();
        let escala = lado as f32 / w.max(h) as f32;
        let (cw, ch) = (
            (w as f32 * escala).round() as u32,
            (h as f32 * escala).round() as u32,
        );
        let copia = image::imageops::resize(&original, cw, ch, FilterType::Lanczos3);
        let revelada = revelar(&mut motor, &copia, escala);
        revelada.save(saida.join("motor-copia.jpg")).unwrap();
        let ref_reduzida = image::imageops::resize(&referencia, cw, ch, FilterType::Lanczos3);
        medir("cópia", &revelada, &ref_reduzida, saida);
    }
}
