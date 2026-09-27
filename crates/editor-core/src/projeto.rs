//! O projeto no disco: gravar, reabrir, reconciliar e coletar.
//!
//! O desenho inteiro está em `docs/editor-em-camadas/03-GRAVACAO-E-CATALOGO.md`.
//! Em resumo:
//!
//! ```text
//! <pasta>/projeto.json          manifesto
//! <pasta>/tiles/<sha256>.bin    tile RGBA8 256×256, deflate, endereçado pelo conteúdo
//! <pasta>/composta-<rev>.png    a imagem editada da revisão <rev>
//! ```
//!
//! 🔑 **Salvar só acrescenta arquivos até o manifesto virar.** Tile tem o nome
//! do próprio conteúdo e nunca é reescrito; a composta de cada revisão tem nome
//! próprio; e o manifesto troca de uma vez por `rename`. Parar no meio deixa o
//! manifesto anterior de pé, com todos os arquivos que ele cita — sem diário e
//! sem passo de recuperação para a gravação em si. O catálogo (SQLite) é o
//! passo seguinte, fora deste crate; a coleta só vem depois dele (C33).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use image::RgbImage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::composicao;
use crate::contrato::VersaoEditada;
use crate::documento::{hex, BaseRef, Camada, Documento};
use crate::historico::{Comando, Historico};
use crate::pincel::Mudanca;
use crate::tiles::{CamadaDePixels, Posicao, Tile, BYTES_DO_TILE};

/// A versão do formato do projeto. Maior que esta é recusado — quem abre não
/// sabe ler, e não grava por cima.
pub const FORMATO: u32 = 1;

pub const MANIFESTO: &str = "projeto.json";
const PASTA_DOS_TILES: &str = "tiles";

#[derive(thiserror::Error, Debug)]
pub enum ErroDoProjeto {
    #[error("o disco recusou: {0}")]
    Disco(#[from] io::Error),
    #[error("o projeto não pôde ser lido: {0}")]
    Formato(String),
    #[error("o projeto foi gravado por uma versão mais nova do app (formato {0})")]
    FormatoNovo(u32),
    #[error(
        "a foto de origem mudou desde a edição (esperada {esperada}, achada {achada}): \
         pintar por cima desalinharia tudo"
    )]
    BaseDiferente { esperada: String, achada: String },
    #[error("um pedaço da camada está corrompido ({0})")]
    TileCorrompido(String),
    #[error("a imagem editada não saiu: {0}")]
    Codificacao(String),
}

// ------------------------------------------------------------------ disco

/// Em qual passo da gravação uma escrita acontece — o que os testes usam para
/// falhar em cada um.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Etapa {
    Tiles,
    Composta,
    Manifesto,
}

/// O acesso ao disco, atrás de uma trait para os testes falharem onde
/// quiserem.
pub trait Disco: Send + Sync {
    /// Grava `bytes` em `destino` **atomicamente**: quem lê vê o arquivo
    /// anterior ou o novo inteiro, nunca a metade.
    fn gravar(&self, destino: &Path, bytes: &[u8], etapa: Etapa) -> io::Result<()>;
    fn ler(&self, caminho: &Path) -> io::Result<Vec<u8>>;
    fn existe(&self, caminho: &Path) -> bool {
        caminho.exists()
    }
    fn apagar(&self, caminho: &Path) -> io::Result<()> {
        std::fs::remove_file(caminho)
    }
    fn listar(&self, pasta: &Path) -> io::Result<Vec<PathBuf>> {
        match std::fs::read_dir(pasta) {
            Ok(itens) => Ok(itens.filter_map(|i| i.ok().map(|i| i.path())).collect()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
    fn criar_pasta(&self, pasta: &Path) -> io::Result<()> {
        std::fs::create_dir_all(pasta)
    }
}

/// O disco de verdade: `tmp` → `fsync` → `rename` → `fsync` da pasta.
pub struct DiscoReal;

fn temporario(destino: &Path) -> PathBuf {
    let mut nome = destino.as_os_str().to_owned();
    nome.push(".tmp");
    PathBuf::from(nome)
}

impl Disco for DiscoReal {
    fn gravar(&self, destino: &Path, bytes: &[u8], _etapa: Etapa) -> io::Result<()> {
        use std::io::Write;
        let tmp = temporario(destino);
        {
            let mut arquivo = std::fs::File::create(&tmp)?;
            arquivo.write_all(bytes)?;
            arquivo.sync_all()?;
        }
        // ⚠️ No Windows o `rename` substitui o destino (`MoveFileExW` com
        // `REPLACE_EXISTING`), mas um antivírus pode segurar o arquivo por um
        // instante: três tentativas curtas antes de desistir.
        let mut tentativa = 0;
        loop {
            match std::fs::rename(&tmp, destino) {
                Ok(()) => break,
                Err(_) if tentativa < 2 => {
                    tentativa += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(e);
                }
            }
        }
        #[cfg(unix)]
        if let Some(pasta) = destino.parent() {
            if let Ok(pasta) = std::fs::File::open(pasta) {
                let _ = pasta.sync_all();
            }
        }
        Ok(())
    }

    fn ler(&self, caminho: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(caminho)
    }
}

/// O disco que falha na `n`-ésima escrita de uma etapa — e, como um corte de
/// energia, deixa o `.tmp` para trás.
pub struct DiscoComFalha {
    pub etapa: Etapa,
    /// Quantas escritas daquela etapa passam antes da que falha.
    pub depois_de: usize,
    feitas: AtomicUsize,
}

impl DiscoComFalha {
    pub fn novo(etapa: Etapa, depois_de: usize) -> Self {
        Self {
            etapa,
            depois_de,
            feitas: AtomicUsize::new(0),
        }
    }
}

impl Disco for DiscoComFalha {
    fn gravar(&self, destino: &Path, bytes: &[u8], etapa: Etapa) -> io::Result<()> {
        if etapa == self.etapa && self.feitas.fetch_add(1, Ordering::SeqCst) >= self.depois_de {
            let metade = &bytes[..bytes.len() / 2];
            let _ = std::fs::write(temporario(destino), metade);
            return Err(io::Error::other(format!("falha simulada em {etapa:?}")));
        }
        DiscoReal.gravar(destino, bytes, etapa)
    }

    fn ler(&self, caminho: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(caminho)
    }
}

// -------------------------------------------------------------- manifesto

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifesto {
    pub formato: u32,
    pub revisao: u64,
    pub base: BaseRef,
    pub camadas: Vec<CamadaSalva>,
    pub historico: HistoricoSalvo,
    /// `None` quando o projeto não tem efeito (C30).
    pub composta: Option<CompostaSalva>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamadaSalva {
    pub nome: String,
    pub visivel: bool,
    pub opacidade: f32,
    /// `"coluna,linha"` → hash do tile.
    pub tiles: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoricoSalvo {
    pub passos: Vec<PassoSalvo>,
    pub posicao: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum PassoSalvo {
    Traco {
        camada: usize,
        antes: Vec<TileSalvo>,
        depois: Vec<TileSalvo>,
    },
    Visibilidade {
        camada: usize,
        antes: bool,
        depois: bool,
    },
    Opacidade {
        camada: usize,
        antes: f32,
        depois: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileSalvo {
    pub c: u32,
    pub l: u32,
    /// `None` = o tile não existia (transparente).
    pub hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompostaSalva {
    pub arquivo: String,
    pub sha256: String,
    pub largura: u32,
    pub altura: u32,
}

fn chave_do_tile(p: Posicao) -> String {
    format!("{},{}", p.0, p.1)
}

fn posicao_da_chave(chave: &str) -> Option<Posicao> {
    let (c, l) = chave.split_once(',')?;
    Some((c.parse().ok()?, l.parse().ok()?))
}

// ---------------------------------------------------------------- projeto

/// O que uma gravação produziu.
#[derive(Clone, Debug, PartialEq)]
pub struct Salvo {
    pub revisao: u64,
    /// `None` quando o documento é neutro (C30): não há imagem editada.
    pub versao: Option<VersaoEditada>,
}

/// O que se lê do projeto ao reabrir.
pub struct Aberto {
    pub documento: Documento,
    pub historico: Historico,
    pub revisao: u64,
}

pub struct Projeto {
    pasta: PathBuf,
    disco: Arc<dyn Disco>,
    /// O hash de cada tile já calculado, pela identidade do `Arc` — o `Arc` fica
    /// guardado junto para o endereço não ser reaproveitado por outro tile.
    /// Uma camada cheia de 24 MP são ~96 MB para hashear; sem isto, cada
    /// salvamento pagaria tudo de novo por uma pincelada.
    hashes: Mutex<HashMap<usize, (Tile, String)>>,
}

impl Projeto {
    pub fn novo(pasta: impl Into<PathBuf>, disco: Arc<dyn Disco>) -> Self {
        Self {
            pasta: pasta.into(),
            disco,
            hashes: Mutex::new(HashMap::new()),
        }
    }

    pub fn pasta(&self) -> &Path {
        &self.pasta
    }

    fn caminho_do_tile(&self, hash: &str) -> PathBuf {
        self.pasta.join(PASTA_DOS_TILES).join(format!("{hash}.bin"))
    }

    fn caminho_da_composta(&self, revisao: u64) -> PathBuf {
        self.pasta.join(nome_da_composta(revisao))
    }

    fn hash_do_tile(&self, tile: &Tile) -> String {
        let chave = Arc::as_ptr(tile) as usize;
        let mut memo = self.hashes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, hash)) = memo.get(&chave) {
            return hash.clone();
        }
        let hash = hex(&Sha256::digest(tile.as_slice()));
        memo.insert(chave, (tile.clone(), hash.clone()));
        hash
    }

    /// Esquece os hashes de tiles que ninguém mais usa (chamado depois de
    /// salvar, para a memória não crescer com o histórico descartado).
    fn podar_os_hashes(&self) {
        let mut memo = self.hashes.lock().unwrap_or_else(|e| e.into_inner());
        memo.retain(|_, (tile, _)| Arc::strong_count(tile) > 1);
    }

    // ------------------------------------------------------------ salvar

    /// Os passos 1–3 da gravação: tiles, imagem editada, manifesto.
    ///
    /// Quem chama confirma no catálogo (passo 4) e só então [`Self::coletar`]
    /// (passo 6). Se isto devolver erro, **nada do que o catálogo aponta foi
    /// tocado**: a revisão anterior continua utilizável (C33).
    pub fn salvar(
        &self,
        edicao_id: &str,
        base: &RgbImage,
        doc: &Documento,
        hist: &Historico,
        revisao: u64,
    ) -> Result<Salvo, ErroDoProjeto> {
        self.disco.criar_pasta(&self.pasta.join(PASTA_DOS_TILES))?;

        // 1. Os tiles — os da camada e os do histórico.
        let mut gravados: BTreeSet<String> = BTreeSet::new();
        let mut gravar_tile = |tile: &Tile| -> Result<String, ErroDoProjeto> {
            let hash = self.hash_do_tile(tile);
            if gravados.insert(hash.clone()) {
                let caminho = self.caminho_do_tile(&hash);
                if !self.disco.existe(&caminho) {
                    let comprimido = miniz_oxide::deflate::compress_to_vec(tile, 1);
                    self.disco.gravar(&caminho, &comprimido, Etapa::Tiles)?;
                }
            }
            Ok(hash)
        };
        let mut camadas = Vec::with_capacity(doc.camadas.len());
        for camada in &doc.camadas {
            let mut tiles = BTreeMap::new();
            for (posicao, tile) in camada.pixels.existentes() {
                tiles.insert(chave_do_tile(*posicao), gravar_tile(tile)?);
            }
            camadas.push(CamadaSalva {
                nome: camada.nome.clone(),
                visivel: camada.visivel,
                opacidade: camada.opacidade,
                tiles,
            });
        }
        let mut passos = Vec::with_capacity(hist.passos().len());
        for passo in hist.passos() {
            passos.push(match passo {
                Comando::Traco { camada, mudanca } => {
                    let mut salvar_lado = |lado: &[(Posicao, Option<Tile>)]| {
                        lado.iter()
                            .map(|(p, t)| {
                                Ok(TileSalvo {
                                    c: p.0,
                                    l: p.1,
                                    hash: t.as_ref().map(&mut gravar_tile).transpose()?,
                                })
                            })
                            .collect::<Result<Vec<_>, ErroDoProjeto>>()
                    };
                    PassoSalvo::Traco {
                        camada: *camada,
                        antes: salvar_lado(&mudanca.antes)?,
                        depois: salvar_lado(&mudanca.depois)?,
                    }
                }
                Comando::Visibilidade {
                    camada,
                    antes,
                    depois,
                } => PassoSalvo::Visibilidade {
                    camada: *camada,
                    antes: *antes,
                    depois: *depois,
                },
                Comando::Opacidade {
                    camada,
                    antes,
                    depois,
                } => PassoSalvo::Opacidade {
                    camada: *camada,
                    antes: *antes,
                    depois: *depois,
                },
            });
        }

        // 2. A imagem editada — só com efeito (C30).
        let composta = if doc.neutro() {
            None
        } else {
            let imagem = composicao::compor(base, doc);
            let png = codificar_png(&imagem)?;
            let sha256 = hex(&Sha256::digest(&png));
            let caminho = self.caminho_da_composta(revisao);
            self.disco.gravar(&caminho, &png, Etapa::Composta)?;
            Some(CompostaSalva {
                arquivo: nome_da_composta(revisao),
                sha256,
                largura: imagem.width(),
                altura: imagem.height(),
            })
        };

        // 3. O manifesto, de uma vez.
        let manifesto = Manifesto {
            formato: FORMATO,
            revisao,
            base: doc.base.clone(),
            camadas,
            historico: HistoricoSalvo {
                passos,
                posicao: hist.posicao(),
            },
            composta,
        };
        let json = serde_json::to_vec_pretty(&manifesto)
            .map_err(|e| ErroDoProjeto::Formato(e.to_string()))?;
        self.disco
            .gravar(&self.pasta.join(MANIFESTO), &json, Etapa::Manifesto)?;

        self.podar_os_hashes();
        Ok(Salvo {
            revisao,
            versao: self.versao_do(edicao_id, &manifesto),
        })
    }

    fn versao_do(&self, edicao_id: &str, manifesto: &Manifesto) -> Option<VersaoEditada> {
        manifesto.composta.as_ref().map(|c| VersaoEditada {
            edicao_id: edicao_id.to_string(),
            revisao: manifesto.revisao,
            arquivo: self.pasta.join(&c.arquivo),
            sha256: c.sha256.clone(),
            largura: c.largura,
            altura: c.altura,
            base_sha256: manifesto.base.sha256.clone(),
        })
    }

    // ------------------------------------------------------------- ler

    /// O manifesto vigente, sem os pixels. `None` = projeto sem gravação.
    pub fn manifesto(&self) -> Result<Option<Manifesto>, ErroDoProjeto> {
        let caminho = self.pasta.join(MANIFESTO);
        if !self.disco.existe(&caminho) {
            return Ok(None);
        }
        let bytes = self.disco.ler(&caminho)?;
        let formato = serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|e| ErroDoProjeto::Formato(e.to_string()))?
            .get("formato")
            .and_then(|f| f.as_u64())
            .unwrap_or(0) as u32;
        if formato > FORMATO {
            return Err(ErroDoProjeto::FormatoNovo(formato));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| ErroDoProjeto::Formato(e.to_string()))
    }

    /// A revisão e a versão que o manifesto descreve.
    pub fn versao_gravada(
        &self,
        edicao_id: &str,
    ) -> Result<Option<(u64, Option<VersaoEditada>)>, ErroDoProjeto> {
        Ok(self
            .manifesto()?
            .map(|m| (m.revisao, self.versao_do(edicao_id, &m))))
    }

    /// A imagem editada existe e é a que a versão diz (hash do arquivo).
    pub fn conferir(&self, versao: &VersaoEditada) -> bool {
        self.disco
            .ler(&versao.arquivo)
            .is_ok_and(|bytes| hex(&Sha256::digest(&bytes)) == versao.sha256)
    }

    /// Reabre o projeto sobre a base neutra, que tem de ser a mesma de quando
    /// foi editado.
    pub fn abrir(&self, base: &RgbImage) -> Result<Option<Aberto>, ErroDoProjeto> {
        self.limpar_os_restos();
        let Some(manifesto) = self.manifesto()? else {
            return Ok(None);
        };
        let achada = BaseRef::da_imagem(base);
        if achada.sha256 != manifesto.base.sha256
            || (achada.largura, achada.altura) != (manifesto.base.largura, manifesto.base.altura)
        {
            return Err(ErroDoProjeto::BaseDiferente {
                esperada: manifesto.base.sha256.clone(),
                achada: achada.sha256,
            });
        }

        // 🔑 Um tile lido uma vez é o mesmo `Arc` para a camada e para todos os
        // passos do histórico que o citam — a reabertura não multiplica memória.
        let mut lidos: HashMap<String, Tile> = HashMap::new();
        let mut ler_tile = |hash: &str| -> Result<Tile, ErroDoProjeto> {
            if let Some(tile) = lidos.get(hash) {
                return Ok(tile.clone());
            }
            let comprimido = self.disco.ler(&self.caminho_do_tile(hash))?;
            let bruto = miniz_oxide::inflate::decompress_to_vec(&comprimido)
                .map_err(|e| ErroDoProjeto::TileCorrompido(format!("{hash}: {e:?}")))?;
            if bruto.len() != BYTES_DO_TILE || hex(&Sha256::digest(&bruto)) != hash {
                return Err(ErroDoProjeto::TileCorrompido(hash.to_string()));
            }
            let tile: Tile = Arc::new(bruto);
            self.hashes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(
                    Arc::as_ptr(&tile) as usize,
                    (tile.clone(), hash.to_string()),
                );
            lidos.insert(hash.to_string(), tile.clone());
            Ok(tile)
        };

        let (largura, altura) = (manifesto.base.largura, manifesto.base.altura);
        let mut camadas = Vec::with_capacity(manifesto.camadas.len());
        for salva in &manifesto.camadas {
            let mut pixels = CamadaDePixels::nova(largura, altura);
            for (chave, hash) in &salva.tiles {
                let posicao = posicao_da_chave(chave)
                    .ok_or_else(|| ErroDoProjeto::Formato(format!("tile {chave}")))?;
                pixels.definir(posicao, Some(ler_tile(hash)?));
            }
            camadas.push(Camada {
                nome: salva.nome.clone(),
                visivel: salva.visivel,
                opacidade: salva.opacidade,
                pixels,
            });
        }
        let mut passos = Vec::with_capacity(manifesto.historico.passos.len());
        for passo in &manifesto.historico.passos {
            passos.push(match passo {
                PassoSalvo::Traco {
                    camada,
                    antes,
                    depois,
                } => {
                    let mut lado = |v: &[TileSalvo]| {
                        v.iter()
                            .map(|t| {
                                Ok((
                                    (t.c, t.l),
                                    t.hash.as_deref().map(&mut ler_tile).transpose()?,
                                ))
                            })
                            .collect::<Result<Vec<_>, ErroDoProjeto>>()
                    };
                    Comando::Traco {
                        camada: *camada,
                        mudanca: Mudanca {
                            antes: lado(antes)?,
                            depois: lado(depois)?,
                        },
                    }
                }
                PassoSalvo::Visibilidade {
                    camada,
                    antes,
                    depois,
                } => Comando::Visibilidade {
                    camada: *camada,
                    antes: *antes,
                    depois: *depois,
                },
                PassoSalvo::Opacidade {
                    camada,
                    antes,
                    depois,
                } => Comando::Opacidade {
                    camada: *camada,
                    antes: *antes,
                    depois: *depois,
                },
            });
        }
        let posicao = manifesto.historico.posicao;
        Ok(Some(Aberto {
            documento: Documento {
                base: manifesto.base.clone(),
                camadas,
            },
            // Aberto é salvo: a posição gravada é o ponto de salvamento.
            historico: Historico::de_partes(passos, posicao, Some(posicao)),
            revisao: manifesto.revisao,
        }))
    }

    // ------------------------------------------------------ recuperação

    /// O manifesto está à frente do catálogo (a gravação parou entre os passos
    /// 3 e 4)? Devolve a revisão a adotar, se ela for utilizável: sem imagem
    /// editada (C30) ou com a imagem no disco e o hash conferido.
    pub fn a_adotar(
        &self,
        edicao_id: &str,
        revisao_no_catalogo: u64,
    ) -> Result<Option<(u64, Option<VersaoEditada>)>, ErroDoProjeto> {
        let Some((revisao, versao)) = self.versao_gravada(edicao_id)? else {
            return Ok(None);
        };
        if revisao <= revisao_no_catalogo {
            return Ok(None);
        }
        match &versao {
            Some(v) if !self.conferir(v) => Ok(None),
            _ => Ok(Some((revisao, versao))),
        }
    }

    /// Apaga os `.tmp` que uma gravação interrompida deixou.
    pub fn limpar_os_restos(&self) -> usize {
        let mut apagados = 0;
        for pasta in [self.pasta.clone(), self.pasta.join(PASTA_DOS_TILES)] {
            for caminho in self.disco.listar(&pasta).unwrap_or_default() {
                if caminho.extension().is_some_and(|e| e == "tmp")
                    && self.disco.apagar(&caminho).is_ok()
                {
                    apagados += 1;
                }
            }
        }
        apagados
    }

    /// O passo 6, depois de o catálogo confirmar `revisao_confirmada`: apaga os
    /// tiles que nem o manifesto nem o histórico citam, as imagens editadas de
    /// revisões anteriores e os `.tmp`.
    ///
    /// 🚨 **Nunca antes do catálogo.** A imagem que o catálogo aponta é a que a
    /// Revelação está usando agora.
    pub fn coletar(&self, revisao_confirmada: u64) -> Result<usize, ErroDoProjeto> {
        let Some(manifesto) = self.manifesto()? else {
            return Ok(0);
        };
        let mut citados: BTreeSet<String> = BTreeSet::new();
        for camada in &manifesto.camadas {
            citados.extend(camada.tiles.values().cloned());
        }
        for passo in &manifesto.historico.passos {
            if let PassoSalvo::Traco { antes, depois, .. } = passo {
                citados.extend(antes.iter().chain(depois).filter_map(|t| t.hash.clone()));
            }
        }
        let mut apagados = self.limpar_os_restos();
        for caminho in self.disco.listar(&self.pasta.join(PASTA_DOS_TILES))? {
            let hash = caminho
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if !citados.contains(&hash) && self.disco.apagar(&caminho).is_ok() {
                apagados += 1;
            }
        }
        let fica = revisao_confirmada.min(manifesto.revisao);
        for caminho in self.disco.listar(&self.pasta)? {
            if let Some(rev) = revisao_da_composta(&caminho) {
                if rev < fica && self.disco.apagar(&caminho).is_ok() {
                    apagados += 1;
                }
            }
        }
        Ok(apagados)
    }
}

pub fn nome_da_composta(revisao: u64) -> String {
    format!("composta-{revisao}.png")
}

fn revisao_da_composta(caminho: &Path) -> Option<u64> {
    let nome = caminho.file_name()?.to_str()?;
    nome.strip_prefix("composta-")?
        .strip_suffix(".png")?
        .parse()
        .ok()
}

/// PNG RGB8 com o chunk `sRGB` (C29) e compressão rápida — salvar uma foto de
/// 24 MP não pode segurar o operador por segundos.
pub fn codificar_png(imagem: &RgbImage) -> Result<Vec<u8>, ErroDoProjeto> {
    let mut saida = Vec::new();
    {
        let mut codificador = png::Encoder::new(&mut saida, imagem.width(), imagem.height());
        codificador.set_color(png::ColorType::Rgb);
        codificador.set_depth(png::BitDepth::Eight);
        codificador.set_compression(png::Compression::Fast);
        codificador.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut escritor = codificador
            .write_header()
            .map_err(|e| ErroDoProjeto::Codificacao(e.to_string()))?;
        escritor
            .write_image_data(imagem.as_raw())
            .map_err(|e| ErroDoProjeto::Codificacao(e.to_string()))?;
        escritor
            .finish()
            .map_err(|e| ErroDoProjeto::Codificacao(e.to_string()))?;
    }
    Ok(saida)
}

/// Lê a imagem editada de volta (RGB8).
pub fn ler_png(bytes: &[u8]) -> Result<RgbImage, ErroDoProjeto> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map(|i| i.to_rgb8())
        .map_err(|e| ErroDoProjeto::Codificacao(e.to_string()))
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::sessao::Sessao;

    fn base() -> Arc<RgbImage> {
        Arc::new(RgbImage::from_fn(700, 520, |x, y| {
            image::Rgb([
                (x * 7 % 256) as u8,
                (y * 3 % 256) as u8,
                ((x ^ y) % 256) as u8,
            ])
        }))
    }

    fn sessao_pintada(base: &Arc<RgbImage>) -> Sessao {
        let doc = Documento::novo(BaseRef::da_imagem(base));
        let mut s = Sessao::nova(base.clone(), doc, Historico::novo(), 300);
        s.pincel.cor = [250, 20, 10];
        s.apertar(100.0, 100.0);
        s.arrastar(600.0, 400.0);
        s.soltar();
        s.pincel.cor = [0, 200, 0];
        s.apertar(300.0, 50.0);
        s.arrastar(300.0, 500.0);
        s.soltar();
        s.mover_opacidade(0.8);
        s.confirmar_opacidade();
        s
    }

    fn projeto(pasta: &Path, disco: Arc<dyn Disco>) -> Projeto {
        Projeto::novo(pasta.join("e1"), disco)
    }

    #[test]
    fn salvar_e_reabrir_devolvem_documento_e_historico() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        s.desfazer(); // um passo para refazer também precisa voltar
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        let salvo = p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        let versao = salvo.versao.expect("com efeito há imagem editada");
        assert!(p.conferir(&versao));
        assert_eq!((versao.largura, versao.altura), (700, 520));
        let lida = ler_png(&std::fs::read(&versao.arquivo).unwrap()).unwrap();
        assert_eq!(lida.as_raw(), composicao::compor(&base, &doc).as_raw());

        let outro = projeto(dir.path(), Arc::new(DiscoReal));
        let aberto = outro.abrir(&base).unwrap().unwrap();
        assert_eq!(aberto.documento, doc);
        assert_eq!(aberto.historico.passos(), hist.passos());
        assert_eq!(aberto.historico.posicao(), hist.posicao());
        assert!(!aberto.historico.alterado());
        assert_eq!(aberto.revisao, 1);

        // E o refazer e o desfazer continuam a partir da reabertura.
        let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 300);
        assert!(s2.refazer());
        assert!(s2.desfazer());
        assert!(s2.desfazer());
        assert!(s2.desfazer());
        assert!(!s2.desfazer());
        assert!(s2.documento().camadas[0].pixels.vazia());
    }

    #[test]
    fn documento_neutro_nao_publica_imagem_editada() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        s.alternar_visibilidade();
        let (doc, hist) = s.instantaneo();
        let salvo = projeto(dir.path(), Arc::new(DiscoReal))
            .salvar("e1", &base, &doc, &hist, 3)
            .unwrap();
        assert!(salvo.versao.is_none());
        assert!(!dir.path().join("e1").join(nome_da_composta(3)).exists());
    }

    #[test]
    fn falha_em_cada_etapa_mantem_a_revisao_anterior_utilizavel() {
        let base = base();
        for etapa in [Etapa::Tiles, Etapa::Composta, Etapa::Manifesto] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = sessao_pintada(&base);
            let (doc1, hist1) = s.instantaneo();
            let bom = projeto(dir.path(), Arc::new(DiscoReal));
            let v1 = bom
                .salvar("e1", &base, &doc1, &hist1, 1)
                .unwrap()
                .versao
                .unwrap();
            bom.coletar(1).unwrap();

            // Mais pintura, e a gravação da revisão 2 cai no meio.
            s.pincel.cor = [1, 2, 3];
            s.apertar(650.0, 20.0);
            s.arrastar(20.0, 500.0);
            s.soltar();
            let (doc2, hist2) = s.instantaneo();
            let ruim = projeto(dir.path(), Arc::new(DiscoComFalha::novo(etapa, 0)));
            let erro = ruim.salvar("e1", &base, &doc2, &hist2, 2);
            assert!(erro.is_err(), "{etapa:?}");

            // O catálogo continua na 1, e ela está inteira.
            assert!(bom.conferir(&v1), "{etapa:?}: a imagem da revisão 1");
            let aberto = bom.abrir(&base).unwrap().unwrap();
            assert_eq!(aberto.revisao, 1, "{etapa:?}");
            assert_eq!(aberto.documento, doc1, "{etapa:?}");
            assert!(bom.a_adotar("e1", 1).unwrap().is_none());
            // E o .tmp que a queda deixou foi embora na reabertura.
            let restos: Vec<_> = walk(dir.path())
                .into_iter()
                .filter(|c| c.extension().is_some_and(|e| e == "tmp"))
                .collect();
            assert!(restos.is_empty(), "{etapa:?}: {restos:?}");
        }
    }

    #[test]
    fn manifesto_a_frente_do_catalogo_e_adotado() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 4).unwrap();
        // O catálogo ficou na 3 (a gravação caiu entre o manifesto e o banco).
        let (rev, versao) = p.a_adotar("e1", 3).unwrap().expect("adotar a 4");
        assert_eq!(rev, 4);
        assert!(versao.is_some());
        // Com a imagem corrompida, não se adota.
        std::fs::write(dir.path().join("e1").join(nome_da_composta(4)), b"lixo").unwrap();
        assert!(p.a_adotar("e1", 3).unwrap().is_none());
    }

    #[test]
    fn a_coleta_nao_apaga_tile_do_historico_nem_a_imagem_confirmada() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        s.pincel.ferramenta = crate::pincel::Ferramenta::Borracha;
        s.pincel.raio = 400.0;
        s.apertar(350.0, 260.0);
        s.soltar();
        let (doc2, hist2) = s.instantaneo();
        let v2 = p.salvar("e1", &base, &doc2, &hist2, 2).unwrap().versao;
        // Catálogo ainda na 1: a composta 1 fica.
        p.coletar(1).unwrap();
        assert!(dir.path().join("e1").join(nome_da_composta(1)).exists());
        p.coletar(2).unwrap();
        assert!(!dir.path().join("e1").join(nome_da_composta(1)).exists());
        if let Some(v2) = v2 {
            assert!(p.conferir(&v2));
        }
        // O desfazer depois de reabrir ainda acha os tiles apagados pela borracha.
        let aberto = p.abrir(&base).unwrap().unwrap();
        let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 300);
        assert!(s2.desfazer());
        assert_eq!(s2.documento(), &doc);
    }

    #[test]
    fn reabrir_sobre_outra_base_e_recusado() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        let mut outra = (*base).clone();
        outra.put_pixel(0, 0, image::Rgb([9, 9, 9]));
        assert!(matches!(
            p.abrir(&outra),
            Err(ErroDoProjeto::BaseDiferente { .. })
        ));
    }

    #[test]
    fn tile_corrompido_e_formato_novo_nao_passam_calados() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        let um_tile = walk(&dir.path().join("e1").join("tiles"))[0].clone();
        std::fs::write(
            &um_tile,
            miniz_oxide::deflate::compress_to_vec(&[0u8; 16], 1),
        )
        .unwrap();
        assert!(matches!(
            Projeto::novo(dir.path().join("e1"), Arc::new(DiscoReal)).abrir(&base),
            Err(ErroDoProjeto::TileCorrompido(_))
        ));
        let manifesto = dir.path().join("e1").join(MANIFESTO);
        let texto = std::fs::read_to_string(&manifesto).unwrap().replacen(
            "\"formato\": 1",
            "\"formato\": 9",
            1,
        );
        std::fs::write(&manifesto, texto).unwrap();
        assert!(matches!(p.abrir(&base), Err(ErroDoProjeto::FormatoNovo(9))));
    }

    fn walk(pasta: &Path) -> Vec<PathBuf> {
        let mut todos = Vec::new();
        for item in std::fs::read_dir(pasta).unwrap().flatten() {
            let caminho = item.path();
            if caminho.is_dir() {
                todos.extend(walk(&caminho));
            } else {
                todos.push(caminho);
            }
        }
        todos
    }
}
