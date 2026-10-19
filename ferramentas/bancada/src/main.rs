//! A bancada: `cargo run --release` abre a janela; `--sem-janela [foto]`
//! imprime a mesma tabela no terminal.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--sem-janela") {
        let caminho = args.iter().find(|a| !a.starts_with("--"));
        std::process::exit(bancada::janela::sem_janela(
            caminho.map(std::path::Path::new),
        ));
    }
    bancada::janela::abrir();
}
