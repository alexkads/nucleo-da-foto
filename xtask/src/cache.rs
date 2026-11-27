//! O cache de compilação na frente do rustc, para uma worktree nova não compilar de novo o que
//! outra já compilou.
//!
//! - Um `RUSTC_WRAPPER` já definido fica como está (o Raven CI da macpro define o dele).
//! - Sem ele, o `ravenrs-xcache` do `PATH` (o do RavenRS, `cargo install --path
//!   crates/ravenrs-xcache` lá), com o incremental desligado, porque uma compilação incremental
//!   não se guarda. No fim o cache é cortado em `NUCLEO_XCACHE_MAX` (20G).
//! - Sem nenhum dos dois, compila sem cache e diz como ter um.
//!
//! O sccache não entra: ele põe o `CARGO_TARGET_DIR` na chave e quase não acerta entre worktrees.

use std::process::Command;

use crate::run::Check;

/// Com que cache a checagem roda.
#[derive(Debug)]
pub enum Cache {
    /// O `RUSTC_WRAPPER` que já estava definido.
    DeFora,
    /// O `ravenrs-xcache` do `PATH`.
    Xcache,
    Nenhum,
}

const XCACHE: &str = "ravenrs-xcache";

impl Cache {
    /// Liga o cache para todo comando de `check`.
    pub fn liga(check: &mut Check) -> Cache {
        if let Some(wrapper) = std::env::var_os("RUSTC_WRAPPER").filter(|w| !w.is_empty()) {
            let wrapper = wrapper.to_string_lossy().into_owned();
            println!("cache de compilação: {wrapper} (RUSTC_WRAPPER)");
            return Cache::DeFora;
        }
        let versao = Command::new(XCACHE).arg("--version").output();
        if let Some(versao) = versao.ok().filter(|saida| saida.status.success()) {
            check.define_env("RUSTC_WRAPPER", XCACHE);
            check.define_env("CARGO_INCREMENTAL", "0");
            let _ = Command::new(XCACHE).args(["stats", "--reset"]).output();
            println!(
                "cache de compilação: {}",
                String::from_utf8_lossy(&versao.stdout).trim()
            );
            return Cache::Xcache;
        }
        println!("cache de compilação: nenhum. Para ter um, no RavenRS: cargo install --path crates/ravenrs-xcache");
        Cache::Nenhum
    }

    /// O que o cache fez nesta checagem, e o corte que o impede de tomar o disco.
    pub fn relata(&self) {
        if let Cache::Xcache = self {
            println!();
            let _ = Command::new(XCACHE).arg("stats").status();
            let maximo = std::env::var("NUCLEO_XCACHE_MAX").unwrap_or_else(|_| "20G".to_owned());
            let _ = Command::new(XCACHE)
                .args(["gc", "--max-size", &maximo])
                .status();
        }
    }
}
