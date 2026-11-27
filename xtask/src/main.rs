//! `cargo xtask <tarefa>`: as checagens do núcleo, um subcomando por finalidade (#9).
//!
//! É Rust, então roda igual no macOS, no Linux e no Windows, e só usa a biblioteca padrão, então
//! compila em segundos antes de qualquer checagem começar.

// Imprimir o andamento é o trabalho deste binário.
#![allow(clippy::print_stdout)]

mod cache;
mod checks;
mod faxina;
mod json;
mod run;
mod selecao;

use std::path::PathBuf;

const AJUDA: &str = "\
cargo xtask <tarefa>: as checagens do núcleo, uma tarefa por finalidade.

  dev             enquanto se trabalha, antes do commit (minutos): o formato, e o clippy e os
                  testes só dos pacotes alterados
  pr              o gate do PR, no Raven CI: o clippy em todo pacote que a mudança alcança, os
                  testes dos alterados e dos dependentes diretos, e o wasm32, a documentação e a
                  bancada quando a mudança os alcança
  release         antes de lançar uma tag: o workspace inteiro, com wasm32, documentação,
                  cargo deny, o Rust mínimo e a API pública contra a última tag

  cobertura       a cobertura por arquivo (cargo-llvm-cov), com piso de 90 %; HTML em target/cobertura
  bancada         a janela que mede o núcleo contra a última tag, com a sua foto
  bancada-testar  os testes da bancada, sem janela
  faxina          apaga já o incremental e as árvores da cobertura e do semver do target

As tarefas não recebem argumentos: cada uma faz uma coisa. O dev e o pr comparam com origin/main,
ou com CHECK_BASE. Os testes rodam no cargo-nextest (cargo install cargo-nextest --locked).
";

type Tarefa = fn(&mut run::Check) -> Result<&'static str, run::Falhou>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(despacha(&args));
}

fn despacha(args: &[String]) -> i32 {
    let raiz = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let (nome, resto) = match args.split_first() {
        Some((nome, resto)) => (nome.as_str(), resto),
        None => ("ajuda", &[][..]),
    };
    let (tarefa, precisa_do_nextest): (Tarefa, bool) = match nome {
        "dev" => (checks::dev, true),
        "pr" => (checks::pr, true),
        "release" => (checks::release, true),
        "cobertura" => (checks::cobertura, false),
        "bancada" => (checks::bancada, false),
        "bancada-testar" => (checks::bancada_testar, true),
        "faxina" => (checks::faxina_agora, false),
        "ajuda" | "help" | "--help" | "-h" => {
            print!("{AJUDA}");
            return 0;
        }
        outro => {
            eprintln!("erro: não há tarefa chamada {outro}\n\n{AJUDA}");
            return 2;
        }
    };
    if !resto.is_empty() {
        eprintln!(
            "erro: cargo xtask {nome} não recebe argumentos, e recebeu: {}",
            resto.join(" ")
        );
        eprintln!("Cada checagem faz uma coisa: dev enquanto se trabalha, pr para um PR, release antes de lançar.");
        return 2;
    }
    let mut check = match run::Check::nova(nome, &raiz) {
        Ok(check) => check,
        Err(falhou) => {
            eprintln!("erro: {falhou}");
            return falhou.codigo.max(1);
        }
    };
    if precisa_do_nextest && !check.tem("cargo", &["nextest", "--version"]) {
        eprintln!("erro: as checagens rodam os testes no cargo-nextest; instale uma vez com:");
        eprintln!("  cargo install cargo-nextest --locked");
        return 1;
    }
    let cache = cache::Cache::liga(&mut check);
    let mut passou = "";
    let feito = tarefa(&mut check).map(|mensagem| passou = mensagem);
    check.termina(feito, passou, &cache)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn um_modo_com_argumento_e_recusado_sem_rodar_nada() {
        for args in [["pr", "--full"], ["dev", "--tudo"], ["release", "rapido"]] {
            let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
            assert_eq!(despacha(&args), 2, "{args:?}");
        }
    }

    #[test]
    fn uma_tarefa_que_nao_existe_e_recusada() {
        assert_eq!(despacha(&["verificar".to_owned()]), 2);
        assert_eq!(despacha(&["ajuda".to_owned()]), 0);
    }
}
