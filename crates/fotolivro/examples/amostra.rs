//! Gera um fotolivro de exemplo a partir de uma pasta de fotos — para calibrar
//! a diagramação a olho, o que o teste automatizado só mede.
//!
//! ```text
//! cargo run -p fotolivro --release --example amostra -- <pasta> <saida.pdf>
//! ```
//!
//! O nome de cada arquivo diz o estado: `01L-nome.jpg` é levada (sai limpa),
//! `02D-nome.jpg` é disponível (já com a marca d'água do servidor, que o
//! exemplo `marca_resistente` do e-commerce aplica).

use std::path::PathBuf;

use fotolivro::{Capa, Montagem};

fn main() {
    let mut args = std::env::args().skip(1);
    let pasta = PathBuf::from(args.next().expect("a pasta das fotos"));
    let saida = PathBuf::from(args.next().expect("o PDF de saída"));
    let mut nomes: Vec<_> = std::fs::read_dir(&pasta)
        .expect("a pasta")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
        .filter(|n| n.ends_with(".jpg") && n.len() > 4)
        .collect();
    nomes.sort();
    let capa = Capa {
        titulo: "Ensaio em Gramado — Família Souza".into(),
        galeria: Some("https://recordarfotos.com.br/meus-ensaios/exemplo".into()),
        site: "recordarfotos.com.br".into(),
        lugar_e_data: "Gramado · 9 de outubro de 2026".into(),
        agendar: Some("https://recordarfotos.com.br/agendar".into()),
    };
    let inicio = std::time::Instant::now();
    // Como o site e o backend fazem: abre uma, entrega, abre a próxima.
    let mut montagem = Montagem::nova(capa, nomes.len());
    for (i, n) in nomes.iter().enumerate() {
        montagem.adicionar(
            image::open(pasta.join(n)).expect("a foto"),
            n.split_once('-').map_or(n.as_str(), |(_, r)| r).to_string(),
            n.get(2..3) == Some("L"),
            Some(format!(
                "https://recordarfotos.com.br/meus-ensaios/exemplo?foto={i}"
            )),
        );
    }
    let n = montagem.quantas();
    let pdf = montagem.gerar().expect("o livro");
    std::fs::write(&saida, &pdf).expect("gravar");
    println!(
        "{} fotos → {} ({:.1} MB) em {:?}",
        n,
        saida.display(),
        pdf.len() as f64 / 1e6,
        inicio.elapsed()
    );
}
