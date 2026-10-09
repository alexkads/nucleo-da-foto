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

use crate::ajuste::Ajuste;
use crate::composicao;
use crate::contrato::VersaoEditada;
use crate::documento::{hex, BaseRef, Bloqueio, Camada, Documento, Mascara, Retoque};
use crate::historico::{Comando, Historico};
use crate::mesclagem::Modo;
use crate::pincel::Mudanca;
use crate::tiles::{CamadaDePixels, Posicao, Tile, BYTES_DO_TILE};
use crate::vetor::{Caminho, Caminhos, LugarDoCaminho, MascaraVetorial};

/// A versão do formato do projeto. Maior que esta é recusado — quem abre não
/// sabe ler, e não grava por cima.
///
/// - **1** (0.1.28): uma camada; propriedades e traços no histórico.
/// - **3** (etapa 3): o passo `mesclar` (⌘E). O número sobe para a 0.1.94,
///   que não sabe desfazer uma mesclagem, recusar com o aviso em vez de falhar.
/// - **2** (etapa 2): várias camadas, o `modo` de mesclagem de cada uma, e os
///   passos de criar, excluir, mover, renomear e mudar o modo. O 1 se lê como
///   está (o modo que falta é o Normal); o app de antes recusa o 2 com a
///   mensagem de "versão mais nova", em vez de compor as camadas errado.
/// - **4** (etapa 10): a máscara de camada (`mascara` na camada salva, o passo
///   `mascara` e o traço com `na_mascara`). A 0.1.101 comporia a camada sem a
///   máscara — recusa com o aviso.
/// - **5** (etapa 11): a camada de ajuste (`ajuste` na camada salva e o passo
///   `ajuste`). A 0.1.102 comporia a camada como vazia — recusa com o aviso.
/// - **6** (etapa 13): o passo `varios` (camada via recorte num desfazer só).
///   A 0.1.108 não saberia desfazê-lo — recusa com o aviso. Os passos de
///   seleção, que entraram no desfazer na mesma etapa, **não** são gravados.
///   Os formatos 1–5 se leem como estão.
/// - **7** (etapa 14): tiles fora da foto — coluna e linha com sinal, ou além
///   da última (o conteúdo que o Mover e o ⌘T levam para fora). A 0.1.111
///   leria a coluna negativa como erro, e a de além da borda como lixo na
///   composta — recusa com o aviso. Os formatos 1–6 se leem como estão.
/// - **8** (etapa 15): a máscara de corte (`recortada` na camada salva e o
///   passo `recorte`). A 0.1.114 comporia a camada recortada solta, por cima
///   de tudo — recusa com o aviso. A malha do Deformar **não** é gravada (o
///   resultado vai como pixels, num passo de traço). Os formatos 1–7 se leem
///   como estão.
/// - **9** (etapa 16): propriedades da máscara (`vinculada`, `densidade`,
///   `difusao`), os cadeados da camada (`bloqueio`) e o passo `bloqueio`. A
///   0.1.115 comporia a máscara sem densidade nem difusão e ignoraria os
///   cadeados — recusa com o aviso. Os formatos 1–8 se leem como estão.
/// - **10** (0.1.117): a camada de ajuste Curvas (`"tipo": "curvas"`, com
///   os pontos de cada curva). A 0.1.116 não saberia ler o tipo — recusa com o
///   aviso. Os formatos 1–9 se leem como estão.
/// - **11** (a Caneta): os caminhos (`caminhos` no manifesto: o de trabalho e
///   os nomeados, com ids, subcaminhos, alças e ligações), a máscara vetorial
///   da camada (`mascara_vetorial`) e os passos `caminho` e
///   `mascara_vetorial`. A 0.1.124 comporia a camada sem a máscara vetorial
///   — recusa com o aviso. Os formatos 1–10 se leem como estão (sem
///   caminhos).
/// - **12** (tratamento de pele): o modo `luz_linear` e o papel de retoque da
///   camada (`retoque`: baixa/alta frequência, clarear/escurecer). Quem lê
///   até o 11 não saberia ler o modo — recusa com o aviso. Os formatos 1–11
///   se leem como estão (sem `retoque`, a camada não tem papel).
/// - **13** (a Caneta, segunda rodada): a camada de preenchimento/forma
///   (`"tipo": "cor_solida"` no ajuste), densidade, difusão e `revela_vazia`
///   da máscara vetorial e as âncoras `automatica` (a Caneta de curvatura).
///   A 0.1.125 não saberia ler o tipo e comporia a máscara sem densidade nem
///   difusão — recusa com o aviso. Os formatos 1–12 se leem como estão.
pub const FORMATO: u32 = 13;

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
    /// As guias (0.1.123). Campo opcional: o formato não muda, e a versão de
    /// antes só não as lê.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guias: Vec<crate::documento::Guia>,
    /// Formato 11: os caminhos do painel — o de trabalho também, para a
    /// recuperação do projeto.
    #[serde(default, skip_serializing_if = "Caminhos::vazio")]
    pub caminhos: Caminhos,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamadaSalva {
    pub nome: String,
    pub visivel: bool,
    pub opacidade: f32,
    /// Ausente no formato 1: Normal.
    #[serde(default)]
    pub modo: Modo,
    /// `"coluna,linha"` → hash do tile.
    pub tiles: BTreeMap<String, String>,
    /// Ausente até o formato 3, e em camada sem máscara.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mascara: Option<MascaraSalva>,
    /// Ausente até o formato 4, e em camada de pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ajuste: Option<Ajuste>,
    /// Formato 8: a máscara de corte (ausente = solta).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recortada: bool,
    /// Formato 9: os cadeados (ausente = nenhum).
    #[serde(default, skip_serializing_if = "sem_bloqueio")]
    pub bloqueio: Bloqueio,
    /// Formato 11: a máscara vetorial (o caminho, ligada, vinculada).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mascara_vetorial: Option<MascaraVetorial>,
    /// Formato 12: o papel no tratamento de pele (ausente = nenhum).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retoque: Option<Retoque>,
}

fn sem_bloqueio(b: &Bloqueio) -> bool {
    !b.algum()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MascaraSalva {
    pub fundo: u8,
    pub ativa: bool,
    pub tiles: BTreeMap<String, String>,
    /// Formato 9: a corrente com a camada (ausente = vinculada).
    #[serde(default = "verdadeiro", skip_serializing_if = "Clone::clone")]
    pub vinculada: bool,
    /// Formato 9: a densidade (ausente = 100%).
    #[serde(default = "um", skip_serializing_if = "e_um")]
    pub densidade: f32,
    /// Formato 9: a difusão em px (ausente = 0).
    #[serde(default, skip_serializing_if = "e_zero")]
    pub difusao: f32,
}

fn verdadeiro() -> bool {
    true
}

fn um() -> f32 {
    1.0
}

fn e_um(v: &f32) -> bool {
    *v == 1.0
}

fn e_zero(v: &f32) -> bool {
    *v == 0.0
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
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        na_mascara: bool,
        antes: Vec<TileSalvo>,
        depois: Vec<TileSalvo>,
    },
    Mascara {
        camada: usize,
        antes: Option<MascaraSalva>,
        depois: Option<MascaraSalva>,
    },
    Ajuste {
        camada: usize,
        antes: Ajuste,
        depois: Ajuste,
    },
    Visibilidade {
        camada: usize,
        antes: bool,
        depois: bool,
    },
    /// Formato 8: a máscara de corte ligada ou liberada.
    Recorte {
        camada: usize,
        antes: bool,
        depois: bool,
    },
    /// Formato 9: os cadeados da camada.
    Bloqueio {
        camada: usize,
        antes: Bloqueio,
        depois: Bloqueio,
    },
    Opacidade {
        camada: usize,
        antes: f32,
        depois: f32,
    },
    Modo {
        camada: usize,
        antes: Modo,
        depois: Modo,
    },
    Renomear {
        camada: usize,
        antes: String,
        depois: String,
    },
    CriarCamada {
        indice: usize,
        camada: CamadaSalva,
    },
    ExcluirCamada {
        indice: usize,
        camada: CamadaSalva,
    },
    MoverCamada {
        de: usize,
        para: usize,
    },
    Mesclar {
        indice: usize,
        de_cima: CamadaSalva,
        antes: Vec<TileSalvo>,
        depois: Vec<TileSalvo>,
    },
    /// Formato 6: vários passos que são um gesto só (camada via recorte).
    Varios {
        nome: String,
        passos: Vec<PassoSalvo>,
    },
    /// Formato 11: um caminho antes e depois (um gesto da Caneta).
    Caminho {
        nome: String,
        lugar: LugarDoCaminho,
        #[serde(default)]
        indice: usize,
        antes: Option<Caminho>,
        depois: Option<Caminho>,
    },
    /// Formato 11: a máscara vetorial antes e depois.
    MascaraVetorial {
        camada: usize,
        antes: Option<MascaraVetorial>,
        depois: Option<MascaraVetorial>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileSalvo {
    /// Com sinal desde o formato 7: o tile pode estar fora da foto.
    pub c: i32,
    pub l: i32,
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

/// Uma camada para o manifesto, gravando os tiles dela.
fn salvar_camada(
    camada: &Camada,
    gravar_tile: &mut dyn FnMut(&Tile) -> Result<String, ErroDoProjeto>,
) -> Result<CamadaSalva, ErroDoProjeto> {
    let mut tiles = BTreeMap::new();
    // Também os tiles de fora da foto (formato 7).
    for (posicao, tile) in camada.pixels.todos() {
        tiles.insert(chave_do_tile(*posicao), gravar_tile(tile)?);
    }
    Ok(CamadaSalva {
        nome: camada.nome.clone(),
        visivel: camada.visivel,
        opacidade: camada.opacidade,
        modo: camada.modo,
        tiles,
        mascara: camada
            .mascara
            .as_ref()
            .map(|m| salvar_mascara(m, gravar_tile))
            .transpose()?,
        ajuste: camada.ajuste,
        recortada: camada.recortada,
        bloqueio: camada.bloqueio,
        mascara_vetorial: camada.mascara_vetorial.clone(),
        retoque: camada.retoque,
    })
}

/// Um passo do histórico para o manifesto, gravando os tiles que ele cita.
fn salvar_passo(
    passo: &Comando,
    gravar_tile: &mut dyn FnMut(&Tile) -> Result<String, ErroDoProjeto>,
) -> Result<PassoSalvo, ErroDoProjeto> {
    Ok(match passo {
        Comando::Ajuste {
            camada,
            antes,
            depois,
        } => PassoSalvo::Ajuste {
            camada: *camada,
            antes: *antes,
            depois: *depois,
        },
        Comando::Mascara {
            camada,
            antes,
            depois,
        } => {
            let mut lado = |m: &Option<Box<Mascara>>| {
                m.as_deref()
                    .map(|m| salvar_mascara(m, &mut *gravar_tile))
                    .transpose()
            };
            PassoSalvo::Mascara {
                camada: *camada,
                antes: lado(antes)?,
                depois: lado(depois)?,
            }
        }
        Comando::Traco {
            camada,
            na_mascara,
            mudanca,
        } => {
            let mut salvar_lado = |lado: &[(Posicao, Option<Tile>)]| {
                lado.iter()
                    .map(|(p, t)| {
                        Ok(TileSalvo {
                            c: p.0,
                            l: p.1,
                            hash: t.as_ref().map(&mut *gravar_tile).transpose()?,
                        })
                    })
                    .collect::<Result<Vec<_>, ErroDoProjeto>>()
            };
            PassoSalvo::Traco {
                camada: *camada,
                na_mascara: *na_mascara,
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
        Comando::Recorte {
            camada,
            antes,
            depois,
        } => PassoSalvo::Recorte {
            camada: *camada,
            antes: *antes,
            depois: *depois,
        },
        Comando::Bloqueio {
            camada,
            antes,
            depois,
        } => PassoSalvo::Bloqueio {
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
        Comando::Modo {
            camada,
            antes,
            depois,
        } => PassoSalvo::Modo {
            camada: *camada,
            antes: *antes,
            depois: *depois,
        },
        Comando::Renomear {
            camada,
            antes,
            depois,
        } => PassoSalvo::Renomear {
            camada: *camada,
            antes: antes.clone(),
            depois: depois.clone(),
        },
        Comando::CriarCamada { indice, camada } => PassoSalvo::CriarCamada {
            indice: *indice,
            camada: salvar_camada(camada, &mut *gravar_tile)?,
        },
        Comando::ExcluirCamada { indice, camada } => PassoSalvo::ExcluirCamada {
            indice: *indice,
            camada: salvar_camada(camada, &mut *gravar_tile)?,
        },
        Comando::MoverCamada { de, para } => PassoSalvo::MoverCamada {
            de: *de,
            para: *para,
        },
        Comando::Mesclar {
            indice,
            de_cima,
            mudanca,
        } => {
            let de_cima = salvar_camada(de_cima, &mut *gravar_tile)?;
            let mut lado = |l: &[(Posicao, Option<Tile>)]| {
                l.iter()
                    .map(|(p, t)| {
                        Ok(TileSalvo {
                            c: p.0,
                            l: p.1,
                            hash: t.as_ref().map(&mut *gravar_tile).transpose()?,
                        })
                    })
                    .collect::<Result<Vec<_>, ErroDoProjeto>>()
            };
            PassoSalvo::Mesclar {
                indice: *indice,
                de_cima,
                antes: lado(&mudanca.antes)?,
                depois: lado(&mudanca.depois)?,
            }
        }
        Comando::Varios { nome, passos } => PassoSalvo::Varios {
            nome: nome.clone(),
            passos: passos
                .iter()
                .map(|p| salvar_passo(p, gravar_tile))
                .collect::<Result<_, _>>()?,
        },
        Comando::Caminho {
            nome,
            lugar,
            indice,
            antes,
            depois,
        } => PassoSalvo::Caminho {
            nome: nome.clone(),
            lugar: *lugar,
            indice: *indice,
            antes: antes.as_deref().cloned(),
            depois: depois.as_deref().cloned(),
        },
        Comando::MascaraVetorial {
            camada,
            antes,
            depois,
        } => PassoSalvo::MascaraVetorial {
            camada: *camada,
            antes: antes.as_deref().cloned(),
            depois: depois.as_deref().cloned(),
        },
        // `Historico::para_gravar` já tirou os passos de seleção e de guias.
        Comando::Selecao { .. } | Comando::Guias { .. } => {
            return Err(ErroDoProjeto::Formato(
                "passo de seleção não vai para o projeto".into(),
            ))
        }
    })
}

/// Um passo do manifesto de volta ao histórico.
///
/// O tipo dos leitores fica por extenso: um apelido com tempo de vida
/// explícito (`dyn FnMut … + 'a`) não casa com o dos fechos de quem chama.
#[allow(clippy::type_complexity)]
fn ler_passo(
    passo: &PassoSalvo,
    ler_tile: &mut dyn FnMut(&str) -> Result<Tile, ErroDoProjeto>,
    ler_camada: &dyn Fn(
        &CamadaSalva,
        &mut dyn FnMut(&str) -> Result<Tile, ErroDoProjeto>,
    ) -> Result<Camada, ErroDoProjeto>,
    ler_mascara: &dyn Fn(
        &MascaraSalva,
        &mut dyn FnMut(&str) -> Result<Tile, ErroDoProjeto>,
    ) -> Result<Mascara, ErroDoProjeto>,
) -> Result<Comando, ErroDoProjeto> {
    Ok(match passo {
        PassoSalvo::Ajuste {
            camada,
            antes,
            depois,
        } => Comando::Ajuste {
            camada: *camada,
            antes: *antes,
            depois: *depois,
        },
        PassoSalvo::Mascara {
            camada,
            antes,
            depois,
        } => {
            let mut lado = |m: &Option<MascaraSalva>| {
                m.as_ref()
                    .map(|m| ler_mascara(m, &mut *ler_tile).map(Box::new))
                    .transpose()
            };
            Comando::Mascara {
                camada: *camada,
                antes: lado(antes)?,
                depois: lado(depois)?,
            }
        }
        PassoSalvo::Traco {
            camada,
            na_mascara,
            antes,
            depois,
        } => {
            let mut lado = |v: &[TileSalvo]| {
                v.iter()
                    .map(|t| {
                        Ok((
                            (t.c, t.l),
                            t.hash.as_deref().map(&mut *ler_tile).transpose()?,
                        ))
                    })
                    .collect::<Result<Vec<_>, ErroDoProjeto>>()
            };
            Comando::Traco {
                camada: *camada,
                na_mascara: *na_mascara,
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
        PassoSalvo::Recorte {
            camada,
            antes,
            depois,
        } => Comando::Recorte {
            camada: *camada,
            antes: *antes,
            depois: *depois,
        },
        PassoSalvo::Bloqueio {
            camada,
            antes,
            depois,
        } => Comando::Bloqueio {
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
        PassoSalvo::Modo {
            camada,
            antes,
            depois,
        } => Comando::Modo {
            camada: *camada,
            antes: *antes,
            depois: *depois,
        },
        PassoSalvo::Renomear {
            camada,
            antes,
            depois,
        } => Comando::Renomear {
            camada: *camada,
            antes: antes.clone(),
            depois: depois.clone(),
        },
        PassoSalvo::CriarCamada { indice, camada } => Comando::CriarCamada {
            indice: *indice,
            camada: Box::new(ler_camada(camada, &mut *ler_tile)?),
        },
        PassoSalvo::ExcluirCamada { indice, camada } => Comando::ExcluirCamada {
            indice: *indice,
            camada: Box::new(ler_camada(camada, &mut *ler_tile)?),
        },
        PassoSalvo::MoverCamada { de, para } => Comando::MoverCamada {
            de: *de,
            para: *para,
        },
        PassoSalvo::Mesclar {
            indice,
            de_cima,
            antes,
            depois,
        } => {
            let de_cima = Box::new(ler_camada(de_cima, &mut *ler_tile)?);
            let mut lado = |v: &[TileSalvo]| {
                v.iter()
                    .map(|t| {
                        Ok((
                            (t.c, t.l),
                            t.hash.as_deref().map(&mut *ler_tile).transpose()?,
                        ))
                    })
                    .collect::<Result<Vec<_>, ErroDoProjeto>>()
            };
            Comando::Mesclar {
                indice: *indice,
                de_cima,
                mudanca: Mudanca {
                    antes: lado(antes)?,
                    depois: lado(depois)?,
                },
            }
        }
        PassoSalvo::Varios { nome, passos } => Comando::Varios {
            nome: nome.clone(),
            passos: passos
                .iter()
                .map(|p| ler_passo(p, &mut *ler_tile, ler_camada, ler_mascara))
                .collect::<Result<_, _>>()?,
        },
        PassoSalvo::Caminho {
            nome,
            lugar,
            indice,
            antes,
            depois,
        } => Comando::Caminho {
            nome: nome.clone(),
            lugar: *lugar,
            indice: *indice,
            antes: antes.clone().map(Box::new),
            depois: depois.clone().map(Box::new),
        },
        PassoSalvo::MascaraVetorial {
            camada,
            antes,
            depois,
        } => Comando::MascaraVetorial {
            camada: *camada,
            antes: antes.clone().map(Box::new),
            depois: depois.clone().map(Box::new),
        },
    })
}

fn salvar_mascara(
    mascara: &Mascara,
    gravar_tile: &mut dyn FnMut(&Tile) -> Result<String, ErroDoProjeto>,
) -> Result<MascaraSalva, ErroDoProjeto> {
    let mut tiles = BTreeMap::new();
    for (posicao, tile) in mascara.pixels.todos() {
        tiles.insert(chave_do_tile(*posicao), gravar_tile(tile)?);
    }
    Ok(MascaraSalva {
        fundo: mascara.fundo,
        ativa: mascara.ativa,
        tiles,
        vinculada: mascara.vinculada,
        densidade: mascara.densidade,
        difusao: mascara.difusao,
    })
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
            camadas.push(salvar_camada(camada, &mut gravar_tile)?);
        }
        // Os passos só de seleção ficam de fora (a seleção não é salva), e a
        // posição conta só os que ficaram.
        let (comandos, posicao) = hist.para_gravar();
        let mut passos = Vec::with_capacity(comandos.len());
        for passo in &comandos {
            passos.push(salvar_passo(passo, &mut gravar_tile)?);
        }

        // 2. A imagem editada — só com efeito (C30). 🔑 Uma camada que só
        // repete a base (a camada da fotografia recém-criada, ou retoques que
        // se anularam) tem pixels, mas compõe a base byte a byte: também não é
        // edição, e nenhuma versão é publicada.
        let imagem = (!doc.neutro())
            .then(|| composicao::compor(base, doc))
            .filter(|imagem| imagem.as_raw() != base.as_raw());
        let composta = if let Some(imagem) = imagem {
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
        } else {
            None
        };

        // 3. O manifesto, de uma vez.
        let manifesto = Manifesto {
            formato: FORMATO,
            revisao,
            base: doc.base.clone(),
            camadas,
            historico: HistoricoSalvo { passos, posicao },
            composta,
            guias: doc.guias.clone(),
            caminhos: doc.caminhos.clone(),
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
        let ler_pixels = |tiles: &BTreeMap<String, String>,
                          ler_tile: &mut dyn FnMut(&str) -> Result<Tile, ErroDoProjeto>|
         -> Result<CamadaDePixels, ErroDoProjeto> {
            let mut pixels = CamadaDePixels::nova(largura, altura);
            for (chave, hash) in tiles {
                let posicao = posicao_da_chave(chave)
                    .ok_or_else(|| ErroDoProjeto::Formato(format!("tile {chave}")))?;
                pixels.definir(posicao, Some(ler_tile(hash)?));
            }
            Ok(pixels)
        };
        let ler_mascara = |salva: &MascaraSalva,
                           ler_tile: &mut dyn FnMut(&str) -> Result<Tile, ErroDoProjeto>|
         -> Result<Mascara, ErroDoProjeto> {
            let mut m = Mascara::com_pixels(salva.fundo, ler_pixels(&salva.tiles, ler_tile)?);
            m.ativa = salva.ativa;
            m.vinculada = salva.vinculada;
            m.densidade = salva.densidade;
            m.difusao = salva.difusao;
            Ok(m)
        };
        let ler_camada = |salva: &CamadaSalva,
                          ler_tile: &mut dyn FnMut(&str) -> Result<Tile, ErroDoProjeto>|
         -> Result<Camada, ErroDoProjeto> {
            Ok(Camada {
                nome: salva.nome.clone(),
                visivel: salva.visivel,
                opacidade: salva.opacidade,
                modo: salva.modo,
                pixels: ler_pixels(&salva.tiles, ler_tile)?,
                mascara: salva
                    .mascara
                    .as_ref()
                    .map(|m| ler_mascara(m, ler_tile))
                    .transpose()?,
                ajuste: salva.ajuste,
                recortada: salva.recortada,
                bloqueio: salva.bloqueio,
                mascara_vetorial: salva.mascara_vetorial.clone(),
                retoque: salva.retoque,
            })
        };
        let mut camadas = Vec::with_capacity(manifesto.camadas.len());
        for salva in &manifesto.camadas {
            camadas.push(ler_camada(salva, &mut ler_tile)?);
        }
        let mut passos = Vec::with_capacity(manifesto.historico.passos.len());
        for passo in &manifesto.historico.passos {
            passos.push(ler_passo(passo, &mut ler_tile, &ler_camada, &ler_mascara)?);
        }
        let posicao = manifesto.historico.posicao;
        Ok(Some(Aberto {
            documento: Documento {
                base: manifesto.base.clone(),
                camadas,
                guias: manifesto.guias.clone(),
                caminhos: manifesto.caminhos.clone(),
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
        // A camada e a máscara dela.
        let citar = |citados: &mut BTreeSet<String>, camada: &CamadaSalva| {
            citados.extend(camada.tiles.values().cloned());
            if let Some(m) = &camada.mascara {
                citados.extend(m.tiles.values().cloned());
            }
        };
        for camada in &manifesto.camadas {
            citar(&mut citados, camada);
        }
        // 🚨 Os passos de dentro de um `varios` citam tiles também: abertos
        // numa pilha, senão a coleta apagaria o que o desfazer devolve.
        let mut pilha: Vec<&PassoSalvo> = manifesto.historico.passos.iter().collect();
        while let Some(passo) = pilha.pop() {
            match passo {
                PassoSalvo::Varios { passos, .. } => pilha.extend(passos.iter()),
                PassoSalvo::Traco { antes, depois, .. } => {
                    citados.extend(antes.iter().chain(depois).filter_map(|t| t.hash.clone()));
                }
                // 🚨 A camada excluída mora só no histórico: o desfazer a
                // devolve com estes tiles.
                PassoSalvo::CriarCamada { camada, .. }
                | PassoSalvo::ExcluirCamada { camada, .. } => {
                    citar(&mut citados, camada);
                }
                // A máscara excluída mora só aqui.
                PassoSalvo::Mascara { antes, depois, .. } => {
                    for m in antes.iter().chain(depois) {
                        citados.extend(m.tiles.values().cloned());
                    }
                }
                // A de cima mora só no histórico, e a de baixo de antes também.
                PassoSalvo::Mesclar {
                    de_cima,
                    antes,
                    depois,
                    ..
                } => {
                    citar(&mut citados, de_cima);
                    citados.extend(antes.iter().chain(depois).filter_map(|t| t.hash.clone()));
                }
                _ => {}
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
    fn varias_camadas_modos_e_a_camada_excluida_voltam_da_gravacao() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        s.nova_camada();
        s.pincel.cor = [255, 255, 255];
        s.apertar(500.0, 300.0);
        s.soltar();
        s.mudar_modo(Modo::Tela);
        s.renomear_camada(1, "Brilho");
        s.duplicar_camada();
        s.mover_camada(-1);
        // A de baixo sai — os tiles dela só existem no histórico agora.
        s.escolher_camada(0);
        let tiles_da_excluida: Vec<Tile> = s.documento().camadas[0]
            .pixels
            .existentes()
            .map(|(_, t)| t.clone())
            .collect();
        assert!(s.excluir_camada());
        let (doc, hist) = s.instantaneo();
        assert_eq!(doc.camadas.len(), 2);

        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("e1").join(MANIFESTO)).unwrap())
                .unwrap();
        assert_eq!(json["formato"], FORMATO);
        assert_eq!(json["camadas"][0]["modo"], "tela");
        p.coletar(1).unwrap();

        let aberto = p.abrir(&base).unwrap().unwrap();
        assert_eq!(aberto.documento, doc);
        assert_eq!(aberto.historico.passos(), hist.passos());
        let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 300);
        assert!(
            s2.desfazer(),
            "desfaz a exclusão depois de reabrir e coletar"
        );
        assert_eq!(s2.documento().camadas.len(), 3);
        let devolvidos: Vec<Tile> = s2.documento().camadas[0]
            .pixels
            .existentes()
            .map(|(_, t)| t.clone())
            .collect();
        assert_eq!(devolvidos, tiles_da_excluida);
        assert_eq!(
            s2.compor().as_raw(),
            {
                let mut s3 = sessao_pintada(&base);
                s3.nova_camada();
                s3.pincel.cor = [255, 255, 255];
                s3.apertar(500.0, 300.0);
                s3.soltar();
                s3.mudar_modo(Modo::Tela);
                s3.duplicar_camada();
                s3.mover_camada(-1);
                s3.compor()
            }
            .as_raw()
        );
    }

    #[test]
    fn a_mesclagem_volta_da_gravacao_e_se_desfaz_depois_da_coleta() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        s.nova_camada();
        s.pincel.cor = [255, 255, 255];
        s.apertar(320.0, 240.0);
        s.soltar();
        s.mudar_modo(Modo::Sobrepor);
        let pilha = s.documento().clone();
        s.mesclar_para_baixo().unwrap();
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        p.coletar(1).unwrap();
        let aberto = p.abrir(&base).unwrap().unwrap();
        assert_eq!(aberto.documento, doc);
        let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 300);
        assert!(s2.desfazer());
        assert_eq!(s2.documento(), &pilha);
    }

    #[test]
    fn a_mascara_e_os_passos_dela_voltam_da_gravacao_mesmo_depois_da_coleta() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        // Uma máscara pintada que depois é excluída: os tiles dela moram só
        // no histórico.
        s.adicionar_mascara(false);
        s.pincel.cor = [0, 0, 0];
        s.apertar(320.0, 240.0);
        s.soltar();
        let com_mascara = s.documento().clone();
        s.excluir_mascara();
        // E outra, que fica, desligada e com um traço.
        s.adicionar_mascara(true);
        s.pincel.cor = [255, 255, 255];
        s.apertar(200.0, 200.0);
        s.soltar();
        s.alternar_mascara_de(0);
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        p.coletar(1).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("e1").join(MANIFESTO)).unwrap())
                .unwrap();
        assert_eq!(json["formato"], FORMATO);
        assert_eq!(json["camadas"][0]["mascara"]["fundo"], 0);

        let aberto = p.abrir(&base).unwrap().unwrap();
        assert_eq!(aberto.documento, doc);
        assert_eq!(aberto.historico.passos(), hist.passos());
        let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 300);
        for _ in 0..4 {
            assert!(s2.desfazer());
        }
        assert_eq!(
            s2.documento(),
            &com_mascara,
            "a máscara excluída volta com os tiles"
        );
        assert!(s2.na_mascara());
    }

    #[test]
    fn a_camada_de_ajuste_e_o_arrasto_dela_voltam_da_gravacao() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        let antes = s.compor();
        s.nova_camada_de_ajuste(crate::Ajuste::Inverter);
        s.mover_ajuste(crate::Ajuste::Inverter);
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("e1").join(MANIFESTO)).unwrap())
                .unwrap();
        assert_eq!(json["camadas"][1]["ajuste"]["tipo"], "inverter");
        let aberto = p.abrir(&base).unwrap().unwrap();
        assert_eq!(aberto.documento, doc);
        let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 300);
        assert_eq!(
            s2.compor().get_pixel(5, 5).0,
            antes.get_pixel(5, 5).0.map(|v| 255 - v)
        );
        assert!(s2.desfazer());
        assert_eq!(s2.compor().as_raw(), antes.as_raw());
    }

    #[test]
    fn a_mascara_pintada_volta_igual_e_a_imagem_editada_e_a_composicao() {
        // A camada de cima com a máscara pintada de preto, de cinza e de
        // branco: a imagem editada que a Revelação lê é a composição, e
        // reabrir devolve a mesma foto e o mesmo histórico.
        let dir = tempfile::tempdir().unwrap();
        let mut s = crate::testes_da_mascara::cenario();
        let base = s.base().clone();
        s.adicionar_mascara(false);
        for (cor, y) in [([0u8; 3], 200.0), ([128; 3], 300.0), ([255; 3], 200.0)] {
            s.pincel.cor = cor;
            s.pincel.dureza = 1.0;
            s.apertar(150.0, y);
            s.arrastar(450.0, y);
            s.soltar();
        }
        let (doc, hist) = s.instantaneo();
        let composta = composicao::compor(&base, &doc);
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        let salvo = p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        let versao = salvo.versao.expect("com efeito há imagem editada");
        let lida = ler_png(&std::fs::read(&versao.arquivo).unwrap()).unwrap();
        assert_eq!(
            lida.as_raw(),
            composta.as_raw(),
            "a imagem editada é a composição"
        );
        p.coletar(1).unwrap();

        let aberto = projeto(dir.path(), Arc::new(DiscoReal))
            .abrir(&base)
            .unwrap()
            .unwrap();
        assert_eq!(aberto.documento, doc);
        // O cenário tem passos de seleção (⌘A, ⌘D): eles não vão para o
        // projeto, e o resto volta igual.
        assert!(hist.passos().iter().any(Comando::so_selecao));
        assert_eq!(aberto.historico.passos(), hist.para_gravar().0);
        let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 300);
        assert_eq!(s2.compor().as_raw(), composta.as_raw());
        // O histórico reaberto desfaz os três traços e a máscara.
        for _ in 0..4 {
            assert!(s2.desfazer());
        }
        assert!(s2.documento().camadas[1].mascara.is_none());
        assert_eq!(s2.compor().get_pixel(400, 300).0, [255, 0, 0]);
    }

    #[test]
    fn o_projeto_do_formato_1_abre_com_o_modo_normal() {
        let dir = tempfile::tempdir().unwrap();
        let base = base();
        let mut s = sessao_pintada(&base);
        let (doc, hist) = s.instantaneo();
        let p = projeto(dir.path(), Arc::new(DiscoReal));
        p.salvar("e1", &base, &doc, &hist, 1).unwrap();
        // O manifesto como a 0.1.28 o gravava: formato 1, sem `modo`.
        let caminho = dir.path().join("e1").join(MANIFESTO);
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&caminho).unwrap()).unwrap();
        json["formato"] = 1.into();
        json["camadas"][0].as_object_mut().unwrap().remove("modo");
        std::fs::write(&caminho, serde_json::to_vec(&json).unwrap()).unwrap();

        let aberto = p.abrir(&base).unwrap().unwrap();
        assert_eq!(aberto.documento.camadas[0].modo, Modo::Normal);
        assert_eq!(aberto.documento, doc);
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
            &format!("\"formato\": {FORMATO}"),
            &format!("\"formato\": {}", FORMATO + 1),
            1,
        );
        std::fs::write(&manifesto, texto).unwrap();
        assert!(matches!(p.abrir(&base), Err(ErroDoProjeto::FormatoNovo(f)) if f == FORMATO + 1));
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
