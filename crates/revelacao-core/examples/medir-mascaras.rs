//! Quanto custam as máscaras locais: rasterização, composição e memória.
//!
//! ```text
//! cargo run --release -p revelacao-core --example medir-mascaras
//! ```
//!
//! 🚨 Sempre em `--release` — ver o CLAUDE.md. Mede a revelação inteira (subida
//! já feita, leitura de volta incluída) em quatro situações, no preview de
//! 2048 px e no arquivo de 24 MP:
//!
//! - sem máscara (a régua);
//! - com a revelação pela primeira vez (rasteriza tudo);
//! - com a mesma revelação (só compõe — a máscara está no cache);
//! - com um stroke a mais no fim (o arrasto: só o novo é rasterizado).

use std::sync::Arc;
use std::time::Instant;

use revelacao_core::locais::*;
use revelacao_core::{Ajustes, Motor};

fn traco(i: usize) -> Componente {
    let pontos = (0..100)
        .map(|k| {
            let t = k as f32 / 99.0;
            let y = 0.1 + 0.8 * (i as f32 / 20.0);
            [0.05 + 0.9 * t, y + 0.03 * (t * 12.0).sin(), 0.5 + 0.5 * t]
        })
        .collect();
    Componente {
        modo: Modo::Somar,
        forma: Forma::Pincel(BrushStroke {
            raio: 0.02,
            feather: 0.5,
            opacidade: 0.7,
            pontos,
        }),
    }
}

fn parametros(tracos: usize) -> ParametrosLocais {
    ParametrosLocais {
        camadas: vec![Camada {
            ajustes: AjustesLocais { exposicao_ev: 0.8 },
            componentes: (0..tracos).map(traco).collect(),
            invertida: false,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn medir(motor: &mut Motor, pixels: &Arc<Vec<u8>>, w: u32, h: u32, rotulo: &str) {
    let comeco = Instant::now();
    motor
        .revelar(pixels, w, h, &Ajustes::default())
        .expect("revelou");
    let ms = comeco.elapsed().as_secs_f64() * 1000.0;
    let m = motor.medidas_dos_locais();
    println!(
        "  {rotulo:<34} {ms:>8.1} ms   camadas refeitas {}  componentes {:>3}  máscara {:>6.1} MB  montagem {:.2} ms",
        m.camadas_refeitas,
        m.componentes_desenhados,
        m.bytes as f64 / 1_048_576.0,
        m.montagem_ms
    );
}

fn main() {
    let mut motor = Motor::abrir().expect("GPU");
    println!("backend: {}", motor.backend());
    for (w, h) in [(2048u32, 1365u32), (6000, 4000)] {
        println!("\n{w}×{h}");
        let pixels = Arc::new(
            (0..w * h)
                .flat_map(|i| [(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255])
                .collect::<Vec<u8>>(),
        );
        motor.definir_locais(&ParametrosLocais::default()).unwrap();
        medir(&mut motor, &pixels, w, h, "aquecimento (sobe a textura)");
        medir(&mut motor, &pixels, w, h, "sem máscara");
        motor.definir_locais(&parametros(20)).unwrap();
        medir(&mut motor, &pixels, w, h, "20 strokes, primeira vez");
        medir(&mut motor, &pixels, w, h, "20 strokes, de novo (cache)");
        motor.definir_locais(&parametros(21)).unwrap();
        medir(&mut motor, &pixels, w, h, "+1 stroke no fim (arrasto)");
    }
}
