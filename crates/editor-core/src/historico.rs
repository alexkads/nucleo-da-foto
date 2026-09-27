//! Desfazer e refazer.
//!
//! Cada passo guarda **o antes e o depois**, e não um estado inteiro: o traço
//! guarda os tiles que tocou (que são `Arc`, então o custo é um contador), e as
//! propriedades guardam os dois valores.
//!
//! 🔑 **"Alterado" é a posição longe do ponto de salvamento**, e não "houve
//! gesto": desfazer até o que foi salvo volta a dizer "sem alterações", como em
//! todo editor.

use crate::documento::Documento;
use crate::pincel::Mudanca;
use crate::retangulo::Retangulo;
use crate::tiles::{retangulo_do_tile, BYTES_DO_TILE};

/// O teto de memória do histórico (tiles guardados), em bytes.
pub const TETO_DO_HISTORICO: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum Comando {
    Traco {
        camada: usize,
        mudanca: Mudanca,
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

impl Comando {
    /// O nome do passo, para a lista e as dicas de desfazer — o `description()`
    /// do histórico do PaintFE.
    pub fn descricao(&self, doc: &Documento) -> String {
        match self {
            Comando::Traco { mudanca, .. } => {
                let apagou = mudanca
                    .antes
                    .iter()
                    .zip(&mudanca.depois)
                    .all(|((_, a), (_, d))| alfa_total(d) <= alfa_total(a));
                if apagou { "Borracha" } else { "Pincel" }.into()
            }
            Comando::Visibilidade { camada, depois, .. } => format!(
                "{} {}",
                if *depois { "Mostrar" } else { "Esconder" },
                nome(doc, *camada)
            ),
            Comando::Opacidade { camada, depois, .. } => format!(
                "Opacidade de {} ({}%)",
                nome(doc, *camada),
                (depois * 100.0).round()
            ),
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Comando::Traco { mudanca, .. } => {
                let conta = |v: &[(_, Option<_>)]| v.iter().filter(|(_, t)| t.is_some()).count();
                (conta(&mudanca.antes) + conta(&mudanca.depois)) * BYTES_DO_TILE
            }
            _ => 0,
        }
    }

    /// Aplica o lado `depois` (refazer) ou o `antes` (desfazer) e devolve a
    /// região da foto que mudou.
    pub fn aplicar(&self, doc: &mut Documento, para_frente: bool) -> Retangulo {
        let (largura, altura) = (doc.largura(), doc.altura());
        match self {
            Comando::Traco { camada, mudanca } => {
                let lado = if para_frente {
                    &mudanca.depois
                } else {
                    &mudanca.antes
                };
                let mut sujo = Retangulo::default();
                if let Some(c) = doc.camadas.get_mut(*camada) {
                    for (posicao, tile) in lado {
                        c.pixels.definir(*posicao, tile.clone());
                        sujo = sujo.uniao(&retangulo_do_tile(*posicao, largura, altura));
                    }
                }
                sujo
            }
            Comando::Visibilidade {
                camada,
                antes,
                depois,
            } => {
                if let Some(c) = doc.camadas.get_mut(*camada) {
                    c.visivel = if para_frente { *depois } else { *antes };
                }
                Retangulo::inteiro(largura, altura)
            }
            Comando::Opacidade {
                camada,
                antes,
                depois,
            } => {
                if let Some(c) = doc.camadas.get_mut(*camada) {
                    c.opacidade = if para_frente { *depois } else { *antes };
                }
                Retangulo::inteiro(largura, altura)
            }
        }
    }
}

fn alfa_total(tile: &Option<crate::tiles::Tile>) -> u64 {
    tile.as_ref()
        .map(|t| t.iter().skip(3).step_by(4).map(|a| *a as u64).sum())
        .unwrap_or(0)
}

fn nome(doc: &Documento, camada: usize) -> String {
    doc.camadas
        .get(camada)
        .map(|c| c.nome.clone())
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Historico {
    passos: Vec<Comando>,
    /// Quantos passos estão aplicados: `passos[..posicao]`.
    posicao: usize,
    /// A posição do último salvamento. `None` = o estado salvo não está mais ao
    /// alcance (foi descartado pelo teto ou por um ramo novo).
    salvo_em: Option<usize>,
    teto: usize,
    /// Quantas vezes um gesto novo descartou passos (desfeitos ou pelo teto).
    /// Com ele, "a posição 5" de antes de um corte não se confunde com a de
    /// depois.
    cortes: u64,
}

impl Default for Historico {
    fn default() -> Self {
        Self::novo()
    }
}

impl Historico {
    /// Vazio e salvo (um documento que acabou de abrir não tem alterações).
    pub fn novo() -> Self {
        Self {
            passos: Vec::new(),
            posicao: 0,
            salvo_em: Some(0),
            teto: TETO_DO_HISTORICO,
            cortes: 0,
        }
    }

    pub fn com_teto(mut self, teto: usize) -> Self {
        self.teto = teto;
        self
    }

    /// Reconstruído da gravação.
    pub fn de_partes(passos: Vec<Comando>, posicao: usize, salvo_em: Option<usize>) -> Self {
        let posicao = posicao.min(passos.len());
        Self {
            salvo_em: salvo_em.filter(|s| *s <= passos.len()),
            passos,
            posicao,
            teto: TETO_DO_HISTORICO,
            cortes: 0,
        }
    }

    pub fn passos(&self) -> &[Comando] {
        &self.passos
    }

    pub fn posicao(&self) -> usize {
        self.posicao
    }

    pub fn salvo_em(&self) -> Option<usize> {
        self.salvo_em
    }

    /// Um gesto novo, **já aplicado** no documento. Corta o que dava para refazer.
    pub fn registrar(&mut self, comando: Comando) {
        if self.passos.len() > self.posicao {
            self.cortes += 1;
        }
        self.passos.truncate(self.posicao);
        if self.salvo_em.is_some_and(|s| s > self.posicao) {
            self.salvo_em = None;
        }
        self.passos.push(comando);
        self.posicao += 1;
        self.respeitar_o_teto();
    }

    fn respeitar_o_teto(&mut self) {
        let mut total: usize = self.passos.iter().map(Comando::bytes).sum();
        let mut tirar = 0;
        // O último passo fica sempre: sem ele, o gesto que acabou de acontecer
        // não teria volta.
        while total > self.teto && tirar + 1 < self.passos.len() {
            total -= self.passos[tirar].bytes();
            tirar += 1;
        }
        if tirar == 0 {
            return;
        }
        self.passos.drain(..tirar);
        self.cortes += 1;
        self.posicao -= tirar;
        self.salvo_em = self.salvo_em.and_then(|s| s.checked_sub(tirar));
    }

    /// O passo que o próximo desfazer volta.
    pub fn a_desfazer(&self) -> Option<&Comando> {
        self.posicao.checked_sub(1).map(|i| &self.passos[i])
    }

    /// O passo que o próximo refazer aplica.
    pub fn a_refazer(&self) -> Option<&Comando> {
        self.passos.get(self.posicao)
    }

    pub fn pode_desfazer(&self) -> bool {
        self.posicao > 0
    }

    pub fn pode_refazer(&self) -> bool {
        self.posicao < self.passos.len()
    }

    pub fn desfazer(&mut self, doc: &mut Documento) -> Option<Retangulo> {
        if !self.pode_desfazer() {
            return None;
        }
        self.posicao -= 1;
        Some(self.passos[self.posicao].aplicar(doc, false))
    }

    pub fn refazer(&mut self, doc: &mut Documento) -> Option<Retangulo> {
        if !self.pode_refazer() {
            return None;
        }
        let sujo = self.passos[self.posicao].aplicar(doc, true);
        self.posicao += 1;
        Some(sujo)
    }

    /// Há alterações que não foram salvas.
    pub fn alterado(&self) -> bool {
        self.salvo_em != Some(self.posicao)
    }

    pub fn marcar_salvo(&mut self) {
        self.salvo_em = Some(self.posicao);
    }

    /// O estado de `outro` (um instantâneo deste histórico, tirado antes) foi
    /// salvo. Se desde então nenhum passo foi descartado, a posição dele ainda
    /// é o mesmo estado; senão o salvo ficou fora de alcance, e continua
    /// "alterado" — o lado seguro.
    pub fn marcar_salvo_o_de(&mut self, outro: &Historico) {
        self.salvo_em = (self.cortes == outro.cortes && outro.posicao <= self.passos.len())
            .then_some(outro.posicao);
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::documento::BaseRef;
    use crate::pincel::{Ferramenta, Pincel, Traco};

    fn doc() -> Documento {
        Documento::novo(BaseRef {
            largura: 600,
            altura: 400,
            sha256: String::new(),
            perfil: "sRGB-8".into(),
        })
    }

    fn tracar(doc: &mut Documento, hist: &mut Historico, x: f32, cor: [u8; 3]) {
        let mut traco = Traco::novo(Pincel {
            ferramenta: Ferramenta::Pincel,
            raio: 20.0,
            dureza: 0.5,
            opacidade: 0.8,
            cor,
        });
        traco.ate(&mut doc.camadas[0].pixels, x, 100.0);
        traco.ate(&mut doc.camadas[0].pixels, x + 200.0, 150.0);
        let mudanca = traco.terminar(&mut doc.camadas[0].pixels).unwrap();
        hist.registrar(Comando::Traco { camada: 0, mudanca });
    }

    #[test]
    fn desfazer_e_refazer_voltam_os_tiles_bit_a_bit() {
        let mut doc = doc();
        let mut hist = Historico::novo();
        let vazio = doc.clone();
        tracar(&mut doc, &mut hist, 50.0, [255, 0, 0]);
        let um = doc.clone();
        tracar(&mut doc, &mut hist, 120.0, [0, 0, 255]);
        let dois = doc.clone();

        hist.desfazer(&mut doc);
        assert_eq!(doc, um);
        hist.desfazer(&mut doc);
        assert_eq!(doc, vazio);
        assert!(hist.desfazer(&mut doc).is_none());
        hist.refazer(&mut doc);
        assert_eq!(doc, um);
        hist.refazer(&mut doc);
        assert_eq!(doc, dois);
        assert!(hist.refazer(&mut doc).is_none());
    }

    #[test]
    fn um_gesto_novo_corta_o_refazer() {
        let mut doc = doc();
        let mut hist = Historico::novo();
        tracar(&mut doc, &mut hist, 50.0, [255, 0, 0]);
        tracar(&mut doc, &mut hist, 120.0, [0, 0, 255]);
        hist.desfazer(&mut doc);
        tracar(&mut doc, &mut hist, 300.0, [0, 255, 0]);
        assert!(!hist.pode_refazer());
        assert_eq!(hist.passos().len(), 2);
    }

    #[test]
    fn desfazer_ate_o_ponto_salvo_limpa_as_alteracoes() {
        let mut doc = doc();
        let mut hist = Historico::novo();
        assert!(!hist.alterado());
        tracar(&mut doc, &mut hist, 50.0, [255, 0, 0]);
        assert!(hist.alterado());
        hist.marcar_salvo();
        assert!(!hist.alterado());
        tracar(&mut doc, &mut hist, 120.0, [0, 0, 255]);
        assert!(hist.alterado());
        hist.desfazer(&mut doc);
        assert!(!hist.alterado(), "voltou ao que foi salvo");
        hist.desfazer(&mut doc);
        assert!(hist.alterado(), "antes do salvo também é alteração");

        // Um ramo novo a partir de antes do salvo torna o salvo inalcançável.
        tracar(&mut doc, &mut hist, 300.0, [0, 255, 0]);
        hist.desfazer(&mut doc);
        assert!(hist.alterado());
    }

    #[test]
    fn propriedades_da_camada_tambem_se_desfazem() {
        let mut doc = doc();
        let mut hist = Historico::novo();
        doc.camadas[0].opacidade = 0.4;
        hist.registrar(Comando::Opacidade {
            camada: 0,
            antes: 1.0,
            depois: 0.4,
        });
        doc.camadas[0].visivel = false;
        hist.registrar(Comando::Visibilidade {
            camada: 0,
            antes: true,
            depois: false,
        });
        hist.desfazer(&mut doc);
        assert!(doc.camadas[0].visivel);
        hist.desfazer(&mut doc);
        assert_eq!(doc.camadas[0].opacidade, 1.0);
    }

    #[test]
    fn o_teto_descarta_os_passos_mais_antigos() {
        let mut doc = doc();
        let mut hist = Historico::novo().com_teto(BYTES_DO_TILE * 8);
        for i in 0..6 {
            tracar(&mut doc, &mut hist, 20.0 + i as f32 * 60.0, [i as u8, 0, 0]);
        }
        let guardado: usize = hist.passos().iter().map(Comando::bytes).sum();
        assert!(guardado <= BYTES_DO_TILE * 8 || hist.passos().len() == 1);
        assert!(hist.passos().len() < 6);
        assert_eq!(hist.posicao(), hist.passos().len());
        assert!(hist.alterado());
    }
}
