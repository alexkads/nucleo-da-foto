//! 🧴 O tratamento de pele na sessão: a separação de frequências (prévia,
//! OK num passo, regenerar sem perder o retoque), a escolha e o isolamento de
//! cada frequência, o original, a intensidade e o Dodge & Burn.
//!
//! 🔑 **O conjunto é uma máscara de corte**: a baixa é a base, a alta é
//! recortada nela em Luz Linear (`frequencias.rs`). Não há grupos no editor;
//! a máscara de corte é o que já compõe "como grupo" — a **opacidade da
//! base vale para o conjunto inteiro**:
//!
//! ```text
//! g    = L' + 2H' − 1            (o resultado tratado, L' e H' retocadas)
//! saída = I + (g − I) · opacidade_da_baixa
//! ```
//!
//! A intensidade do tratamento é essa opacidade: mistura o tratado com a
//! referência `I` (o que está embaixo do conjunto — a foto de onde a separação
//! saiu), e onde nada foi retocado `g = I` e a saída é `I` em qualquer
//! intensidade. Mexer na opacidade **da alta** mudaria a textura mesmo sem
//! retoque; a intensidade nunca faz isso.

use super::*;
use crate::ajuste::Curva;
use crate::documento::Retoque;
use crate::frequencias::{self, Separacao};

/// De onde a separação parte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrigemDaSeparacao {
    /// A foto como aparece (todas as camadas visíveis) — o conjunto entra no
    /// topo. O "carimbar visível" do fluxo clássico, sem a camada a mais.
    #[default]
    Visivel,
    /// A camada escolhida como aparece (com o que está embaixo dela) — o
    /// conjunto entra logo acima dela (acima do conjunto de recorte dela), e
    /// as camadas de cima continuam por cima.
    CamadaSelecionada,
}

/// Uma das frequências do conjunto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frequencia {
    Baixa,
    Alta,
}

/// O que a tela mostra durante (e depois de) uma separação.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VistaDaSeparacao {
    /// A foto recomposta (a de sempre).
    #[default]
    Recomposta,
    /// Só a baixa frequência.
    Baixa,
    /// Só a alta frequência (a textura em volta do cinza).
    Alta,
    /// A foto sem o conjunto — o original.
    Original,
}

/// O que a conta da separação precisa, para correr fora da thread da tela.
pub struct PedidoDeSeparacao {
    /// A versão da sessão quando o pedido saiu: a prévia é recusada se o
    /// documento mudou desde então.
    pub versao: u64,
    base: Arc<RgbImage>,
    doc: Documento,
    /// Onde o conjunto novo entra (ou a baixa do que é regenerado).
    pub indice: usize,
    /// Regenerar: a baixa e a alta do conjunto que recebe os pixels novos.
    pub regenera: Option<(usize, usize)>,
}

impl PedidoDeSeparacao {
    /// A origem `I`: a composta do pedido, como camada (pesado — fora da
    /// thread da tela).
    pub fn fonte(&self) -> CamadaDePixels {
        CamadaDePixels::da_imagem(&composicao::compor(&self.base, &self.doc))
    }
}

/// A separação à vista, sem passo no histórico.
pub(super) enum Previa {
    /// As duas camadas novas estão em `indice` e `indice + 1`.
    Nova { indice: usize },
    /// As camadas `baixa` e `alta` estão com os pixels novos; `antes` são as
    /// duas como eram.
    Regenera {
        baixa: usize,
        alta: usize,
        antes: Box<[Camada; 2]>,
    },
}

impl Previa {
    fn indices(&self) -> (usize, usize) {
        match self {
            Previa::Nova { indice } => (*indice, *indice + 1),
            Previa::Regenera { baixa, alta, .. } => (*baixa, *alta),
        }
    }
}

/// As curvas do Dodge & Burn: um ponto no meio — clarear sobe 128 → 160,
/// escurecer desce 128 → 96 (cerca de meio ponto de exposição nos
/// meios-tons). O pincel com fluxo baixo na máscara dosa onde e quanto.
fn curvas_do_dodge_and_burn(clarear: bool) -> Ajuste {
    let mut rgb = Curva::identidade();
    rgb.com_ponto(128, if clarear { 160 } else { 96 });
    Ajuste::Curvas {
        rgb,
        vermelho: Curva::identidade(),
        verde: Curva::identidade(),
        azul: Curva::identidade(),
    }
}

/// Os nomes das camadas do Dodge & Burn.
pub const NOME_DO_CLAREAR: &str = "Clarear (D&B)";
pub const NOME_DO_ESCURECER: &str = "Escurecer (D&B)";

impl Sessao {
    // ------------------------------------------------- separação: pedido

    /// O pedido da separação com a origem escolhida.
    pub fn pedido_de_separacao(&self, origem: OrigemDaSeparacao) -> PedidoDeSeparacao {
        let mut doc = self.doc_sem_a_previa();
        let indice = match origem {
            OrigemDaSeparacao::Visivel => doc.camadas.len(),
            OrigemDaSeparacao::CamadaSelecionada => {
                let ativa = self.ativa().min(doc.camadas.len().saturating_sub(1));
                let base = doc.base_do_recorte(ativa).unwrap_or(ativa);
                (doc.fim_do_conjunto(base) + 1).min(doc.camadas.len())
            }
        };
        doc.camadas.truncate(indice);
        PedidoDeSeparacao {
            versao: self.versao,
            base: self.base.clone(),
            doc,
            indice,
            regenera: None,
        }
    }

    /// "Regenerar separação": a origem é o conjunto **como está retocado**
    /// (na intensidade cheia), e os pixels novos entram nas mesmas duas
    /// camadas — os retoques ficam, assados na nova divisão. `None` sem
    /// conjunto.
    pub fn pedido_de_regeneracao(&self) -> Option<PedidoDeSeparacao> {
        if self.separacao.is_some() {
            return None;
        }
        let (baixa, alta) = self.conjunto_de_pele()?;
        let mut doc = self.doc.clone();
        doc.camadas.truncate(alta + 1);
        doc.camadas[baixa].opacidade = 1.0;
        doc.camadas[baixa].visivel = true;
        Some(PedidoDeSeparacao {
            versao: self.versao,
            base: self.base.clone(),
            doc,
            indice: baixa,
            regenera: Some((baixa, alta)),
        })
    }

    /// O documento sem as camadas da prévia (o que o pedido enxerga).
    fn doc_sem_a_previa(&self) -> Documento {
        let mut doc = self.doc.clone();
        match &self.separacao {
            Some(Previa::Nova { indice }) => {
                doc.camadas.drain(*indice..*indice + 2);
            }
            Some(Previa::Regenera {
                baixa, alta, antes, ..
            }) => {
                doc.camadas[*baixa] = antes[0].clone();
                doc.camadas[*alta] = antes[1].clone();
            }
            None => {}
        }
        doc
    }

    // ------------------------------------------------- separação: prévia

    /// A prévia da separação: as duas camadas entram (ou trocam de pixels),
    /// **sem passo** no histórico. Recusada (falso) quando o documento mudou
    /// desde o pedido — a conta é de outra foto.
    pub fn mostrar_separacao(
        &mut self,
        pedido: &PedidoDeSeparacao,
        raio: f32,
        separacao: Separacao,
    ) -> bool {
        if self.separacao.is_none() {
            if self.mudou_desde(pedido.versao) {
                return false;
            }
            self.fechar_o_que_esta_aberto();
            let [baixa, alta] = frequencias::camadas(separacao, raio);
            match pedido.regenera {
                None => {
                    let indice = pedido.indice.min(self.doc.camadas.len());
                    self.doc.camadas.insert(indice, alta);
                    self.doc.camadas.insert(indice, baixa);
                    self.separacao = Some(Previa::Nova { indice });
                }
                Some((b, a)) => {
                    let antes =
                        Box::new([self.doc.camadas[b].clone(), self.doc.camadas[a].clone()]);
                    self.trocar_os_pixels(b, a, baixa, alta);
                    self.separacao = Some(Previa::Regenera {
                        baixa: b,
                        alta: a,
                        antes,
                    });
                }
            }
        } else {
            let (b, a) = self
                .separacao
                .as_ref()
                .map(Previa::indices)
                .unwrap_or_default();
            let [baixa, alta] = frequencias::camadas(separacao, raio);
            self.trocar_os_pixels(b, a, baixa, alta);
        }
        let tudo = Retangulo::inteiro(self.doc.largura(), self.doc.altura());
        self.refazer_a_vista(&tudo);
        true
    }

    /// As camadas `b` e `a` passam a ter os pixels (e o raio) das novas; o
    /// resto (nome, opacidade, máscara, recorte) fica.
    fn trocar_os_pixels(&mut self, b: usize, a: usize, baixa: Camada, alta: Camada) {
        let cb = &mut self.doc.camadas[b];
        cb.pixels = baixa.pixels;
        cb.retoque = baixa.retoque;
        let ca = &mut self.doc.camadas[a];
        ca.pixels = alta.pixels;
        ca.retoque = alta.retoque;
    }

    /// A separação está em prévia.
    pub fn separando(&self) -> bool {
        self.separacao.is_some()
    }

    /// O que a tela mostra do conjunto (o da prévia, ou o da camada
    /// escolhida). Falso sem conjunto.
    pub fn exibir_separacao(&mut self, vista: VistaDaSeparacao) -> bool {
        let indices = match &self.separacao {
            Some(p) => Some(p.indices()),
            None => self.conjunto_de_pele(),
        };
        let Some((b, a)) = indices else {
            return false;
        };
        self.exibir(match vista {
            VistaDaSeparacao::Recomposta => Exibicao::Foto,
            VistaDaSeparacao::Baixa => Exibicao::SoACamada(b),
            VistaDaSeparacao::Alta => Exibicao::SoACamada(a),
            VistaDaSeparacao::Original => Exibicao::SemOConjunto(b),
        });
        true
    }

    /// O que a tela mostra do conjunto agora.
    pub fn vista_da_separacao(&self) -> VistaDaSeparacao {
        let indices = match &self.separacao {
            Some(p) => Some(p.indices()),
            None => self.conjunto_de_pele(),
        };
        match (self.exibicao(), indices) {
            (Exibicao::SoACamada(i), Some((b, _))) if i == b => VistaDaSeparacao::Baixa,
            (Exibicao::SoACamada(i), Some((_, a))) if i == a => VistaDaSeparacao::Alta,
            (Exibicao::SemOConjunto(i), Some((b, _))) if i == b => VistaDaSeparacao::Original,
            _ => VistaDaSeparacao::Recomposta,
        }
    }

    /// OK: a prévia vira **um** passo ("Separação de frequências" ou
    /// "Regenerar separação"), e a alta passa a ser a escolhida, com o
    /// carimbo amostrando só ela. Falso sem prévia.
    pub fn confirmar_separacao(&mut self) -> bool {
        let Some(previa) = self.separacao.take() else {
            return false;
        };
        self.exibir(Exibicao::Foto);
        let (nome, passos, alta) = match previa {
            Previa::Nova { indice } => {
                let mut tiradas: Vec<Camada> = self.doc.camadas.drain(indice..indice + 2).collect();
                let alta = tiradas.pop().expect("a alta");
                let baixa = tiradas.pop().expect("a baixa");
                (
                    "Separação de frequências",
                    vec![
                        Comando::CriarCamada {
                            indice,
                            camada: Box::new(baixa),
                        },
                        Comando::CriarCamada {
                            indice: indice + 1,
                            camada: Box::new(alta),
                        },
                    ],
                    indice + 1,
                )
            }
            Previa::Regenera { baixa, alta, antes } => {
                let [velha_baixa, velha_alta] = *antes;
                let nova_baixa =
                    std::mem::replace(&mut self.doc.camadas[baixa], velha_baixa.clone());
                let nova_alta = std::mem::replace(&mut self.doc.camadas[alta], velha_alta.clone());
                (
                    "Regenerar separação",
                    vec![
                        Comando::ExcluirCamada {
                            indice: alta,
                            camada: Box::new(velha_alta),
                        },
                        Comando::ExcluirCamada {
                            indice: baixa,
                            camada: Box::new(velha_baixa),
                        },
                        Comando::CriarCamada {
                            indice: baixa,
                            camada: Box::new(nova_baixa),
                        },
                        Comando::CriarCamada {
                            indice: alta,
                            camada: Box::new(nova_alta),
                        },
                    ],
                    alta,
                )
            }
        };
        self.executar(Comando::Varios {
            nome: nome.into(),
            passos,
        });
        self.escolher_camada(alta);
        self.amostra_da_frequencia();
        true
    }

    /// Cancelar: o documento volta a ser o que era, sem passo.
    pub fn cancelar_separacao(&mut self) {
        let Some(previa) = self.separacao.take() else {
            return;
        };
        match previa {
            Previa::Nova { indice } => {
                self.doc.camadas.drain(indice..indice + 2);
            }
            Previa::Regenera { baixa, alta, antes } => {
                let [b, a] = *antes;
                self.doc.camadas[baixa] = b;
                self.doc.camadas[alta] = a;
            }
        }
        self.exibir(Exibicao::Foto);
        let tudo = Retangulo::inteiro(self.doc.largura(), self.doc.altura());
        self.refazer_a_vista(&tudo);
    }

    // ------------------------------------------------- o conjunto

    /// O conjunto do tratamento de pele: (baixa, alta) — o da camada
    /// escolhida, ou o de cima de todos. `None` sem separação no documento.
    pub fn conjunto_de_pele(&self) -> Option<(usize, usize)> {
        let camadas = &self.doc.camadas;
        let da_ativa = camadas.get(self.ativa()).and_then(|c| match c.retoque {
            Some(Retoque::Baixa { .. }) => Some(self.ativa()),
            Some(Retoque::Alta { .. }) => self.doc.base_do_recorte(self.ativa()),
            _ => None,
        });
        let baixas = da_ativa.into_iter().chain(
            (0..camadas.len())
                .rev()
                .filter(|&i| matches!(camadas[i].retoque, Some(Retoque::Baixa { .. }))),
        );
        for b in baixas {
            if !matches!(camadas[b].retoque, Some(Retoque::Baixa { .. })) {
                continue;
            }
            let fim = self.doc.fim_do_conjunto(b);
            if let Some(a) =
                (b + 1..=fim).find(|&i| matches!(camadas[i].retoque, Some(Retoque::Alta { .. })))
            {
                return Some((b, a));
            }
        }
        None
    }

    /// Escolhe a baixa ou a alta do conjunto. Falso sem conjunto.
    pub fn escolher_frequencia(&mut self, f: Frequencia) -> bool {
        let Some((b, a)) = self.conjunto_de_pele() else {
            return false;
        };
        self.escolher_camada(match f {
            Frequencia::Baixa => b,
            Frequencia::Alta => a,
        });
        self.amostra_da_frequencia();
        true
    }

    /// A frequência escolhida agora (pixels, não a máscara).
    pub fn frequencia_escolhida(&self) -> Option<Frequencia> {
        if self.na_mascara() {
            return None;
        }
        match self.camada_ativa().retoque {
            Some(Retoque::Baixa { .. }) => Some(Frequencia::Baixa),
            Some(Retoque::Alta { .. }) => Some(Frequencia::Alta),
            _ => None,
        }
    }

    /// Numa frequência, a amostra do carimbo e da recuperação passa a ser só
    /// a camada — amostrar a composta levaria tom para a textura. O operador
    /// pode voltar a escolher outra amostra na barra.
    pub(super) fn amostra_da_frequencia(&mut self) {
        if self.frequencia_escolhida().is_some() {
            self.carimbo.amostra = crate::carimbo::AmostraDoCarimbo::CamadaAtual;
        }
    }

    /// A conta da recuperação na escolhida: aditiva na alta frequência.
    pub(super) fn adaptacao_da_recuperacao(&self) -> crate::recuperacao::Adaptacao {
        match self.frequencia_escolhida() {
            Some(Frequencia::Alta) => crate::recuperacao::Adaptacao::Aditiva,
            _ => crate::recuperacao::Adaptacao::Multiplicativa,
        }
    }

    /// A intensidade do tratamento (a opacidade da baixa, que vale para o
    /// conjunto). `None` sem conjunto.
    pub fn intensidade_do_tratamento(&self) -> Option<f32> {
        let (b, _) = self.conjunto_de_pele()?;
        Some(self.doc.camadas[b].opacidade)
    }

    /// Arrasto do controle de intensidade — um passo ao soltar
    /// ([`Sessao::confirmar_opacidade`]), sem trocar a escolhida.
    pub fn mover_intensidade(&mut self, valor: f32) -> bool {
        let Some((b, _)) = self.conjunto_de_pele() else {
            return false;
        };
        self.mover_opacidade_de(b, valor);
        true
    }

    // ------------------------------------------------- Dodge & Burn

    /// "Dodge & Burn": duas camadas de Curvas em **Luminosidade** (mudam a
    /// luz sem mexer na cor), com a máscara preta — nada muda até pintar de
    /// branco nelas. Entram no topo, num passo, e a máscara do Clarear fica
    /// escolhida. Não depende da separação.
    pub fn criar_dodge_and_burn(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let camada = |nome: &str, clarear: bool| {
            let mut c = Camada::de_ajuste(nome, curvas_do_dodge_and_burn(clarear), largura, altura);
            c.modo = Modo::Luminosidade;
            c.mascara = Some(Mascara::nova(0, largura, altura));
            c.retoque = Some(if clarear {
                Retoque::Clarear
            } else {
                Retoque::Escurecer
            });
            c
        };
        let indice = self.doc.camadas.len();
        self.executar(Comando::Varios {
            nome: "Dodge & Burn".into(),
            passos: vec![
                Comando::CriarCamada {
                    indice,
                    camada: Box::new(camada(NOME_DO_ESCURECER, false)),
                },
                Comando::CriarCamada {
                    indice: indice + 1,
                    camada: Box::new(camada(NOME_DO_CLAREAR, true)),
                },
            ],
        });
        self.escolher_dodge_and_burn(true);
        true
    }

    /// A camada do Dodge & Burn (a de cima com o papel).
    pub fn camada_do_dodge_and_burn(&self, clarear: bool) -> Option<usize> {
        let papel = if clarear {
            Retoque::Clarear
        } else {
            Retoque::Escurecer
        };
        (0..self.doc.camadas.len())
            .rev()
            .find(|&i| self.doc.camadas[i].retoque == Some(papel))
    }

    /// Clarear ou Escurecer: a máscara da camada, com o pincel macio de
    /// fluxo baixo pintando branco. Falso sem as camadas.
    pub fn escolher_dodge_and_burn(&mut self, clarear: bool) -> bool {
        let Some(indice) = self.camada_do_dodge_and_burn(clarear) else {
            return false;
        };
        if !self.escolher_mascara(indice) {
            return false;
        }
        if let Some(pre) = crate::pincel::PREDEFINICOES
            .iter()
            .find(|p| p.nome.starts_with("Dodge & Burn"))
        {
            self.pincel = pre.aplicada(self.pincel);
        }
        self.pincel.ferramenta = crate::pincel::Ferramenta::Pincel;
        self.pincel.modo = Modo::Normal;
        self.pincel.cor = [255; 3];
        self.pincel.cor_de_fundo = [0; 3];
        true
    }
}
