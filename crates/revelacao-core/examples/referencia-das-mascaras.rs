//! A referência do desktop para conferir as máscaras no navegador.
//!
//! ```text
//! cargo run --release -p revelacao-core --example referencia-das-mascaras -- <pasta>
//! ```
//!
//! Grava em `<pasta>`:
//!
//! - `receita.json`: a receita local (pincel com pressão e feather, os dois
//!   gradientes, um Subtrair e uma camada invertida);
//! - `referencia.rgba`: a foto sintética (`320×200`, a mesma que a página de
//!   teste gera em JavaScript) revelada pelo motor do desktop, sem enquadramento;
//! - `sem-mascara.rgba`: a mesma, sem a receita — para a página provar que a
//!   máscara fez efeito, e não só que os dois lados concordam.
//!
//! A página abre o `Exportador` do `revelacao-web` (WebGPU, ou WebGL2 com
//! `navigator.gpu` removido), exporta a mesma foto com a mesma receita e compara.

use std::sync::Arc;

use revelacao_core::locais::*;
use revelacao_core::{Ajustes, Motor};

pub const LARGURA: u32 = 320;
pub const ALTURA: u32 = 200;

/// A foto sintética — `foto()` da página de teste faz a mesma conta.
fn foto() -> Vec<u8> {
    (0..ALTURA)
        .flat_map(|y| {
            (0..LARGURA).flat_map(move |x| {
                [
                    (x * 255 / LARGURA) as u8,
                    (y * 255 / ALTURA) as u8,
                    ((x ^ y) & 127) as u8 + 64,
                    255,
                ]
            })
        })
        .collect()
}

fn receita() -> ReceitaLocal {
    let pincel = |pontos: Vec<[f32; 3]>, raio, feather, opacidade, modo| Componente {
        modo,
        forma: Forma::Pincel(BrushStroke {
            raio,
            feather,
            opacidade,
            pontos,
        }),
    };
    ReceitaLocal {
        camadas: vec![
            Camada {
                ajustes: AjustesLocais { exposicao_ev: 1.5 },
                componentes: vec![
                    pincel(
                        vec![
                            [0.1, 0.2, 0.4],
                            [0.5, 0.3, 1.0],
                            [0.2, 0.35, 0.7],
                            [0.7, 0.4, 1.0],
                        ],
                        0.06,
                        0.5,
                        0.8,
                        Modo::Somar,
                    ),
                    Componente {
                        modo: Modo::Somar,
                        forma: Forma::Linear(GradienteLinear {
                            inicio: [0.5, 1.0],
                            fim: [0.5, 0.65],
                        }),
                    },
                    Componente {
                        modo: Modo::Somar,
                        forma: Forma::Radial(GradienteRadial {
                            centro: [0.8, 0.5],
                            raio_x: 0.12,
                            raio_y: 0.07,
                            angulo: 25.0,
                            feather: 0.4,
                            fora: false,
                        }),
                    },
                    pincel(vec![[0.4, 0.3, 1.0]], 0.04, 0.2, 1.0, Modo::Subtrair),
                ],
                invertida: false,
            },
            Camada {
                ajustes: AjustesLocais { exposicao_ev: -1.0 },
                componentes: vec![pincel(
                    vec![[0.3, 0.8, 1.0], [0.6, 0.85, 1.0]],
                    0.05,
                    0.3,
                    1.0,
                    Modo::Somar,
                )],
                invertida: true,
            },
        ],
        ..Default::default()
    }
}

fn main() {
    let pasta = std::env::args().nth(1).expect("a pasta de saída");
    let pasta = std::path::Path::new(&pasta);
    std::fs::create_dir_all(pasta).unwrap();

    let receita = receita();
    std::fs::write(pasta.join("receita.json"), receita.em_json().unwrap()).unwrap();

    let mut motor = Motor::abrir().expect("GPU");
    let pixels = Arc::new(foto());
    let ajustes = Ajustes::default();
    motor.definir_locais(&receita).unwrap();
    let com = motor
        .revelar(&pixels, LARGURA, ALTURA, &ajustes)
        .unwrap()
        .into_rgba8()
        .into_raw();
    std::fs::write(pasta.join("referencia.rgba"), &com).unwrap();

    motor.definir_locais(&ReceitaLocal::default()).unwrap();
    let sem = motor
        .revelar(&pixels, LARGURA, ALTURA, &ajustes)
        .unwrap()
        .into_rgba8()
        .into_raw();
    std::fs::write(pasta.join("sem-mascara.rgba"), &sem).unwrap();
    println!("gravado em {} ({})", pasta.display(), motor.backend());
}
