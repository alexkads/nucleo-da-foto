//! Quais pacotes a mudança toca e alcança.
//!
//! 🔑 O que o seletor deixa de fora não é conferido por `dev` nem por `pr`: por isso ele tem testes
//! próprios, e na dúvida escolhe o workspace inteiro.
//!
//! - **Alterados:** os membros com arquivo mudado, contados também os não commitados. O arquivo é
//!   do membro cuja pasta é o prefixo mais longo do caminho. Uma dependência por caminho que não é
//!   membro (o `vendor/rawler` do `raw-codec`) conta como parte do membro que a usa.
//! - **Testados:** os alterados e os dependentes diretos deles, normais ou de teste.
//! - **Alcançados:** os alterados e todo dependente, em qualquer profundidade.
//! - **Tudo:** mudou algo com que todo pacote compila (o `Cargo.toml` da raiz, o `Cargo.lock`, o
//!   `rustfmt.toml`, o `deny.toml`, o `.cargo/`).

use std::collections::BTreeSet;
use std::path::Path;

use crate::json::Json;
use crate::run::{falha, Check, Falhou};

/// Os arquivos da raiz com que todo pacote compila: mudar um deles seleciona o workspace inteiro.
const DA_RAIZ: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rustfmt.toml",
    "deny.toml",
    "clippy.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
];

/// A bancada tem o próprio workspace: não é membro, e os testes dela são uma etapa à parte.
const BANCADA: &str = "ferramentas/bancada/";

/// Um membro do workspace, como o seletor precisa dele.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Membro {
    pub nome: String,
    /// A pasta, relativa à raiz e com `/`: `crates/foto-codec`.
    pub pasta: String,
    /// Os membros de que ele depende, normais, de build ou de teste.
    pub dependencias: Vec<String>,
    /// As pastas das dependências por caminho que não são membros: `vendor/rawler`.
    pub pastas_de_fora: Vec<String>,
    /// Se tem biblioteca com doctests.
    pub doctest: bool,
}

/// O que a mudança toca e alcança, em ordem alfabética.
#[derive(Debug, Default, PartialEq)]
pub struct Selecao {
    pub tudo: bool,
    pub alterados: Vec<String>,
    pub testados: Vec<String>,
    pub alcancados: Vec<String>,
    /// Os testados que têm doctests.
    pub documentados: Vec<String>,
    /// Mudou algo em `ferramentas/bancada`.
    pub bancada: bool,
}

fn dentro(arquivo: &str, pasta: &str) -> bool {
    arquivo
        .strip_prefix(pasta)
        .is_some_and(|resto| resto.starts_with('/'))
}

/// A seleção para estes membros e estes arquivos alterados (caminhos relativos à raiz, com `/`).
pub fn seleciona(membros: &[Membro], arquivos: &[String]) -> Selecao {
    let mut selecao = Selecao::default();
    let mut alterados = BTreeSet::new();
    for arquivo in arquivos {
        let arquivo = arquivo.trim_start_matches("./");
        if DA_RAIZ.contains(&arquivo) || arquivo.starts_with(".cargo/") {
            selecao.tudo = true;
            continue;
        }
        if arquivo.starts_with(BANCADA) {
            selecao.bancada = true;
            continue;
        }
        let dono = membros
            .iter()
            .filter(|membro| dentro(arquivo, &membro.pasta))
            .max_by_key(|membro| membro.pasta.len());
        if let Some(membro) = dono {
            alterados.insert(membro.nome.clone());
            continue;
        }
        for membro in membros {
            if membro
                .pastas_de_fora
                .iter()
                .any(|pasta| dentro(arquivo, pasta))
            {
                alterados.insert(membro.nome.clone());
            }
        }
    }
    if selecao.tudo {
        alterados = membros.iter().map(|membro| membro.nome.clone()).collect();
    }

    let dependentes_de = |nomes: &BTreeSet<String>| -> BTreeSet<String> {
        membros
            .iter()
            .filter(|membro| membro.dependencias.iter().any(|dep| nomes.contains(dep)))
            .map(|membro| membro.nome.clone())
            .collect()
    };
    let testados: BTreeSet<String> = alterados
        .union(&dependentes_de(&alterados))
        .cloned()
        .collect();
    let mut alcancados = alterados.clone();
    loop {
        let novos: Vec<String> = dependentes_de(&alcancados)
            .difference(&alcancados)
            .cloned()
            .collect();
        if novos.is_empty() {
            break;
        }
        alcancados.extend(novos);
    }
    selecao.documentados = membros
        .iter()
        .filter(|membro| membro.doctest && testados.contains(&membro.nome))
        .map(|membro| membro.nome.clone())
        .collect();
    selecao.documentados.sort();
    selecao.alterados = alterados.into_iter().collect();
    selecao.testados = testados.into_iter().collect();
    selecao.alcancados = alcancados.into_iter().collect();
    selecao
}

fn relativo(caminho: &str, raiz: &str) -> Option<String> {
    let caminho = caminho.replace('\\', "/");
    let raiz = raiz.replace('\\', "/");
    caminho
        .strip_prefix(&raiz)
        .map(|resto| resto.trim_matches('/').to_owned())
}

/// Os membros, lidos do `cargo metadata --no-deps`.
pub fn membros_do_metadata(metadata: &Json) -> Vec<Membro> {
    let raiz = metadata
        .campo("workspace_root")
        .and_then(Json::texto)
        .unwrap_or_default();
    let pacotes = metadata
        .campo("packages")
        .map(Json::itens)
        .unwrap_or_default();
    let pasta_de = |manifesto: &str| {
        let pasta = Path::new(manifesto)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        relativo(&pasta, raiz).unwrap_or(pasta)
    };
    let nomes: BTreeSet<&str> = pacotes
        .iter()
        .filter_map(|p| p.campo("name").and_then(Json::texto))
        .collect();
    pacotes
        .iter()
        .map(|pacote| {
            let texto = |campo: &str| {
                pacote
                    .campo(campo)
                    .and_then(Json::texto)
                    .unwrap_or_default()
            };
            let mut membro = Membro {
                nome: texto("name").to_owned(),
                pasta: pasta_de(texto("manifest_path")),
                ..Membro::default()
            };
            for dep in pacote
                .campo("dependencies")
                .map(Json::itens)
                .unwrap_or_default()
            {
                let Some(caminho) = dep.campo("path").and_then(Json::texto) else {
                    continue;
                };
                let nome = dep.campo("name").and_then(Json::texto).unwrap_or_default();
                if nomes.contains(nome) {
                    membro.dependencias.push(nome.to_owned());
                } else if let Some(pasta) = relativo(caminho, raiz) {
                    membro.pastas_de_fora.push(pasta);
                }
            }
            membro.dependencias.sort();
            membro.dependencias.dedup();
            membro.doctest = pacote
                .campo("targets")
                .map(Json::itens)
                .unwrap_or_default()
                .iter()
                .any(|alvo| {
                    alvo.campo("doctest") == Some(&Json::Logico(true))
                        && alvo
                            .campo("kind")
                            .map(Json::itens)
                            .unwrap_or_default()
                            .iter()
                            .any(|k| k.texto() == Some("lib"))
                });
            membro
        })
        .collect()
}

/// Contra o que se compara: `CHECK_BASE`, ou o `origin/main`.
pub fn base() -> String {
    std::env::var("CHECK_BASE").unwrap_or_else(|_| "origin/main".to_owned())
}

/// Os arquivos alterados desde que o branch saiu da base, com os não commitados e os novos.
pub fn arquivos_alterados(check: &Check) -> Result<Vec<String>, Falhou> {
    let base = base();
    let bifurcacao = check
        .saida("git", &["merge-base", "HEAD", &base])
        .map_err(|_| {
            falha(format!(
                "não achei onde o branch saiu de {base}: faça `git fetch` ou defina CHECK_BASE"
            ))
        })?;
    let mut arquivos: BTreeSet<String> = BTreeSet::new();
    for linha in check
        .saida("git", &["diff", "--name-only", bifurcacao.trim()])?
        .lines()
    {
        arquivos.insert(linha.to_owned());
    }
    for linha in check
        .saida("git", &["ls-files", "--others", "--exclude-standard"])?
        .lines()
    {
        arquivos.insert(linha.to_owned());
    }
    Ok(arquivos.into_iter().filter(|a| !a.is_empty()).collect())
}

/// A seleção desta árvore: os membros do `cargo metadata` e os arquivos do `git`.
pub fn desta_arvore(check: &Check) -> Result<Selecao, Falhou> {
    let metadata = check.metadata()?;
    let membros = membros_do_metadata(&metadata);
    Ok(seleciona(&membros, &arquivos_alterados(check)?))
}

#[cfg(test)]
mod testes {
    use super::*;

    fn membro(nome: &str, pasta: &str, dependencias: &[&str]) -> Membro {
        Membro {
            nome: nome.to_owned(),
            pasta: pasta.to_owned(),
            dependencias: dependencias.iter().map(|d| (*d).to_owned()).collect(),
            pastas_de_fora: Vec::new(),
            doctest: true,
        }
    }

    /// Um pedaço do grafo de verdade: o fotolivro é usado pelo fotolivro-web, o foto-codec por
    /// quase todos, e o raw-codec leva o rawler vendorizado.
    fn nucleo() -> Vec<Membro> {
        let mut raw = membro("raw-codec", "crates/raw-codec", &[]);
        raw.pastas_de_fora.push("vendor/rawler".to_owned());
        let mut web = membro("fotolivro-web", "crates/fotolivro-web", &["fotolivro"]);
        web.doctest = false;
        vec![
            membro("foto-codec", "crates/foto-codec", &[]),
            raw,
            membro(
                "revelacao-core",
                "crates/revelacao-core",
                &["foto-codec", "raw-codec"],
            ),
            membro("fotolivro", "crates/fotolivro", &["foto-codec"]),
            web,
            membro("xtask", "xtask", &[]),
        ]
    }

    fn arquivos(lista: &[&str]) -> Vec<String> {
        lista.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn um_crate_folha_testa_ele_e_quem_depende_dele() {
        let selecao = seleciona(&nucleo(), &arquivos(&["crates/fotolivro/src/lib.rs"]));
        assert!(!selecao.tudo);
        assert_eq!(selecao.alterados, ["fotolivro"]);
        assert_eq!(selecao.testados, ["fotolivro", "fotolivro-web"]);
        assert_eq!(selecao.alcancados, ["fotolivro", "fotolivro-web"]);
        assert_eq!(
            selecao.documentados,
            ["fotolivro"],
            "o fotolivro-web não tem doctest"
        );
    }

    #[test]
    fn o_dependente_indireto_e_alcancado_mas_nao_testado() {
        let selecao = seleciona(&nucleo(), &arquivos(&["crates/foto-codec/src/jpeg.rs"]));
        assert_eq!(
            selecao.testados,
            ["foto-codec", "fotolivro", "revelacao-core"]
        );
        assert_eq!(
            selecao.alcancados,
            ["foto-codec", "fotolivro", "fotolivro-web", "revelacao-core"]
        );
    }

    #[test]
    fn o_vendor_conta_como_o_crate_que_o_usa() {
        let selecao = seleciona(&nucleo(), &arquivos(&["vendor/rawler/src/decoders/cr3.rs"]));
        assert_eq!(selecao.alterados, ["raw-codec"]);
        assert_eq!(selecao.testados, ["raw-codec", "revelacao-core"]);
    }

    #[test]
    fn o_que_todo_pacote_usa_seleciona_tudo() {
        for arquivo in [
            "Cargo.lock",
            "Cargo.toml",
            "deny.toml",
            "rustfmt.toml",
            ".cargo/config.toml",
        ] {
            let selecao = seleciona(&nucleo(), &arquivos(&[arquivo]));
            assert!(selecao.tudo, "{arquivo}");
            assert_eq!(selecao.alterados.len(), nucleo().len(), "{arquivo}");
        }
    }

    #[test]
    fn o_manifesto_de_um_membro_e_dele_e_nao_da_raiz() {
        let selecao = seleciona(&nucleo(), &arquivos(&["crates/fotolivro/Cargo.toml"]));
        assert!(!selecao.tudo);
        assert_eq!(selecao.alterados, ["fotolivro"]);
    }

    #[test]
    fn documentacao_e_scripts_nao_selecionam_nada() {
        let selecao = seleciona(
            &nucleo(),
            &arquivos(&[
                "README.md",
                "CHANGELOG.md",
                "scripts/construir-web.sh",
                ".github/x.md",
            ]),
        );
        assert_eq!(selecao, Selecao::default());
    }

    #[test]
    fn a_bancada_e_uma_etapa_a_parte() {
        let selecao = seleciona(&nucleo(), &arquivos(&["ferramentas/bancada/src/main.rs"]));
        assert!(selecao.bancada);
        assert!(selecao.alterados.is_empty());
    }

    #[test]
    fn a_pasta_de_nome_parecido_nao_e_do_membro() {
        // `crates/fotolivro-web/…` não está dentro de `crates/fotolivro`.
        let selecao = seleciona(&nucleo(), &arquivos(&["crates/fotolivro-web/src/lib.rs"]));
        assert_eq!(selecao.alterados, ["fotolivro-web"]);
        let selecao = seleciona(&nucleo(), &arquivos(&["xtask/src/main.rs"]));
        assert_eq!(selecao.alterados, ["xtask"]);
    }

    #[test]
    fn os_membros_saem_do_cargo_metadata() {
        let json = r#"{"workspace_root":"/r/nucleo","packages":[
            {"name":"raw-codec","manifest_path":"/r/nucleo/crates/raw-codec/Cargo.toml",
             "dependencies":[{"name":"image","path":null},{"name":"rawler","path":"/r/nucleo/vendor/rawler"}],
             "targets":[{"kind":["lib"],"doctest":true},{"kind":["test"],"doctest":false}]},
            {"name":"revelacao-core","manifest_path":"/r/nucleo/crates/revelacao-core/Cargo.toml",
             "dependencies":[{"name":"raw-codec","path":"/r/nucleo/crates/raw-codec","kind":"dev"},
                             {"name":"raw-codec","path":"/r/nucleo/crates/raw-codec"}],
             "targets":[{"kind":["bin"],"doctest":false}]}]}"#;
        let membros = membros_do_metadata(&Json::ler(json).unwrap());
        assert_eq!(membros[0].pasta, "crates/raw-codec");
        assert_eq!(membros[0].pastas_de_fora, ["vendor/rawler"]);
        assert!(membros[0].dependencias.is_empty());
        assert!(membros[0].doctest);
        assert_eq!(
            membros[1].dependencias,
            ["raw-codec"],
            "a de teste e a normal contam uma vez"
        );
        assert!(!membros[1].doctest);
    }
}
