//! A comparação lado a lado dos motores numa foto: para cada caso (uma área a
//! remover), o original, a área, o PatchMatch e a LaMa — pelos mesmos motores
//! do editor. Grava um PNG por caso e imprime tempo e onde rodou.
//!
//! ```text
//! VLB_MODELOS=… cargo run --release -p preenchimento --example comparar_metodos -- foto.jpg saida/
//! ```
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use preenchimento::{lama, patchmatch::PatchMatch, Controle, Entrada, Motor};

/// (nome, caixa da área x0 y0 x1 y1, elipse?)
const CASOS: [(&str, [u32; 4], bool); 6] = [
    ("papel-de-parede-relogio", [430, 640, 890, 1100], true),
    ("radio-sobre-a-mesa", [590, 1330, 990, 1625], false),
    ("lanterna", [425, 1320, 565, 1650], false),
    ("mala-no-piso", [230, 2100, 490, 2530], false),
    ("tecido-xadrez", [3300, 2520, 3520, 2760], true),
    ("bule-com-sombra", [2330, 1520, 2530, 1715], false),
];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let foto = image::open(&a[1]).unwrap().to_rgb8();
    let saida = std::path::PathBuf::from(&a[2]);
    std::fs::create_dir_all(&saida).unwrap();
    let ia = lama::LaMa {
        pasta: ia_local::modelos::pasta_padrao(),
        backend: ia_local::execucao::Backend::Automatico,
    };
    let motores: [(&str, &dyn Motor); 2] = [("patchmatch", &PatchMatch), ("lama", &ia)];
    println!("caso\tmotor\tms\tonde\treducao");
    for (nome, [x0, y0, x1, y1], elipse) in CASOS {
        let (bw, bh) = (x1 - x0, y1 - y0);
        // A região: a área com a margem de ¾ do lado de cada lado.
        let m = bw.max(bh) * 3 / 4 + 16;
        let (rx, ry) = (x0.saturating_sub(m), y0.saturating_sub(m));
        let (rw, rh) = (
            (x1 + m).min(foto.width()) - rx,
            (y1 + m).min(foto.height()) - ry,
        );
        let mut rgba = Vec::new();
        let mut destino = Vec::new();
        for y in ry..ry + rh {
            for x in rx..rx + rw {
                let p = foto.get_pixel(x, y).0;
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
                let dentro = if elipse {
                    let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
                    let (dx, dy) = (
                        (x as f32 - cx) / (bw as f32 / 2.0),
                        (y as f32 - cy) / (bh as f32 / 2.0),
                    );
                    dx * dx + dy * dy < 1.0
                } else {
                    (x0..x1).contains(&x) && (y0..y1).contains(&y)
                };
                destino.push(dentro);
            }
        }
        let original = image::RgbImage::from_fn(rw, rh, |x, y| *foto.get_pixel(rx + x, ry + y));
        let mut marcado = original.clone();
        for (i, d) in destino.iter().enumerate() {
            if *d {
                let p = marcado.get_pixel_mut(i as u32 % rw, i as u32 / rw);
                p.0 = [(p.0[0] / 2) + 115, p.0[1] / 2, p.0[2] / 2];
            }
        }
        marcado
            .save(saida.join(format!("{nome}-0-area.png")))
            .unwrap();
        original
            .save(saida.join(format!("{nome}-0-original.png")))
            .unwrap();
        for (motor_nome, motor) in motores {
            let entrada = Entrada {
                rgba: &rgba,
                largura: rw,
                altura: rh,
                destino: &destino,
                amostragem: None,
                semente: 1,
                contexto: 0.6,
            };
            let parado = AtomicBool::new(false);
            let inicio = Instant::now();
            let r = motor
                .preencher(
                    &entrada,
                    &Controle {
                        cancelado: &parado,
                        progresso: &|_| {},
                    },
                )
                .expect("remendo");
            let ms = inicio.elapsed().as_millis();
            let mut img = original.clone();
            for y in 0..r.altura {
                for x in 0..r.largura {
                    let (gx, gy) = (r.x0 + x, r.y0 + y);
                    if destino[(gy * rw + gx) as usize] {
                        let k = ((y * r.largura + x) * 4) as usize;
                        img.put_pixel(
                            gx,
                            gy,
                            image::Rgb([r.rgba[k], r.rgba[k + 1], r.rgba[k + 2]]),
                        );
                    }
                }
            }
            img.save(saida.join(format!("{nome}-{motor_nome}.png")))
                .unwrap();
            println!(
                "{nome}\t{motor_nome}\t{ms}\t{}\t{:.2}",
                r.executado_em, r.reducao
            );
        }
    }
}
