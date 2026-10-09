//! ✒️ A Caneta na sessão: o caminho que se edita, os gestos que viram passos
//! do histórico e os comandos do painel Caminhos (salvar, renomear,
//! duplicar, excluir, fazer seleção, máscara vetorial, preencher, contornar).
//!
//! 🔑 **O caminho mora no documento** (o de trabalho e os nomeados em
//! `Documento::caminhos`, a máscara vetorial na camada), e o que o painel
//! escolheu é o [`Sessao::alvo_vetorial`]. Um gesto muda o caminho **ao vivo**
//! no documento; o soltar registra um [`Comando::Caminho`] com o antes e o
//! depois — e o Esc no meio volta o antes, sem passo.
//!
//! 🔑 **Começar a desenhar sem caminho escolhido cria o caminho de trabalho**,
//! no lugar do anterior, como no Photoshop — e o passo guarda o anterior: o
//! desfazer o traz de volta. Caminhos nomeados nunca são trocados assim.

use super::Sessao;
use crate::historico::Comando;
use crate::operacoes;
use crate::retangulo::Retangulo;
use crate::selecao::{Operacao, Selecao};
use crate::vetor::caneta::{
    Acao, FerramentaVetorial, Medida, Modificadores, ModoDaCaneta, Resultado,
};
use crate::vetor::cobertura::{self, Opcoes};
use crate::vetor::{edicao, Caminho, LugarDoCaminho, MascaraVetorial, Ponto, NOME_DO_TRABALHO};

/// Um gesto vetorial em curso: onde, o caminho de antes (para o passo) e a
/// posição do nomeado na lista.
#[derive(Clone, Debug)]
pub(super) struct GestoVetorial {
    lugar: LugarDoCaminho,
    antes: Option<Caminho>,
    indice: usize,
}

/// "Transformar caminho" em curso (⌘T com uma ferramenta de caminho): a
/// caixa dos componentes, o caminho de antes e a transformação de agora — a
/// mesma caixa e as mesmas alças do ⌘T dos pixels.
#[derive(Clone, Debug)]
pub(super) struct CaminhoSolto {
    lugar: LugarDoCaminho,
    indice: usize,
    antes: Caminho,
    subs: Vec<u64>,
    caixa: crate::transformar::Caixa,
    pub(super) t: crate::transformar::Transformacao,
}

/// A afim da transformação na caixa, em `f64`: `p ↦ (a·x + b·y + c, d·x +
/// e·y + f)` — a mesma conta de `Transformacao::aplicar`.
pub fn afim_da_transformacao(
    caixa: &crate::transformar::Caixa,
    t: &crate::transformar::Transformacao,
) -> [f64; 6] {
    let cx = caixa.x as f64 + caixa.largura as f64 / 2.0;
    let cy = caixa.y as f64 + caixa.altura as f64 / 2.0;
    let (sx, sy) = (t.escala_x as f64, t.escala_y as f64);
    let (sen, cos) = (t.angulo as f64).sin_cos();
    let (a, b, d, e) = (sx * cos, -sy * sen, sx * sen, sy * cos);
    [
        a,
        b,
        cx + t.dx as f64 - a * cx - b * cy,
        d,
        e,
        cy + t.dy as f64 - d * cx - e * cy,
    ]
}

/// As opções de "Fazer seleção" (o diálogo do Photoshop).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpcoesDaSelecaoDoCaminho {
    /// Difusão em pixels do documento (0 = a borda como a curva).
    pub difusao: u32,
    pub suavizar: bool,
    pub operacao: Operacao,
}

impl Default for OpcoesDaSelecaoDoCaminho {
    fn default() -> Self {
        Self {
            difusao: 0,
            suavizar: true,
            operacao: Operacao::Nova,
        }
    }
}

impl Sessao {
    // ---------------------------------------------------------- o alvo

    /// O caminho escolhido no painel (o que a Caneta edita e os comandos
    /// usam). `None`: nenhum — o próximo desenho cria o de trabalho.
    pub fn alvo_vetorial(&self) -> Option<LugarDoCaminho> {
        self.alvo_vetorial
            .filter(|l| self.doc.caminho(*l).is_some())
    }

    pub fn caminho_alvo(&self) -> Option<&Caminho> {
        self.alvo_vetorial().and_then(|l| self.doc.caminho(l))
    }

    /// Escolhe o caminho do painel (ou nenhum — "ocultar", como clicar no
    /// vazio do painel do Photoshop). O desenho aberto termina; nada muda no
    /// caminho.
    pub fn escolher_caminho(&mut self, lugar: Option<LugarDoCaminho>) {
        self.terminar_gesto_vetorial();
        let lugar = lugar.filter(|l| self.doc.caminho(*l).is_some());
        if lugar != self.alvo_vetorial {
            self.caneta.conferir(None);
        }
        self.alvo_vetorial = lugar;
        self.versao += 1;
    }

    /// O nome do caminho para o painel e o Histórico ("Caminho de trabalho",
    /// "Caminho 1", "Camada 1 – máscara vetorial").
    pub fn nome_do_caminho(&self, lugar: LugarDoCaminho) -> String {
        match lugar {
            LugarDoCaminho::Mascara(i) => format!(
                "{} – máscara vetorial",
                self.doc.camadas.get(i).map_or("", |c| c.nome.as_str())
            ),
            _ => self
                .doc
                .caminho(lugar)
                .map(|c| c.nome.clone())
                .unwrap_or_default(),
        }
    }

    fn indice_do_lugar(&self, lugar: LugarDoCaminho) -> usize {
        match lugar {
            LugarDoCaminho::Nomeado(id) => self
                .doc
                .caminhos
                .indice_do_nomeado(id)
                .unwrap_or(self.doc.caminhos.nomeados.len()),
            _ => 0,
        }
    }

    /// Onde a foto muda quando o caminho em `lugar` vai de `antes` a
    /// `depois` — só a máscara vetorial (ligada, numa camada visível) muda.
    fn sujo_do_caminho(
        &self,
        lugar: LugarDoCaminho,
        antes: Option<&Caminho>,
        depois: Option<&Caminho>,
    ) -> Retangulo {
        let LugarDoCaminho::Mascara(i) = lugar else {
            return Retangulo::default();
        };
        let Some(c) = self.doc.camadas.get(i) else {
            return Retangulo::default();
        };
        if !c.visivel || c.mascara_vetorial_ativa().is_none() {
            return Retangulo::default();
        }
        let (l, a) = (self.doc.largura(), self.doc.altura());
        let Some(m) = c.mascara_vetorial_ativa() else {
            return Retangulo::default();
        };
        let so_a_caixa = |c: Option<&Caminho>| c.is_none_or(|c| c.alcanca_so_a_caixa());
        let caixa = |c: Option<&Caminho>| c.map_or(Retangulo::default(), |c| c.retangulo(l, a));
        let sujo = if !so_a_caixa(antes) || !so_a_caixa(depois) {
            Retangulo::inteiro(l, a)
        } else {
            m.alcance(caixa(antes).uniao(&caixa(depois)), l, a)
        };
        crate::composicao::interseccao(&sujo, &c.area())
    }

    /// Registra um passo de caminho **já aplicado** no documento.
    fn registrar_caminho(
        &mut self,
        nome: &str,
        lugar: LugarDoCaminho,
        indice: usize,
        antes: Option<Caminho>,
    ) -> bool {
        let depois = self.doc.caminho(lugar).cloned();
        if depois == antes {
            return false;
        }
        let sujo = self.sujo_do_caminho(lugar, antes.as_ref(), depois.as_ref());
        self.hist.registrar(Comando::Caminho {
            nome: nome.to_string(),
            lugar,
            indice,
            antes: antes.map(Box::new),
            depois: depois.map(Box::new),
        });
        self.refazer_a_vista(&sujo);
        true
    }

    /// Executa um passo de caminho (aplica e registra) e escolhe o lugar.
    fn executar_caminho(
        &mut self,
        nome: &str,
        lugar: LugarDoCaminho,
        depois: Option<Caminho>,
    ) -> bool {
        let antes = self.doc.caminho(lugar).cloned();
        let indice = self.indice_do_lugar(lugar);
        if antes == depois {
            return false;
        }
        self.executar(Comando::Caminho {
            nome: nome.to_string(),
            lugar,
            indice,
            antes: antes.map(Box::new),
            depois: depois.map(Box::new),
        });
        true
    }

    // ------------------------------------------------------ os gestos

    pub fn usar_ferramenta_vetorial(&mut self, f: FerramentaVetorial) {
        self.terminar_gesto_vetorial();
        self.caneta.usar(f);
        self.versao += 1;
    }

    /// O botão desceu com uma ferramenta de caminho em `p` (pixels do
    /// documento). Devolve se algo mudou na tela.
    pub fn caneta_apertar(&mut self, p: Ponto, m: Modificadores, medida: Medida) -> bool {
        self.fechar_o_que_esta_aberto();
        let efetiva = self.caneta.ferramenta_efetiva(m);
        self.forma_do_gesto = None;
        if self.caneta.opcoes.modo == ModoDaCaneta::Forma && efetiva.desenha() {
            self.talvez_nova_camada_de_forma(p, m, medida);
        }
        let (lugar, base, antes) = match self.alvo_vetorial() {
            Some(l) => {
                let c = self.doc.caminho(l).cloned();
                (
                    l,
                    c.clone().unwrap_or_else(|| self.caminho_novo_de_trabalho()),
                    c,
                )
            }
            // Sem caminho escolhido, só a Caneta começa um: o de trabalho,
            // no lugar do anterior (que o passo guarda).
            None if efetiva == FerramentaVetorial::Caneta => (
                LugarDoCaminho::Trabalho,
                self.caminho_novo_de_trabalho(),
                self.doc.caminhos.trabalho.clone(),
            ),
            None => return false,
        };
        let indice = self.indice_do_lugar(lugar);
        if self.alvo_vetorial != Some(lugar) {
            self.caneta.conferir(None);
        }
        self.alvo_vetorial = Some(lugar);
        let mut c = base;
        let r = self.caneta.apertar(&mut c, p, m, medida);
        self.versao += 1;
        match r {
            Resultado::Passo(nome) => {
                self.doc.definir_caminho(lugar, indice, Some(c));
                self.registrar_caminho(nome, lugar, indice, antes);
            }
            Resultado::AoVivo | Resultado::Nada | Resultado::Cancelado => {
                if self.caneta.em_gesto() {
                    self.doc.definir_caminho(lugar, indice, Some(c));
                    self.gesto_vetorial = Some(GestoVetorial {
                        lugar,
                        antes,
                        indice,
                    });
                } else if antes.as_ref() != Some(&c) && !c.vazio() {
                    self.doc.definir_caminho(lugar, indice, Some(c));
                    self.registrar_caminho("Caminho", lugar, indice, antes);
                }
            }
        }
        true
    }

    /// O modo Forma: um clique que começaria um componente novo cria uma
    /// camada de forma (Cor sólida com a cor de frente e máscara vetorial que
    /// esconde tudo enquanto não há área), logo acima da escolhida, e o
    /// desenho vai na máscara dela — ⇧ soma à forma escolhida, como o
    /// "Combinar formas" do Photoshop. Desenhando (ou numa ponta, num
    /// segmento…), segue na forma de agora.
    fn talvez_nova_camada_de_forma(&mut self, p: Ponto, m: Modificadores, medida: Medida) {
        let na_forma = self
            .alvo_vetorial()
            .is_some_and(|l| matches!(l, LugarDoCaminho::Mascara(i) if self.e_camada_de_forma(i)));
        let vazio = Caminho::novo(0, "");
        let c = if na_forma {
            self.caminho_alvo()
                .cloned()
                .unwrap_or_else(|| vazio.clone())
        } else {
            vazio.clone()
        };
        let acao = self.caneta.decidir(&c, p, m, medida);
        let nova = acao == Acao::NovoComponente && !(m.shift && na_forma);
        if !nova && na_forma {
            return;
        }
        let (l, a) = (self.doc.largura(), self.doc.altura());
        let numero = self
            .doc
            .camadas
            .iter()
            .filter_map(|c| c.nome.strip_prefix("Forma ")?.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let mut camada = crate::documento::Camada::nova(&format!("Forma {numero}"), l, a);
        camada.ajuste = Some(crate::ajuste::Ajuste::CorSolida {
            cor: self.pincel.cor,
        });
        let mut mascara = MascaraVetorial::nova(Caminho::novo(0, "Forma"));
        mascara.revela_vazia = false;
        camada.mascara_vetorial = Some(mascara);
        let indice = (self.ativa() + 1).min(self.doc.camadas.len());
        self.executar(Comando::CriarCamada {
            indice,
            camada: Box::new(camada),
        });
        self.alvo_vetorial = Some(LugarDoCaminho::Mascara(indice));
        self.caneta.conferir(None);
        self.forma_do_gesto = Some(indice);
    }

    /// A camada `i` é de forma (Cor sólida com máscara vetorial).
    pub fn e_camada_de_forma(&self, i: usize) -> bool {
        self.doc.camadas.get(i).is_some_and(|c| {
            matches!(c.ajuste, Some(crate::ajuste::Ajuste::CorSolida { .. }))
                && c.mascara_vetorial.is_some()
        })
    }

    /// A camada de forma criada pelo gesto que terminou sem nada (Esc, ou um
    /// clique que não ficou): sai, e o histórico volta ao de antes dela.
    fn largar_forma_vazia(&mut self) {
        let Some(i) = self.forma_do_gesto.take() else {
            return;
        };
        let vazia = self
            .doc
            .caminho(LugarDoCaminho::Mascara(i))
            .is_none_or(|c| c.vazio());
        let ultimo_e_ela = matches!(
            self.hist.a_desfazer(),
            Some(Comando::CriarCamada { indice, .. }) if *indice == i
        );
        if vazia && ultimo_e_ela {
            self.desfazer();
        }
    }

    /// O caminho de trabalho novo, vazio.
    fn caminho_novo_de_trabalho(&mut self) -> Caminho {
        let id = self.doc.caminhos.gerar_id();
        Caminho::novo(id, NOME_DO_TRABALHO)
    }

    /// O ponteiro andou: arrasta o gesto (ao vivo) ou só atualiza a faixa
    /// elástica e o cursor. Devolve se a tela precisa de quadro novo.
    pub fn caneta_arrastar(&mut self, p: Ponto, m: Modificadores, medida: Medida) -> bool {
        let Some(g) = self.gesto_vetorial.clone() else {
            let mudou = self.caneta.ponteiro != Some(p);
            self.caneta.ponteiro = Some(p);
            return mudou;
        };
        let Some(mut c) = self.doc.caminho(g.lugar).cloned() else {
            return false;
        };
        let r = self.caneta.arrastar(&mut c, p, m, medida);
        if r == Resultado::AoVivo {
            // 🔑 Só o vetor muda durante o arrasto: a máscara vetorial não é
            // rasterizada a cada movimento (a foto acompanha ao soltar).
            self.doc.definir_caminho(g.lugar, g.indice, Some(c));
            self.versao += 1;
        }
        true
    }

    /// O botão subiu: o gesto vira um passo (se mudou alguma coisa).
    pub fn caneta_soltar(&mut self, medida: Medida) -> bool {
        let Some(g) = self.gesto_vetorial.take() else {
            return false;
        };
        let Some(mut c) = self.doc.caminho(g.lugar).cloned() else {
            return false;
        };
        let r = self.caneta.soltar(&mut c, medida);
        self.doc.definir_caminho(g.lugar, g.indice, Some(c.clone()));
        self.versao += 1;
        match r {
            Resultado::Passo(nome) => {
                self.registrar_caminho(nome, g.lugar, g.indice, g.antes);
                self.forma_do_gesto = None;
            }
            _ => {
                // Nada mudou: o caminho de trabalho criado por este clique
                // (vazio) não fica.
                if c.vazio() && g.antes.as_ref().is_none_or(|a| a.vazio()) {
                    self.doc.definir_caminho(g.lugar, g.indice, g.antes);
                }
                self.largar_forma_vazia();
            }
        }
        true
    }

    /// Um gesto em curso termina como está (trocar de ferramenta, desfazer,
    /// outro comando).
    pub(super) fn terminar_gesto_vetorial(&mut self) {
        if self.gesto_vetorial.is_some() {
            self.caneta_soltar(Medida { por_ponto: 1.0 });
        }
    }

    /// Há um gesto vetorial com o botão apertado.
    pub fn caneta_em_gesto(&self) -> bool {
        self.gesto_vetorial.is_some()
    }

    /// Esc: cancela o arrasto (o caminho volta), ou termina o desenho, ou
    /// solta a escolha dos pontos. Devolve se a tecla foi da Caneta.
    pub fn caneta_esc(&mut self) -> bool {
        if let Some(g) = self.gesto_vetorial.take() {
            if let Some(mut c) = self.doc.caminho(g.lugar).cloned() {
                self.caneta.esc(&mut c);
            }
            self.doc.definir_caminho(g.lugar, g.indice, g.antes);
            self.versao += 1;
            self.largar_forma_vazia();
            return true;
        }
        let tinha = self.caneta.construindo().is_some()
            || !self.caneta.pontos_escolhidos().is_empty()
            || !self.caneta.componentes_escolhidos().is_empty();
        if let Some(lugar) = self.alvo_vetorial() {
            let mut c = self
                .doc
                .caminho(lugar)
                .cloned()
                .unwrap_or_else(|| Caminho::novo(0, ""));
            self.caneta.esc(&mut c);
        }
        self.versao += 1;
        tinha
    }

    /// Enter com a Caneta: termina o desenho aberto. Devolve se havia.
    pub fn caneta_encerrar(&mut self) -> bool {
        let ok = self.caneta.encerrar();
        if ok {
            self.versao += 1;
        }
        ok
    }

    /// Delete / ⌫ com uma ferramenta de caminho (ver
    /// [`crate::vetor::caneta::Caneta::excluir`]). Falso quando não havia o
    /// que excluir do caminho.
    pub fn caneta_excluir(&mut self) -> bool {
        self.terminar_gesto_vetorial();
        let Some(lugar) = self.alvo_vetorial() else {
            return false;
        };
        let antes = self.doc.caminho(lugar).cloned();
        let Some(mut c) = antes.clone() else {
            return false;
        };
        let indice = self.indice_do_lugar(lugar);
        match self.caneta.excluir(&mut c) {
            Resultado::Passo(nome) => {
                self.doc.definir_caminho(lugar, indice, Some(c));
                self.registrar_caminho(nome, lugar, indice, antes)
            }
            _ => false,
        }
    }

    /// As setas: as âncoras ou componentes escolhidos andam `d` pixels.
    pub fn caneta_empurrar(&mut self, d: (f64, f64)) -> bool {
        self.terminar_gesto_vetorial();
        let Some(lugar) = self.alvo_vetorial() else {
            return false;
        };
        let antes = self.doc.caminho(lugar).cloned();
        let Some(mut c) = antes.clone() else {
            return false;
        };
        let indice = self.indice_do_lugar(lugar);
        match self.caneta.empurrar(&mut c, d) {
            Resultado::Passo(nome) => {
                self.doc.definir_caminho(lugar, indice, Some(c));
                self.registrar_caminho(nome, lugar, indice, antes)
            }
            _ => false,
        }
    }

    /// Muda o caminho escolhido com uma edição sem gesto (operação dos
    /// componentes, regra de preenchimento, ligação de âncoras), num passo.
    pub fn editar_caminho_alvo(
        &mut self,
        nome: &str,
        editar: impl FnOnce(&mut Caminho) -> bool,
    ) -> bool {
        self.terminar_gesto_vetorial();
        let Some(lugar) = self.alvo_vetorial() else {
            return false;
        };
        let Some(mut c) = self.doc.caminho(lugar).cloned() else {
            return false;
        };
        if !editar(&mut c) {
            return false;
        }
        self.executar_caminho(nome, lugar, Some(c))
    }

    // ------------------------------------------------- o painel Caminhos

    /// "Novo caminho": um nomeado vazio, escolhido — o próximo desenho vai
    /// nele.
    pub fn novo_caminho(&mut self) -> LugarDoCaminho {
        self.terminar_gesto_vetorial();
        let id = self.doc.caminhos.gerar_id();
        let nome = self.doc.caminhos.proximo_nome();
        let lugar = LugarDoCaminho::Nomeado(id);
        self.executar_caminho("Novo caminho", lugar, Some(Caminho::novo(id, &nome)));
        self.alvo_vetorial = Some(lugar);
        self.caneta.conferir(None);
        lugar
    }

    /// "Salvar caminho": o de trabalho vira um nomeado (com o nome dado) —
    /// um passo só. Devolve o lugar novo.
    pub fn salvar_caminho_de_trabalho(&mut self, nome: &str) -> Option<LugarDoCaminho> {
        self.terminar_gesto_vetorial();
        let trabalho = self.doc.caminhos.trabalho.clone()?;
        let id = self.doc.caminhos.gerar_id();
        let mut nomeado = trabalho.clone();
        nomeado.id = id;
        nomeado.nome = if nome.trim().is_empty() {
            self.doc.caminhos.proximo_nome()
        } else {
            nome.trim().to_string()
        };
        let lugar = LugarDoCaminho::Nomeado(id);
        let indice = self.doc.caminhos.nomeados.len();
        self.executar(Comando::Varios {
            nome: "Salvar caminho".into(),
            passos: vec![
                Comando::Caminho {
                    nome: "Salvar caminho".into(),
                    lugar: LugarDoCaminho::Trabalho,
                    indice: 0,
                    antes: Some(Box::new(trabalho)),
                    depois: None,
                },
                Comando::Caminho {
                    nome: "Salvar caminho".into(),
                    lugar,
                    indice,
                    antes: None,
                    depois: Some(Box::new(nomeado)),
                },
            ],
        });
        if matches!(self.alvo_vetorial, Some(LugarDoCaminho::Trabalho) | None) {
            self.alvo_vetorial = Some(lugar);
        }
        self.caneta.conferir(self.doc.caminho(lugar));
        Some(lugar)
    }

    pub fn renomear_caminho(&mut self, lugar: LugarDoCaminho, nome: &str) -> bool {
        let nome = nome.trim();
        if nome.is_empty() || matches!(lugar, LugarDoCaminho::Mascara(_)) {
            return false;
        }
        let Some(mut c) = self.doc.caminho(lugar).cloned() else {
            return false;
        };
        if c.nome == nome {
            return false;
        }
        let passo = format!("Renomear {} para {nome}", c.nome);
        c.nome = nome.to_string();
        // Renomear o de trabalho é salvá-lo (como no Photoshop).
        if lugar == LugarDoCaminho::Trabalho {
            return self.salvar_caminho_de_trabalho(nome).is_some();
        }
        self.executar_caminho(&passo, lugar, Some(c))
    }

    /// "Duplicar caminho": uma cópia nomeada, logo abaixo, escolhida.
    pub fn duplicar_caminho(&mut self, lugar: LugarDoCaminho) -> Option<LugarDoCaminho> {
        self.terminar_gesto_vetorial();
        let mut c = self.doc.caminho(lugar)?.clone();
        let id = self.doc.caminhos.gerar_id();
        c.id = id;
        c.nome = match lugar {
            LugarDoCaminho::Mascara(_) => self.doc.caminhos.proximo_nome(),
            _ => format!("{} cópia", c.nome),
        };
        let novo = LugarDoCaminho::Nomeado(id);
        self.executar_caminho("Duplicar caminho", novo, Some(c));
        self.alvo_vetorial = Some(novo);
        self.caneta.conferir(None);
        Some(novo)
    }

    /// "Excluir caminho" (a máscara vetorial sai pelo comando dela).
    pub fn excluir_caminho(&mut self, lugar: LugarDoCaminho) -> bool {
        self.terminar_gesto_vetorial();
        if let LugarDoCaminho::Mascara(i) = lugar {
            return self.excluir_mascara_vetorial(i);
        }
        let ok = self.executar_caminho("Excluir caminho", lugar, None);
        if ok && self.alvo_vetorial == Some(lugar) {
            self.alvo_vetorial = None;
            self.caneta.conferir(None);
        }
        ok
    }

    /// A cobertura do caminho escolhido (antisserrilhada ou não), na foto.
    pub fn cobertura_do_caminho(&self, suavizar: bool) -> Option<Selecao> {
        let c = self.caminho_alvo()?;
        if !c.tem_area() {
            return None;
        }
        Some(cobertura::rasterizar(
            c,
            self.doc.largura(),
            self.doc.altura(),
            &Opcoes::da_selecao(suavizar),
        ))
    }

    /// "Fazer seleção" (⌘↵): o caminho escolhido vira seleção — rasterizado
    /// em resolução cheia, com o antisserrilhado e a difusão pedidos, junto
    /// da seleção de agora pela operação. **A seleção é independente do
    /// caminho**: editar o caminho depois não a muda.
    pub fn fazer_selecao_do_caminho(&mut self, opcoes: OpcoesDaSelecaoDoCaminho) -> bool {
        self.terminar_gesto_vetorial();
        let Some(mut nova) = self.cobertura_do_caminho(opcoes.suavizar) else {
            return false;
        };
        if opcoes.difusao > 0 {
            nova = nova.difusa(opcoes.difusao);
        }
        let antes = self.versao_da_selecao;
        self.entrar_na_selecao(nova, opcoes.operacao, "Fazer seleção");
        self.versao_da_selecao != antes
    }

    // ------------------------------------------------- transformar caminho

    /// ⌘T com uma ferramenta de caminho: a caixa em volta dos componentes
    /// escolhidos (ou do caminho inteiro), com as alças do ⌘T. Enter aplica
    /// num passo ("Transformar caminho"), Esc volta. Falso sem caminho com
    /// âncoras.
    pub fn comecar_a_transformar_caminho(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let Some(lugar) = self.alvo_vetorial() else {
            return false;
        };
        let Some(c) = self.doc.caminho(lugar).cloned() else {
            return false;
        };
        let mut subs: Vec<u64> = self
            .caneta
            .componentes_escolhidos()
            .iter()
            .copied()
            .collect();
        subs.extend(self.caneta.pontos_escolhidos().iter().map(|r| r.sub));
        subs.sort_unstable();
        subs.dedup();
        if subs.is_empty() {
            subs = c.subcaminhos.iter().map(|s| s.id).collect();
        }
        let caixa = c
            .subcaminhos
            .iter()
            .filter(|s| subs.contains(&s.id))
            .filter_map(|s| s.limites())
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)));
        let Some((x0, y0, x1, y1)) = caixa else {
            return false;
        };
        let (x0, y0) = (x0.floor(), y0.floor());
        let (x1, y1) = (x1.ceil().max(x0 + 1.0), y1.ceil().max(y0 + 1.0));
        self.caminho_solto = Some(CaminhoSolto {
            lugar,
            indice: self.indice_do_lugar(lugar),
            antes: c,
            subs,
            caixa: crate::transformar::Caixa::nova(
                x0 as i32,
                y0 as i32,
                (x1 - x0) as u32,
                (y1 - y0) as u32,
            ),
            t: crate::transformar::Transformacao::default(),
        });
        self.versao += 1;
        true
    }

    pub fn transformando_o_caminho(&self) -> bool {
        self.caminho_solto.is_some()
    }

    pub(super) fn caixa_do_caminho_solto(
        &self,
    ) -> Option<(crate::transformar::Caixa, crate::transformar::Transformacao)> {
        self.caminho_solto.as_ref().map(|c| (c.caixa, c.t))
    }

    /// A transformação de agora no caminho, ao vivo (sem passo).
    pub(super) fn transformar_o_caminho_solto(&mut self, t: crate::transformar::Transformacao) {
        let Some(solto) = self.caminho_solto.as_mut() else {
            return;
        };
        if solto.t == t {
            return;
        }
        solto.t = t;
        let mut c = solto.antes.clone();
        edicao::transformar_subcaminhos(
            &mut c,
            &solto.subs,
            afim_da_transformacao(&solto.caixa, &t),
        );
        let (lugar, indice) = (solto.lugar, solto.indice);
        self.doc.definir_caminho(lugar, indice, Some(c));
        self.versao += 1;
    }

    /// Enter: o caminho transformado num passo.
    pub(super) fn aplicar_o_caminho_solto(&mut self) -> bool {
        let Some(solto) = self.caminho_solto.take() else {
            return false;
        };
        self.versao += 1;
        let nome = if solto.t.so_desloca() {
            "Mover caminho"
        } else {
            "Transformar caminho"
        };
        self.registrar_caminho(nome, solto.lugar, solto.indice, Some(solto.antes))
    }

    /// Esc: o caminho volta.
    pub(super) fn cancelar_o_caminho_solto(&mut self) -> bool {
        let Some(solto) = self.caminho_solto.take() else {
            return false;
        };
        self.doc
            .definir_caminho(solto.lugar, solto.indice, Some(solto.antes));
        self.versao += 1;
        true
    }

    // ------------------------------------------------- máscara vetorial

    /// "Adicionar máscara vetorial" na camada escolhida: com o caminho
    /// escolhido (uma cópia — o caminho fica onde está), ou vazia, que
    /// revela tudo até se desenhar nela. A máscara fica escolhida no painel
    /// Caminhos: o próximo desenho é nela.
    pub fn criar_mascara_vetorial(&mut self) -> bool {
        let caminho = self.caminho_alvo().cloned();
        self.criar_mascara_vetorial_com(caminho)
    }

    /// "Revelar tudo": a máscara vetorial vazia, mesmo com um caminho
    /// escolhido.
    pub fn criar_mascara_vetorial_vazia(&mut self) -> bool {
        self.criar_mascara_vetorial_com(None)
    }

    fn criar_mascara_vetorial_com(&mut self, caminho: Option<Caminho>) -> bool {
        self.terminar_gesto_vetorial();
        let i = self.ativa();
        if self.doc.camadas[i].mascara_vetorial.is_some() {
            return false;
        }
        let caminho = match caminho {
            Some(c) => {
                let mut c = c.clone();
                c.nome = "Máscara vetorial".into();
                c
            }
            None => Caminho::novo(0, "Máscara vetorial"),
        };
        self.executar(Comando::MascaraVetorial {
            camada: i,
            antes: None,
            depois: Some(Box::new(MascaraVetorial::nova(caminho))),
        });
        self.alvo_vetorial = Some(LugarDoCaminho::Mascara(i));
        self.caneta.conferir(None);
        true
    }

    fn trocar_mascara_vetorial(
        &mut self,
        i: usize,
        mudar: impl FnOnce(&mut Option<MascaraVetorial>),
    ) -> bool {
        self.terminar_gesto_vetorial();
        let Some(c) = self.doc.camadas.get(i) else {
            return false;
        };
        let antes = c.mascara_vetorial.clone();
        let mut depois = antes.clone();
        mudar(&mut depois);
        if depois == antes {
            return false;
        }
        self.executar(Comando::MascaraVetorial {
            camada: i,
            antes: antes.map(Box::new),
            depois: depois.map(Box::new),
        });
        true
    }

    /// Liga ou desliga a máscara vetorial (o caminho continua).
    pub fn alternar_mascara_vetorial(&mut self, i: usize) -> bool {
        self.trocar_mascara_vetorial(i, |m| {
            if let Some(m) = m {
                m.ativa = !m.ativa;
            }
        })
    }

    /// A corrente: se o Mover e o ⌘T levam o caminho junto com a camada.
    pub fn alternar_vinculo_vetorial(&mut self, i: usize) -> bool {
        self.trocar_mascara_vetorial(i, |m| {
            if let Some(m) = m {
                m.vinculada = !m.vinculada;
            }
        })
    }

    pub fn excluir_mascara_vetorial(&mut self, i: usize) -> bool {
        let ok = self.trocar_mascara_vetorial(i, |m| *m = None);
        if ok && self.alvo_vetorial == Some(LugarDoCaminho::Mascara(i)) {
            self.alvo_vetorial = None;
            self.caneta.conferir(None);
        }
        ok
    }

    /// As Propriedades da máscara vetorial ao vivo (o arrasto do slider): a
    /// densidade (`0..=1`) e a difusão (px). O passo sai no
    /// [`Self::confirmar_mascara_vetorial`], com o arrasto inteiro.
    pub fn mover_propriedades_da_mascara_vetorial(
        &mut self,
        i: usize,
        densidade: Option<f32>,
        difusao: Option<f32>,
    ) {
        let Some(m) = self
            .doc
            .camadas
            .get(i)
            .and_then(|c| c.mascara_vetorial.clone())
        else {
            return;
        };
        if self.vetorial_antes.is_none() {
            self.vetorial_antes = Some((i, m.clone()));
        }
        let mut nova = m.clone();
        if let Some(d) = densidade {
            nova.densidade = d.clamp(0.0, 1.0);
        }
        if let Some(r) = difusao {
            nova.difusao = r.clamp(0.0, crate::vetor::DIFUSAO_MAXIMA_VETORIAL);
        }
        if nova == m {
            return;
        }
        let (l, a) = (self.doc.largura(), self.doc.altura());
        let caixa = m.caminho.retangulo(l, a);
        let sujo = m.alcance(caixa, l, a).uniao(&nova.alcance(caixa, l, a));
        let camada = &mut self.doc.camadas[i];
        camada.mascara_vetorial = Some(nova);
        let sujo = crate::composicao::interseccao(&sujo, &camada.area());
        self.refazer_a_vista(&sujo);
    }

    /// O arrasto das Propriedades da máscara vetorial terminou: um passo.
    pub fn confirmar_mascara_vetorial(&mut self) {
        let Some((i, antes)) = self.vetorial_antes.take() else {
            return;
        };
        let depois = self
            .doc
            .camadas
            .get(i)
            .and_then(|c| c.mascara_vetorial.clone());
        if depois.as_ref() == Some(&antes) {
            return;
        }
        self.hist.registrar(Comando::MascaraVetorial {
            camada: i,
            antes: Some(Box::new(antes)),
            depois: depois.map(Box::new),
        });
        self.versao += 1;
    }

    /// O caminho da máscara vetorial anda com a camada (o Mover, o ⌘T): a
    /// afim `m` de [`edicao::transformar`]. Um passo para entrar no gesto do
    /// Mover — `None` sem máscara vinculada.
    pub(super) fn passo_da_mascara_vetorial_junto(
        &mut self,
        camada: usize,
        m: [f64; 6],
    ) -> Option<Comando> {
        let c = self.doc.camadas.get(camada)?;
        let antes = c.mascara_vetorial.as_ref().filter(|m| m.vinculada)?.clone();
        let mut depois = antes.clone();
        edicao::transformar(&mut depois.caminho, m);
        if depois == antes {
            return None;
        }
        let passo = Comando::MascaraVetorial {
            camada,
            antes: Some(Box::new(antes)),
            depois: Some(Box::new(depois)),
        };
        let sujo = passo.aplicar(&mut self.doc, true);
        self.refazer_a_vista(&sujo);
        Some(passo)
    }

    // ------------------------------------------- preencher e contornar

    /// "Preencher caminho": a cor de frente na camada escolhida (ou na
    /// máscara dela), na área do caminho **dentro da seleção** de agora, com
    /// a `opacidade` — um passo de pixels, com os cadeados respeitados. O
    /// caminho não muda.
    pub fn preencher_caminho(&mut self, opacidade: f32) -> bool {
        let cor = self.pincel.cor;
        self.preencher_caminho_com(cor, opacidade, 0)
    }

    /// "Preencher caminho…" com as opções do diálogo: a cor (frente, fundo,
    /// preto, branco, 50% cinza), a opacidade e o raio de difusão da borda
    /// (a borda antisserrilhada sempre).
    pub fn preencher_caminho_com(&mut self, cor: [u8; 3], opacidade: f32, difusao: u32) -> bool {
        self.terminar_gesto_vetorial();
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        if !self.pode_pintar() || self.pixels_bloqueados() {
            return false;
        }
        let Some(mut area) = self.cobertura_do_caminho(true) else {
            return false;
        };
        if difusao > 0 {
            area = area.difusa(difusao);
        }
        if let Some(sel) = self.selecao.as_deref() {
            area.combinar(sel, Operacao::Intersecao);
        }
        let opacidade = opacidade.clamp(0.0, 1.0);
        if opacidade < 1.0 {
            area = escalar(&area, opacidade);
        }
        if area.nada() {
            return false;
        }
        let na_mascara = self.na_mascara();
        let mudanca = operacoes::preencher(
            self.doc.camadas[camada].alvo_mut(na_mascara),
            Some(&area),
            cor,
        );
        if !self.registrar_mudanca(camada, na_mascara, mudanca) {
            return false;
        }
        self.renomear_o_ultimo("Preencher caminho");
        true
    }

    /// "Contornar caminho": o pincel de agora (cor, tamanho, dureza,
    /// opacidade) passa por cada componente, respeitando a seleção e os
    /// cadeados — um passo só com todos os componentes.
    pub fn contornar_caminho(&mut self) -> bool {
        self.terminar_gesto_vetorial();
        self.fechar_o_que_esta_aberto();
        let Some(c) = self.caminho_alvo().cloned() else {
            return false;
        };
        let antes = self.hist.posicao();
        // O traço segue a curva exata: sem a suavização do pincel (o cordão
        // atrasaria o traço nas curvas).
        let suavizacao = std::mem::replace(&mut self.pincel.suavizacao, 0.0);
        for s in &c.subcaminhos {
            let pontos = crate::vetor::geometria::achatar(s, 0.25, s.fechado);
            let Some(primeiro) = pontos.first() else {
                continue;
            };
            if !self.apertar(primeiro.x as f32, primeiro.y as f32) {
                continue;
            }
            for q in pontos.iter().skip(1).chain(s.fechado.then_some(primeiro)) {
                self.arrastar(q.x as f32, q.y as f32);
            }
            self.soltar();
        }
        self.pincel.suavizacao = suavizacao;
        let feitos = self.hist.posicao().saturating_sub(antes);
        match feitos {
            0 => false,
            1 => {
                self.renomear_o_ultimo("Contornar caminho");
                true
            }
            n => {
                self.hist.juntar_os_ultimos(n, "Contornar caminho");
                true
            }
        }
    }

    /// "Contornar caminho…" com a ferramenta escolhida no diálogo (pincel,
    /// borracha, desfoque, nitidez, subexposição, superexposição), com o
    /// tamanho e as opções de agora; a ferramenta na mão volta depois.
    pub fn contornar_caminho_com(&mut self, ferramenta: crate::pincel::Ferramenta) -> bool {
        let antes = self.pincel.ferramenta;
        self.pincel.ferramenta = ferramenta;
        let ok = self.contornar_caminho();
        self.pincel.ferramenta = antes;
        ok
    }

    /// O último passo ganha o nome do gesto (um traço de pincel vira
    /// "Contornar caminho" no Histórico, e não "Pincel").
    fn renomear_o_ultimo(&mut self, nome: &str) {
        self.hist.renomear_o_ultimo(nome);
    }
}

/// A seleção com cada valor vezes `fator` (a opacidade do preenchimento).
fn escalar(s: &Selecao, fator: f32) -> Selecao {
    let mut nova = s.clone();
    let vazia = Selecao::vazia(s.largura(), s.altura());
    nova.combinar_com(&vazia, |a, _| (a as f32 * fator).round() as u8);
    nova
}
