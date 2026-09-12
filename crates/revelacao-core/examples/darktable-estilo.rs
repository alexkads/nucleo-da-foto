//! Aplica os módulos do darktable, um a um e o estilo inteiro, sobre uma imagem.
//!
//! É a metade nossa da régua: a outra metade é o `darktable-cli` 5.6.1 com os
//! mesmos parâmetros, e o `scripts/conferir-darktable.py` do e-commerce compara
//! as duas saídas pixel a pixel.
//!
//! ```text
//! cargo run --release -p revelacao-core --example darktable-estilo -- \
//!     <entrada.jpg | entrada.rgb LARGURAxALTURA> <parametros.json> <pasta de saída>
//! ```
//!
//! Para cada variante escreve `<nome>.rgb` (8 bits, sem cabeçalho) e `<nome>.jpg`.
//! As variantes são `base` (só a tubulação de cor), cada módulo ativo sozinho,
//! e `estilo` — os cinco na ordem do pipeline do darktable.
use std::{fs, path::Path, time::Instant};

use image::codecs::jpeg::JpegEncoder;
use revelacao_core::darktable::{
    color_balance_rgb, exposure, monochrome, shadhi, vignette, ColorBalanceRgb, Exposure,
    Monochrome, Rgb, ShadowsHighlights, Tubulacao, Vignette,
};
use serde_json::Value;

fn f(v: &Value, k: &str) -> f32 {
    v[k].as_f64()
        .unwrap_or_else(|| panic!("falta o campo `{k}`")) as f32
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (entrada, resto) = args
        .split_first()
        .expect("uso: <entrada> [LxA] <parametros.json> <saída>");
    let (largura, altura, rgb8): (usize, usize, Vec<u8>) = if entrada.ends_with(".rgb") {
        let [dims, ..] = resto else {
            panic!("entrada .rgb pede LARGURAxALTURA")
        };
        let (l, a) = dims.split_once('x').expect("LARGURAxALTURA");
        (
            l.parse().unwrap(),
            a.parse().unwrap(),
            fs::read(entrada).unwrap(),
        )
    } else {
        let img = image::open(entrada).expect("abrir a imagem").to_rgb8();
        (img.width() as usize, img.height() as usize, img.into_raw())
    };
    let resto = if entrada.ends_with(".rgb") {
        &resto[1..]
    } else {
        resto
    };
    let [json, saida, ..] = resto else {
        panic!("faltam parametros.json e a pasta de saída")
    };
    let p: Value = serde_json::from_str(&fs::read_to_string(json).unwrap()).unwrap();
    fs::create_dir_all(saida).unwrap();

    let tub = Tubulacao::nova();
    let original: Vec<Rgb> = rgb8
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| tub.entrar(*c))
        .collect();

    let pe = &p["exposure"];
    let ex = Exposure {
        black: f(pe, "black"),
        exposure: f(pe, "exposure"),
    };
    let ps = &p["shadhi"];
    let sh = ShadowsHighlights {
        radius: f(ps, "radius"),
        shadows: f(ps, "shadows"),
        whitepoint: f(ps, "whitepoint"),
        highlights: f(ps, "highlights"),
        compress: f(ps, "compress"),
        shadows_ccorrect: f(ps, "shadows_ccorrect"),
        highlights_ccorrect: f(ps, "highlights_ccorrect"),
        flags: ps["flags"].as_u64().unwrap() as u32,
        low_approximation: f(ps, "low_approximation"),
    };
    assert_eq!(
        ps["shadhi_algo"].as_i64(),
        Some(1),
        "só o algoritmo bilateral está portado"
    );
    let pm = &p["monochrome"];
    let mo = Monochrome {
        a: f(pm, "a"),
        b: f(pm, "b"),
        size: f(pm, "size"),
        highlights: f(pm, "highlights"),
    };
    let pv = &p["vignette"];
    let vi = Vignette {
        scale: f(pv, "scale"),
        falloff_scale: f(pv, "falloff_scale"),
        brightness: f(pv, "brightness"),
        saturation: f(pv, "saturation"),
        center: [f(pv, "center_x"), f(pv, "center_y")],
        autoratio: pv["autoratio"].as_i64() == Some(1),
        whratio: f(pv, "whratio"),
        shape: f(pv, "shape"),
        unbound: pv["unbound"].as_i64() == Some(1),
    };
    assert_eq!(
        pv["dithering"].as_i64(),
        Some(0),
        "pontilhamento não está portado"
    );
    let pc = &p["colorbalancergb"];
    assert_eq!(
        pc["saturation_formula"].as_i64(),
        Some(1),
        "só a fórmula dt UCS está portada"
    );
    let cb = ColorBalanceRgb {
        shadows_y: f(pc, "shadows_Y"),
        shadows_c: f(pc, "shadows_C"),
        shadows_h: f(pc, "shadows_H"),
        midtones_y: f(pc, "midtones_Y"),
        midtones_c: f(pc, "midtones_C"),
        midtones_h: f(pc, "midtones_H"),
        highlights_y: f(pc, "highlights_Y"),
        highlights_c: f(pc, "highlights_C"),
        highlights_h: f(pc, "highlights_H"),
        global_y: f(pc, "global_Y"),
        global_c: f(pc, "global_C"),
        global_h: f(pc, "global_H"),
        shadows_weight: f(pc, "shadows_weight"),
        white_fulcrum: f(pc, "white_fulcrum"),
        highlights_weight: f(pc, "highlights_weight"),
        chroma_shadows: f(pc, "chroma_shadows"),
        chroma_highlights: f(pc, "chroma_highlights"),
        chroma_global: f(pc, "chroma_global"),
        chroma_midtones: f(pc, "chroma_midtones"),
        saturation_global: f(pc, "saturation_global"),
        saturation_highlights: f(pc, "saturation_highlights"),
        saturation_midtones: f(pc, "saturation_midtones"),
        saturation_shadows: f(pc, "saturation_shadows"),
        hue_angle: f(pc, "hue_angle"),
        brilliance_global: f(pc, "brilliance_global"),
        brilliance_highlights: f(pc, "brilliance_highlights"),
        brilliance_midtones: f(pc, "brilliance_midtones"),
        brilliance_shadows: f(pc, "brilliance_shadows"),
        mask_grey_fulcrum: f(pc, "mask_grey_fulcrum"),
        vibrance: f(pc, "vibrance"),
        grey_fulcrum: f(pc, "grey_fulcrum"),
        contrast: f(pc, "contrast"),
    };

    type Passo<'a> = Box<dyn Fn(&mut Vec<Rgb>) + 'a>;
    let passo_ex: Passo = Box::new(|px| exposure(px, ex));
    let passo_sh: Passo = Box::new(|px| shadhi(px, largura, altura, &tub, sh, 1.0));
    let passo_mo: Passo = Box::new(|px| monochrome(px, largura, altura, &tub, mo, 1.0));
    let passo_vi: Passo = Box::new(|px| vignette(px, largura, altura, vi));
    let passo_cb: Passo = Box::new(|px| color_balance_rgb(px, &tub, &cb));
    let modulos: [(&str, &Passo); 5] = [
        ("exposure", &passo_ex),
        ("shadhi", &passo_sh),
        ("monochrome", &passo_mo),
        ("vignette", &passo_vi),
        ("colorbalancergb", &passo_cb),
    ];

    let gravar = |nome: &str, px: &[Rgb]| {
        let bytes: Vec<u8> = px.iter().flat_map(|c| tub.sair(*c)).collect();
        fs::write(Path::new(saida).join(format!("{nome}.rgb")), &bytes).unwrap();
        let arquivo = fs::File::create(Path::new(saida).join(format!("{nome}.jpg"))).unwrap();
        JpegEncoder::new_with_quality(arquivo, 95)
            .encode(
                &bytes,
                largura as u32,
                altura as u32,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
    };

    gravar("base", &original);
    for (nome, passo) in modulos.iter() {
        let t = Instant::now();
        let mut px = original.clone();
        passo(&mut px);
        gravar(nome, &px);
        eprintln!("{nome:16} {:>7.2?}", t.elapsed());
    }
    let t = Instant::now();
    let mut px = original.clone();
    for (_, passo) in modulos.iter() {
        passo(&mut px);
    }
    gravar("estilo", &px);
    eprintln!("{:16} {:>7.2?}", "estilo", t.elapsed());
}
