//! Quanto custam os caminhos quentes do RAW — `cargo bench -p raw-codec`.
//!
//! Imprime a mediana de cada caminho, para comparar antes e depois de uma
//! mudança na mesma máquina. Não é teste: nada aqui falha.

#![allow(clippy::print_stdout)]

use std::hint::black_box;

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
    // Um CR3 de 40 MB sem XMP: a busca anda o arquivo inteiro.
    let sem_xmp: Vec<u8> = (0..40_000_000u32).map(|i| (i % 251) as u8).collect();
    medir("xmp: procurar num arquivo de 40 MB sem XMP", 10, || {
        black_box(raw_codec::xmp(black_box(&sem_xmp)));
    });
}
