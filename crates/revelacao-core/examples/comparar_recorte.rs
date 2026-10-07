//! Um recorte da foto com um círculo a remover, pelo PatchMatch (a qualidade
//! recomendada do editor) — para comparar com outros métodos lado a lado.
//!
//! ```text
//! cargo run --release -p revelacao-core --example comparar_recorte -- foto.jpg x0 y0 lado cx cy raio saida.ppm
//! ```
use revelacao_core::preenchimento::{sintetizar, Controle, Pedido, Qualidade};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n = |i: usize| a[i].parse::<u32>().unwrap();
    let foto = image::open(&a[1]).unwrap().to_rgb8();
    let (x0, y0, lado, cx, cy, raio) = (n(2), n(3), n(4), n(5), n(6), n(7));
    let mut rgba = Vec::new();
    let mut destino = Vec::new();
    for y in y0..y0 + lado {
        for x in x0..x0 + lado {
            let p = foto.get_pixel(x, y).0;
            rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            destino.push(((x as f32 - cx as f32).hypot(y as f32 - cy as f32)) < raio as f32);
        }
    }
    let inicio = std::time::Instant::now();
    let pedido = Pedido {
        rgba: &rgba,
        largura: lado,
        altura: lado,
        destino: &destino,
        amostragem: None,
        qualidade: Qualidade::recomendada(),
        semente: 1,
    };
    let r = sintetizar(&pedido, &Controle::sem_controle()).unwrap();
    eprintln!("patchmatch: {:?}", inicio.elapsed());
    let mut saida = Vec::new();
    for y in 0..lado {
        for x in 0..lado {
            let i = (y * lado + x) as usize;
            let (rx, ry) = (x as i64 - r.x0 as i64, y as i64 - r.y0 as i64);
            let p = if destino[i]
                && rx >= 0
                && ry >= 0
                && rx < r.largura as i64
                && ry < r.altura as i64
            {
                let k = ((ry as u32 * r.largura + rx as u32) * 4) as usize;
                [r.rgba[k], r.rgba[k + 1], r.rgba[k + 2]]
            } else {
                [rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2]]
            };
            saida.extend_from_slice(&p);
        }
    }
    let mut bytes = format!("P6\n{lado} {lado}\n255\n").into_bytes();
    bytes.extend_from_slice(&saida);
    std::fs::write(&a[8], bytes).unwrap();
}
