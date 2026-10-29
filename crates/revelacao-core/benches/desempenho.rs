//! Quanto custam os caminhos quentes da revelação na CPU —
//! `cargo bench -p revelacao-core`.
//!
//! Imprime a mediana de cada caminho, para comparar antes e depois de uma
//! mudança na mesma máquina. Não é teste: nada aqui falha. O que roda na GPU
//! fica de fora (depende da placa, não do código) — exceto o último bloco, que
//! compara os dois caminhos da prévia da tela **na mesma GPU**: o de antes
//! (revelar inteira + enquadrar, converter e histograma na CPU) e o de agora
//! (`Motor::revelar_para_tela`, tudo na GPU).

#![allow(clippy::print_stdout)]

use std::hint::black_box;

use image::{DynamicImage, RgbaImage};
use revelacao_core::guia;
use revelacao_core::preenchimento::preencher_buraco;
use revelacao_core::transformacao::{self, Corte};
use revelacao_core::{Ajustes, Motor, OrdemDosCanais, PedidoDeTela};

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
        image::Rgba([
            (x * 7 + y) as u8,
            ((y * 3) ^ x) as u8,
            ((x + y) / 5) as u8,
            255,
        ])
    }));
    let sem_corte = Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false);
    let reto = Corte::novo(0.1, 0.1, 0.8, 0.8, 0, 0.0, false, false);
    let girado = Corte::novo(0.1, 0.1, 0.8, 0.8, 1, 0.0, false, false);
    let inclinado = Corte::novo(0.1, 0.1, 0.8, 0.8, 0, 3.0, false, false);

    medir("enquadrar 2560²: sem corte, tomando a foto", 20, || {
        black_box(transformacao::aplicar_dono(
            revelada.clone(),
            &sem_corte,
            true,
        ));
    });
    medir(
        "  (o clone que a linha acima paga para ter o que tomar)",
        20,
        || {
            black_box(revelada.clone());
        },
    );
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

    // A guia da Claridade, Textura e Remover névoa: na cópia de trabalho e na
    // exportação de um arquivo de 12 MP.
    let trabalho = revelada.to_rgba8();
    medir(
        "guia (Claridade/Textura/Névoa) da cópia de 2560²",
        5,
        || {
            black_box(guia::calcular(trabalho.as_raw(), 2560, 2560));
        },
    );
    let arquivo = RgbaImage::from_fn(4000, 3000, |x, y| {
        image::Rgba([
            (x * 7 + y) as u8,
            ((y * 3) ^ x) as u8,
            ((x + y) / 5) as u8,
            255,
        ])
    });
    medir(
        "guia (Claridade/Textura/Névoa) de um arquivo de 12 MP",
        3,
        || {
            black_box(guia::calcular(arquivo.as_raw(), 4000, 3000));
        },
    );

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
        black_box(preencher_buraco(
            &estampa,
            w,
            h,
            (160, 110, 240, 190),
            &no_buraco,
            7,
        ));
    });

    previa_da_tela();
}

/// A prévia de um arrasto de slider, 2560×1707, ponta a ponta na thread do
/// motor: o caminho de antes contra o de agora (tudo na GPU). O que o de agora
/// tira é exatamente o que antes rodava **na thread da interface**, a cada
/// resultado: enquadrar, trocar R↔B e o histograma.
fn previa_da_tela() {
    let Some(mut motor) = Motor::abrir() else {
        println!("(sem GPU: a prévia da tela fica de fora)");
        return;
    };
    let (l, a) = (2560u32, 1707);
    let pixels = std::sync::Arc::new(
        (0..a)
            .flat_map(|y| {
                (0..l).flat_map(move |x| {
                    [
                        (x * 7 + y) as u8,
                        ((y * 3) ^ x) as u8,
                        ((x + y) / 5) as u8,
                        255,
                    ]
                })
            })
            .collect::<Vec<u8>>(),
    );
    let ajustes = Ajustes {
        exposure: 0.4,
        ..Ajustes::default()
    };
    let perspectiva = revelacao_core::perspectiva::Perspectiva {
        rotacao: [4.0, -3.0, 1.5],
        ..Default::default()
    };
    let casos = [
        ("sem corte", Corte::inteiro()),
        (
            "recorte reto",
            Corte::novo(0.1, 0.1, 0.8, 0.8, 0, 0.0, false, false),
        ),
        (
            "endireitar 3°",
            Corte::novo(0.1, 0.1, 0.8, 0.8, 0, 3.0, false, false),
        ),
        (
            "perspectiva",
            Corte::novo(0.05, 0.05, 0.9, 0.9, 0, 1.0, false, false).com_perspectiva(perspectiva),
        ),
    ];
    println!("\nprévia da tela, 2560×1707, backend {}:", motor.backend());
    for (nome, corte) in casos {
        motor.definir_corte(&corte);
        medir(
            &format!("  antes · {nome}: revelar + CPU (enquadrar, BGRA, histograma)"),
            10,
            || {
                let revelada = motor.revelar(&pixels, l, a, &ajustes).expect("GPU");
                let exibida = transformacao::aplicar(&revelada, &corte, true);
                let mut bytes = exibida.into_rgba8();
                for p in bytes.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                }
                // O histograma amostrado da tela (250 mil pixels).
                let passo = (bytes.pixels().len() / 250_000).max(1);
                let mut h = [0u32; 256];
                for p in bytes.pixels().step_by(passo) {
                    h[p.0[2] as usize] += 1;
                }
                black_box((bytes, h));
            },
        );
        medir(
            &format!("  agora · {nome}: revelar_para_tela (tudo na GPU)"),
            10,
            || {
                let r = motor
                    .revelar_para_tela(
                        &pixels,
                        l,
                        a,
                        &ajustes,
                        PedidoDeTela {
                            recortar: true,
                            ordem: OrdemDosCanais::Bgra,
                            inteira: false,
                        },
                    )
                    .expect("GPU");
                let passo = (r.tela.bytes.len() / 4 / 250_000).max(1);
                let mut h = [0u32; 256];
                for p in r.tela.bytes.as_chunks::<4>().0.iter().step_by(passo) {
                    h[p[2] as usize] += 1;
                }
                black_box((r, h));
            },
        );
    }
}
