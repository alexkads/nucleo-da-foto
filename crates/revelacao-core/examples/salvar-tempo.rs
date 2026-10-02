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
    // Um P&B com o que pesa no RecordarFotos P&B (`use_cases::presets::
    // RECORDARFOTOS_PB`): Sombras locais (a guia), viragem e as duas vinhetas.
    let estilo = Ajustes {
        bw_ativo: 1.0,
        processo: 1.0,
        shadows: 100.0,
        clarity: 0.11,
        split_shadow_hue: 51.41,
        split_shadow_sat: 36.5,
        split_highlight_hue: 50.63,
        split_highlight_sat: 70.5,
        lens_vignette_amount: 98.0,
        pcv_amount: 43.75,
        ..Default::default()
    };

    let mut revelada = None;
    for (nome, ajustes) in [
        ("revelar neutro (1ª: cria texturas)", &neutro),
        ("revelar neutro (2ª)", &neutro),
        ("revelar P&B (1ª: calcula a guia)", &estilo),
        ("revelar P&B (guia em cache)", &estilo),
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
