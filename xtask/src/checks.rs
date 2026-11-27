//! As três camadas e as tarefas avulsas.
//!
//! 🔑 A cobertura não se perde: o que sai do `dev` está no `pr`, e o que sai do `pr` está no
//! `release`. O `release` é o antigo `make verificar` inteiro, mais o MSRV e o semver.

use crate::faxina;
use crate::run::{falha, Check, Falhou, Feito};
use crate::selecao::{self, Selecao};

/// O que vai para o navegador: os wrappers e os crates do núcleo que eles levam. Só estes compilam
/// em `wasm32`.
const WASM: &[&str] = &[
    "conversao-web",
    "fotolivro-web",
    "raw-web",
    "revelacao-web",
    "tela-do-cliente-web",
    "revelacao-core",
    "foto-codec",
    "raw-codec",
    "fotolivro",
];

/// 🚨 Um por um: com `--workspace`, o `cargo-semver-checks` pula os crates com `publish = false`
/// (todos aqui) e sai verde sem ter conferido nada.
const BIBLIOTECAS: &[&str] = &[
    "foto-codec",
    "raw-codec",
    "revelacao-core",
    "editor-core",
    "recuperacao-core",
    "fotolivro",
    "visualizador-pdf",
    "ia-local",
    "preenchimento",
];

/// O piso: a cobertura de linhas do núcleo (sem o vendor) não desce daqui.
const PISO_DA_COBERTURA: &str = "90";
/// O rawler é vendorizado: entra na compilação, não na conta da cobertura.
const FORA_DA_COBERTURA: &[&str] = &["--ignore-filename-regex", "vendor/"];

/// A pasta da bancada, que tem o próprio workspace (o GPUI dela não entra no grafo dos apps).
const BANCADA: &str = "ferramentas/bancada";

type Passou = Result<&'static str, Falhou>;

fn strs(donos: &[String]) -> Vec<&str> {
    donos.iter().map(String::as_str).collect()
}

/// `-p a -p b …` para o cargo.
fn pacotes<S: AsRef<str>>(nomes: &[S]) -> Vec<String> {
    nomes
        .iter()
        .flat_map(|nome| ["-p".to_owned(), nome.as_ref().to_owned()])
        .collect()
}

fn workspace() -> Vec<String> {
    vec!["--workspace".to_owned()]
}

/// O que toda checagem roda primeiro, em segundos: a faxina se o target passou do teto, e o
/// formato.
fn comeco(check: &mut Check) -> Feito {
    check.etapa("faxina do target, se passou de 8 GiB");
    faxina::faxina(check, false);
    check.etapa("rustfmt");
    check.roda("cargo", &["fmt", "--all", "--", "--check"])
}

fn seleciona(check: &mut Check) -> Result<Selecao, Falhou> {
    check.etapa(&format!(
        "o que a mudança alcança, contra {}",
        selecao::base()
    ));
    let selecao = selecao::desta_arvore(check)?;
    if selecao.tudo {
        println!("mudou algo com que todo pacote compila: o workspace inteiro");
    } else {
        println!("alterados:  {}", selecao.alterados.join(" "));
        println!("testados:   {}", selecao.testados.join(" "));
        println!("alcançados: {}", selecao.alcancados.join(" "));
        if selecao.bancada {
            println!("e a bancada (ferramentas/bancada)");
        }
    }
    Ok(selecao)
}

fn clippy(check: &mut Check, titulo: &str, alvo: &[String]) -> Feito {
    check.etapa(titulo);
    let mut args = vec!["clippy".to_owned()];
    args.extend(alvo.iter().cloned());
    args.extend(["--all-targets", "--", "-D", "warnings"].map(str::to_owned));
    check.roda("cargo", &strs(&args))
}

/// O clippy em `wasm32` dos pacotes do navegador que estão em `nomes` (todos, se `None`).
fn wasm(check: &mut Check, nomes: Option<&[String]>) -> Feito {
    let lista: Vec<&str> = WASM
        .iter()
        .copied()
        .filter(|p| nomes.is_none_or(|n| n.iter().any(|x| x == p)))
        .collect();
    if lista.is_empty() {
        return Ok(());
    }
    check.etapa("clippy (wasm32), nos pacotes que vão para o navegador");
    let mut args = vec![
        "clippy".to_owned(),
        "--target".to_owned(),
        "wasm32-unknown-unknown".to_owned(),
    ];
    args.extend(pacotes(&lista));
    args.extend(["--", "-D", "warnings"].map(str::to_owned));
    check.roda("cargo", &strs(&args))
}

/// Os testes no nextest, compilação e execução à parte para a tabela dizer onde foi o tempo, e
/// depois os doctests, que o nextest não roda, pelo `cargo test --doc`.
fn testes(
    check: &mut Check,
    titulo: &str,
    alvo: &[String],
    documentados: Option<&[String]>,
) -> Feito {
    check.etapa(titulo);
    let mut compila = vec!["nextest".to_owned(), "run".to_owned()];
    compila.extend(alvo.iter().cloned());
    let mut roda = compila.clone();
    compila.push("--no-run".to_owned());
    roda.push("--no-tests=pass".to_owned());
    check.parte("compilar os testes", "cargo", &strs(&compila))?;
    check.parte("rodar os testes", "cargo", &strs(&roda))?;
    let doc = match documentados {
        None => vec![
            "test".to_owned(),
            "--doc".to_owned(),
            "--workspace".to_owned(),
        ],
        Some([]) => return Ok(()),
        Some(nomes) => [vec!["test".to_owned(), "--doc".to_owned()], pacotes(nomes)].concat(),
    };
    check.parte("doctests (compilar e rodar)", "cargo", &strs(&doc))
}

/// A documentação sem nenhum aviso: link quebrado, item privado citado.
fn doc(check: &mut Check, nomes: Option<&[String]>) -> Feito {
    let alvo = match nomes {
        None => vec![
            "--workspace".to_owned(),
            "--exclude".to_owned(),
            "xtask".to_owned(),
        ],
        Some(nomes) => {
            let sem_xtask: Vec<&String> = nomes.iter().filter(|n| *n != "xtask").collect();
            if sem_xtask.is_empty() {
                return Ok(());
            }
            pacotes(&sem_xtask)
        }
    };
    check.etapa("cargo doc sem nenhum aviso");
    let mut args = vec!["doc".to_owned(), "--no-deps".to_owned()];
    args.extend(alvo);
    let mut comando = check.comando("cargo", &strs(&args));
    comando.env("RUSTDOCFLAGS", "-D warnings");
    check.roda_comando(comando, "cargo", &strs(&args))
}

/// Licenças, vulnerabilidades conhecidas e fontes das dependências.
fn deny(check: &mut Check) -> Feito {
    check.etapa("dependências: licenças, vulnerabilidades, fontes (cargo-deny)");
    if !check.tem("cargo", &["deny", "--version"]) {
        return Err(falha(
            "falta o cargo-deny: cargo install cargo-deny --locked",
        ));
    }
    check.roda("cargo", &["deny", "check"])
}

/// Um comando na pasta da bancada, que tem o próprio workspace.
fn na_bancada(check: &Check, args: &[&str]) -> Feito {
    let mut comando = check.comando("cargo", args);
    comando.current_dir(check.raiz.join(BANCADA));
    check.roda_comando(comando, "cargo", args)
}

fn bancada_testes(check: &mut Check) -> Feito {
    check.etapa("a bancada: formato, clippy e testes (todo caso sai igual na tag e aqui)");
    na_bancada(check, &["fmt", "--", "--check"])?;
    na_bancada(check, &["clippy", "--all-targets", "--", "-D", "warnings"])?;
    na_bancada(check, &["nextest", "run", "--no-tests=pass"])?;
    na_bancada(check, &["test", "--doc"])
}

/// `cargo xtask dev`: enquanto se trabalha. O formato, e o clippy e os testes só dos pacotes
/// alterados. Os dependentes, o wasm32, a documentação e a bancada ficam para o `pr`.
pub fn dev(check: &mut Check) -> Passou {
    comeco(check)?;
    let selecao = seleciona(check)?;
    if selecao.tudo {
        check.modo = "dev, tudo".to_owned();
        clippy(check, "clippy (nativo), o workspace inteiro", &workspace())?;
        return Ok("Checagens do dev passaram (os testes do workspace inteiro: cargo xtask pr)");
    }
    if selecao.alterados.is_empty() {
        println!(
            "nenhum pacote mudou contra {}: nada a compilar",
            selecao::base()
        );
        return Ok("Checagens do dev passaram");
    }
    let alterados = pacotes(&selecao.alterados);
    clippy(check, "clippy (nativo), nos pacotes alterados", &alterados)?;
    let documentados: Vec<String> = selecao
        .documentados
        .iter()
        .filter(|n| selecao.alterados.contains(n))
        .cloned()
        .collect();
    testes(
        check,
        "testes, nos pacotes alterados",
        &alterados,
        Some(&documentados),
    )?;
    Ok("Checagens do dev passaram (para o PR: cargo xtask pr)")
}

/// `cargo xtask pr`: o gate do PR. O clippy em todo pacote que a mudança alcança, os testes dos
/// alterados e dos dependentes diretos, e o wasm32, a documentação e a bancada quando a mudança os
/// alcança. Mudou o `Cargo.lock` ou o `deny.toml`: o workspace inteiro e o `cargo deny`.
pub fn pr(check: &mut Check) -> Passou {
    comeco(check)?;
    let selecao = seleciona(check)?;
    if selecao.bancada {
        bancada_testes(check)?;
    }
    if selecao.tudo {
        check.modo = "pr, tudo".to_owned();
        clippy(check, "clippy (nativo), o workspace inteiro", &workspace())?;
        wasm(check, None)?;
        testes(check, "testes, o workspace inteiro", &workspace(), None)?;
        doc(check, None)?;
        deny(check)?;
        return Ok("Checagens do PR passaram (antes de lançar: cargo xtask release)");
    }
    if selecao.alcancados.is_empty() {
        println!(
            "nenhum pacote mudou contra {}: nada a compilar",
            selecao::base()
        );
        return Ok("Checagens do PR passaram");
    }
    clippy(
        check,
        "clippy (nativo), em todo pacote que a mudança alcança",
        &pacotes(&selecao.alcancados),
    )?;
    wasm(check, Some(&selecao.alcancados))?;
    testes(
        check,
        "testes, nos alterados e nos dependentes diretos",
        &pacotes(&selecao.testados),
        Some(&selecao.documentados),
    )?;
    doc(check, Some(&selecao.alcancados))?;
    Ok("Checagens do PR passaram (antes de lançar: cargo xtask release)")
}

/// O `rust-version` de `[workspace.package]`.
fn msrv(check: &Check) -> Result<String, Falhou> {
    let manifesto = std::fs::read_to_string(check.raiz.join("Cargo.toml"))
        .map_err(|erro| falha(format!("Cargo.toml: {erro}")))?;
    rust_version(&manifesto).ok_or_else(|| falha("o Cargo.toml não declara rust-version"))
}

fn rust_version(manifesto: &str) -> Option<String> {
    manifesto.lines().find_map(|linha| {
        let (chave, valor) = linha.split_once('=')?;
        (chave.trim() == "rust-version").then(|| valor.trim().trim_matches('"').to_owned())
    })
}

/// `cargo xtask release`: antes de lançar uma tag, tudo. O formato, o clippy nativo e wasm32, todos
/// os testes, a documentação, as dependências, o Rust mínimo e a API pública contra a última tag.
pub fn release(check: &mut Check) -> Passou {
    comeco(check)?;
    clippy(check, "clippy (nativo), o workspace inteiro", &workspace())?;
    wasm(check, None)?;
    testes(
        check,
        "testes: unidade, integração e doctests, o workspace inteiro",
        &workspace(),
        None,
    )?;
    doc(check, None)?;
    deny(check)?;

    let versao = msrv(check)?;
    check.etapa(&format!("o Rust mínimo declarado ({versao})"));
    check.roda(
        "rustup",
        &["toolchain", "install", &versao, "--profile", "minimal"],
    )?;
    check.roda(
        "cargo",
        &[
            &format!("+{versao}"),
            "check",
            "--workspace",
            "--all-targets",
        ],
    )?;

    check.etapa("a API pública de cada biblioteca contra a última tag (cargo-semver-checks)");
    if !check.tem("cargo", &["semver-checks", "--version"]) {
        return Err(falha(
            "falta o cargo-semver-checks: cargo install cargo-semver-checks --locked",
        ));
    }
    let tag = check.saida("git", &["describe", "--tags", "--abbrev=0"])?;
    let tag = tag.trim();
    println!("última tag: {tag}");
    let semver = BIBLIOTECAS.iter().try_for_each(|biblioteca| {
        check.roda(
            "cargo",
            &["semver-checks", "-p", biblioteca, "--baseline-rev", tag],
        )
    });
    // Cada crate compilado duas vezes (~2 GiB): só serve a esta conferência.
    let _ = std::fs::remove_dir_all(check.target.join("semver-checks"));
    semver?;
    Ok("Todas as checagens passaram: pode lançar")
}

/// `cargo xtask cobertura`: a cobertura por arquivo, com o piso, e o HTML em `target/cobertura`.
pub fn cobertura(check: &mut Check) -> Passou {
    check.etapa("cobertura do workspace (cargo-llvm-cov), sem o vendor");
    if !check.tem("cargo", &["llvm-cov", "--version"]) {
        return Err(falha(
            "falta o cargo-llvm-cov: cargo install cargo-llvm-cov --locked",
        ));
    }
    let html = check
        .target
        .join("cobertura")
        .to_string_lossy()
        .into_owned();
    let mut gera = vec!["llvm-cov", "--workspace", "--exclude", "xtask"];
    gera.extend(FORA_DA_COBERTURA);
    gera.extend(["--html", "--output-dir", &html]);
    let feito = check.roda("cargo", &gera).and_then(|()| {
        check.etapa(&format!("piso de {PISO_DA_COBERTURA} % das linhas"));
        let mut piso = vec!["llvm-cov", "report"];
        piso.extend(FORA_DA_COBERTURA);
        piso.extend(["--summary-only", "--fail-under-lines", PISO_DA_COBERTURA]);
        check.roda("cargo", &piso)
    });
    // A árvore instrumentada (~3 GiB) só serve a esta conta; o HTML fica em target/cobertura.
    let _ = std::fs::remove_dir_all(check.target.join("llvm-cov-target"));
    feito?;
    println!("relatório: {html}/html/index.html");
    Ok("Cobertura acima do piso")
}

/// `cargo xtask bancada`: a janela que mede o núcleo contra a última tag, com a sua foto.
pub fn bancada(check: &mut Check) -> Passou {
    check.etapa("a bancada (ferramentas/bancada), em release");
    na_bancada(check, &["run", "--release"])?;
    Ok("Bancada fechada")
}

/// `cargo xtask bancada-testar`: os testes da bancada, sem janela.
pub fn bancada_testar(check: &mut Check) -> Passou {
    bancada_testes(check)?;
    Ok("Testes da bancada passaram")
}

/// `cargo xtask faxina`: apaga já o incremental e as árvores da cobertura e do semver.
pub fn faxina_agora(check: &mut Check) -> Passou {
    check.etapa("faxina do target");
    faxina::faxina(check, true);
    Ok("Faxina feita")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn os_pacotes_viram_uma_flag_cada() {
        assert_eq!(pacotes(&["a", "b"]), ["-p", "a", "-p", "b"]);
    }

    #[test]
    fn le_o_rust_version_do_workspace() {
        let manifesto = "[workspace.package]\nversion = \"0.1.4\"\n# rust-version antigo\nrust-version = \"1.92\"\n";
        assert_eq!(rust_version(manifesto).as_deref(), Some("1.92"));
        assert_eq!(rust_version("[package]\nname = \"x\"\n"), None);
    }

    #[test]
    fn toda_biblioteca_do_semver_e_do_wasm_e_membro_do_workspace() {
        let manifesto = include_str!("../../Cargo.toml");
        for nome in BIBLIOTECAS.iter().chain(WASM) {
            assert!(
                manifesto.contains(&format!("\"crates/{nome}\"")),
                "{nome} não é membro"
            );
        }
    }
}
