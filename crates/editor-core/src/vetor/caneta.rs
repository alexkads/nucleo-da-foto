//! 🖋️ A Caneta e as ferramentas de caminho, como máquina de estados — sem
//! janela, com o ponteiro já em pixels do documento.
//!
//! ```text
//!               apertar no vazio                     soltar
//!   Ocioso ─────────────────────────► CriandoAncora ─────────► Construindo
//!     ▲  apertar numa ponta aberta        ▲  (arrasto: alças)      │ │
//!     │ ───────────► RetomandoExtremo ────┘ soltar ───────────────►┘ │
//!     │                                                               │
//!     └──── Esc / Enter / ⌘ + clique fora / fechar na primeira ◄──────┘
//!
//!   Seleção direta:  MovendoAncoras · MovendoAlca · Retangulo
//!   Seleção de caminho: MovendoComponentes · Retangulo
//!   Converter ponto (ou ⌥ na Caneta): ConvertendoPonto · MovendoAlca
//! ```
//!
//! 🔑 **Um gesto, um passo.** O apertar guarda o caminho como estava
//! ([`Gesto`]); cada movimento refaz a prévia **a partir dele** (nada se
//! acumula); o soltar devolve [`Resultado::Passo`] com o nome — quem chama
//! registra o antes e o depois no histórico. Esc no meio do arrasto devolve o
//! caminho de antes e o estado de antes.
//!
//! 🔑 **A decisão do clique é uma função só** ([`Caneta::decidir`]): o mesmo
//! alvo que o apertar usa é o que a janela mostra no cursor antes do clique —
//! fechar, continuar, unir, adicionar, excluir, converter.

use std::collections::BTreeSet;

use super::edicao::{self, Extremo};
use super::geometria::{alvo_em, em_45, Alvo, Visiveis};
use super::{Caminho, Lado, OperacaoDoComponente, Ponto, RefAncora};

/// As ferramentas de caminho da barra.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FerramentaVetorial {
    /// P — a Caneta clássica.
    Caneta,
    /// P (⇧P) — a Caneta de curvatura: cada clique é um ponto por onde a
    /// curva passa lisa; duplo clique (ou ⌥) faz canto; arrastar um ponto o
    /// move e a curva se refaz.
    Curvatura,
    /// P (⇧P) — a Caneta de forma livre: o traço à mão vira curvas
    /// ajustadas ("Ajuste da curva" é o erro máximo, em pontos da tela).
    FormaLivre,
    /// Adicionar ponto de ancoragem (sem letra, como no Photoshop).
    AdicionarPonto,
    /// Excluir ponto de ancoragem (sem letra).
    ExcluirPonto,
    /// Converter ponto (sem letra).
    ConverterPonto,
    /// A — componentes inteiros.
    SelecaoDeCaminho,
    /// A — âncoras e alças.
    SelecaoDireta,
}

impl FerramentaVetorial {
    pub fn e_de_selecao(self) -> bool {
        matches!(
            self,
            FerramentaVetorial::SelecaoDeCaminho | FerramentaVetorial::SelecaoDireta
        )
    }
}

/// Os modificadores do gesto, já na regra da plataforma: `comando` é o ⌘ no
/// macOS e o Ctrl no Windows e no Linux; `alt` é o ⌥ e o Alt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modificadores {
    pub shift: bool,
    pub alt: bool,
    pub comando: bool,
    /// O segundo clique de um duplo clique (a Caneta de curvatura faz canto).
    pub duplo: bool,
}

/// Pixels do documento por ponto da tela — o que torna as tolerâncias de
/// clique estáveis na tela, com qualquer zoom (o giro não muda distâncias).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medida {
    pub por_ponto: f64,
}

impl Medida {
    /// O alcance do clique numa âncora, alça ou segmento: 6 pontos da tela.
    pub fn clique(&self) -> f64 {
        6.0 * self.por_ponto
    }

    /// Quanto o ponteiro anda antes de o clique virar arrasto: 3 pontos.
    pub fn arrasto(&self) -> f64 {
        3.0 * self.por_ponto
    }
}

/// As opções da barra da Caneta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpcoesDaCaneta {
    /// "Adicionar/excluir automaticamente": sobre um segmento a Caneta
    /// adiciona, sobre uma âncora exclui.
    pub auto_adicionar_excluir: bool,
    /// "Faixa elástica": a prévia do próximo segmento até o ponteiro.
    pub previa: bool,
    /// A operação do próximo componente desenhado.
    pub operacao: OperacaoDoComponente,
    /// "Ajuste da curva" da forma livre: o erro máximo do ajuste, em pontos
    /// da tela (o Photoshop vai de 0,5 a 10; o padrão é 2).
    pub ajuste_da_curva: f64,
}

impl Default for OpcoesDaCaneta {
    /// O padrão do Photoshop: adicionar/excluir ligado, faixa elástica
    /// desligada, componentes somam.
    fn default() -> Self {
        Self {
            auto_adicionar_excluir: true,
            previa: false,
            operacao: OperacaoDoComponente::Somar,
            ajuste_da_curva: 2.0,
        }
    }
}

/// O que vem depois de soltar a âncora que está sendo puxada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depois {
    /// Continua desenhando por esta ponta.
    Construir { sub: u64, extremo: Extremo },
    /// O subcaminho acabou de fechar: o desenho termina.
    Fechado,
}

/// O estado da ferramenta — um só, explícito.
#[derive(Clone, Debug, PartialEq)]
pub enum Estado {
    Ocioso,
    /// Um subcaminho aberto à espera do próximo clique, crescendo por
    /// `extremo`.
    Construindo {
        sub: u64,
        extremo: Extremo,
    },
    /// O botão apertado numa âncora nova (ou na primeira, ao fechar): o
    /// arrasto puxa as alças. `lado` é a alça que aponta para a frente do
    /// desenho; com ⌥ no meio do arrasto, só ela anda.
    CriandoAncora {
        r: RefAncora,
        lado: Lado,
        depois: Depois,
        arrastou: bool,
    },
    /// O botão apertado na ponta de um subcaminho aberto: soltar continua o
    /// desenho dali; arrastar puxa as alças dela antes.
    RetomandoExtremo {
        r: RefAncora,
        extremo: Extremo,
        arrastou: bool,
    },
    /// As âncoras escolhidas andando juntas (com as alças).
    MovendoAncoras {
        refs: Vec<RefAncora>,
        inicio: Ponto,
        /// Para onde volta ao soltar (a Caneta de curvatura continua
        /// desenhando depois de pôr ou mover um ponto).
        voltar: Option<(u64, Extremo)>,
    },
    /// Uma alça andando — com a ligação da âncora, ou sozinha
    /// (`independente`, o ⌥).
    MovendoAlca {
        r: RefAncora,
        lado: Lado,
        independente: bool,
        /// Para onde volta ao soltar (a Caneta que ajustava a última âncora).
        voltar: Option<(u64, Extremo)>,
    },
    /// A Converter ponto (ou ⌥ na Caneta) apertada numa âncora: arrastar
    /// puxa alças novas; clicar sem arrastar a faz canto.
    ConvertendoPonto {
        r: RefAncora,
        arrastou: bool,
        voltar: Option<(u64, Extremo)>,
    },
    /// Um segmento curvo dobrando pelo ponto agarrado em `t`.
    DobrandoSegmento {
        sub: u64,
        indice: usize,
        t: f64,
        inicio: Ponto,
    },
    /// Componentes inteiros andando (a Seleção de caminho). Com `fontes`,
    /// o ⌥ + arrasto: as cópias delas é que andam (refeitas a cada movimento a
    /// partir do caminho de antes, com os mesmos ids).
    MovendoComponentes {
        subs: Vec<u64>,
        inicio: Ponto,
        fontes: Option<Vec<u64>>,
    },
    /// O traço à mão da Caneta de forma livre (pontos do documento), que pode
    /// continuar a ponta de um aberto.
    DesenhandoLivre {
        pontos: Vec<Ponto>,
        continuar: Option<(u64, Extremo)>,
    },
    /// O retângulo de seleção (das âncoras ou dos componentes).
    Retangulo {
        inicio: Ponto,
        atual: Ponto,
        somar: bool,
    },
}

/// O que a ferramenta fará com um clique em `p` — a mesma decisão para o
/// apertar e para o cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Acao {
    Nada,
    /// Um subcaminho novo (o primeiro ponto).
    NovoComponente,
    /// A próxima âncora do subcaminho em construção.
    Continuar {
        sub: u64,
        extremo: Extremo,
    },
    /// Fechar no primeiro ponto.
    Fechar {
        sub: u64,
        primeira: RefAncora,
        extremo: Extremo,
    },
    /// Retomar uma ponta aberta.
    Retomar {
        r: RefAncora,
        extremo: Extremo,
    },
    /// Ligar a ponta em construção à ponta de outro componente.
    Unir {
        de: RefAncora,
        ate: RefAncora,
    },
    Adicionar {
        sub: u64,
        indice: usize,
        t: f64,
    },
    Excluir(RefAncora),
    /// Converter a âncora (arrastar puxa alças, clicar faz canto).
    Converter(RefAncora),
    /// Tirar ou refazer a alça da frente da última âncora (⌥ nela).
    AjustarUltima {
        r: RefAncora,
        extremo: Extremo,
    },
    MoverAlca {
        r: RefAncora,
        lado: Lado,
        independente: bool,
    },
    MoverAncoras(RefAncora),
    /// Arrastar um segmento curvo com a Seleção direta: a curva dobra pelo
    /// ponto agarrado (as duas alças do segmento mudam; as pontas ficam).
    DobrarSegmento {
        sub: u64,
        indice: usize,
        t: f64,
    },
    /// Duplo clique numa âncora com a Caneta de curvatura: suave ↔ canto.
    AlternarCanto(RefAncora),
    MoverComponente(u64),
    /// Arrastar no vazio: o retângulo.
    Retangulo,
    /// ⌘ + clique fora com a Caneta: o caminho fica aberto.
    Encerrar,
}

/// O que um evento fez.
#[derive(Clone, Debug, PartialEq)]
pub enum Resultado {
    /// Nada mudou no caminho (a escolha dos pontos pode ter mudado).
    Nada,
    /// O caminho mudou, sem passo ainda (a prévia de um arrasto).
    AoVivo,
    /// Um gesto terminou com mudança: um passo com este nome.
    Passo(&'static str),
    /// O arrasto foi cancelado e o caminho voltou ao de antes.
    Cancelado,
}

/// O começo de um gesto: o caminho e o estado de antes (Esc volta a eles).
#[derive(Clone, Debug)]
struct Gesto {
    antes: Caminho,
    estado: Estado,
    nome: &'static str,
    /// A escolha de antes (o ⌥ + arrasto escolhe as cópias, que o Esc tira).
    pontos: BTreeSet<RefAncora>,
    componentes: BTreeSet<u64>,
}

/// A ferramenta: qual está na mão, as opções, o estado e o que está
/// escolhido (âncoras na Seleção direta, componentes na Seleção de caminho).
#[derive(Clone, Debug)]
pub struct Caneta {
    pub ferramenta: FerramentaVetorial,
    pub opcoes: OpcoesDaCaneta,
    estado: Estado,
    gesto: Option<Gesto>,
    pontos: BTreeSet<RefAncora>,
    componentes: BTreeSet<u64>,
    /// Onde o ponteiro está (para a faixa elástica e o cursor).
    pub ponteiro: Option<Ponto>,
    /// A última das duas de seleção usada — a letra A e o ⌘ temporário
    /// voltam a ela.
    pub ultima_de_selecao: FerramentaVetorial,
}

impl Default for Caneta {
    fn default() -> Self {
        Self::nova()
    }
}

impl Caneta {
    pub fn nova() -> Self {
        Self {
            ferramenta: FerramentaVetorial::Caneta,
            opcoes: OpcoesDaCaneta::default(),
            estado: Estado::Ocioso,
            gesto: None,
            pontos: BTreeSet::new(),
            componentes: BTreeSet::new(),
            ponteiro: None,
            ultima_de_selecao: FerramentaVetorial::SelecaoDireta,
        }
    }

    pub fn estado(&self) -> &Estado {
        &self.estado
    }

    /// Há um arrasto em curso (o botão apertado).
    pub fn em_gesto(&self) -> bool {
        self.gesto.is_some()
    }

    /// Um subcaminho aberto à espera do próximo clique.
    pub fn construindo(&self) -> Option<(u64, Extremo)> {
        match self.estado {
            Estado::Construindo { sub, extremo } => Some((sub, extremo)),
            Estado::CriandoAncora {
                depois: Depois::Construir { sub, extremo },
                ..
            } => Some((sub, extremo)),
            _ => None,
        }
    }

    pub fn pontos_escolhidos(&self) -> &BTreeSet<RefAncora> {
        &self.pontos
    }

    pub fn componentes_escolhidos(&self) -> &BTreeSet<u64> {
        &self.componentes
    }

    pub fn escolher_componentes(&mut self, subs: impl IntoIterator<Item = u64>) {
        self.componentes = subs.into_iter().collect();
    }

    /// Troca a ferramenta. **A geometria confirmada fica**; um arrasto em
    /// curso termina como está (quem chama registra o passo), e o desenho
    /// aberto é encerrado — o caminho continua aberto, nada se apaga.
    pub fn usar(&mut self, ferramenta: FerramentaVetorial) {
        if ferramenta.e_de_selecao() {
            self.ultima_de_selecao = ferramenta;
        }
        if ferramenta != self.ferramenta && self.gesto.is_none() {
            if let Estado::Construindo { sub, .. } = self.estado {
                // O componente em construção fica escolhido na Seleção de
                // caminho, como no Photoshop.
                self.componentes = [sub].into();
            }
            self.estado = Estado::Ocioso;
        }
        self.ferramenta = ferramenta;
    }

    /// O caminho mudou por fora (desfazer, outro caminho escolhido): o que
    /// a ferramenta lembrava e não existe mais sai.
    pub fn conferir(&mut self, c: Option<&Caminho>) {
        let Some(c) = c else {
            self.estado = Estado::Ocioso;
            self.gesto = None;
            self.pontos.clear();
            self.componentes.clear();
            return;
        };
        self.pontos.retain(|r| c.ancora(*r).is_some());
        self.componentes.retain(|s| c.subcaminho(*s).is_some());
        if let Estado::Construindo { sub, extremo } = self.estado {
            let ok = c
                .subcaminho(sub)
                .is_some_and(|s| !s.fechado && !s.ancoras.is_empty());
            if !ok {
                self.estado = Estado::Ocioso;
            } else if c.subcaminho(sub).is_some_and(|s| s.ancoras.len() == 1) {
                self.estado = Estado::Construindo {
                    sub,
                    extremo: if extremo == Extremo::Inicio {
                        Extremo::Fim
                    } else {
                        extremo
                    },
                };
            }
        }
    }

    /// A ferramenta que o clique usa agora: o ⌘ (Ctrl) com uma ferramenta de
    /// desenho dá a última de seleção, só enquanto segurado.
    pub fn ferramenta_efetiva(&self, m: Modificadores) -> FerramentaVetorial {
        if m.comando && !self.ferramenta.e_de_selecao() {
            self.ultima_de_selecao
        } else {
            self.ferramenta
        }
    }

    /// As alças que aparecem e se pegam: as das âncoras escolhidas e das
    /// vizinhas delas; na Caneta, as da ponta em construção.
    fn alcas_visiveis(&self, c: &Caminho) -> BTreeSet<RefAncora> {
        let mut v = BTreeSet::new();
        for r in &self.pontos {
            v.insert(*r);
            if let Some(s) = c.subcaminho(r.sub) {
                if let Some(i) = s.indice_da_ancora(r.ancora) {
                    let n = s.ancoras.len();
                    let vizinhos = [
                        (i > 0 || s.fechado).then(|| (i + n - 1) % n),
                        (i + 1 < n || s.fechado).then(|| (i + 1) % n),
                    ];
                    for k in vizinhos.into_iter().flatten() {
                        v.insert(RefAncora {
                            sub: r.sub,
                            ancora: s.ancoras[k].id,
                        });
                    }
                }
            }
        }
        if let Some((sub, extremo)) = self.construindo() {
            if let Some(r) = edicao::ancora_do_extremo(c, sub, extremo) {
                v.insert(r);
            }
        }
        if let Estado::CriandoAncora { r, .. } = self.estado {
            v.insert(r);
        }
        v
    }

    /// As âncoras cujas alças a tela desenha.
    pub fn alcas_a_mostrar(&self, c: &Caminho) -> BTreeSet<RefAncora> {
        self.alcas_visiveis(c)
    }

    fn alvo(&self, c: &Caminho, p: Ponto, medida: Medida, area: bool) -> Option<Alvo> {
        let visiveis = self.alcas_visiveis(c);
        let alcas = |r: RefAncora| visiveis.contains(&r);
        alvo_em(c, p, medida.clique(), &Visiveis { alcas_de: &alcas }, area)
    }

    /// O que um clique em `p` faria agora (o cursor mostra o mesmo).
    pub fn decidir(&self, c: &Caminho, p: Ponto, m: Modificadores, medida: Medida) -> Acao {
        let ferramenta = self.ferramenta_efetiva(m);
        match ferramenta {
            FerramentaVetorial::Caneta => self.decidir_na_caneta(c, p, m, medida),
            FerramentaVetorial::Curvatura => self.decidir_na_curvatura(c, p, m, medida),
            // Só para o selo: o apertar da forma livre começa o traço.
            FerramentaVetorial::FormaLivre => match self.alvo(c, p, medida, false) {
                Some(Alvo::Ancora(r)) => edicao::extremo_de(c, r)
                    .map_or(Acao::NovoComponente, |extremo| Acao::Retomar { r, extremo }),
                _ => Acao::NovoComponente,
            },
            FerramentaVetorial::AdicionarPonto => match self.alvo(c, p, medida, false) {
                Some(Alvo::Segmento { sub, indice, t }) => Acao::Adicionar { sub, indice, t },
                _ => Acao::Nada,
            },
            FerramentaVetorial::ExcluirPonto => match self.alvo(c, p, medida, false) {
                Some(Alvo::Ancora(r)) => Acao::Excluir(r),
                _ => Acao::Nada,
            },
            FerramentaVetorial::ConverterPonto => match self.alvo(c, p, medida, false) {
                Some(Alvo::Ancora(r)) => Acao::Converter(r),
                Some(Alvo::Alca(r, lado)) => Acao::MoverAlca {
                    r,
                    lado,
                    independente: true,
                },
                _ => Acao::Nada,
            },
            FerramentaVetorial::SelecaoDireta => match self.alvo(c, p, medida, false) {
                Some(Alvo::Alca(r, lado)) => Acao::MoverAlca {
                    r,
                    lado,
                    independente: m.alt,
                },
                Some(Alvo::Ancora(r)) => Acao::MoverAncoras(r),
                Some(Alvo::Segmento { sub, indice, t }) => c
                    .subcaminho(sub)
                    .and_then(|s| s.segmento(indice))
                    .map_or(Acao::Retangulo, |seg| {
                        if seg.reto() {
                            // A reta anda inteira (as duas pontas), como lá.
                            Acao::MoverAncoras(RefAncora {
                                sub,
                                ancora: c.subcaminho(sub).unwrap().ancoras[seg.de].id,
                            })
                        } else {
                            Acao::DobrarSegmento { sub, indice, t }
                        }
                    }),
                _ => Acao::Retangulo,
            },
            FerramentaVetorial::SelecaoDeCaminho => match self.alvo(c, p, medida, true) {
                Some(Alvo::Ancora(r)) | Some(Alvo::Alca(r, _)) => Acao::MoverComponente(r.sub),
                Some(Alvo::Segmento { sub, .. }) | Some(Alvo::Area { sub }) => {
                    Acao::MoverComponente(sub)
                }
                None => Acao::Retangulo,
            },
        }
    }

    fn decidir_na_curvatura(
        &self,
        c: &Caminho,
        p: Ponto,
        m: Modificadores,
        medida: Medida,
    ) -> Acao {
        let alvo = self.alvo(c, p, medida, false);
        if let Some((sub, extremo)) = self.construindo() {
            let outra = edicao::ancora_do_extremo(
                c,
                sub,
                match extremo {
                    Extremo::Fim => Extremo::Inicio,
                    Extremo::Inicio => Extremo::Fim,
                },
            );
            let tamanho = c.subcaminho(sub).map_or(0, |s| s.ancoras.len());
            return match alvo {
                Some(Alvo::Ancora(r)) if m.duplo => Acao::AlternarCanto(r),
                Some(Alvo::Ancora(r)) if Some(r) == outra && tamanho >= 2 => Acao::Fechar {
                    sub,
                    primeira: r,
                    extremo,
                },
                Some(Alvo::Ancora(r)) => Acao::MoverAncoras(r),
                Some(Alvo::Segmento { sub: s, indice, t }) => Acao::Adicionar { sub: s, indice, t },
                _ => Acao::Continuar { sub, extremo },
            };
        }
        match alvo {
            Some(Alvo::Ancora(r)) if m.duplo => Acao::AlternarCanto(r),
            Some(Alvo::Ancora(r)) => Acao::MoverAncoras(r),
            Some(Alvo::Alca(r, lado)) => Acao::MoverAlca {
                r,
                lado,
                independente: m.alt,
            },
            Some(Alvo::Segmento { sub, indice, t }) => Acao::Adicionar { sub, indice, t },
            _ => Acao::NovoComponente,
        }
    }

    fn decidir_na_caneta(&self, c: &Caminho, p: Ponto, m: Modificadores, medida: Medida) -> Acao {
        let alvo = self.alvo(c, p, medida, false);
        let auto = self.opcoes.auto_adicionar_excluir;
        if let Some((sub, extremo)) = self.construindo() {
            let ponta = edicao::ancora_do_extremo(c, sub, extremo);
            let outra = edicao::ancora_do_extremo(
                c,
                sub,
                match extremo {
                    Extremo::Fim => Extremo::Inicio,
                    Extremo::Inicio => Extremo::Fim,
                },
            );
            let tamanho = c.subcaminho(sub).map_or(0, |s| s.ancoras.len());
            match alvo {
                // A ponta em construção: ⌥ (ou o clique nela) ajusta a alça da
                // frente.
                Some(Alvo::Ancora(r)) if Some(r) == ponta => {
                    return Acao::AjustarUltima { r, extremo };
                }
                Some(Alvo::Alca(r, lado)) if Some(r) == ponta => {
                    return Acao::MoverAlca {
                        r,
                        lado,
                        independente: true,
                    };
                }
                // A outra ponta do mesmo subcaminho: fechar.
                Some(Alvo::Ancora(r)) if Some(r) == outra && tamanho >= 2 => {
                    return Acao::Fechar {
                        sub,
                        primeira: r,
                        extremo,
                    };
                }
                // A ponta de outro componente aberto: unir.
                Some(Alvo::Ancora(r))
                    if r.sub != sub && edicao::extremo_de(c, r).is_some() && !m.alt =>
                {
                    if let Some(de) = ponta {
                        return Acao::Unir { de, ate: r };
                    }
                }
                Some(Alvo::Ancora(r)) if m.alt => return Acao::Converter(r),
                Some(Alvo::Alca(r, lado)) if m.alt => {
                    return Acao::MoverAlca {
                        r,
                        lado,
                        independente: true,
                    }
                }
                Some(Alvo::Ancora(r)) if auto => return Acao::Excluir(r),
                Some(Alvo::Segmento { sub: s, indice, t }) if auto => {
                    return Acao::Adicionar { sub: s, indice, t }
                }
                _ => {}
            }
            return Acao::Continuar { sub, extremo };
        }
        match alvo {
            Some(Alvo::Ancora(r)) => {
                if let Some(extremo) = edicao::extremo_de(c, r) {
                    if !m.alt {
                        return Acao::Retomar { r, extremo };
                    }
                }
                if m.alt {
                    return Acao::Converter(r);
                }
                if auto {
                    return Acao::Excluir(r);
                }
                Acao::NovoComponente
            }
            Some(Alvo::Alca(r, lado)) if m.alt => Acao::MoverAlca {
                r,
                lado,
                independente: true,
            },
            Some(Alvo::Segmento { sub, indice, t }) if auto && !m.alt => {
                Acao::Adicionar { sub, indice, t }
            }
            _ => Acao::NovoComponente,
        }
    }

    /// O botão desceu em `p`.
    pub fn apertar(
        &mut self,
        c: &mut Caminho,
        p: Ponto,
        m: Modificadores,
        medida: Medida,
    ) -> Resultado {
        let r = self.apertar_cru(c, p, m, medida);
        edicao::recalcular_automaticas(c);
        r
    }

    /// O ponteiro andou (com ou sem o botão).
    pub fn arrastar(
        &mut self,
        c: &mut Caminho,
        p: Ponto,
        m: Modificadores,
        medida: Medida,
    ) -> Resultado {
        let r = self.arrastar_cru(c, p, m, medida);
        edicao::recalcular_automaticas(c);
        r
    }

    /// O botão subiu.
    pub fn soltar(&mut self, c: &mut Caminho, medida: Medida) -> Resultado {
        // 🔑 As âncoras automáticas (a Caneta de curvatura) se refazem antes
        // de comparar com o caminho de antes do gesto.
        edicao::recalcular_automaticas(c);
        let r = self.soltar_cru(c, medida);
        edicao::recalcular_automaticas(c);
        r
    }

    /// Delete / ⌫ (ver [`Self::excluir_cru`]).
    pub fn excluir(&mut self, c: &mut Caminho) -> Resultado {
        let r = self.excluir_cru(c);
        edicao::recalcular_automaticas(c);
        r
    }

    /// As setas (ver [`Self::empurrar_cru`]).
    pub fn empurrar(&mut self, c: &mut Caminho, d: (f64, f64)) -> Resultado {
        let r = self.empurrar_cru(c, d);
        edicao::recalcular_automaticas(c);
        r
    }

    fn comecar_gesto(&mut self, c: &Caminho, nome: &'static str) {
        self.gesto = Some(Gesto {
            antes: c.clone(),
            estado: self.estado.clone(),
            nome,
            pontos: self.pontos.clone(),
            componentes: self.componentes.clone(),
        });
    }

    /// O botão desceu em `p`.
    fn apertar_cru(
        &mut self,
        c: &mut Caminho,
        p: Ponto,
        m: Modificadores,
        medida: Medida,
    ) -> Resultado {
        if self.gesto.is_some() {
            // Um apertar sem o soltar de antes (o soltar se perdeu): o gesto
            // velho termina como está.
            self.gesto = None;
        }
        self.ponteiro = Some(p);
        let ferramenta = self.ferramenta_efetiva(m);
        // ⌘ + clique fora de tudo com a Caneta: o desenho termina, aberto.
        if m.comando
            && !self.ferramenta.e_de_selecao()
            && self.construindo().is_some()
            && self.alvo(c, p, medida, false).is_none()
        {
            self.estado = Estado::Ocioso;
            self.pontos.clear();
            return Resultado::Nada;
        }
        if ferramenta == FerramentaVetorial::FormaLivre {
            // Começar numa ponta aberta continua o componente dela.
            let continuar = match self.alvo(c, p, medida, false) {
                Some(Alvo::Ancora(r)) => edicao::extremo_de(c, r).map(|e| (r.sub, e)),
                _ => None,
            };
            let inicio = continuar
                .and_then(|(sub, e)| edicao::ancora_do_extremo(c, sub, e))
                .and_then(|r| c.ancora(r))
                .map_or(p, |a| a.ponto);
            self.comecar_gesto(c, "Forma livre");
            self.pontos.clear();
            self.estado = Estado::DesenhandoLivre {
                pontos: vec![inicio],
                continuar,
            };
            return Resultado::Nada;
        }
        let acao = self.decidir(c, p, m, medida);
        if ferramenta == FerramentaVetorial::Curvatura {
            if let Some(r) = self.apertar_na_curvatura(c, p, m, acao) {
                return r;
            }
        }
        match acao {
            Acao::Nada | Acao::Encerrar => Resultado::Nada,
            Acao::NovoComponente => {
                self.comecar_gesto(c, "Ponto de ancoragem");
                let r = edicao::novo_subcaminho(c, p, self.opcoes.operacao);
                self.pontos.clear();
                self.componentes.clear();
                self.estado = Estado::CriandoAncora {
                    r,
                    lado: Lado::Saida,
                    depois: Depois::Construir {
                        sub: r.sub,
                        extremo: Extremo::Fim,
                    },
                    arrastou: false,
                };
                Resultado::AoVivo
            }
            Acao::Continuar { sub, extremo } => {
                let ponta = edicao::ancora_do_extremo(c, sub, extremo).and_then(|r| c.ancora(r));
                let q = match (ponta, m.shift) {
                    (Some(a), true) => em_45(a.ponto, p),
                    _ => p,
                };
                self.comecar_gesto(c, "Ponto de ancoragem");
                let Some(r) = edicao::acrescentar(c, sub, extremo, q) else {
                    self.gesto = None;
                    return Resultado::Nada;
                };
                self.estado = Estado::CriandoAncora {
                    r,
                    lado: extremo.alca_para_frente(),
                    depois: Depois::Construir { sub, extremo },
                    arrastou: false,
                };
                Resultado::AoVivo
            }
            Acao::Fechar {
                sub,
                primeira,
                extremo,
            } => {
                self.comecar_gesto(c, "Fechar caminho");
                edicao::fechar(c, sub);
                self.estado = Estado::CriandoAncora {
                    r: primeira,
                    // Fechando pelo fim, o segmento chega à primeira pela
                    // entrada, e o arrasto (que segue em frente) é a saída.
                    lado: extremo.alca_para_frente(),
                    depois: Depois::Fechado,
                    arrastou: false,
                };
                Resultado::AoVivo
            }
            Acao::Retomar { r, extremo } => {
                self.comecar_gesto(c, "Mover alça");
                self.pontos = [r].into();
                self.estado = Estado::RetomandoExtremo {
                    r,
                    extremo,
                    arrastou: false,
                };
                Resultado::Nada
            }
            Acao::Unir { de, ate } => match edicao::ligar_subcaminhos(c, de, ate) {
                Some(_) => {
                    self.estado = Estado::Ocioso;
                    self.pontos.clear();
                    Resultado::Passo("Unir componentes")
                }
                None => Resultado::Nada,
            },
            Acao::Adicionar { sub, indice, t } => match edicao::inserir_ancora(c, sub, indice, t) {
                Some(r) => {
                    if ferramenta != FerramentaVetorial::Caneta || self.construindo().is_none() {
                        self.pontos = [r].into();
                    }
                    Resultado::Passo("Adicionar ponto de ancoragem")
                }
                None => Resultado::Nada,
            },
            Acao::Excluir(r) => {
                if edicao::excluir_ancora(c, r) {
                    self.pontos.remove(&r);
                    self.conferir(Some(c));
                    Resultado::Passo("Excluir ponto de ancoragem")
                } else {
                    Resultado::Nada
                }
            }
            Acao::Converter(r) => {
                let voltar = self.construindo();
                self.comecar_gesto(c, "Converter ponto");
                self.pontos = [r].into();
                self.estado = Estado::ConvertendoPonto {
                    r,
                    arrastou: false,
                    voltar,
                };
                Resultado::Nada
            }
            Acao::AjustarUltima { r, extremo } => {
                // Apertar na última âncora: a alça da frente sai (o próximo
                // segmento começa reto); arrastando, uma nova só para a frente.
                self.comecar_gesto(c, "Converter ponto");
                let lado = extremo.alca_para_frente();
                edicao::tirar_alca(c, r, lado);
                self.estado = Estado::MovendoAlca {
                    r,
                    lado,
                    independente: true,
                    voltar: self.construindo(),
                };
                Resultado::AoVivo
            }
            Acao::MoverAlca {
                r,
                lado,
                independente,
            } => {
                let voltar = self.construindo();
                self.comecar_gesto(
                    c,
                    if independente {
                        "Converter ponto"
                    } else {
                        "Mover alça"
                    },
                );
                self.estado = Estado::MovendoAlca {
                    r,
                    lado,
                    independente,
                    voltar,
                };
                Resultado::Nada
            }
            Acao::MoverAncoras(r) => {
                if m.alt {
                    // ⌥ + clique: o componente inteiro.
                    if let Some(s) = c.subcaminho(r.sub) {
                        self.pontos = s
                            .ancoras
                            .iter()
                            .map(|a| RefAncora {
                                sub: r.sub,
                                ancora: a.id,
                            })
                            .collect();
                    }
                } else if m.shift {
                    if !self.pontos.insert(r) {
                        self.pontos.remove(&r);
                        return Resultado::Nada;
                    }
                } else if !self.pontos.contains(&r) {
                    self.pontos = [r].into();
                }
                // Clicar num segmento escolhe as duas pontas dele.
                if let Some(Alvo::Segmento { sub, indice, .. }) = self.alvo(c, p, medida, false) {
                    if let Some(seg) = c.subcaminho(sub).and_then(|s| s.segmento(indice)) {
                        let s = c.subcaminho(sub).unwrap();
                        if !m.shift {
                            self.pontos.clear();
                        }
                        for k in [seg.de, seg.ate] {
                            self.pontos.insert(RefAncora {
                                sub,
                                ancora: s.ancoras[k].id,
                            });
                        }
                    }
                }
                self.comecar_gesto(c, "Mover pontos");
                self.estado = Estado::MovendoAncoras {
                    refs: self.pontos.iter().copied().collect(),
                    inicio: p,
                    voltar: None,
                };
                Resultado::Nada
            }
            Acao::DobrarSegmento { sub, indice, t } => {
                // As pontas ficam escolhidas (as alças delas aparecem).
                if let Some(seg) = c.subcaminho(sub).and_then(|s| s.segmento(indice)) {
                    let s = c.subcaminho(sub).unwrap();
                    if !m.shift {
                        self.pontos.clear();
                    }
                    for k in [seg.de, seg.ate] {
                        self.pontos.insert(RefAncora {
                            sub,
                            ancora: s.ancoras[k].id,
                        });
                    }
                }
                self.comecar_gesto(c, "Dobrar curva");
                self.estado = Estado::DobrandoSegmento {
                    sub,
                    indice,
                    t,
                    inicio: p,
                };
                Resultado::Nada
            }
            Acao::AlternarCanto(_) => Resultado::Nada,
            Acao::MoverComponente(sub) => {
                if m.shift {
                    if !self.componentes.insert(sub) {
                        self.componentes.remove(&sub);
                        return Resultado::Nada;
                    }
                } else if !self.componentes.contains(&sub) {
                    self.componentes = [sub].into();
                }
                let escolhidos: Vec<u64> = self.componentes.iter().copied().collect();
                // ⌥ + arrasto: duplica e leva a cópia (como no Photoshop).
                let (subs, fontes) = if m.alt {
                    self.comecar_gesto(c, "Duplicar componente");
                    let copias = edicao::duplicar_subcaminhos(c, &escolhidos);
                    self.componentes = copias.iter().copied().collect();
                    (copias, Some(escolhidos))
                } else {
                    self.comecar_gesto(c, "Mover componente");
                    (escolhidos, None)
                };
                let duplicou = fontes.is_some();
                self.estado = Estado::MovendoComponentes {
                    subs,
                    inicio: p,
                    fontes,
                };
                if duplicou {
                    Resultado::AoVivo
                } else {
                    Resultado::Nada
                }
            }
            Acao::Retangulo => {
                if !m.shift {
                    if ferramenta == FerramentaVetorial::SelecaoDireta {
                        self.pontos.clear();
                    } else {
                        self.componentes.clear();
                    }
                }
                self.comecar_gesto(c, "");
                self.estado = Estado::Retangulo {
                    inicio: p,
                    atual: p,
                    somar: m.shift,
                };
                Resultado::Nada
            }
        }
    }

    /// O apertar da Caneta de curvatura: ponto novo (suave, ou canto com ⌥)
    /// que já se arrasta, ponto existente que anda, duplo clique que alterna
    /// canto, clique no segmento que insere, clique no primeiro que fecha.
    /// `None`: o resto é como na Caneta (as alças, o nada).
    fn apertar_na_curvatura(
        &mut self,
        c: &mut Caminho,
        p: Ponto,
        m: Modificadores,
        acao: Acao,
    ) -> Option<Resultado> {
        Some(match acao {
            Acao::NovoComponente => {
                self.comecar_gesto(c, "Ponto de curvatura");
                let r = edicao::novo_subcaminho(c, p, self.opcoes.operacao);
                edicao::tornar_automatica(c, r, m.alt);
                self.pontos = [r].into();
                self.componentes.clear();
                self.estado = Estado::MovendoAncoras {
                    refs: vec![r],
                    inicio: p,
                    voltar: Some((r.sub, Extremo::Fim)),
                };
                Resultado::AoVivo
            }
            Acao::Continuar { sub, extremo } => {
                self.comecar_gesto(c, "Ponto de curvatura");
                let Some(r) = edicao::acrescentar(c, sub, extremo, p) else {
                    self.gesto = None;
                    return Some(Resultado::Nada);
                };
                edicao::tornar_automatica(c, r, m.alt);
                self.pontos = [r].into();
                self.estado = Estado::MovendoAncoras {
                    refs: vec![r],
                    inicio: p,
                    voltar: Some((sub, extremo)),
                };
                Resultado::AoVivo
            }
            Acao::Fechar { sub, .. } => {
                if edicao::fechar(c, sub) {
                    edicao::recalcular_automaticas(c);
                    self.estado = Estado::Ocioso;
                    self.pontos.clear();
                    Resultado::Passo("Fechar caminho")
                } else {
                    Resultado::Nada
                }
            }
            Acao::Adicionar { sub, indice, t } => match edicao::inserir_ancora(c, sub, indice, t) {
                Some(r) => {
                    edicao::tornar_automatica(c, r, false);
                    self.pontos = [r].into();
                    Resultado::Passo("Ponto de curvatura")
                }
                None => Resultado::Nada,
            },
            Acao::MoverAncoras(r) => {
                let voltar = self
                    .construindo()
                    .or_else(|| edicao::extremo_de(c, r).map(|e| (r.sub, e)));
                self.pontos = [r].into();
                self.comecar_gesto(c, "Mover pontos");
                self.estado = Estado::MovendoAncoras {
                    refs: vec![r],
                    inicio: p,
                    voltar,
                };
                Resultado::Nada
            }
            Acao::AlternarCanto(r) => {
                let canto = c
                    .ancora(r)
                    .is_some_and(|a| a.automatica && a.ligacao != super::Ligacao::Canto);
                edicao::tornar_automatica(c, r, canto);
                edicao::recalcular_automaticas(c);
                self.pontos = [r].into();
                Resultado::Passo("Converter ponto")
            }
            _ => return None,
        })
    }

    /// O ponteiro andou (com ou sem o botão).
    fn arrastar_cru(
        &mut self,
        c: &mut Caminho,
        p: Ponto,
        m: Modificadores,
        medida: Medida,
    ) -> Resultado {
        self.ponteiro = Some(p);
        let Some(gesto) = self.gesto.clone() else {
            return Resultado::Nada;
        };
        let longe = |a: Ponto| a.distancia(p) >= medida.arrasto();
        match self.estado.clone() {
            Estado::CriandoAncora {
                r,
                lado,
                depois,
                arrastou,
            } => {
                let Some(ancora) = c.ancora(r).map(|a| a.ponto) else {
                    return Resultado::Nada;
                };
                if !arrastou && !longe(ancora) {
                    return Resultado::Nada;
                }
                let alvo = if m.shift { em_45(ancora, p) } else { p };
                if m.alt {
                    // ⌥ no meio do arrasto: só a alça da frente; a de trás
                    // fica onde estava.
                    edicao::puxar_uma_alca(c, r, lado, Some(alvo));
                } else {
                    // Também ao fechar: o arrasto refaz as duas alças da
                    // primeira âncora.
                    edicao::puxar_alcas(c, r, lado, alvo);
                }
                self.estado = Estado::CriandoAncora {
                    r,
                    lado,
                    depois,
                    arrastou: true,
                };
                Resultado::AoVivo
            }
            Estado::RetomandoExtremo {
                r,
                extremo,
                arrastou,
            } => {
                let Some(a) = c.ancora(r).map(|a| a.ponto) else {
                    return Resultado::Nada;
                };
                if !arrastou && !longe(a) {
                    return Resultado::Nada;
                }
                *c = gesto.antes.clone();
                let alvo = if m.shift { em_45(a, p) } else { p };
                let lado = extremo.alca_para_frente();
                if m.alt {
                    edicao::puxar_uma_alca(c, r, lado, Some(alvo));
                } else {
                    edicao::puxar_alcas(c, r, lado, alvo);
                }
                self.estado = Estado::RetomandoExtremo {
                    r,
                    extremo,
                    arrastou: true,
                };
                Resultado::AoVivo
            }
            Estado::ConvertendoPonto {
                r,
                arrastou,
                voltar,
            } => {
                let Some(a) = gesto.antes.ancora(r).map(|a| a.ponto) else {
                    return Resultado::Nada;
                };
                if !arrastou && !longe(a) {
                    return Resultado::Nada;
                }
                *c = gesto.antes.clone();
                let alvo = if m.shift { em_45(a, p) } else { p };
                edicao::puxar_alcas(c, r, Lado::Saida, alvo);
                self.estado = Estado::ConvertendoPonto {
                    r,
                    arrastou: true,
                    voltar,
                };
                Resultado::AoVivo
            }
            Estado::MovendoAlca {
                r,
                lado,
                independente,
                ..
            } => {
                let Some(a) = c.ancora(r).map(|a| a.ponto) else {
                    return Resultado::Nada;
                };
                let alvo = if m.shift { em_45(a, p) } else { p };
                *c = gesto.antes.clone();
                edicao::mover_alca(c, r, lado, alvo, independente || m.alt);
                Resultado::AoVivo
            }
            Estado::MovendoAncoras { refs, inicio, .. } => {
                let mut d = p.menos(inicio);
                if m.shift {
                    let q = em_45(inicio, p);
                    d = q.menos(inicio);
                }
                *c = gesto.antes.clone();
                edicao::mover_ancoras(c, &refs, d);
                Resultado::AoVivo
            }
            Estado::DobrandoSegmento {
                sub,
                indice,
                t,
                inicio,
            } => {
                if p.distancia(inicio) < medida.arrasto() && *c == gesto.antes {
                    return Resultado::Nada;
                }
                *c = gesto.antes.clone();
                edicao::dobrar_segmento(c, sub, indice, t, p.menos(inicio));
                Resultado::AoVivo
            }
            Estado::MovendoComponentes {
                subs,
                inicio,
                fontes,
            } => {
                let mut d = p.menos(inicio);
                if m.shift {
                    d = em_45(inicio, p).menos(inicio);
                }
                *c = gesto.antes.clone();
                match &fontes {
                    // As cópias saem com os mesmos ids do apertar: o gerador
                    // parte do mesmo `proximo_id` de antes.
                    Some(fontes) => {
                        let copias = edicao::duplicar_subcaminhos(c, fontes);
                        edicao::mover_subcaminhos(c, &copias, d);
                    }
                    None => {
                        edicao::mover_subcaminhos(c, &subs, d);
                    }
                }
                Resultado::AoVivo
            }
            Estado::DesenhandoLivre {
                mut pontos,
                continuar,
            } => {
                if pontos
                    .last()
                    .is_none_or(|u| u.distancia(p) >= medida.por_ponto)
                {
                    pontos.push(p);
                }
                self.estado = Estado::DesenhandoLivre { pontos, continuar };
                Resultado::Nada
            }
            Estado::Retangulo { inicio, somar, .. } => {
                self.estado = Estado::Retangulo {
                    inicio,
                    atual: p,
                    somar,
                };
                Resultado::Nada
            }
            Estado::Ocioso | Estado::Construindo { .. } => Resultado::Nada,
        }
    }

    /// O botão subiu.
    fn soltar_cru(&mut self, c: &mut Caminho, medida: Medida) -> Resultado {
        let Some(gesto) = self.gesto.take() else {
            return Resultado::Nada;
        };
        let estado = std::mem::replace(&mut self.estado, Estado::Ocioso);
        let mudou = *c != gesto.antes;
        match estado {
            Estado::CriandoAncora { r, depois, .. } => {
                match depois {
                    Depois::Construir { sub, extremo } => {
                        self.estado = Estado::Construindo { sub, extremo };
                        self.pontos = [r].into();
                    }
                    Depois::Fechado => {
                        self.estado = Estado::Ocioso;
                        self.pontos.clear();
                    }
                }
                if mudou {
                    Resultado::Passo(gesto.nome)
                } else {
                    Resultado::Nada
                }
            }
            Estado::RetomandoExtremo { r, extremo, .. } => {
                let extremo = if c.subcaminho(r.sub).is_some_and(|s| s.ancoras.len() == 1) {
                    Extremo::Fim
                } else {
                    extremo
                };
                self.estado = Estado::Construindo {
                    sub: r.sub,
                    extremo,
                };
                self.pontos = [r].into();
                if mudou {
                    Resultado::Passo("Mover alça")
                } else {
                    Resultado::Nada
                }
            }
            Estado::ConvertendoPonto {
                r,
                arrastou,
                voltar,
            } => {
                if !arrastou {
                    *c = gesto.antes.clone();
                    edicao::converter_em_canto(c, r);
                }
                self.estado = voltar.map_or(Estado::Ocioso, |(sub, extremo)| Estado::Construindo {
                    sub,
                    extremo,
                });
                if *c != gesto.antes {
                    Resultado::Passo("Converter ponto")
                } else {
                    Resultado::Nada
                }
            }
            Estado::MovendoAlca { voltar, .. } => {
                self.estado = voltar.map_or(Estado::Ocioso, |(sub, extremo)| Estado::Construindo {
                    sub,
                    extremo,
                });
                if mudou {
                    Resultado::Passo(gesto.nome)
                } else {
                    Resultado::Nada
                }
            }
            Estado::MovendoAncoras { voltar, .. } => {
                self.estado = voltar.map_or(Estado::Ocioso, |(sub, extremo)| Estado::Construindo {
                    sub,
                    extremo,
                });
                if mudou {
                    Resultado::Passo(gesto.nome)
                } else {
                    Resultado::Nada
                }
            }
            Estado::MovendoComponentes { .. } | Estado::DobrandoSegmento { .. } => {
                if mudou {
                    Resultado::Passo(gesto.nome)
                } else {
                    Resultado::Nada
                }
            }
            Estado::DesenhandoLivre { pontos, continuar } => {
                self.estado = Estado::Ocioso;
                let tolerancia = self.opcoes.ajuste_da_curva.clamp(0.5, 10.0) * medida.por_ponto;
                let (ancoras, fechado) = super::ajuste::ajustar(&pontos, tolerancia);
                if ancoras.len() < 2 {
                    return Resultado::Nada;
                }
                match continuar {
                    Some((sub, extremo)) => {
                        edicao::continuar_com(c, sub, extremo, ancoras);
                    }
                    None => {
                        let sub =
                            edicao::novo_subcaminho_com(c, ancoras, fechado, self.opcoes.operacao);
                        self.componentes = [sub].into();
                    }
                }
                Resultado::Passo("Forma livre")
            }
            Estado::Retangulo {
                inicio,
                atual,
                somar,
            } => {
                let (x0, x1) = (inicio.x.min(atual.x), inicio.x.max(atual.x));
                let (y0, y1) = (inicio.y.min(atual.y), inicio.y.max(atual.y));
                if inicio.distancia(atual) >= medida.arrasto() {
                    let dentro = |q: Ponto| q.x >= x0 && q.x <= x1 && q.y >= y0 && q.y <= y1;
                    {
                        if self.ferramenta_efetiva_ultima() == FerramentaVetorial::SelecaoDireta {
                            if !somar {
                                self.pontos.clear();
                            }
                            for s in &c.subcaminhos {
                                for a in s.ancoras.iter().filter(|a| dentro(a.ponto)) {
                                    self.pontos.insert(RefAncora {
                                        sub: s.id,
                                        ancora: a.id,
                                    });
                                }
                            }
                        } else {
                            if !somar {
                                self.componentes.clear();
                            }
                            for s in &c.subcaminhos {
                                if s.ancoras.iter().any(|a| dentro(a.ponto)) {
                                    self.componentes.insert(s.id);
                                }
                            }
                        }
                    }
                }
                self.estado = Estado::Ocioso;
                Resultado::Nada
            }
            outro => {
                self.estado = outro;
                Resultado::Nada
            }
        }
    }

    fn ferramenta_efetiva_ultima(&self) -> FerramentaVetorial {
        if self.ferramenta.e_de_selecao() {
            self.ferramenta
        } else {
            self.ultima_de_selecao
        }
    }

    /// Esc: no meio de um arrasto, cancela **só o gesto** (o caminho e o
    /// estado voltam ao de antes do apertar); desenhando, termina o desenho
    /// com o caminho aberto; com pontos ou componentes escolhidos, solta a
    /// escolha. Nunca apaga nada. Devolve se a tecla foi usada.
    pub fn esc(&mut self, c: &mut Caminho) -> Resultado {
        if let Some(gesto) = self.gesto.take() {
            *c = gesto.antes;
            self.estado = gesto.estado;
            self.pontos = gesto.pontos;
            self.componentes = gesto.componentes;
            return Resultado::Cancelado;
        }
        if self.construindo().is_some() {
            self.estado = Estado::Ocioso;
            self.pontos.clear();
            return Resultado::Nada;
        }
        if !self.pontos.is_empty() || !self.componentes.is_empty() {
            self.pontos.clear();
            self.componentes.clear();
        }
        Resultado::Nada
    }

    /// Enter: termina o desenho em curso, deixando o caminho aberto.
    /// Devolve se havia desenho.
    pub fn encerrar(&mut self) -> bool {
        if self.gesto.is_some() {
            return false;
        }
        if self.construindo().is_some() {
            self.estado = Estado::Ocioso;
            self.pontos.clear();
            return true;
        }
        false
    }

    /// Delete / ⌫: desenhando, tira a última âncora (a da ponta); com
    /// âncoras escolhidas, exclui-as (o fechado abre, o aberto parte); com
    /// componentes escolhidos, exclui-os. Sem nada disso, não faz nada (e
    /// devolve [`Resultado::Nada`] — a janela segue com o Delete de sempre).
    fn excluir_cru(&mut self, c: &mut Caminho) -> Resultado {
        if self.gesto.is_some() {
            return Resultado::Nada;
        }
        if let Some((sub, extremo)) = self.construindo() {
            if let Some(r) = edicao::ancora_do_extremo(c, sub, extremo) {
                edicao::excluir_ancora(c, r);
                self.pontos.clear();
                self.conferir(Some(c));
                if let Some((sub, extremo)) = self.construindo() {
                    if let Some(r) = edicao::ancora_do_extremo(c, sub, extremo) {
                        self.pontos = [r].into();
                    }
                }
                return Resultado::Passo("Excluir ponto de ancoragem");
            }
        }
        let ferramenta = self.ferramenta_efetiva_ultima();
        if !self.pontos.is_empty()
            && (ferramenta == FerramentaVetorial::SelecaoDireta || self.componentes.is_empty())
        {
            let refs: Vec<RefAncora> = self.pontos.iter().copied().collect();
            self.pontos.clear();
            if edicao::excluir_ancoras_partindo(c, &refs) {
                self.conferir(Some(c));
                return Resultado::Passo("Excluir pontos de ancoragem");
            }
        }
        if !self.componentes.is_empty() {
            let subs: Vec<u64> = self.componentes.iter().copied().collect();
            self.componentes.clear();
            if edicao::excluir_subcaminhos(c, &subs) {
                self.conferir(Some(c));
                return Resultado::Passo("Excluir componente");
            }
        }
        Resultado::Nada
    }

    /// As setas: as âncoras escolhidas (ou os componentes) andam `d` pixels
    /// do documento — um passo cada toque.
    fn empurrar_cru(&mut self, c: &mut Caminho, d: (f64, f64)) -> Resultado {
        if self.gesto.is_some() {
            return Resultado::Nada;
        }
        let ferramenta = self.ferramenta_efetiva_ultima();
        if ferramenta == FerramentaVetorial::SelecaoDeCaminho && !self.componentes.is_empty() {
            let subs: Vec<u64> = self.componentes.iter().copied().collect();
            if edicao::mover_subcaminhos(c, &subs, d) {
                return Resultado::Passo("Mover componente");
            }
        }
        if !self.pontos.is_empty() {
            let refs: Vec<RefAncora> = self.pontos.iter().copied().collect();
            if edicao::mover_ancoras(c, &refs, d) {
                return Resultado::Passo("Mover pontos");
            }
        }
        if !self.componentes.is_empty() {
            let subs: Vec<u64> = self.componentes.iter().copied().collect();
            if edicao::mover_subcaminhos(c, &subs, d) {
                return Resultado::Passo("Mover componente");
            }
        }
        Resultado::Nada
    }

    /// Escolhe todos os componentes (ou âncoras) do caminho.
    pub fn escolher_tudo(&mut self, c: &Caminho) {
        self.componentes = c.subcaminhos.iter().map(|s| s.id).collect();
        self.pontos = c
            .subcaminhos
            .iter()
            .flat_map(|s| {
                s.ancoras.iter().map(move |a| RefAncora {
                    sub: s.id,
                    ancora: a.id,
                })
            })
            .collect();
    }

    /// A faixa elástica: o próximo segmento, da ponta em construção até o
    /// ponteiro (com a alça da frente dela), se a opção está ligada. Na
    /// Caneta de curvatura, sempre: os dois últimos segmentos como ficariam
    /// com um ponto no ponteiro (a curva já dobra antes do clique).
    pub fn previa(&self, c: &Caminho, shift: bool) -> Vec<[Ponto; 4]> {
        let curvatura = self.ferramenta == FerramentaVetorial::Curvatura;
        if (!self.opcoes.previa && !curvatura) || self.gesto.is_some() {
            return Vec::new();
        }
        let Estado::Construindo { sub, extremo } = self.estado else {
            return Vec::new();
        };
        let (Some(r), Some(p)) = (edicao::ancora_do_extremo(c, sub, extremo), self.ponteiro) else {
            return Vec::new();
        };
        let Some(a) = c.ancora(r) else {
            return Vec::new();
        };
        if curvatura {
            let mut t = c.clone();
            let Some(novo) = edicao::acrescentar(&mut t, sub, extremo, p) else {
                return Vec::new();
            };
            edicao::tornar_automatica(&mut t, novo, false);
            edicao::recalcular_automaticas(&mut t);
            let Some(s) = t.subcaminho(sub) else {
                return Vec::new();
            };
            let n = s.quantos_segmentos();
            let ultimos: Vec<usize> = match extremo {
                Extremo::Fim => (n.saturating_sub(2)..n).collect(),
                Extremo::Inicio => (0..n.min(2)).collect(),
            };
            return ultimos
                .into_iter()
                .filter_map(|i| s.segmento(i))
                .map(|g| g.p)
                .collect();
        }
        let p = if shift { em_45(a.ponto, p) } else { p };
        vec![[a.ponto, a.controle(extremo.alca_para_frente()), p, p]]
    }

    /// O traço à mão em curso (a Caneta de forma livre), para a tela.
    pub fn traco_livre(&self) -> Option<&[Ponto]> {
        match &self.estado {
            Estado::DesenhandoLivre { pontos, .. } => Some(pontos),
            _ => None,
        }
    }

    /// O retângulo de seleção em curso, `(x0, y0, x1, y1)`.
    pub fn retangulo(&self) -> Option<(Ponto, Ponto)> {
        match self.estado {
            Estado::Retangulo { inicio, atual, .. } => Some((inicio, atual)),
            _ => None,
        }
    }
}
