//! Mantém o `target/` num tamanho de trabalho. Era o `scripts/faxina-se-preciso.sh`.
//!
//! Em 2026-10-18, numa leva de testes, cobertura e semver, os `target/` do PhotoKads chegaram a
//! 72 GiB (21 só neste núcleo), com 19 GiB livres no disco.
//!
//! - Sai o **incremental** de cada perfil e alvo, e as árvores da cobertura
//!   (`target/llvm-cov-target`) e do semver (`target/semver-checks`), que só servem enquanto o
//!   comando roda.
//! - **Não é `cargo clean`:** as dependências compiladas ficam, e a próxima build leva segundos.
//! - **O gatilho é o tamanho:** as checagens chamam a faxina no começo, e ela só age acima de 8 GiB.
//!   `cargo xtask faxina` apaga já.
//! - Nunca falha a checagem que a chamou.

use std::fs;
use std::path::{Path, PathBuf};

use crate::run::Check;

/// Acima disto as checagens limpam antes de começar.
const TETO_GIB: f64 = 8.0;

/// As árvores que só servem enquanto o comando delas roda.
pub const ARVORES_DE_UMA_VEZ: &[&str] = &["llvm-cov-target", "semver-checks"];

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// O tamanho de uma pasta, somando os arquivos sem seguir links.
fn tamanho(pasta: &Path) -> u64 {
    let Ok(entradas) = fs::read_dir(pasta) else {
        return 0;
    };
    entradas
        .flatten()
        .map(|entrada| match entrada.file_type() {
            Ok(tipo) if tipo.is_dir() => tamanho(&entrada.path()),
            Ok(tipo) if tipo.is_file() => entrada.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// As pastas `incremental` até três níveis abaixo do target: `debug/incremental`, e nos alvos
/// cruzados `wasm32-unknown-unknown/debug/incremental`.
pub fn incrementais(target: &Path) -> Vec<PathBuf> {
    let mut achadas = Vec::new();
    let mut fila = vec![(target.to_owned(), 0)];
    while let Some((pasta, nivel)) = fila.pop() {
        let Ok(entradas) = fs::read_dir(&pasta) else {
            continue;
        };
        for entrada in entradas.flatten() {
            if !entrada.file_type().is_ok_and(|tipo| tipo.is_dir()) {
                continue;
            }
            if entrada.file_name() == "incremental" {
                achadas.push(entrada.path());
            } else if nivel < 2 {
                fila.push((entrada.path(), nivel + 1));
            }
        }
    }
    achadas.sort();
    achadas
}

/// Limpa o target: sempre com `agora`, e senão só acima do teto.
pub fn faxina(check: &Check, agora: bool) {
    let target = &check.target;
    if !target.is_dir() {
        println!("🧹 [Faxina] Não há {}: nada a fazer.", target.display());
        return;
    }
    let antes = tamanho(target);
    if !agora && gib(antes) < TETO_GIB {
        println!(
            "🧹 [Faxina] target com {:.1} GiB, abaixo do teto de {TETO_GIB} GiB: nada a fazer.",
            gib(antes)
        );
        return;
    }
    println!(
        "🧹 [Faxina] target com {:.1} GiB: limpando os fragmentos incrementais.",
        gib(antes)
    );
    let arvores = ARVORES_DE_UMA_VEZ
        .iter()
        .map(|nome| target.join(nome))
        .filter(|p| p.is_dir());
    for pasta in incrementais(target).into_iter().chain(arvores) {
        println!(
            "   • {:.1} GiB em {}",
            gib(tamanho(&pasta)),
            pasta.display()
        );
        if let Err(erro) = fs::remove_dir_all(&pasta) {
            println!("     (não saiu: {erro})");
        }
    }
    // O cargo-sweep tira o que sobrou de builds antigas sem tocar no que a árvore de hoje usa.
    if check.tem("cargo", &["sweep", "--version"]) {
        println!("   • cargo sweep: artefatos sem uso há mais de 14 dias");
        let _ = check.comando("cargo", &["sweep", "--time", "14"]).output();
    } else {
        println!(
            "   • (dica: `cargo install cargo-sweep` tira também os artefatos de builds antigas)"
        );
    }
    let depois = tamanho(target);
    println!(
        "🧹 [Faxina] {:.1} GiB → {:.1} GiB (liberados {:.1} GiB).",
        gib(antes),
        gib(depois),
        gib(antes.saturating_sub(depois))
    );
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn acha_o_incremental_nativo_e_o_cruzado_e_nada_mais_fundo() {
        let raiz = std::env::temp_dir().join(format!("xtask-faxina-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        for pasta in [
            "debug/incremental/a",
            "release/incremental",
            "wasm32-unknown-unknown/debug/incremental",
            "debug/deps",
            "a/b/c/incremental",
        ] {
            fs::create_dir_all(raiz.join(pasta)).unwrap();
        }
        fs::write(raiz.join("debug/incremental/a/x"), [0u8; 10]).unwrap();
        let achadas = incrementais(&raiz);
        fs::remove_dir_all(&raiz).unwrap();
        let nomes: Vec<String> = achadas
            .iter()
            .map(|p| {
                p.strip_prefix(&raiz)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            nomes,
            [
                "debug/incremental",
                "release/incremental",
                "wasm32-unknown-unknown/debug/incremental"
            ]
        );
    }
}
