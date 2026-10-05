//! Uma edição aberta: a base, o documento, o histórico, a vista e o traço em
//! curso — tudo o que a janela precisa, sem a janela.
//!
//! 🔑 **Todo gesto passa por aqui e devolve o que sujou**; a vista (e a lupa,
//! com a foto ampliada) é refeita no mesmo passo. A janela só pergunta quais
//! ladrilhos subir para a GPU. É o que deixa a sessão inteira ser testada sem
//! GPUI.
//!
//! 🔑 **A camada escolhida é da sessão, e não do documento**: é onde o pincel
//! pinta e o que os controles da camada mexem, como a camada realçada do
//! Photoshop. Desfazer volta a escolha para a camada que o passo mexeu.

use std::sync::Arc;

use image::RgbImage;

use crate::composicao;
use crate::documento::{Camada, Documento};
use crate::historico::{Comando, Historico};
use crate::mesclagem::Modo;
use crate::operacoes;
use crate::pincel::Mudanca;
use crate::pincel::{Pincel, Traco};
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao, Selecao};
use crate::vista::Vista;

/// Um pedido de lupa: o que a vista do pedaço precisa para ser montada fora
/// da thread da tela. Barato de tirar: a base e os tiles são `Arc`.
pub struct PedidoDeLupa {
    pub id: u64,
    pub regiao: Retangulo,
    pub fator: u32,
    base: Arc<RgbImage>,
    doc: Documento,
}

impl PedidoDeLupa {
    /// A montagem — o trabalho pesado, para o executor de fundo.
    pub fn montar(self) -> (u64, Vista) {
        (
            self.id,
            Vista::da_regiao(&self.base, &self.doc, &self.regiao, self.fator),
        )
    }
}

pub struct Sessao {
    base: Arc<RgbImage>,
    doc: Documento,
    hist: Historico,
    vista: Vista,
    /// A vista em resolução maior do pedaço visível, com a foto ampliada.
    lupa: Option<Vista>,
    /// O último pedido de lupa, e o que a foto mudou desde que ele saiu — a
    /// lupa que chega é refeita ali antes de entrar.
    lupa_pedida: Option<(u64, Retangulo)>,
    pedidos: u64,
    pub pincel: Pincel,
    ativa: usize,
    traco: Option<Traco>,
    /// A camada e a opacidade dela quando o arrasto do slider começou — o
    /// passo do desfazer é o arrasto inteiro, e não cada valor do caminho.
    opacidade_antes: Option<(usize, f32)>,
    /// O letreiro. `Arc`: o traço leva uma cópia barata.
    selecao: Option<Arc<Selecao>>,
    /// Sobe a cada mudança de pixel ou de seleção — quem desenha miniaturas e
    /// bordas sabe quando refazer.
    versao: u64,
}

impl Sessao {
    /// `lado_da_vista` é o maior lado da vista reduzida, em pixels.
    pub fn nova(base: Arc<RgbImage>, doc: Documento, hist: Historico, lado_da_vista: u32) -> Self {
        debug_assert_eq!((base.width(), base.height()), (doc.largura(), doc.altura()));
        let vista = Vista::nova(&base, &doc, lado_da_vista);
        // Abre na camada de cima, como o Photoshop abre um arquivo.
        let ativa = doc.camadas.len().saturating_sub(1);
        Self {
            base,
            doc,
            hist,
            vista,
            lupa: None,
            lupa_pedida: None,
            pedidos: 0,
            pincel: Pincel::default(),
            ativa,
            traco: None,
            opacidade_antes: None,
            selecao: None,
            versao: 0,
        }
    }

    pub fn base(&self) -> &Arc<RgbImage> {
        &self.base
    }

    pub fn documento(&self) -> &Documento {
        &self.doc
    }

    pub fn historico(&self) -> &Historico {
        &self.hist
    }

    pub fn vista(&self) -> &Vista {
        &self.vista
    }

    pub fn vista_mut(&mut self) -> &mut Vista {
        &mut self.vista
    }

    /// Sobe a cada mudança de pixel, de pilha ou de seleção.
    pub fn versao(&self) -> u64 {
        self.versao
    }

    fn refazer_a_vista(&mut self, sujo: &Retangulo) {
        self.versao += 1;
        if sujo.vazio() {
            return;
        }
        self.vista.refazer(&self.base, &self.doc, sujo);
        if let Some(lupa) = self.lupa.as_mut() {
            lupa.refazer(&self.base, &self.doc, sujo);
        }
        if let Some((_, desde)) = self.lupa_pedida.as_mut() {
            *desde = desde.uniao(sujo);
        }
    }

    // ---------------------------------------------------------------- lupa

    pub fn lupa(&self) -> Option<&Vista> {
        self.lupa.as_ref()
    }

    pub fn lupa_mut(&mut self) -> Option<&mut Vista> {
        self.lupa.as_mut()
    }

    /// Já há uma lupa pedida que ainda não chegou?
    pub fn lupa_a_caminho(&self) -> bool {
        self.lupa_pedida.is_some()
    }

    /// Pede a lupa de `regiao` em `fator`. A montagem fica com quem chama.
    pub fn pedir_lupa(&mut self, regiao: Retangulo, fator: u32) -> PedidoDeLupa {
        self.pedidos += 1;
        self.lupa_pedida = Some((self.pedidos, Retangulo::default()));
        PedidoDeLupa {
            id: self.pedidos,
            regiao,
            fator,
            base: self.base.clone(),
            doc: self.doc.clone(),
        }
    }

    /// A lupa montada chegou. Só entra a do último pedido, refeita onde a foto
    /// mudou enquanto ela era montada. Devolve se entrou.
    pub fn receber_lupa(&mut self, id: u64, mut lupa: Vista) -> bool {
        match self.lupa_pedida {
            Some((pedido, desde)) if pedido == id => {
                lupa.refazer(&self.base, &self.doc, &desde);
                lupa.sujar_tudo();
                self.lupa = Some(lupa);
                self.lupa_pedida = None;
                true
            }
            _ => false,
        }
    }

    /// Sem lupa (a foto voltou a caber na vista inteira).
    pub fn largar_lupa(&mut self) {
        self.lupa = None;
        self.lupa_pedida = None;
    }

    // ------------------------------------------------------------- camadas

    /// A camada onde o pincel pinta.
    pub fn ativa(&self) -> usize {
        self.ativa.min(self.doc.camadas.len().saturating_sub(1))
    }

    pub fn camada_ativa(&self) -> &Camada {
        &self.doc.camadas[self.ativa()]
    }

    /// Fecha o que estiver em curso — antes de qualquer gesto que não seja o
    /// próprio traço ou o próprio slider.
    fn fechar_o_que_esta_aberto(&mut self) {
        self.soltar();
        self.confirmar_opacidade();
    }

    pub fn escolher_camada(&mut self, indice: usize) {
        if indice < self.doc.camadas.len() && indice != self.ativa() {
            self.fechar_o_que_esta_aberto();
            self.ativa = indice;
        }
    }

    /// Aplica e registra um passo, e leva a escolha para onde ele mexeu.
    fn executar(&mut self, comando: Comando) {
        let sujo = comando.aplicar(&mut self.doc, true);
        if let Some(i) = comando.camada_depois(true, self.doc.camadas.len()) {
            self.ativa = i;
        }
        self.hist.registrar(comando);
        self.refazer_a_vista(&sujo);
    }

    /// Uma camada transparente logo acima da escolhida, que passa a ser a
    /// escolhida.
    pub fn nova_camada(&mut self) {
        self.fechar_o_que_esta_aberto();
        let camada = Camada::nova(
            &self.doc.proximo_nome(),
            self.doc.largura(),
            self.doc.altura(),
        );
        let indice = (self.ativa() + 1).min(self.doc.camadas.len());
        self.executar(Comando::CriarCamada {
            indice,
            camada: Box::new(camada),
        });
    }

    /// A escolhida, copiada logo acima dela. 🔑 A cópia divide os tiles (`Arc`)
    /// até alguém pintar numa das duas.
    pub fn duplicar_camada(&mut self) {
        self.fechar_o_que_esta_aberto();
        if self.doc.camadas.is_empty() {
            return;
        }
        let mut copia = self.camada_ativa().clone();
        copia.nome = format!("{} cópia", copia.nome);
        let indice = self.ativa() + 1;
        self.executar(Comando::CriarCamada {
            indice,
            camada: Box::new(copia),
        });
    }

    /// Exclui a escolhida. A última não sai: sem camada não há onde pintar.
    pub fn excluir_camada(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        if self.doc.camadas.len() <= 1 {
            return false;
        }
        let indice = self.ativa();
        let camada = Box::new(self.doc.camadas[indice].clone());
        self.executar(Comando::ExcluirCamada { indice, camada });
        true
    }

    /// Sobe (`1`) ou desce (`-1`) a escolhida uma posição na pilha.
    pub fn mover_camada(&mut self, direcao: i32) -> bool {
        self.fechar_o_que_esta_aberto();
        let de = self.ativa();
        let para = de as i64 + direcao.signum() as i64;
        if direcao == 0 || para < 0 || para >= self.doc.camadas.len() as i64 {
            return false;
        }
        self.executar(Comando::MoverCamada {
            de,
            para: para as usize,
        });
        true
    }

    /// Dá outro nome a uma camada. Nome vazio ou igual não vira passo.
    pub fn renomear_camada(&mut self, indice: usize, nome: &str) -> bool {
        self.fechar_o_que_esta_aberto();
        let nome = nome.trim();
        let Some(camada) = self.doc.camadas.get(indice) else {
            return false;
        };
        if nome.is_empty() || nome == camada.nome {
            return false;
        }
        let antes = camada.nome.clone();
        self.executar(Comando::Renomear {
            camada: indice,
            antes,
            depois: nome.to_string(),
        });
        true
    }

    /// O modo de mesclagem da escolhida.
    pub fn mudar_modo(&mut self, modo: Modo) {
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        let antes = self.doc.camadas[camada].modo;
        if antes != modo {
            self.executar(Comando::Modo {
                camada,
                antes,
                depois: modo,
            });
        }
    }

    // ------------------------------------------------------------- seleção

    pub fn selecao(&self) -> Option<&Selecao> {
        self.selecao.as_deref()
    }

    /// Uma forma desenhada entra na seleção. Nova sem nada selecionado no fim
    /// (um clique) desmarca, como no Photoshop.
    pub fn selecionar(&mut self, forma: &Forma, operacao: Operacao) {
        self.fechar_o_que_esta_aberto();
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let nova = Selecao::da_forma(largura, altura, forma);
        let resultado = match (operacao, self.selecao.as_deref()) {
            (Operacao::Nova, _) | (_, None) => {
                if operacao == Operacao::Subtrair {
                    None
                } else {
                    Some(nova)
                }
            }
            (_, Some(atual)) => {
                let mut junta = atual.clone();
                junta.combinar(&nova, operacao);
                Some(junta)
            }
        };
        self.selecao = resultado.filter(|s| !s.nada()).map(Arc::new);
        self.versao += 1;
    }

    /// ⌘A.
    pub fn selecionar_tudo(&mut self) {
        self.fechar_o_que_esta_aberto();
        self.selecao = Some(Arc::new(Selecao::tudo(
            self.doc.largura(),
            self.doc.altura(),
        )));
        self.versao += 1;
    }

    /// ⌘D.
    pub fn desmarcar(&mut self) {
        self.fechar_o_que_esta_aberto();
        if self.selecao.take().is_some() {
            self.versao += 1;
        }
    }

    /// ⇧⌘I. Sem seleção não faz nada (no Photoshop também).
    pub fn inverter_selecao(&mut self) {
        self.fechar_o_que_esta_aberto();
        if let Some(s) = self.selecao.as_mut() {
            Arc::make_mut(s).inverter();
            if s.nada() {
                self.selecao = None;
            }
            self.versao += 1;
        }
    }

    /// Um passo de pixels na camada escolhida, feito de uma vez.
    fn registrar_mudanca(&mut self, camada: usize, mudanca: Option<Mudanca>) -> bool {
        let Some(mudanca) = mudanca else {
            return false;
        };
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let sujo = mudanca
            .depois
            .iter()
            .fold(Retangulo::default(), |a, (p, _)| {
                a.uniao(&crate::tiles::retangulo_do_tile(*p, largura, altura))
            });
        self.hist.registrar(Comando::Traco { camada, mudanca });
        self.refazer_a_vista(&sujo);
        true
    }

    /// Delete: apaga a seleção na camada escolhida. Sem seleção não faz nada.
    pub fn apagar_selecao(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let Some(selecao) = self.selecao.clone() else {
            return false;
        };
        let camada = self.ativa();
        let mudanca = operacoes::apagar(&mut self.doc.camadas[camada].pixels, &selecao);
        self.registrar_mudanca(camada, mudanca)
    }

    /// ⌥Delete: preenche a seleção (ou a camada inteira) com a cor do pincel.
    pub fn preencher_selecao(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        if !self.doc.camadas[camada].visivel {
            return false;
        }
        let selecao = self.selecao.clone();
        let mudanca = operacoes::preencher(
            &mut self.doc.camadas[camada].pixels,
            selecao.as_deref(),
            self.pincel.cor,
        );
        self.registrar_mudanca(camada, mudanca)
    }

    /// ⌘E: a escolhida entra na de baixo, com o modo e a opacidade dela.
    /// Recusa na de baixo de todas e com uma das duas escondida.
    pub fn mesclar_para_baixo(&mut self) -> Result<(), &'static str> {
        self.fechar_o_que_esta_aberto();
        let indice = self.ativa();
        if indice == 0 {
            return Err("Não há camada embaixo desta para mesclar");
        }
        if !self.doc.camadas[indice].visivel || !self.doc.camadas[indice - 1].visivel {
            return Err("Mostre as duas camadas antes de mesclar");
        }
        let de_cima = self.doc.camadas[indice].clone();
        let mut abaixo = self.doc.camadas[indice - 1].pixels.clone();
        let mudanca = operacoes::mesclar_na_de_baixo(&mut abaixo, &de_cima).unwrap_or(Mudanca {
            antes: Vec::new(),
            depois: Vec::new(),
        });
        self.executar(Comando::Mesclar {
            indice,
            de_cima: Box::new(de_cima),
            mudanca,
        });
        Ok(())
    }

    // --------------------------------------------------------------- traço

    /// O ponteiro desceu em `(x, y)`, em pixels da foto. Devolve `false` sem
    /// pintar quando a camada escolhida está escondida — pintar no que não se
    /// vê é o erro que o Photoshop também recusa.
    pub fn apertar(&mut self, x: f32, y: f32) -> bool {
        // Um traço que ficou aberto (o ponteiro saiu da janela sem soltar) fecha
        // antes: ele é um passo próprio do desfazer.
        self.fechar_o_que_esta_aberto();
        let ativa = self.ativa();
        if !self.doc.camadas[ativa].visivel {
            return false;
        }
        let mut traco = Traco::novo(self.pincel).dentro_de(self.selecao.clone());
        let sujo = traco.ate(&mut self.doc.camadas[ativa].pixels, x, y);
        self.traco = Some(traco);
        self.refazer_a_vista(&sujo);
        true
    }

    /// O ponteiro andou, apertado.
    pub fn arrastar(&mut self, x: f32, y: f32) {
        let ativa = self.ativa();
        let Some(traco) = self.traco.as_mut() else {
            return;
        };
        let sujo = traco.ate(&mut self.doc.camadas[ativa].pixels, x, y);
        self.refazer_a_vista(&sujo);
    }

    /// O ponteiro subiu: o traço vira um passo do desfazer. Devolve se houve
    /// mudança.
    pub fn soltar(&mut self) -> bool {
        let Some(traco) = self.traco.take() else {
            return false;
        };
        let camada = self.ativa();
        match traco.terminar(&mut self.doc.camadas[camada].pixels) {
            Some(mudanca) => {
                self.hist.registrar(Comando::Traco { camada, mudanca });
                true
            }
            None => false,
        }
    }

    pub fn tracando(&self) -> bool {
        self.traco.is_some()
    }

    // ------------------------------------------- propriedades da camada

    /// Mostra ou esconde a camada `indice`.
    pub fn alternar_visibilidade_de(&mut self, indice: usize) {
        self.fechar_o_que_esta_aberto();
        let Some(camada) = self.doc.camadas.get(indice) else {
            return;
        };
        let antes = camada.visivel;
        let sujo = Comando::Visibilidade {
            camada: indice,
            antes,
            depois: !antes,
        };
        // Sem `executar`: mostrar ou esconder não muda qual está escolhida.
        let area = sujo.aplicar(&mut self.doc, true);
        self.hist.registrar(sujo);
        self.refazer_a_vista(&area);
    }

    /// Mostra ou esconde a escolhida.
    pub fn alternar_visibilidade(&mut self) {
        self.alternar_visibilidade_de(self.ativa());
    }

    /// O slider da opacidade andou: a camada escolhida muda na hora, o
    /// histórico só no [`Self::confirmar_opacidade`].
    pub fn mover_opacidade(&mut self, valor: f32) {
        self.soltar();
        let valor = valor.clamp(0.0, 1.0);
        let indice = self.ativa();
        if self.opacidade_antes.is_some_and(|(i, _)| i != indice) {
            self.confirmar_opacidade();
        }
        let camada = &mut self.doc.camadas[indice];
        self.opacidade_antes
            .get_or_insert((indice, camada.opacidade));
        if camada.opacidade == valor {
            return;
        }
        camada.opacidade = valor;
        let area = camada.area();
        self.refazer_a_vista(&area);
    }

    /// O arrasto do slider acabou.
    pub fn confirmar_opacidade(&mut self) {
        let Some((camada, antes)) = self.opacidade_antes.take() else {
            return;
        };
        let Some(depois) = self.doc.camadas.get(camada).map(|c| c.opacidade) else {
            return;
        };
        if antes != depois {
            self.hist.registrar(Comando::Opacidade {
                camada,
                antes,
                depois,
            });
        }
    }

    // ------------------------------------------------------ desfazer

    pub fn desfazer(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let passo = self.hist.a_desfazer().cloned();
        match self.hist.desfazer(&mut self.doc) {
            Some(sujo) => {
                if let Some(i) = passo.and_then(|p| p.camada_depois(false, self.doc.camadas.len()))
                {
                    self.ativa = i;
                }
                self.refazer_a_vista(&sujo);
                true
            }
            None => false,
        }
    }

    pub fn refazer(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let passo = self.hist.a_refazer().cloned();
        match self.hist.refazer(&mut self.doc) {
            Some(sujo) => {
                if let Some(i) = passo.and_then(|p| p.camada_depois(true, self.doc.camadas.len())) {
                    self.ativa = i;
                }
                self.refazer_a_vista(&sujo);
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------- salvar

    /// Há alterações que não foram salvas (inclui um traço ou um arrasto de
    /// slider em curso).
    pub fn alterado(&self) -> bool {
        self.hist.alterado() || self.traco.is_some() || self.opacidade_antes.is_some()
    }

    /// Fecha o que estiver em curso e devolve **uma cópia** do documento e do
    /// histórico para gravar em segundo plano. Barato: os tiles são `Arc`.
    ///
    /// 🔑 É o `build_pfe` do PaintFE: o instantâneo sai na thread da tela, e o
    /// trabalho pesado (compor, codificar, gravar) vai para fora dela.
    pub fn instantaneo(&mut self) -> (Documento, Historico) {
        self.fechar_o_que_esta_aberto();
        (self.doc.clone(), self.hist.clone())
    }

    /// A gravação do instantâneo `salvo` terminou bem.
    ///
    /// ⚠️ **Pelo instantâneo, e não "agora"**: se o operador pintou enquanto a
    /// gravação corria, o que ele pintou depois continua alterado.
    pub fn salvo(&mut self, salvo: &Historico) {
        self.hist.marcar_salvo_o_de(salvo);
    }

    /// A imagem editada em resolução cheia.
    pub fn compor(&self) -> RgbImage {
        composicao::compor(&self.base, &self.doc)
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::documento::{BaseRef, NOME_DA_PRIMEIRA};
    use crate::pincel::Ferramenta;
    use crate::selecao::{Forma, Operacao};

    fn sessao() -> Sessao {
        let base = Arc::new(RgbImage::from_fn(800, 600, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 50])
        }));
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        Sessao::nova(base, doc, Historico::novo(), 400)
    }

    #[test]
    fn pintar_e_apagar_mexem_so_na_camada() {
        let mut s = sessao();
        let base = s.base().clone();
        s.pincel.cor = [255, 0, 0];
        s.apertar(100.0, 100.0);
        s.arrastar(300.0, 100.0);
        assert!(s.soltar());
        assert_eq!(s.compor().get_pixel(200, 100).0, [255, 0, 0]);
        assert!(s.alterado());

        s.pincel.ferramenta = Ferramenta::Borracha;
        s.pincel.raio = 80.0;
        s.pincel.dureza = 1.0;
        s.apertar(50.0, 100.0);
        s.arrastar(350.0, 100.0);
        s.soltar();
        assert!(s.documento().camadas[0].pixels.vazia());
        assert_eq!(
            s.compor().as_raw(),
            base.as_raw(),
            "a base continua intacta"
        );
        assert_eq!(s.base().as_raw(), base.as_raw());
    }

    #[test]
    fn a_opacidade_do_slider_e_um_passo_so() {
        let mut s = sessao();
        s.apertar(100.0, 100.0);
        s.soltar();
        for v in [0.9, 0.7, 0.5, 0.3] {
            s.mover_opacidade(v);
        }
        s.confirmar_opacidade();
        assert_eq!(s.historico().passos().len(), 2);
        s.desfazer();
        assert_eq!(s.documento().camadas[0].opacidade, 1.0);
    }

    #[test]
    fn o_que_foi_pintado_durante_a_gravacao_continua_alterado() {
        let mut s = sessao();
        s.apertar(100.0, 100.0);
        s.soltar();
        let (_, hist) = s.instantaneo();
        s.apertar(200.0, 200.0);
        s.soltar();
        s.salvo(&hist);
        assert!(s.alterado());
        s.desfazer();
        assert!(!s.alterado(), "voltou ao que foi gravado");
    }

    #[test]
    fn o_pincel_pinta_na_camada_escolhida() {
        let mut s = sessao();
        s.nova_camada();
        assert_eq!(s.ativa(), 1);
        assert_eq!(s.camada_ativa().nome, "Camada 1");
        s.pincel.cor = [0, 255, 0];
        assert!(s.apertar(100.0, 100.0));
        s.soltar();
        assert!(s.documento().camadas[0].pixels.vazia());
        assert!(!s.documento().camadas[1].pixels.vazia());

        s.escolher_camada(0);
        s.alternar_visibilidade();
        assert!(!s.apertar(300.0, 300.0), "camada escondida não pinta");
        assert!(!s.tracando());

        // Desfazer o traço leva a escolha de volta para a camada dele.
        s.alternar_visibilidade();
        s.desfazer(); // mostrar
        s.desfazer(); // esconder
        s.desfazer(); // traço
        assert_eq!(s.ativa(), 1);
        s.desfazer(); // criar
        assert_eq!(s.documento().camadas.len(), 1);
        assert_eq!(s.ativa(), 0);
        assert!(!s.alterado());
    }

    #[test]
    fn duplicar_excluir_mover_renomear_e_modo() {
        let mut s = sessao();
        s.pincel.cor = [10, 20, 30];
        s.apertar(50.0, 50.0);
        s.soltar();
        s.duplicar_camada();
        assert_eq!(s.documento().camadas.len(), 2);
        assert_eq!(s.camada_ativa().nome, format!("{NOME_DA_PRIMEIRA} cópia"));
        assert_eq!(
            s.documento().camadas[0].pixels,
            s.documento().camadas[1].pixels,
            "a cópia traz os pixels"
        );

        s.mudar_modo(Modo::Multiplicacao);
        assert_eq!(s.camada_ativa().modo, Modo::Multiplicacao);
        assert!(s.renomear_camada(1, "  Sombra  "));
        assert!(!s.renomear_camada(1, "Sombra"), "o mesmo nome não é passo");
        assert!(!s.renomear_camada(1, "   "));
        assert_eq!(s.camada_ativa().nome, "Sombra");

        assert!(!s.mover_camada(1), "a de cima não sobe mais");
        assert!(s.mover_camada(-1));
        assert_eq!(s.ativa(), 0);
        assert_eq!(s.documento().camadas[0].nome, "Sombra");

        assert!(s.excluir_camada());
        assert_eq!(s.documento().camadas.len(), 1);
        assert!(!s.excluir_camada(), "a última fica");
        s.desfazer();
        assert_eq!(s.documento().camadas[0].nome, "Sombra");
        assert_eq!(s.ativa(), 0);
    }

    #[test]
    fn a_lupa_acompanha_o_pincel_e_o_que_mudou_enquanto_era_montada() {
        let mut s = sessao();
        let pedido = s.pedir_lupa(Retangulo::novo(0, 0, 400, 300), 1);
        // Pinta enquanto a lupa é montada "em outra thread".
        s.pincel.cor = [255, 255, 0];
        s.pincel.dureza = 1.0;
        s.apertar(100.0, 100.0);
        s.soltar();
        let (id, lupa) = pedido.montar();
        assert_eq!(
            lupa.imagem().get_pixel(100, 100).0,
            [100, 100, 50],
            "montada antes"
        );
        assert!(s.receber_lupa(id, lupa));
        assert_eq!(
            s.lupa().unwrap().imagem().get_pixel(100, 100).0,
            [255, 255, 0]
        );

        // Um pedido velho não entra por cima do novo.
        let velho = s.pedir_lupa(Retangulo::novo(0, 0, 100, 100), 1);
        let novo = s.pedir_lupa(Retangulo::novo(0, 0, 200, 200), 1);
        let (id_velho, lupa_velha) = velho.montar();
        assert!(!s.receber_lupa(id_velho, lupa_velha));
        let (id, lupa) = novo.montar();
        assert!(s.receber_lupa(id, lupa));

        s.apertar(150.0, 150.0);
        s.soltar();
        assert_eq!(
            s.lupa().unwrap().imagem().get_pixel(150, 150).0,
            [255, 255, 0]
        );
        s.largar_lupa();
        assert!(s.lupa().is_none());
    }

    #[test]
    fn o_pincel_nao_passa_da_selecao() {
        let mut s = sessao();
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(100, 100, 100, 100)),
            Operacao::Nova,
        );
        s.pincel.cor = [255, 0, 0];
        s.pincel.dureza = 1.0;
        s.apertar(50.0, 150.0);
        s.arrastar(300.0, 150.0);
        s.soltar();
        let c = &s.documento().camadas[0].pixels;
        assert_eq!(c.pixel(150, 150)[3], 255);
        assert_eq!(c.pixel(90, 150)[3], 0, "à esquerda da seleção");
        assert_eq!(c.pixel(210, 150)[3], 0, "à direita");

        // Um clique sem área desmarca.
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(10, 10, 0, 0)),
            Operacao::Nova,
        );
        assert!(s.selecao().is_none());
    }

    #[test]
    fn apagar_e_preencher_a_selecao_entram_no_desfazer() {
        let mut s = sessao();
        s.pincel.cor = [0, 0, 255];
        assert!(s.preencher_selecao(), "sem seleção, a camada toda");
        assert_eq!(s.compor().get_pixel(700, 500).0, [0, 0, 255]);
        s.selecionar(
            &Forma::Elipse(Retangulo::novo(200, 200, 200, 200)),
            Operacao::Nova,
        );
        s.inverter_selecao();
        assert!(s.apagar_selecao());
        assert_eq!(
            s.compor().get_pixel(300, 300).0,
            [0, 0, 255],
            "o meio ficou"
        );
        assert_eq!(
            s.compor().get_pixel(10, 10).0,
            [10, 10, 50],
            "fora foi apagado"
        );
        s.desfazer();
        assert_eq!(s.compor().get_pixel(10, 10).0, [0, 0, 255]);
        s.desmarcar();
        assert!(!s.apagar_selecao(), "Delete sem seleção não faz nada");
    }

    #[test]
    fn mesclar_para_baixo_e_desfazer() {
        let mut s = sessao();
        s.pincel.cor = [200, 200, 0];
        s.apertar(100.0, 100.0);
        s.soltar();
        s.nova_camada();
        s.pincel.cor = [0, 0, 0];
        s.apertar(110.0, 100.0);
        s.soltar();
        s.mudar_modo(Modo::Multiplicacao);
        let antes = s.compor();
        let pilha = s.documento().clone();
        assert!(s.mesclar_para_baixo().is_ok());
        assert_eq!(s.documento().camadas.len(), 1);
        assert_eq!(s.ativa(), 0);
        for (a, b) in antes.as_raw().iter().zip(s.compor().as_raw()) {
            assert!((*a as i32 - *b as i32).abs() <= 1);
        }
        assert_eq!(
            s.historico().a_desfazer().unwrap().descricao(s.documento()),
            "Mesclar Camada 1 para baixo"
        );
        s.desfazer();
        assert_eq!(s.documento(), &pilha);
        assert_eq!(s.ativa(), 1);
        s.escolher_camada(0);
        assert!(s.mesclar_para_baixo().is_err(), "a de baixo de todas");
    }

    #[test]
    fn a_visibilidade_se_desfaz_e_a_vista_acompanha() {
        let mut s = sessao();
        s.pincel.cor = [0, 0, 255];
        s.apertar(400.0, 300.0);
        s.soltar();
        let pintada = s.vista().imagem().clone();
        s.alternar_visibilidade();
        assert_ne!(s.vista().imagem().as_raw(), pintada.as_raw());
        s.desfazer();
        assert_eq!(s.vista().imagem().as_raw(), pintada.as_raw());
    }
}
