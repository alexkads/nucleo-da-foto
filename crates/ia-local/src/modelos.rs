//! 📦 Os modelos de IA: baixados **pelo operador**, verificados pelo hash e
//! guardados na pasta de dados do app. Depois de instalados, funcionam sem
//! internet — nenhuma foto, máscara ou resultado sai da máquina.
//!
//! Cada modelo é declarado por quem o usa (o preenchimento declara a LaMa),
//! com a origem, a versão (o commit), a licença dos **pesos** (que não é a do
//! código do app) e o SHA-256 publicado.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Um modelo que o app sabe usar — declarado em `const` por quem o usa (a
/// LaMa mora em `preenchimento::lama::MODELO`).
///
/// 🔑 [`sha256`](Self::sha256) é o que faz o arquivo ser **o** publicado:
/// trocar o arquivo na origem sem trocar o hash aqui faz todo download falhar
/// com [`ErroDeModelo::HashDiferente`], e nada é instalado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modelo {
    /// O identificador estável: dá nome ao registro (`<id>.json`) na pasta.
    /// Não muda entre versões do app.
    pub id: &'static str,
    /// O nome que o operador lê nas opções.
    pub nome: &'static str,
    /// O arquivo, na pasta dos modelos.
    pub arquivo: &'static str,
    /// A versão: o repositório e o commit de onde o arquivo vem.
    pub versao: &'static str,
    /// De onde [`baixar`] busca, presa a um commit — nunca a um ramo que anda.
    pub url: &'static str,
    /// O tamanho publicado, em bytes: o total do progresso quando o servidor
    /// não manda `Content-Length`.
    pub bytes: u64,
    /// O SHA-256 publicado, em hexadecimal minúsculo (64 caracteres).
    pub sha256: &'static str,
    /// A licença **dos pesos**, que não é a do código do app.
    pub licenca: &'static str,
    /// Quem treinou e quem exportou, para os créditos.
    pub origem: &'static str,
}

/// A variável que aponta outra pasta de modelos (testes e roteiros).
pub const VAR_DA_PASTA: &str = "VLB_MODELOS";

/// A pasta dos modelos: a de dados do app (`…/VintageLightbox/modelos`).
///
/// [`VAR_DA_PASTA`], se definida e não vazia, vence. Sem pasta de dados no
/// sistema, cai numa pasta temporária — o modelo funciona, mas não sobrevive
/// a uma limpeza. Não cria a pasta: quem grava ([`baixar`], [`importar`])
/// cria.
pub fn pasta_padrao() -> PathBuf {
    if let Some(p) = std::env::var_os(VAR_DA_PASTA).filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    directories::ProjectDirs::from("br", "RecordarFotos", "VintageLightbox")
        .map(|d| d.data_dir().join("modelos"))
        .unwrap_or_else(|| std::env::temp_dir().join("VintageLightbox-modelos"))
}

/// O que se sabe de um modelo instalado (gravado ao lado dele).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registro {
    /// O SHA-256 do arquivo instalado, em hexadecimal minúsculo.
    pub sha256: String,
    /// O tamanho do arquivo instalado: é o que [`estado`] confere.
    pub bytes: u64,
    /// `"download"` ou `"importado"`.
    pub como: String,
    /// O hash bate com o publicado pela origem.
    pub verificado: bool,
}

/// O que [`estado`] encontrou na pasta para um modelo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Estado {
    /// Não há arquivo: a tela oferece baixar ou importar.
    Ausente,
    /// Pronto para usar.
    Instalado {
        /// O arquivo do modelo, para abrir a sessão.
        caminho: PathBuf,
        /// O que foi gravado ao instalar (verificado ou importado).
        registro: Registro,
    },
    /// O arquivo está lá, mas não bate com o registro (cortado, trocado) —
    /// ou o registro sumiu. Não se usa: a tela pede para remover e baixar de
    /// novo.
    Corrompido {
        /// O arquivo que está lá.
        caminho: PathBuf,
    },
}

/// Por que o modelo não foi instalado (ou removido). Em todo caso, nada pela
/// metade fica parecendo instalado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErroDeModelo {
    /// A conexão falhou ou caiu no meio — o porquê, como o `ureq` disse.
    Rede(String),
    /// Ler, gravar, copiar ou renomear na pasta falhou.
    Disco(String),
    /// O baixado ou o importado não é o arquivo publicado.
    HashDiferente {
        /// O publicado ([`Modelo::sha256`]).
        esperado: String,
        /// O do arquivo que chegou.
        obtido: String,
    },
    /// O operador cancelou o download; o `.parte` foi apagado.
    Cancelado,
    /// O arquivo importado não tem o hash publicado e a conferência de
    /// assinatura o recusou — o porquê vem dela.
    Incompativel(String),
}

impl ErroDeModelo {
    /// A frase para o operador. O [`HashDiferente`](Self::HashDiferente) não
    /// mostra os hashes: estão no `Debug`, para o registro.
    pub fn mensagem(&self) -> String {
        match self {
            ErroDeModelo::Rede(m) => format!("O download falhou: {m}"),
            ErroDeModelo::Disco(m) => format!("Não foi possível gravar o modelo: {m}"),
            ErroDeModelo::HashDiferente { .. } => {
                "O arquivo não é o modelo publicado (o hash não confere) — nada foi instalado"
                    .into()
            }
            ErroDeModelo::Cancelado => "Download cancelado".into(),
            ErroDeModelo::Incompativel(m) => format!("O arquivo não é um modelo compatível: {m}"),
        }
    }
}

fn caminho_do_registro(pasta: &Path, m: &Modelo) -> PathBuf {
    pasta.join(format!("{}.json", m.id))
}

/// O estado do modelo na pasta. Barato: confere o tamanho contra o
/// registro, sem refazer o hash (que foi feito ao instalar).
pub fn estado(pasta: &Path, m: &Modelo) -> Estado {
    let caminho = pasta.join(m.arquivo);
    let Ok(meta) = std::fs::metadata(&caminho) else {
        return Estado::Ausente;
    };
    let registro = std::fs::read(caminho_do_registro(pasta, m))
        .ok()
        .and_then(|b| serde_json::from_slice::<Registro>(&b).ok());
    match registro {
        Some(r) if r.bytes == meta.len() => Estado::Instalado {
            caminho,
            registro: r,
        },
        _ => Estado::Corrompido { caminho },
    }
}

fn gravar_registro(pasta: &Path, m: &Modelo, r: &Registro) -> Result<(), ErroDeModelo> {
    let json = serde_json::to_vec_pretty(r).map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    std::fs::write(caminho_do_registro(pasta, m), json)
        .map_err(|e| ErroDeModelo::Disco(e.to_string()))
}

/// Baixa o modelo de `url` (normalmente `m.url`), com o progresso
/// `(recebidos, total)`, e só o instala se o SHA-256 bater. O arquivo vai
/// para um `.parte` e troca de nome no fim: um download interrompido nunca
/// parece instalado.
///
/// Bloqueia até o fim (208 MB, no caso da LaMa): chame fora da thread da
/// tela. `cancelado` é olhado a cada leitura (até 64 KiB).
///
/// ```no_run
/// use std::sync::atomic::AtomicBool;
/// use ia_local::modelos::{self, Modelo};
///
/// # const MODELO: Modelo = Modelo { id: "m", nome: "m", arquivo: "m.onnx",
/// #     versao: "", url: "https://exemplo.invalid/m.onnx", bytes: 0,
/// #     sha256: "", licenca: "", origem: "" };
/// let cancelado = AtomicBool::new(false);
/// let caminho = modelos::baixar(
///     &modelos::pasta_padrao(),
///     &MODELO,
///     MODELO.url,
///     &|recebidos, total| println!("{recebidos} de {total}"),
///     &cancelado,
/// )?;
/// # Ok::<(), modelos::ErroDeModelo>(())
/// ```
///
/// # Erros
///
/// [`ErroDeModelo::Rede`], [`Disco`](ErroDeModelo::Disco),
/// [`Cancelado`](ErroDeModelo::Cancelado) ou
/// [`HashDiferente`](ErroDeModelo::HashDiferente) — neste, o arquivo baixado é
/// apagado e o instalado antes (se havia) fica como estava.
pub fn baixar(
    pasta: &Path,
    m: &Modelo,
    url: &str,
    progresso: &dyn Fn(u64, u64),
    cancelado: &AtomicBool,
) -> Result<PathBuf, ErroDeModelo> {
    std::fs::create_dir_all(pasta).map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    let parte = pasta.join(format!("{}.parte", m.arquivo));
    let resposta = ureq::get(url)
        .call()
        .map_err(|e| ErroDeModelo::Rede(e.to_string()))?;
    let total = resposta
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(m.bytes);
    let mut leitor = resposta.into_body().into_reader();
    let mut arquivo =
        std::fs::File::create(&parte).map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    let mut recebidos = 0u64;
    loop {
        if cancelado.load(Ordering::Relaxed) {
            drop(arquivo);
            let _ = std::fs::remove_file(&parte);
            return Err(ErroDeModelo::Cancelado);
        }
        let n = leitor
            .read(&mut buffer)
            .map_err(|e| ErroDeModelo::Rede(e.to_string()))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        arquivo
            .write_all(&buffer[..n])
            .map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
        recebidos += n as u64;
        progresso(recebidos, total);
    }
    arquivo
        .sync_all()
        .map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    drop(arquivo);
    let obtido = hex(&hash.finalize());
    if obtido != m.sha256 {
        let _ = std::fs::remove_file(&parte);
        return Err(ErroDeModelo::HashDiferente {
            esperado: m.sha256.into(),
            obtido,
        });
    }
    instalar(pasta, m, &parte, &obtido, recebidos, "download", true)
}

/// Importa um arquivo local (o modelo baixado por outro caminho, ou copiado
/// de outro balcão). O hash publicado instala como verificado; outro hash só
/// entra se `compativel` disser que o arquivo tem a assinatura do modelo —
/// e fica marcado como não verificado.
///
/// `origem` não é tocado: é copiado para a pasta antes de qualquer conferência.
///
/// # Erros
///
/// [`ErroDeModelo::Disco`] se a cópia falhar;
/// [`ErroDeModelo::Incompativel`] se o hash não bater e `compativel` recusar.
pub fn importar(
    pasta: &Path,
    m: &Modelo,
    origem: &Path,
    compativel: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<PathBuf, ErroDeModelo> {
    std::fs::create_dir_all(pasta).map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    let parte = pasta.join(format!("{}.parte", m.arquivo));
    std::fs::copy(origem, &parte).map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    let (obtido, bytes) = hash_do_arquivo(&parte)?;
    let verificado = obtido == m.sha256;
    if !verificado {
        if let Err(motivo) = compativel(&parte) {
            let _ = std::fs::remove_file(&parte);
            return Err(ErroDeModelo::Incompativel(motivo));
        }
    }
    instalar(pasta, m, &parte, &obtido, bytes, "importado", verificado)
}

fn instalar(
    pasta: &Path,
    m: &Modelo,
    parte: &Path,
    sha256: &str,
    bytes: u64,
    como: &str,
    verificado: bool,
) -> Result<PathBuf, ErroDeModelo> {
    let destino = pasta.join(m.arquivo);
    std::fs::rename(parte, &destino).map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    gravar_registro(
        pasta,
        m,
        &Registro {
            sha256: sha256.into(),
            bytes,
            como: como.into(),
            verificado,
        },
    )?;
    Ok(destino)
}

/// Tira o modelo (e o registro) da pasta. O que já não está lá não é erro.
///
/// # Erros
///
/// [`ErroDeModelo::Disco`] se um dos arquivos existir e não puder ser
/// apagado (em uso, sem permissão).
pub fn remover(pasta: &Path, m: &Modelo) -> Result<(), ErroDeModelo> {
    for p in [pasta.join(m.arquivo), caminho_do_registro(pasta, m)] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ErroDeModelo::Disco(e.to_string())),
        }
    }
    Ok(())
}

fn hash_do_arquivo(caminho: &Path) -> Result<(String, u64), ErroDeModelo> {
    let mut f = std::fs::File::open(caminho).map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    let mut bytes = 0u64;
    loop {
        let n = f
            .read(&mut buffer)
            .map_err(|e| ErroDeModelo::Disco(e.to_string()))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((hex(&hash.finalize()), bytes))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::net::TcpListener;

    /// Um modelo de mentira, com o hash de `conteudo`.
    fn modelo_de(conteudo: &[u8]) -> Modelo {
        let h: &'static str = Box::leak(hex(&Sha256::digest(conteudo)).into_boxed_str());
        Modelo {
            id: "teste",
            nome: "Modelo de teste",
            arquivo: "teste.onnx",
            versao: "0",
            url: "",
            bytes: conteudo.len() as u64,
            sha256: h,
            licenca: "MIT",
            origem: "o teste",
        }
    }

    /// Um servidor HTTP de um pedido só, servindo `corpo`.
    fn servidor(corpo: Vec<u8>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let endereco = format!("http://{}/modelo.onnx", l.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut pedido = [0u8; 1024];
                let _ = s.read(&mut pedido);
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    corpo.len()
                );
                let _ = s.write_all(&corpo);
            }
        });
        endereco
    }

    #[test]
    fn baixa_confere_o_hash_instala_e_remove() {
        let dir = tempfile::tempdir().unwrap();
        let conteudo: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let m = modelo_de(&conteudo);
        assert_eq!(estado(dir.path(), &m), Estado::Ausente);
        let vistos = std::sync::Mutex::new(Vec::new());
        let caminho = baixar(
            dir.path(),
            &m,
            &servidor(conteudo.clone()),
            &|r, t| vistos.lock().unwrap().push((r, t)),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(std::fs::read(&caminho).unwrap(), conteudo);
        let vistos = vistos.into_inner().unwrap();
        assert_eq!(
            vistos.last(),
            Some(&(300_000, 300_000)),
            "o progresso chega ao total"
        );
        match estado(dir.path(), &m) {
            Estado::Instalado { registro, .. } => {
                assert!(registro.verificado);
                assert_eq!(registro.como, "download");
            }
            outro => panic!("{outro:?}"),
        }
        // O arquivo cortado é acusado, e remover limpa.
        std::fs::write(&caminho, &conteudo[..10]).unwrap();
        assert!(matches!(estado(dir.path(), &m), Estado::Corrompido { .. }));
        remover(dir.path(), &m).unwrap();
        assert_eq!(estado(dir.path(), &m), Estado::Ausente);
    }

    #[test]
    fn hash_errado_e_cancelado_nao_instalam_nada() {
        let dir = tempfile::tempdir().unwrap();
        let m = modelo_de(b"o modelo de verdade");
        let r = baixar(
            dir.path(),
            &m,
            &servidor(b"outra coisa".to_vec()),
            &|_, _| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(r, Err(ErroDeModelo::HashDiferente { .. })));
        assert_eq!(estado(dir.path(), &m), Estado::Ausente);
        assert!(
            !dir.path().join(format!("{}.parte", m.arquivo)).exists(),
            "nada pela metade"
        );
        let r = baixar(
            dir.path(),
            &m,
            &servidor(vec![7; 100_000]),
            &|_, _| {},
            &AtomicBool::new(true),
        );
        assert_eq!(r, Err(ErroDeModelo::Cancelado));
        assert_eq!(estado(dir.path(), &m), Estado::Ausente);
        // Sem rede (porta fechada): erro de rede, não de disco.
        let fechada = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let r = baixar(
            dir.path(),
            &m,
            &format!("http://{fechada}/x"),
            &|_, _| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(r, Err(ErroDeModelo::Rede(_))), "{r:?}");
    }

    #[test]
    fn importar_verifica_pelo_hash_ou_pela_assinatura() {
        let dir = tempfile::tempdir().unwrap();
        let origem = dir.path().join("baixado-por-fora.onnx");
        std::fs::write(&origem, b"o modelo de verdade").unwrap();
        let pasta = dir.path().join("modelos");
        let m = modelo_de(b"o modelo de verdade");
        importar(
            &pasta,
            &m,
            &origem,
            &|_| Err("não devia ser chamado".into()),
        )
        .unwrap();
        assert!(
            matches!(estado(&pasta, &m), Estado::Instalado { registro, .. } if registro.verificado)
        );
        remover(&pasta, &m).unwrap();
        // Outro arquivo: só entra se tiver a assinatura, e fica não verificado.
        std::fs::write(&origem, b"outro export").unwrap();
        let r = importar(&pasta, &m, &origem, &|_| {
            Err("as entradas não são image/mask".into())
        });
        assert!(matches!(r, Err(ErroDeModelo::Incompativel(_))));
        assert_eq!(estado(&pasta, &m), Estado::Ausente);
        importar(&pasta, &m, &origem, &|_| Ok(())).unwrap();
        assert!(
            matches!(estado(&pasta, &m), Estado::Instalado { registro, .. } if !registro.verificado && registro.como == "importado")
        );
    }
}
