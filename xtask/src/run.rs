//! Como uma checagem roda: as etapas uma depois da outra, cada uma cronometrada, a primeira falha
//! parando tudo, e a tabela de tempos no fim, passando ou falhando.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::json::Json;

/// Por que a checagem parou: o comando que falhou e o código com que terminou.
#[derive(Debug)]
pub struct Falhou {
    /// O que rodou, como se digitaria.
    pub oque: String,
    /// O código de saída, ou 1 quando nem começou ou foi morto.
    pub codigo: i32,
}

impl fmt::Display for Falhou {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` falhou com código {}", self.oque, self.codigo)
    }
}

/// Uma falha que não é de comando: um seletor que não leu o git, uma ferramenta que falta.
pub fn falha(oque: impl Into<String>) -> Falhou {
    Falhou {
        oque: oque.into(),
        codigo: 1,
    }
}

/// O que uma etapa dá: feito, ou a falha que para a checagem.
pub type Feito = Result<(), Falhou>;

fn como_digitado(programa: &str, args: &[&str]) -> String {
    std::iter::once(programa)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Uma checagem rodando.
#[derive(Debug)]
pub struct Check {
    /// O nome: `dev`, `pr` ou `release`, e depois o que achou (`pr, tudo`).
    pub modo: String,
    /// A raiz do repositório, onde todo comando roda.
    pub raiz: PathBuf,
    /// A pasta de build, como o `cargo metadata` diz.
    pub target: PathBuf,
    /// As variáveis que todo comando recebe além das deste processo: o cache de compilação.
    env: Vec<(String, OsString)>,
    etapa: Option<(String, Instant)>,
    inicio: Instant,
    /// Segundos, tipo (`etapa`, `parte` ou `FALHOU`) e título, uma linha por etapa.
    tempos: Vec<(u64, &'static str, String)>,
    /// Se a pasta de build começou vazia: muda tudo no tempo.
    build_vazio: bool,
}

impl Check {
    /// Uma checagem chamada `modo`, rodando no repositório em `raiz`.
    pub fn nova(modo: &str, raiz: &Path) -> Result<Check, Falhou> {
        let mut check = Check {
            modo: modo.to_owned(),
            raiz: raiz.to_owned(),
            target: raiz.join("target"),
            env: Vec::new(),
            etapa: None,
            inicio: Instant::now(),
            tempos: Vec::new(),
            build_vazio: true,
        };
        let metadata = check.metadata()?;
        if let Some(target) = metadata.campo("target_directory").and_then(Json::texto) {
            check.target = PathBuf::from(target);
        }
        check.build_vazio =
            fs::read_dir(check.target.join("debug/deps")).map_or(true, |mut d| d.next().is_none());
        Ok(check)
    }

    /// O `cargo metadata --no-deps` deste workspace.
    pub fn metadata(&self) -> Result<Json, Falhou> {
        let texto = self.saida(
            "cargo",
            &[
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ],
        )?;
        Json::ler(&texto).map_err(|erro| falha(format!("cargo metadata: {erro}")))
    }

    /// Define uma variável para todo comando que vem depois.
    pub fn define_env(&mut self, nome: &str, valor: impl Into<OsString>) {
        self.env.retain(|(conhecido, _)| conhecido != nome);
        self.env.push((nome.to_owned(), valor.into()));
    }

    /// Começa uma etapa e fecha a anterior.
    pub fn etapa(&mut self, titulo: &str) {
        self.fecha("etapa");
        println!("\n\x1b[1m==> {titulo}\x1b[0m");
        self.etapa = Some((titulo.to_owned(), Instant::now()));
    }

    fn fecha(&mut self, tipo: &'static str) {
        if let Some((titulo, comeco)) = self.etapa.take() {
            self.tempos.push((comeco.elapsed().as_secs(), tipo, titulo));
        }
    }

    /// Um comando pronto para rodar na raiz, com as variáveis da checagem.
    pub fn comando(&self, programa: &str, args: &[&str]) -> Command {
        let mut comando = Command::new(programa);
        comando.args(args).current_dir(&self.raiz);
        for (nome, valor) in &self.env {
            comando.env(nome, valor);
        }
        comando
    }

    /// Roda um comando na raiz; falha a checagem se não terminar com código 0.
    pub fn roda(&self, programa: &str, args: &[&str]) -> Feito {
        self.roda_comando(self.comando(programa, args), programa, args)
    }

    /// Roda um comando montado à parte (com outra pasta ou uma variável só dele).
    pub fn roda_comando(&self, mut comando: Command, programa: &str, args: &[&str]) -> Feito {
        let oque = como_digitado(programa, args);
        let status = comando
            .status()
            .map_err(|erro| falha(format!("{oque}: {erro}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Falhou {
                oque,
                codigo: status.code().unwrap_or(1),
            })
        }
    }

    /// Roda um comando cuja saída se lê: o que escreveu no stdout, se terminou com código 0.
    pub fn saida(&self, programa: &str, args: &[&str]) -> Result<String, Falhou> {
        let oque = como_digitado(programa, args);
        let saida = self
            .comando(programa, args)
            .output()
            .map_err(|erro| falha(format!("{oque}: {erro}")))?;
        if saida.status.success() {
            Ok(String::from_utf8_lossy(&saida.stdout).into_owned())
        } else {
            std::io::stderr().write_all(&saida.stderr).ok();
            Err(Falhou {
                oque,
                codigo: saida.status.code().unwrap_or(1),
            })
        }
    }

    /// Se a ferramenta responde a `--version`: `cargo deny`, `cargo nextest`.
    pub fn tem(&self, programa: &str, args: &[&str]) -> bool {
        self.comando(programa, args)
            .output()
            .is_ok_and(|saida| saida.status.success())
    }

    /// Uma parte da etapa atual, cronometrada à parte.
    pub fn parte(&mut self, titulo: &str, programa: &str, args: &[&str]) -> Feito {
        let comeco = Instant::now();
        let feito = self.roda(programa, args);
        self.tempos
            .push((comeco.elapsed().as_secs(), "parte", titulo.to_owned()));
        feito
    }

    /// Termina a checagem: a tabela de tempos, gravada em `target/check-timings/last.txt`, e a
    /// linha que diz como foi. Devolve o código de saída do processo.
    pub fn termina(mut self, feito: Feito, passou: &str, cache: &crate::cache::Cache) -> i32 {
        let codigo = match &feito {
            Ok(()) => 0,
            Err(falhou) => falhou.codigo.max(1),
        };
        self.fecha(if codigo == 0 { "etapa" } else { "FALHOU" });
        let total = self.inicio.elapsed().as_secs();
        let tabela = tabela(&self.modo, self.build_vazio, codigo, total, &self.tempos);
        println!("\n{tabela}");
        let pasta = self.target.join("check-timings");
        if fs::create_dir_all(&pasta)
            .and_then(|()| fs::write(pasta.join("last.txt"), &tabela))
            .is_ok()
        {
            println!("(gravada em {})", pasta.join("last.txt").display());
        }
        cache.relata();
        match feito {
            Ok(()) => println!("\n\x1b[1;32m{passou} em {}.\x1b[0m", minutos(total)),
            Err(falhou) => eprintln!("\n\x1b[1;31m{falhou}.\x1b[0m"),
        }
        codigo
    }
}

fn minutos(segundos: u64) -> String {
    format!("{}m{:02}s", segundos / 60, segundos % 60)
}

/// A tabela de tempos, em texto, igual na tela e no arquivo.
pub fn tabela(
    modo: &str,
    build_vazio: bool,
    codigo: i32,
    total: u64,
    tempos: &[(u64, &str, String)],
) -> String {
    let build = if build_vazio {
        "build vazio"
    } else {
        "build reaproveitado"
    };
    let situacao = if codigo == 0 {
        "passou".to_owned()
    } else {
        format!("falhou ({codigo})")
    };
    let mut texto = format!(
        "cargo xtask {modo}: {situacao}, {build}, total {}\n",
        minutos(total)
    );
    texto.push_str("| tempo | etapa |\n|---:|---|\n");
    for (segundos, tipo, titulo) in tempos {
        let titulo = match *tipo {
            "parte" => format!("↳ {titulo}"),
            "FALHOU" => format!("**{titulo} (falhou)**"),
            _ => titulo.clone(),
        };
        texto.push_str(&format!("| {} | {titulo} |\n", minutos(*segundos)));
    }
    texto
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_tabela_marca_as_partes_e_a_etapa_que_falhou() {
        let tempos = [
            (3, "etapa", "rustfmt".to_owned()),
            (61, "parte", "compilar os testes".to_owned()),
            (125, "FALHOU", "testes".to_owned()),
        ];
        let texto = tabela("pr", false, 101, 189, &tempos);
        assert!(
            texto.starts_with("cargo xtask pr: falhou (101), build reaproveitado, total 3m09s\n")
        );
        assert!(texto.contains("| 0m03s | rustfmt |"));
        assert!(texto.contains("| 1m01s | ↳ compilar os testes |"));
        assert!(texto.contains("| 2m05s | **testes (falhou)** |"));
    }
}
