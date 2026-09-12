//! Quanto custa, etapa a etapa, o trabalho local de uma foto no "Salvar na
//! galeria": decodificar, revelar na GPU, enquadrar e codificar o JPEG.
//!
//! É a metade do tempo que não depende da rede. No navegador o decode é o
//! nativo (mais rápido que o `image`) e o resto roda em wasm (mais lento que
//! este nativo) — os números servem para ver **onde** o tempo está.
//!
//! ```text
//! cargo run --release -p revelacao-core --example salvar-tempo -- <foto.jpg>
//! ```
use std::{sync::Arc, time::Instant};

use revelacao_core::{jpeg, transformacao, Ajustes, Corte, Motor};

fn main() {
    let foto = std::env::args().nth(1).expect("uso: <foto.jpg>");
    let medir = |nome: &str, t: Instant| {
        println!("{nome:40} {:>8.1} ms", t.elapsed().as_secs_f64() * 1000.0)
    };

    let t = Instant::now();
    let imagem = image::open(&foto).expect("abrir").to_rgba8();
    medir("decodificar (image, nativo)", t);
    let (w, h) = imagem.dimensions();
    println!("foto {w}×{h}");

    let t = Instant::now();
    let pixels = Arc::new(imagem.as_raw().to_vec());
    medir("cópia dos pixels (o to_vec do wasm)", t);

    let mut motor = Motor::abrir().expect("sem GPU");
    let neutro = Ajustes::default();
    let estilo = Ajustes {
        dt_exposure_ativo: 1.0,
        dt_exposure_black: -0.0019,
        dt_exposure_exposure: 0.163,
        dt_shadhi_ativo: 1.0,
        dt_shadhi_shadows: 65.38,
        dt_shadhi_highlights: -20.51,
        dt_monochrome_ativo: 1.0,
        dt_vignette_ativo: 1.0,
        dt_vignette_scale: 87.82,
        dt_vignette_falloff_scale: 45.51,
        dt_vignette_brightness: 0.99999,
        dt_vignette_saturation: 0.147,
        dt_vignette_autoratio: 1.0,
        dt_vignette_shape: 0.48,
        dt_cb_ativo: 1.0,
        dt_cb_shadows_c: 0.1747,
        dt_cb_shadows_h: 71.54,
        dt_cb_midtones_h: 73.85,
        dt_cb_highlights_y: 0.0449,
        dt_cb_highlights_c: 0.0833,
        dt_cb_highlights_h: 71.54,
        dt_cb_saturation_highlights: 0.1603,
        dt_cb_saturation_midtones: 0.1346,
        dt_cb_brilliance_midtones: 0.1474,
        ..Default::default()
    };

    let mut revelada = None;
    for (nome, ajustes) in [
        ("revelar neutro (1ª: cria texturas)", &neutro),
        ("revelar neutro (2ª)", &neutro),
        ("revelar estilo P&B (grades em CPU)", &estilo),
        ("revelar estilo P&B (grades em cache)", &estilo),
    ] {
        let t = Instant::now();
        revelada = Some(motor.revelar(&pixels, w, h, ajustes).expect("revelar"));
        medir(nome, t);
    }
    let revelada = revelada.unwrap();

    let t = Instant::now();
    let enquadrada = transformacao::aplicar(&revelada, &Corte::inteiro(), true);
    medir("enquadrar sem corte (clone)", t);

    {
        let qualidade = 92u8;
        let t = Instant::now();
        let bytes = jpeg::codificar(&enquadrada, qualidade).expect("codificar");
        medir(&format!("codificar JPEG q{qualidade} (image)"), t);
        println!("{:40} {:>8.1} MB", "tamanho", bytes.len() as f64 / 1e6);
    }
}
