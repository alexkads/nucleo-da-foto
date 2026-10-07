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

use crate::ajuste::Ajuste;
use crate::composicao;
use crate::documento::{Camada, Documento, Mascara};
use crate::historico::{Comando, Historico};
use crate::mesclagem::Modo;
use crate::operacoes;
use crate::pincel::Mudanca;
use crate::pincel::{Pincel, Traco};
use crate::retangulo::Retangulo;
use crate::selecao::{Acabamento, Amostra, Forma, Molde, Operacao, Selecao};
use crate::tiles::CamadaDePixels;
use crate::transformar::{self, Conteudo, Transformacao};
use crate::vista::Vista;

/// A tolerância da lata de tinta — o padrão do Photoshop.
pub const TOLERANCIA_DA_LATA: u8 = 32;

/// As opções do carimbo (S) na barra do Photoshop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcoesDoCarimbo {
    /// Alinhado (o padrão): o primeiro traço depois de escolher a origem fixa
    /// a distância, e os seguintes copiam à mesma distância. Desligado, cada
    /// traço novo volta a copiar da origem escolhida.
    pub alinhado: bool,
    pub amostra: crate::carimbo::AmostraDoCarimbo,
}

impl Default for OpcoesDoCarimbo {
    fn default() -> Self {
        Self {
            alinhado: true,
            amostra: crate::carimbo::AmostraDoCarimbo::AtualEAbaixo,
        }
    }
}

/// De onde a varinha mágica lê a cor ("Amostrar todas as camadas" do
/// Photoshop, desligado ou ligado).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AmostraDaVarinha {
    /// Só os pixels da camada escolhida (ou da máscara dela, se é a máscara
    /// que está escolhida) — sem a fotografia base nem as outras camadas; o
    /// transparente conta como cor.
    CamadaAtual,
    /// A foto como aparece: a base e todas as camadas visíveis.
    #[default]
    Todas,
}

/// As opções da varinha mágica (W) na barra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcoesDaVarinha {
    /// 0–255 por canal (32 no Photoshop).
    pub tolerancia: u8,
    pub contigua: bool,
    /// Antisserrilhado na borda do que ela pega.
    pub suavizar: bool,
    pub amostra: AmostraDaVarinha,
}

impl Default for OpcoesDaVarinha {
    fn default() -> Self {
        Self {
            tolerancia: 32,
            contigua: true,
            suavizar: true,
            amostra: AmostraDaVarinha::Todas,
        }
    }
}

/// Por que a varinha não pegou nada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarinhaRecusada {
    /// O clique caiu fora da foto.
    ForaDaFoto,
    /// "Camada atual" numa camada de ajuste sem a máscara escolhida: não há
    /// pixel para ler.
    CamadaSemPixels,
    /// "Camada atual" com a camada escondida (o Photoshop também recusa).
    CamadaEscondida,
}

/// A seleção solta para o "Transformar seleção": a de antes, o molde dela e
/// a transformação de agora. Nenhum pixel muda.
struct SelecaoSolta {
    antes: Arc<Selecao>,
    molde: Molde,
    t: Transformacao,
}

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
    /// Alinhado e amostra do carimbo — opções da ferramenta, fora do desfazer.
    pub carimbo: OpcoesDoCarimbo,
    /// Pontos da tela por pixel da foto, o zoom de agora — a suavização do
    /// pincel é medida na tela (ver `Traco::com_cordao`). A janela atualiza.
    pub escala_da_tela: f32,
    ativa: usize,
    /// Pinta na máscara da escolhida, e não nos pixels dela (a miniatura da
    /// máscara clicada, como no Photoshop).
    na_mascara: bool,
    traco: Option<Traco>,
    /// A camada e a opacidade dela quando o arrasto do slider começou — o
    /// passo do desfazer é o arrasto inteiro, e não cada valor do caminho.
    opacidade_antes: Option<(usize, f32)>,
    /// O ajuste da camada quando o arrasto de um slider dele começou.
    ajuste_antes: Option<(usize, Ajuste)>,
    /// Onde a vista está em rascunho (o arrasto do ajuste) — refeita exata
    /// ao soltar.
    rascunho: Retangulo,
    /// O letreiro. `Arc`: o traço leva uma cópia barata.
    selecao: Option<Arc<Selecao>>,
    /// Sobe a cada mudança de pixel ou de seleção — quem desenha miniaturas e
    /// bordas sabe quando refazer.
    versao: u64,
    /// Sobe só quando a seleção muda — a borda dela não se refaz a cada
    /// pincelada.
    versao_da_selecao: u64,
    /// A origem do carimbo (⌥+clique), em pixels da foto.
    origem: Option<(f32, f32)>,
    /// Alinhado (o padrão do Photoshop): o primeiro traço depois de escolher a
    /// origem fixa a distância, e os seguintes copiam à mesma distância.
    distancia_do_carimbo: Option<(f32, f32)>,
    /// A camada (e se é a máscara dela) antes de o arrasto do Mover começar.
    movendo: Option<(usize, bool, crate::tiles::CamadaDePixels)>,
    /// O conteúdo solto da camada (⌘T, ou o Mover com seleção).
    flutuante: Option<Flutuante>,
    /// A seleção de quando o arrasto do contorno começou (arrastar por dentro
    /// dela com uma ferramenta de seleção move só o contorno).
    contorno_movendo: Option<Arc<Selecao>>,
    /// Onde o último traço terminou — ⇧ + clique liga até ali com uma reta.
    fim_do_ultimo_traco: Option<(f32, f32)>,
    /// "Transformar seleção" em curso: só o contorno, sem os pixels.
    selecao_solta: Option<SelecaoSolta>,
}

/// O conteúdo de uma camada tirado dela para ser transformado.
struct Flutuante {
    camada: usize,
    na_mascara: bool,
    /// A camada como era — para o desfazer e para cancelar.
    original: CamadaDePixels,
    /// O que fica na camada sem o conteúdo.
    fundo: CamadaDePixels,
    conteudo: Conteudo,
    /// A seleção de quando começou (o Mover a leva junto).
    selecao: Option<Arc<Selecao>>,
    t: Transformacao,
    /// Onde a camada mudou até agora (para a vista).
    area: Retangulo,
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
            carimbo: OpcoesDoCarimbo::default(),
            escala_da_tela: 1.0,
            ativa,
            na_mascara: false,
            traco: None,
            opacidade_antes: None,
            ajuste_antes: None,
            rascunho: Retangulo::default(),
            selecao: None,
            versao: 0,
            versao_da_selecao: 0,
            origem: None,
            distancia_do_carimbo: None,
            movendo: None,
            flutuante: None,
            contorno_movendo: None,
            fim_do_ultimo_traco: None,
            selecao_solta: None,
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
        self.confirmar_ajuste();
        self.terminar_de_mover();
        self.aplicar_transformacao();
        self.terminar_de_mover_o_contorno();
    }

    /// Escolhe a camada — os pixels dela, e não a máscara.
    pub fn escolher_camada(&mut self, indice: usize) {
        if indice < self.doc.camadas.len() && (indice != self.ativa() || self.na_mascara) {
            self.fechar_o_que_esta_aberto();
            self.ativa = indice;
            self.na_mascara = false;
        }
        // A camada de ajuste pinta na máscara.
        self.cores_em_cinza_na_mascara();
    }

    /// Escolhe a máscara da camada `indice` para pintar. Falso sem máscara.
    pub fn escolher_mascara(&mut self, indice: usize) -> bool {
        if self
            .doc
            .camadas
            .get(indice)
            .is_none_or(|c| c.mascara.is_none())
        {
            return false;
        }
        if indice != self.ativa() || !self.na_mascara {
            self.fechar_o_que_esta_aberto();
            self.ativa = indice;
            self.na_mascara = true;
        }
        self.cores_em_cinza_na_mascara();
        true
    }

    /// X: a cor de frente e a de fundo trocam de lugar.
    pub fn trocar_cores(&mut self) {
        std::mem::swap(&mut self.pincel.cor, &mut self.pincel.cor_de_fundo);
    }

    /// D: preto na frente, branco no fundo.
    pub fn cores_padrao(&mut self) {
        self.pincel.cor = [0; 3];
        self.pincel.cor_de_fundo = [255; 3];
    }

    /// Na máscara só valem cinzas: como o Photoshop, as duas cores viram o
    /// cinza delas quando o pincel vai para a máscara — o quadrado da cor
    /// mostra o que vai ser pintado, e não um vermelho que pinta 30% de cinza.
    fn cores_em_cinza_na_mascara(&mut self) {
        if !self.na_mascara() {
            return;
        }
        let cinza = |c: [u8; 3]| {
            let v = ((77 * c[0] as u32 + 150 * c[1] as u32 + 29 * c[2] as u32 + 128) >> 8) as u8;
            [v; 3]
        };
        self.pincel.cor = cinza(self.pincel.cor);
        self.pincel.cor_de_fundo = cinza(self.pincel.cor_de_fundo);
    }

    /// O pincel está pintando na máscara da escolhida. Numa camada de ajuste,
    /// sempre: ela não tem pixels.
    pub fn na_mascara(&self) -> bool {
        let camada = self.camada_ativa();
        (self.na_mascara || camada.ajuste.is_some()) && camada.mascara.is_some()
    }

    /// Há onde pintar na escolhida: visível, e com máscara se for de ajuste.
    fn pode_pintar(&self) -> bool {
        let camada = self.camada_ativa();
        camada.visivel && (camada.ajuste.is_none() || camada.mascara.is_some())
    }

    // ------------------------------------------------------------ ajuste

    /// Uma camada de ajuste logo acima da escolhida, com a máscara branca —
    /// ou, com seleção, a máscara dela. "Níveis 1", "Níveis 2"…
    pub fn nova_camada_de_ajuste(&mut self, ajuste: Ajuste) {
        self.fechar_o_que_esta_aberto();
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let prefixo = format!("{} ", ajuste.nome());
        let maior = self
            .doc
            .camadas
            .iter()
            .filter_map(|c| c.nome.strip_prefix(&prefixo)?.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        let mut camada =
            Camada::de_ajuste(&format!("{prefixo}{}", maior + 1), ajuste, largura, altura);
        if let Some(selecao) = self.selecao.as_deref() {
            let mut m = Mascara::nova(0, largura, altura);
            operacoes::preencher(&mut m.pixels, Some(selecao), [255; 3]);
            camada.mascara = Some(m);
        }
        let indice = (self.ativa() + 1).min(self.doc.camadas.len());
        self.executar(Comando::CriarCamada {
            indice,
            camada: Box::new(camada),
        });
        self.cores_em_cinza_na_mascara();
    }

    /// Um slider do ajuste da escolhida andou: a foto muda na hora, o
    /// histórico só no [`Self::confirmar_ajuste`].
    pub fn mover_ajuste(&mut self, ajuste: Ajuste) {
        self.soltar();
        let indice = self.ativa();
        let Some(atual) = self.doc.camadas[indice].ajuste else {
            return;
        };
        if self.ajuste_antes.is_some_and(|(i, _)| i != indice) {
            self.confirmar_ajuste();
        }
        let ajuste = ajuste.limitado();
        if std::mem::discriminant(&ajuste) != std::mem::discriminant(&atual) {
            return;
        }
        self.ajuste_antes.get_or_insert((indice, atual));
        if atual == ajuste {
            return;
        }
        self.doc.camadas[indice].ajuste = Some(ajuste);
        let area = self.doc.camadas[indice].area();
        // Em rascunho durante o arrasto: o ajuste muda a foto inteira.
        self.versao += 1;
        self.vista.rascunhar(&self.base, &self.doc, &area);
        if let Some(lupa) = self.lupa.as_mut() {
            lupa.rascunhar(&self.base, &self.doc, &area);
        }
        if let Some((_, desde)) = self.lupa_pedida.as_mut() {
            *desde = desde.uniao(&area);
        }
        self.rascunho = self.rascunho.uniao(&area);
    }

    /// O arrasto do slider do ajuste acabou.
    pub fn confirmar_ajuste(&mut self) {
        let Some((camada, antes)) = self.ajuste_antes.take() else {
            return;
        };
        let rascunho = std::mem::take(&mut self.rascunho);
        self.refazer_a_vista(&rascunho);
        let Some(depois) = self.doc.camadas.get(camada).and_then(|c| c.ajuste) else {
            return;
        };
        if antes != depois {
            self.hist.registrar(Comando::Ajuste {
                camada,
                antes,
                depois,
            });
        }
    }

    /// Aplica e registra um passo, e leva a escolha para onde ele mexeu.
    fn executar(&mut self, comando: Comando) {
        let sujo = comando.aplicar(&mut self.doc, true);
        self.seguir_o_passo(&comando, true);
        self.hist.registrar(comando);
        self.refazer_a_vista(&sujo);
    }

    /// A escolha vai para onde o passo mexeu — a camada, e a máscara dela
    /// quando o traço foi lá.
    fn seguir_o_passo(&mut self, passo: &Comando, para_frente: bool) {
        match passo {
            Comando::Selecao { antes, depois, .. } => {
                self.selecao = if para_frente { depois } else { antes }.clone();
                self.versao += 1;
                self.versao_da_selecao += 1;
                return;
            }
            Comando::Varios { passos, .. } => {
                if para_frente {
                    passos.iter().for_each(|p| self.seguir_o_passo(p, true));
                } else {
                    passos
                        .iter()
                        .rev()
                        .for_each(|p| self.seguir_o_passo(p, false));
                }
                return;
            }
            _ => {}
        }
        if let Some(i) = passo.camada_depois(para_frente, self.doc.camadas.len()) {
            if i != self.ativa {
                self.na_mascara = false;
            }
            self.ativa = i;
        }
        match passo {
            Comando::Traco { na_mascara, .. } => self.na_mascara = *na_mascara,
            Comando::Mascara { antes, depois, .. } => {
                let fica = if para_frente { depois } else { antes };
                let entrou = if para_frente { antes } else { depois }.is_none();
                if fica.is_none() {
                    self.na_mascara = false;
                } else if entrou {
                    self.na_mascara = true;
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ máscara

    /// O botão da máscara: uma máscara que revela tudo (ou, com `esconder`
    /// — o ⌥ —, que esconde tudo) na escolhida, que passa a receber o pincel.
    /// 🔑 Com seleção, a máscara já nasce dela: revela o selecionado (com ⌥,
    /// esconde), como no Photoshop. Falso se a camada já tem máscara.
    pub fn adicionar_mascara(&mut self, esconder: bool) -> bool {
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        if self.doc.camadas[camada].mascara.is_some() {
            return false;
        }
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let mascara = match self.selecao.as_deref() {
            None => Mascara::nova(if esconder { 0 } else { 255 }, largura, altura),
            Some(selecao) => {
                let mut m = Mascara::nova(if esconder { 255 } else { 0 }, largura, altura);
                let cor = if esconder { [0; 3] } else { [255; 3] };
                operacoes::preencher(&mut m.pixels, Some(selecao), cor);
                m
            }
        };
        self.executar(Comando::Mascara {
            camada,
            antes: None,
            depois: Some(Box::new(mascara)),
        });
        self.cores_em_cinza_na_mascara();
        true
    }

    /// Tira a máscara da escolhida (a lixeira com a máscara escolhida).
    pub fn excluir_mascara(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        let Some(antes) = self.doc.camadas[camada].mascara.clone() else {
            return false;
        };
        self.executar(Comando::Mascara {
            camada,
            antes: Some(Box::new(antes)),
            depois: None,
        });
        true
    }

    /// ⇧ + clique na miniatura da máscara: liga ou desliga.
    pub fn alternar_mascara_de(&mut self, indice: usize) -> bool {
        self.fechar_o_que_esta_aberto();
        let Some(antes) = self.doc.camadas.get(indice).and_then(|c| c.mascara.clone()) else {
            return false;
        };
        let mut depois = antes.clone();
        depois.ativa = !antes.ativa;
        let comando = Comando::Mascara {
            camada: indice,
            antes: Some(Box::new(antes)),
            depois: Some(Box::new(depois)),
        };
        // Como a visibilidade: não muda o que está escolhido.
        let sujo = comando.aplicar(&mut self.doc, true);
        self.hist.registrar(comando);
        self.refazer_a_vista(&sujo);
        true
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

    // ---------------------------------------------------- carimbo e cor

    /// ⌥+clique com o carimbo: daqui ele copia. O próximo traço fixa a
    /// distância (alinhado).
    pub fn definir_origem(&mut self, x: f32, y: f32) {
        self.origem = Some((x, y));
        self.distancia_do_carimbo = None;
    }

    pub fn origem(&self) -> Option<(f32, f32)> {
        self.origem
    }

    /// De onde o carimbo copia quando o ponteiro está em `(x, y)` — a mira que
    /// a tela desenha. Antes do primeiro traço, é a própria origem.
    pub fn mira_do_carimbo(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        // Sem alinhar, fora de um traço a mira volta à origem.
        if !self.carimbo.alinhado && self.traco.is_none() {
            return self.origem;
        }
        match (self.origem, self.distancia_do_carimbo) {
            (_, Some(d)) => Some((x + d.0, y + d.1)),
            (Some(o), None) => Some(o),
            _ => None,
        }
    }

    /// A prévia da origem com o ponteiro em `(x, y)` e o pincel de `raio`: o
    /// quadrado do destino em volta do ponteiro (pixels da foto) e o que o
    /// carimbo copiaria para ele, com a amostra escolhida. `None` sem origem.
    pub fn previa_do_carimbo(
        &self,
        x: f32,
        y: f32,
        raio: f32,
    ) -> Option<(Retangulo, image::RgbaImage)> {
        let (mx, my) = self.mira_do_carimbo(x, y)?;
        let r = raio.max(1.0);
        let (l, a) = (self.doc.largura() as f32, self.doc.altura() as f32);
        let x0 = (x - r).floor().clamp(0.0, l);
        let y0 = (y - r).floor().clamp(0.0, a);
        let x1 = (x + r).ceil().clamp(0.0, l);
        let y1 = (y + r).ceil().clamp(0.0, a);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let ret = Retangulo::novo(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
        let mut fonte = crate::carimbo::Fonte::da_amostra(
            self.base.clone(),
            &self.doc,
            self.ativa(),
            self.carimbo.amostra,
            (mx - x, my - y),
        );
        Some((ret, fonte.recorte(&ret)))
    }

    /// O conta-gotas: a cor da foto como ela aparece em `(x, y)`.
    pub fn cor_em(&self, x: f32, y: f32) -> Option<[u8; 3]> {
        let (l, a) = (self.doc.largura() as f32, self.doc.altura() as f32);
        if x < 0.0 || y < 0.0 || x >= l || y >= a {
            return None;
        }
        let ret = Retangulo::novo(x as u32, y as u32, 1, 1);
        Some(
            composicao::compor_recorte(&self.base, &self.doc, &ret)
                .get_pixel(0, 0)
                .0,
        )
    }

    // ------------------------------------------- preenchimento por conteúdo

    /// A foto como ela aparece até a camada escolhida (inclusive), no recorte
    /// `ret` — a fonte do preenchimento por conteúdo e do pincel de correção,
    /// como a do carimbo.
    pub fn foto_ate_a_ativa(&self, ret: &Retangulo) -> RgbImage {
        let mut doc = self.doc.clone();
        doc.camadas.truncate(self.ativa() + 1);
        composicao::compor_recorte(&self.base, &doc, ret)
    }

    /// O documento (pixels, pilha, máscaras ou seleção) mudou desde a
    /// `versao` — o remendo calculado sobre ela já não serve.
    pub fn mudou_desde(&self, versao: u64) -> bool {
        self.versao != versao
    }

    /// O Preenchimento sensível ao conteúdo confirmado: o remendo entra numa
    /// **camada de retoque nova**, logo acima da escolhida — criar a camada e
    /// pintar o remendo nela são **um** passo do desfazer. Só os tiles que o
    /// remendo toca existem nela; a visibilidade, a opacidade e a máscara dela
    /// ficam para ajustar depois. `em_camada_nova = false` cola na escolhida
    /// (só numa camada de pixels).
    ///
    /// 🚨 Recusa se o documento mudou desde `versao` (a do instantâneo de
    /// onde o remendo saiu): aplicar um remendo calculado sobre outra versão
    /// em silêncio desalinharia o retoque.
    pub fn aplicar_preenchimento(
        &mut self,
        versao: u64,
        ret: &Retangulo,
        rgba: &[u8],
        peso: &dyn Fn(u32, u32) -> u8,
        em_camada_nova: bool,
    ) -> Result<(), &'static str> {
        if self.mudou_desde(versao) {
            return Err("A foto mudou enquanto o preenchimento era calculado — refaça a prévia");
        }
        self.fechar_o_que_esta_aberto();
        let ativa = self.ativa();
        if !em_camada_nova {
            if self.doc.camadas[ativa].ajuste.is_some() || self.na_mascara() {
                return Err("O preenchimento vai nos pixels: escolha uma camada de pixels, ou use uma camada nova");
            }
            return if self.colar_remendo(ativa, ret, rgba, peso) {
                Ok(())
            } else {
                Err("O remendo não mudou nada")
            };
        }
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let prefixo = "Preenchimento ";
        let maior = self
            .doc
            .camadas
            .iter()
            .filter_map(|c| c.nome.strip_prefix(prefixo)?.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        let mut camada = Camada::nova(&format!("{prefixo}{}", maior + 1), largura, altura);
        if operacoes::colar(&mut camada.pixels, ret, rgba, peso).is_none() {
            return Err("O remendo não mudou nada");
        }
        self.executar(Comando::CriarCamada {
            indice: ativa + 1,
            camada: Box::new(camada),
        });
        Ok(())
    }

    /// Cola o remendo na camada `camada` (a que estava escolhida quando ele
    /// foi pedido) — um passo do desfazer. Falso quando não mudou nada, ou a
    /// camada já não existe.
    pub fn colar_remendo(
        &mut self,
        camada: usize,
        ret: &Retangulo,
        rgba: &[u8],
        peso: &dyn Fn(u32, u32) -> u8,
    ) -> bool {
        self.fechar_o_que_esta_aberto();
        if camada >= self.doc.camadas.len() {
            return false;
        }
        let mudanca = operacoes::colar(&mut self.doc.camadas[camada].pixels, ret, rgba, peso);
        self.registrar_mudanca(camada, false, mudanca)
    }

    // ------------------------------------------------------------ mover

    /// O arrasto do Mover começou: a camada escolhida de agora é a referência.
    /// Escondida não anda (como o pincel). Com seleção, anda só o que está
    /// selecionado — e a seleção vai junto.
    pub fn comecar_a_mover(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let ativa = self.ativa();
        if !self.pode_pintar() {
            return false;
        }
        if self.selecao.is_some() {
            return self.comecar_a_transformar();
        }
        let na_mascara = self.na_mascara();
        self.movendo = Some((
            ativa,
            na_mascara,
            self.doc.camadas[ativa].alvo(na_mascara).clone(),
        ));
        true
    }

    /// A camada deslocada `(dx, dy)` do começo, na hora.
    pub fn mover_por(&mut self, dx: i64, dy: i64) {
        if self.flutuante.is_some() {
            self.definir_transformacao(Transformacao::deslocamento(dx as f32, dy as f32));
            return;
        }
        let Some((camada, na_mascara, original)) = self.movendo.as_ref() else {
            return;
        };
        let (camada, na_mascara) = (*camada, *na_mascara);
        let nova = operacoes::deslocada(original, dx, dy);
        let antes = self.doc.camadas[camada].area();
        *self.doc.camadas[camada].alvo_mut(na_mascara) = nova;
        let sujo = antes.uniao(&self.doc.camadas[camada].area());
        self.refazer_a_vista(&sujo);
    }

    /// O arrasto acabou: o deslocamento vira um passo do desfazer.
    pub fn terminar_de_mover(&mut self) -> bool {
        if self.flutuante.is_some() {
            return self.aplicar_transformacao();
        }
        let Some((camada, na_mascara, original)) = self.movendo.take() else {
            return false;
        };
        let mudanca = operacoes::diferenca(&original, self.doc.camadas[camada].alvo(na_mascara));
        match mudanca {
            Some(m) => {
                self.hist.registrar(Comando::Traco {
                    camada,
                    na_mascara,
                    mudanca: m,
                });
                true
            }
            None => false,
        }
    }

    pub fn movendo(&self) -> bool {
        self.movendo.is_some()
    }

    // ------------------------------------------------- transformação livre

    /// ⌘T: solta o conteúdo da camada escolhida (ou só o selecionado). Falso
    /// com a camada escondida ou vazia.
    pub fn comecar_a_transformar(&mut self) -> bool {
        self.soltar();
        self.confirmar_opacidade();
        if self.flutuante.is_some() || self.selecao_solta.is_some() {
            return true;
        }
        let camada = self.ativa();
        if !self.pode_pintar() {
            return false;
        }
        let na_mascara = self.na_mascara();
        let original = self.doc.camadas[camada].alvo(na_mascara).clone();
        let selecao = self.selecao.clone();
        let Some(conteudo) = Conteudo::da_camada(&original, selecao.as_deref()) else {
            return false;
        };
        let mut fundo = original.clone();
        match selecao.as_deref() {
            Some(s) => {
                operacoes::apagar(&mut fundo, s);
            }
            None => fundo = CamadaDePixels::nova(original.largura(), original.altura()),
        }
        let area = conteudo.caixa;
        self.flutuante = Some(Flutuante {
            camada,
            na_mascara,
            original,
            fundo,
            conteudo,
            selecao,
            t: Transformacao::default(),
            area,
        });
        true
    }

    /// "Transformar seleção": a caixa aparece em volta do selecionado e só o
    /// contorno se transforma — nenhum pixel muda (o ⌘T é o dos pixels).
    /// Falso sem seleção.
    pub fn comecar_a_transformar_a_selecao(&mut self) -> bool {
        if self.selecao_solta.is_some() {
            return true;
        }
        self.fechar_o_que_esta_aberto();
        let Some(antes) = self.selecao.clone() else {
            return false;
        };
        let molde = Molde::de(&antes);
        if molde.caixa().vazio() {
            return false;
        }
        self.selecao_solta = Some(SelecaoSolta {
            antes,
            molde,
            t: Transformacao::default(),
        });
        true
    }

    /// O "Transformar seleção" está aberto (e não o ⌘T dos pixels).
    pub fn transformando_a_selecao(&self) -> bool {
        self.selecao_solta.is_some()
    }

    /// A caixa do conteúdo e a transformação de agora — o que a tela desenha.
    pub fn transformacao(&self) -> Option<(Retangulo, Transformacao)> {
        if let Some(s) = &self.selecao_solta {
            return Some((s.molde.caixa(), s.t));
        }
        self.flutuante.as_ref().map(|f| (f.conteudo.caixa, f.t))
    }

    /// A camada passa a mostrar o conteúdo transformado por `t` (ou a
    /// seleção, no "Transformar seleção").
    pub fn definir_transformacao(&mut self, t: Transformacao) {
        if let Some(solta) = self.selecao_solta.as_mut() {
            if solta.t == t {
                return;
            }
            solta.t = t;
            let caixa = solta.molde.caixa();
            let (l, a) = (self.doc.largura(), self.doc.altura());
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for (x, y) in t.cantos(&caixa) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
            let x0 = (x0.floor() - 1.0).max(0.0) as u32;
            let y0 = (y0.floor() - 1.0).max(0.0) as u32;
            let x1 = ((x1.ceil() + 1.0).max(0.0) as u32).min(l);
            let y1 = ((y1.ceil() + 1.0).max(0.0) as u32).min(a);
            let destino = Retangulo::novo(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0));
            // A inversa é afim: três coeficientes, e uma soma por pixel.
            let c = t.inversa(&caixa, 0.0, 0.0);
            let ex = t.inversa(&caixa, 1.0, 0.0);
            let ey = t.inversa(&caixa, 0.0, 1.0);
            let nova = solta.molde.transformado(
                &destino,
                c,
                (ex.0 - c.0, ex.1 - c.1),
                (ey.0 - c.0, ey.1 - c.1),
            );
            self.selecao = (!nova.nada()).then(|| Arc::new(nova));
            self.versao += 1;
            self.versao_da_selecao += 1;
            return;
        }
        let Some(f) = self.flutuante.as_mut() else {
            return;
        };
        if f.t == t {
            return;
        }
        f.t = t;
        let (largura, altura) = (f.original.largura(), f.original.altura());
        let desenhado = transformar::desenhar(&f.conteudo, &t, largura, altura);
        let nova = transformar::sobre(&f.fundo, &desenhado);
        let area_nova = desenhado
            .existentes()
            .fold(Retangulo::default(), |a, (p, _)| {
                a.uniao(&crate::tiles::retangulo_do_tile(*p, largura, altura))
            });
        let sujo = f.area.uniao(&area_nova);
        f.area = area_nova.uniao(&f.conteudo.caixa);
        let (camada, na_mascara) = (f.camada, f.na_mascara);
        *self.doc.camadas[camada].alvo_mut(na_mascara) = nova;
        self.refazer_a_vista(&sujo);
    }

    /// Enter: a transformação vira um passo do desfazer. A seleção anda junto
    /// num deslocamento; com escala ou giro, sai (o recorte já não é o mesmo).
    pub fn aplicar_transformacao(&mut self) -> bool {
        if let Some(solta) = self.selecao_solta.take() {
            // Um passo de seleção: não muda pixel, não pede para salvar.
            return match self.passo_da_selecao(Some(solta.antes), "Transformar seleção") {
                Some(passo) => {
                    self.hist.registrar(passo);
                    true
                }
                None => false,
            };
        }
        let Some(f) = self.flutuante.take() else {
            return false;
        };
        let selecao_antes = self.selecao.clone();
        if f.selecao.is_some() {
            self.selecao = if f.t.so_desloca() {
                f.selecao
                    .as_deref()
                    .map(|s| Arc::new(s.deslocada(f.t.dx.round() as i64, f.t.dy.round() as i64)))
            } else {
                None
            };
            self.versao += 1;
            self.versao_da_selecao += 1;
        }
        // 🔑 Um passo só: os pixels e a seleção que foi junto voltam no mesmo
        // desfazer.
        let pixels =
            operacoes::diferenca(&f.original, self.doc.camadas[f.camada].alvo(f.na_mascara)).map(
                |m| Comando::Traco {
                    camada: f.camada,
                    na_mascara: f.na_mascara,
                    mudanca: m,
                },
            );
        let selecao = self.passo_da_selecao(selecao_antes, "Mover seleção");
        let nome = if f.t.so_desloca() {
            "Mover"
        } else {
            "Transformação livre"
        };
        match (pixels, selecao) {
            // Com o nome do gesto no Histórico ("Mover", "Transformação
            // livre"), e não o do traço que ele grava ("Pincel").
            (Some(p), None) => self.hist.registrar(Comando::Varios {
                nome: nome.into(),
                passos: vec![p],
            }),
            (Some(p), Some(s)) => self.hist.registrar(Comando::Varios {
                nome: nome.into(),
                passos: vec![p, s],
            }),
            (None, Some(s)) => self.hist.registrar(s),
            (None, None) => return false,
        }
        true
    }

    /// Esc: a camada volta a ser o que era.
    pub fn cancelar_transformacao(&mut self) {
        if let Some(solta) = self.selecao_solta.take() {
            self.selecao = Some(solta.antes);
            self.versao += 1;
            self.versao_da_selecao += 1;
            return;
        }
        let Some(f) = self.flutuante.take() else {
            return;
        };
        *self.doc.camadas[f.camada].alvo_mut(f.na_mascara) = f.original;
        self.refazer_a_vista(&f.area);
    }

    pub fn transformando(&self) -> bool {
        self.flutuante.is_some() || self.selecao_solta.is_some()
    }

    /// ⌘J com seleção: uma camada nova só com o selecionado, logo acima. Sem
    /// seleção, duplica a camada inteira. ⇧⌘J (`recortar`) tira o pedaço da
    /// de origem — num segundo passo do desfazer.
    pub fn camada_via_copia(&mut self, recortar: bool) -> bool {
        self.fechar_o_que_esta_aberto();
        let Some(selecao) = self.selecao.clone() else {
            if recortar {
                return false;
            }
            self.duplicar_camada();
            return true;
        };
        let origem = self.ativa();
        let original = self.doc.camadas[origem].pixels.clone();
        let Some(conteudo) = Conteudo::da_camada(&original, Some(&selecao)) else {
            return false;
        };
        let pixels = transformar::desenhar(
            &conteudo,
            &Transformacao::default(),
            original.largura(),
            original.altura(),
        );
        let mut camada = Camada::nova(
            &self.doc.proximo_nome(),
            original.largura(),
            original.altura(),
        );
        camada.pixels = pixels;
        let criar = Comando::CriarCamada {
            indice: origem + 1,
            camada: Box::new(camada),
        };
        if !recortar {
            self.executar(criar);
            return true;
        }
        // ⇧⌘J é **um** passo, como no Photoshop ("Camada via recorte"): um
        // desfazer devolve o pedaço à origem e tira a camada nova juntos.
        let mut fonte = self.doc.camadas[origem].pixels.clone();
        let passos = match operacoes::apagar(&mut fonte, &selecao) {
            Some(mudanca) => vec![
                Comando::Traco {
                    camada: origem,
                    na_mascara: false,
                    mudanca,
                },
                criar,
            ],
            None => vec![criar],
        };
        self.executar(Comando::Varios {
            nome: "Camada via recorte".into(),
            passos,
        });
        true
    }

    // ------------------------------------------------------------- seleção

    pub fn versao_da_selecao(&self) -> u64 {
        self.versao_da_selecao
    }

    pub fn selecao(&self) -> Option<&Selecao> {
        self.selecao.as_deref()
    }

    /// Uma forma desenhada entra na seleção. Nova sem nada selecionado no fim
    /// (um clique) desmarca, como no Photoshop.
    pub fn selecionar(&mut self, forma: &Forma, operacao: Operacao) {
        self.selecionar_com(forma, operacao, Acabamento::default());
    }

    /// Uma forma desenhada entra na seleção com o acabamento da ferramenta
    /// (antisserrilhado, difusão) — a forma ganha o acabamento **antes** de
    /// se juntar à seleção de agora; a de agora não muda por isso.
    pub fn selecionar_com(&mut self, forma: &Forma, operacao: Operacao, acabamento: Acabamento) {
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let nome = match forma {
            Forma::Retangulo(_) => "Seleção retangular",
            Forma::Elipse(_) | Forma::ElipseNaCaixa(..) => "Seleção elíptica",
            Forma::Laco(_) => "Laço",
        };
        let nova = Selecao::da_forma_com(largura, altura, forma, acabamento);
        self.entrar_na_selecao(nova, operacao, nome);
    }

    /// O laço poligonal concluído: um passo só, com o nome do Photoshop.
    pub fn selecionar_poligono(
        &mut self,
        vertices: Vec<(f32, f32)>,
        operacao: Operacao,
        acabamento: Acabamento,
    ) {
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        let nova = Selecao::da_forma_com(largura, altura, &Forma::Laco(vertices), acabamento);
        self.entrar_na_selecao(nova, operacao, "Laço poligonal");
    }

    /// A varinha mágica (W) em `(x, y)`: a cor da foto **como ela aparece**
    /// (todas as camadas — a foto aqui é a base, e não uma camada), com a
    /// tolerância e o contíguo do Photoshop.
    pub fn varinha(
        &mut self,
        x: f32,
        y: f32,
        tolerancia: u8,
        contigua: bool,
        operacao: Operacao,
    ) -> bool {
        let opcoes = OpcoesDaVarinha {
            tolerancia,
            contigua,
            suavizar: false,
            amostra: AmostraDaVarinha::Todas,
        };
        self.varinha_com(x, y, opcoes, operacao).is_ok()
    }

    /// A fonte que a varinha lê, do tamanho do documento — separada da
    /// operação de seleção. Na camada atual, só os pixels dela (com o
    /// transparente), no espaço do documento: a base não entra.
    pub fn amostra_da_varinha(
        &self,
        amostra: AmostraDaVarinha,
    ) -> Result<Amostra, VarinhaRecusada> {
        let (largura, altura) = (self.doc.largura(), self.doc.altura());
        match amostra {
            AmostraDaVarinha::Todas => Ok(Amostra::da_imagem(&self.compor())),
            AmostraDaVarinha::CamadaAtual => {
                let camada = self.camada_ativa();
                if !camada.visivel {
                    return Err(VarinhaRecusada::CamadaEscondida);
                }
                match (&camada.mascara, self.na_mascara()) {
                    (Some(m), true) => Ok(Amostra::da_mascara(m)),
                    _ if camada.ajuste.is_some() => Err(VarinhaRecusada::CamadaSemPixels),
                    _ => Ok(Amostra::da_camada(&camada.pixels, largura, altura)),
                }
            }
        }
    }

    /// A varinha mágica (W) em `(x, y)` com as opções da barra: tolerância,
    /// contígua, antisserrilhado e de onde ler a cor. Um passo do desfazer.
    pub fn varinha_com(
        &mut self,
        x: f32,
        y: f32,
        opcoes: OpcoesDaVarinha,
        operacao: Operacao,
    ) -> Result<(), VarinhaRecusada> {
        if x < 0.0 || y < 0.0 || x >= self.doc.largura() as f32 || y >= self.doc.altura() as f32 {
            return Err(VarinhaRecusada::ForaDaFoto);
        }
        self.fechar_o_que_esta_aberto();
        let amostra = self.amostra_da_varinha(opcoes.amostra)?;
        let nova = Selecao::por_cor_em(
            &amostra,
            (x as u32, y as u32),
            opcoes.tolerancia,
            opcoes.contigua,
            opcoes.suavizar,
        );
        self.entrar_na_selecao(nova, operacao, "Varinha mágica");
        Ok(())
    }

    /// ⌘ + clique na miniatura: a seleção do que a camada tem pintado (o
    /// alfa), ou do que a máscara dela revela. Uma camada de ajuste sem
    /// máscara não tem o que dar.
    pub fn selecionar_da_camada(
        &mut self,
        indice: usize,
        da_mascara: bool,
        operacao: Operacao,
    ) -> bool {
        let Some(camada) = self.doc.camadas.get(indice) else {
            return false;
        };
        let nova = match (&camada.mascara, da_mascara || camada.ajuste.is_some()) {
            (Some(m), true) => Selecao::da_mascara(m),
            (None, true) if camada.ajuste.is_some() => return false,
            _ => Selecao::do_alfa(&camada.pixels),
        };
        self.entrar_na_selecao(nova, operacao, "Carregar seleção");
        true
    }

    /// Difusão (⇧F6) de `raio` pixels. Sem seleção, nada.
    pub fn difundir_selecao(&mut self, raio: u32) -> bool {
        self.modificar_selecao("Difundir", |s| s.difusa(raio))
    }

    /// Expandir (`px` > 0) ou contrair (`px` < 0) a seleção.
    pub fn expandir_selecao(&mut self, px: i32) -> bool {
        let nome = if px >= 0 { "Expandir" } else { "Contrair" };
        self.modificar_selecao(nome, |s| s.expandida(px))
    }

    fn modificar_selecao(&mut self, nome: &str, mudar: impl FnOnce(&Selecao) -> Selecao) -> bool {
        self.fechar_o_que_esta_aberto();
        let Some(atual) = self.selecao.as_deref() else {
            return false;
        };
        let nova = mudar(atual);
        self.trocar_selecao(Some(nova), nome);
        true
    }

    /// Uma seleção nova entra na de agora conforme a operação. Nova sem nada
    /// selecionado no fim (um clique) desmarca, como no Photoshop; tirar ou
    /// cruzar sem seleção não deixa nada.
    fn entrar_na_selecao(&mut self, nova: Selecao, operacao: Operacao, nome: &str) {
        self.fechar_o_que_esta_aberto();
        let resultado = match (operacao, self.selecao.as_deref()) {
            (Operacao::Nova, _) => Some(nova),
            (Operacao::Subtrair | Operacao::Intersecao, None) => None,
            (_, None) => Some(nova),
            (_, Some(atual)) => {
                let mut junta = atual.clone();
                junta.combinar(&nova, operacao);
                Some(junta)
            }
        };
        self.trocar_selecao(resultado, nome);
    }

    /// A seleção passa a ser `nova` (vazia = nenhuma), num passo do desfazer
    /// com o `nome` do Photoshop. Igual à de agora não vira passo.
    fn trocar_selecao(&mut self, nova: Option<Selecao>, nome: &str) {
        let depois = nova.filter(|s| !s.nada()).map(Arc::new);
        if depois == self.selecao {
            return;
        }
        let antes = std::mem::replace(&mut self.selecao, depois.clone());
        self.hist.registrar(Comando::Selecao {
            nome: nome.to_string(),
            antes,
            depois,
        });
        self.versao += 1;
        self.versao_da_selecao += 1;
    }

    /// O passo de seleção de `antes` até a de agora — para juntar a um gesto
    /// que também mexe em pixel (o Mover que leva a seleção).
    fn passo_da_selecao(&self, antes: Option<Arc<Selecao>>, nome: &str) -> Option<Comando> {
        (antes != self.selecao).then(|| Comando::Selecao {
            nome: nome.to_string(),
            antes,
            depois: self.selecao.clone(),
        })
    }

    /// ⌘A.
    pub fn selecionar_tudo(&mut self) {
        self.fechar_o_que_esta_aberto();
        let tudo = Selecao::tudo(self.doc.largura(), self.doc.altura());
        self.trocar_selecao(Some(tudo), "Selecionar tudo");
    }

    /// Desmarca **dentro do último passo** (o preenchimento por conteúdo
    /// aplicado): um desfazer devolve o que ele fez e a seleção juntos.
    pub fn desmarcar_junto_do_ultimo(&mut self) {
        let Some(antes) = self.selecao.take() else {
            return;
        };
        let nome = self
            .hist
            .a_desfazer()
            .map(|p| p.descricao(&self.doc))
            .unwrap_or_else(|| "Desmarcar".into());
        self.hist.juntar_ao_ultimo(
            Comando::Selecao {
                nome: "Desmarcar".into(),
                antes: Some(antes),
                depois: None,
            },
            &nome,
        );
        self.versao += 1;
        self.versao_da_selecao += 1;
    }

    /// ⌘D.
    pub fn desmarcar(&mut self) {
        self.fechar_o_que_esta_aberto();
        self.trocar_selecao(None, "Desmarcar");
    }

    /// ⇧⌘I. Sem seleção não faz nada (no Photoshop também).
    pub fn inverter_selecao(&mut self) {
        self.fechar_o_que_esta_aberto();
        if let Some(atual) = self.selecao.as_deref() {
            let mut nova = atual.clone();
            nova.inverter();
            self.trocar_selecao(Some(nova), "Inverter seleção");
        }
    }

    // ------------------------------------------------- mover o contorno

    /// Arrastar por dentro da seleção com uma ferramenta de seleção move **só o
    /// contorno**, sem os pixels (no Photoshop, "Nova seleção" + arrastar
    /// dentro dela). Falso sem seleção.
    pub fn comecar_a_mover_o_contorno(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let Some(atual) = self.selecao.clone() else {
            return false;
        };
        self.contorno_movendo = Some(atual);
        true
    }

    /// O contorno deslocado `(dx, dy)` do começo do arrasto, na hora.
    pub fn mover_o_contorno_por(&mut self, dx: i64, dy: i64) {
        let Some(original) = self.contorno_movendo.as_deref() else {
            return;
        };
        self.selecao = Some(Arc::new(original.deslocada(dx, dy)));
        self.versao += 1;
        self.versao_da_selecao += 1;
    }

    /// O arrasto do contorno acabou: um passo "Mover seleção".
    pub fn terminar_de_mover_o_contorno(&mut self) -> bool {
        let Some(antes) = self.contorno_movendo.take() else {
            return false;
        };
        match self.passo_da_selecao(Some(antes), "Mover seleção") {
            Some(passo) => {
                self.hist.registrar(passo);
                true
            }
            None => false,
        }
    }

    pub fn movendo_o_contorno(&self) -> bool {
        self.contorno_movendo.is_some()
    }

    /// Um passo de pixels na camada escolhida, feito de uma vez.
    fn registrar_mudanca(
        &mut self,
        camada: usize,
        na_mascara: bool,
        mudanca: Option<Mudanca>,
    ) -> bool {
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
        self.hist.registrar(Comando::Traco {
            camada,
            na_mascara,
            mudanca,
        });
        self.refazer_a_vista(&sujo);
        true
    }

    /// Delete: apaga a seleção na camada escolhida. Sem seleção não faz nada.
    pub fn apagar_selecao(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let Some(selecao) = self.selecao.clone() else {
            return false;
        };
        let (camada, na_mascara) = (self.ativa(), self.na_mascara());
        let mudanca = operacoes::apagar(self.doc.camadas[camada].alvo_mut(na_mascara), &selecao);
        self.registrar_mudanca(camada, na_mascara, mudanca)
    }

    /// ⌥Delete: preenche a seleção (ou a camada inteira) com a cor do pincel.
    pub fn preencher_selecao(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        if !self.pode_pintar() {
            return false;
        }
        let selecao = self.selecao.clone();
        let na_mascara = self.na_mascara();
        let mudanca = operacoes::preencher(
            self.doc.camadas[camada].alvo_mut(na_mascara),
            selecao.as_deref(),
            self.pincel.cor,
        );
        self.registrar_mudanca(camada, na_mascara, mudanca)
    }

    /// O degradê (G) de `de` até `ate`, em pixels da foto, na seleção ou na
    /// camada inteira. Na camada, da cor do pincel para o transparente, por
    /// cima do que ela tem; na máscara, opaco da cor até o oposto dela (preto
    /// → branco), refazendo a máscara ali — o uso clássico de esmaecer.
    pub fn degrade(&mut self, de: (f32, f32), ate: (f32, f32)) -> bool {
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        if !self.pode_pintar() {
            return false;
        }
        let na_mascara = self.na_mascara();
        let cor = self.pincel.cor;
        let ate_a_cor = na_mascara.then(|| {
            let cinza = (77 * cor[0] as u32 + 150 * cor[1] as u32 + 29 * cor[2] as u32) >> 8;
            if cinza < 128 {
                [255; 3]
            } else {
                [0; 3]
            }
        });
        let selecao = self.selecao.clone();
        let mudanca = operacoes::degrade(
            self.doc.camadas[camada].alvo_mut(na_mascara),
            selecao.as_deref(),
            cor,
            ate_a_cor,
            de,
            ate,
        );
        self.registrar_mudanca(camada, na_mascara, mudanca)
    }

    /// A lata de tinta (⇧G) em `(x, y)`: a área parecida em volta, na camada
    /// (ou na máscara) escolhida, com a cor do pincel. Tolerância 32.
    pub fn lata_de_tinta(&mut self, x: f32, y: f32) -> bool {
        self.fechar_o_que_esta_aberto();
        let camada = self.ativa();
        if !self.pode_pintar() || x < 0.0 || y < 0.0 {
            return false;
        }
        let na_mascara = self.na_mascara();
        let fundo = self.doc.camadas[camada]
            .mascara
            .as_ref()
            .filter(|_| na_mascara)
            .map(|m| m.fundo);
        let selecao = self.selecao.clone();
        let cor = self.pincel.cor;
        let mudanca = operacoes::lata_de_tinta(
            self.doc.camadas[camada].alvo_mut(na_mascara),
            selecao.as_deref(),
            cor,
            (x as u32, y as u32),
            TOLERANCIA_DA_LATA,
            fundo,
        );
        self.registrar_mudanca(camada, na_mascara, mudanca)
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
        if self.doc.camadas[indice - 1].ajuste.is_some() {
            return Err("A camada de baixo é de ajuste: não tem pixels para receber");
        }
        let de_cima = self.doc.camadas[indice].clone();
        let mut abaixo = self.doc.camadas[indice - 1].pixels.clone();
        let mudanca = if de_cima.ajuste.is_some() {
            operacoes::ajustar_a_de_baixo(&mut abaixo, &de_cima)
        } else {
            operacoes::mesclar_na_de_baixo(&mut abaixo, &de_cima)
        }
        .unwrap_or(Mudanca {
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
        self.comecar_traco(x, y, false)
    }

    /// ⇧ + clique: o traço começa com uma reta do fim do anterior até
    /// `(x, y)` ("Any painting tool + Shift-click", na tabela da Adobe). Sem
    /// traço anterior, é um clique comum. O arrasto que vier depois continua o
    /// mesmo traço — um passo só do desfazer.
    pub fn apertar_em_reta(&mut self, x: f32, y: f32) -> bool {
        self.comecar_traco(x, y, true)
    }

    /// Onde o último traço terminou.
    pub fn fim_do_ultimo_traco(&self) -> Option<(f32, f32)> {
        self.fim_do_ultimo_traco
    }

    fn comecar_traco(&mut self, x: f32, y: f32, em_reta: bool) -> bool {
        // Um traço que ficou aberto (o ponteiro saiu da janela sem soltar) fecha
        // antes: ele é um passo próprio do desfazer.
        self.fechar_o_que_esta_aberto();
        let ativa = self.ativa();
        if !self.pode_pintar() {
            return false;
        }
        // Na máscara só se pinta cinza: o carimbo, o tom e o foco leem a foto.
        if self.na_mascara() && self.pincel.ferramenta.le_a_foto() {
            return false;
        }
        // Na máscara, a borracha pinta a cor de fundo (branco: revela), como no
        // Photoshop — e não "volta ao fundo da máscara", que numa máscara que
        // esconde tudo esconderia de novo.
        let pincel =
            if self.na_mascara() && self.pincel.ferramenta == crate::pincel::Ferramenta::Borracha {
                crate::pincel::Pincel {
                    ferramenta: crate::pincel::Ferramenta::Pincel,
                    cor: self.pincel.cor_de_fundo,
                    ..self.pincel
                }
            } else {
                self.pincel
            };
        let mut traco = Traco::novo(pincel)
            .dentro_de(self.selecao.clone())
            .com_cordao(self.escala_da_tela);
        if self.pincel.ferramenta.le_a_foto()
            && self.pincel.ferramenta != crate::pincel::Ferramenta::Carimbo
        {
            // Tom e foco: a foto até a camada escolhida, no próprio lugar.
            traco = traco.copiando_de(crate::carimbo::Fonte::nova(
                self.base.clone(),
                &self.doc,
                ativa,
                (0.0, 0.0),
            ));
        }
        if self.pincel.ferramenta == crate::pincel::Ferramenta::Carimbo {
            let Some(origem) = self.origem else {
                return false;
            };
            // Alinhado: a distância do primeiro traço vale para os seguintes.
            // Sem alinhar, cada traço começa copiando da origem.
            let distancia = if self.carimbo.alinhado {
                *self
                    .distancia_do_carimbo
                    .get_or_insert((origem.0 - x, origem.1 - y))
            } else {
                let d = (origem.0 - x, origem.1 - y);
                self.distancia_do_carimbo = Some(d);
                d
            };
            traco = traco.copiando_de(crate::carimbo::Fonte::da_amostra(
                self.base.clone(),
                &self.doc,
                ativa,
                self.carimbo.amostra,
                distancia,
            ));
        }
        let na_mascara = self.na_mascara();
        let alvo = self.doc.camadas[ativa].alvo_mut(na_mascara);
        let sujo = match self.fim_do_ultimo_traco.filter(|_| em_reta) {
            Some(de) => traco.reta(alvo, de, (x, y)),
            None => traco.ate(alvo, x, y),
        };
        self.traco = Some(traco);
        self.refazer_a_vista(&sujo);
        true
    }

    /// O ponteiro andou, apertado.
    pub fn arrastar(&mut self, x: f32, y: f32) {
        let ativa = self.ativa();
        let na_mascara = self.na_mascara();
        let Some(traco) = self.traco.as_mut() else {
            return;
        };
        let sujo = traco.ate(self.doc.camadas[ativa].alvo_mut(na_mascara), x, y);
        self.refazer_a_vista(&sujo);
    }

    /// O ponteiro subiu: o traço vira um passo do desfazer. Devolve se houve
    /// mudança.
    pub fn soltar(&mut self) -> bool {
        let Some(traco) = self.traco.take() else {
            return false;
        };
        self.fim_do_ultimo_traco = traco.ponta().or(self.fim_do_ultimo_traco);
        let camada = self.ativa();
        let na_mascara = self.na_mascara();
        match traco.terminar(self.doc.camadas[camada].alvo_mut(na_mascara)) {
            Some(mudanca) => {
                self.hist.registrar(Comando::Traco {
                    camada,
                    na_mascara,
                    mudanca,
                });
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
                if let Some(p) = passo {
                    self.seguir_o_passo(&p, false);
                }
                self.refazer_a_vista(&sujo);
                true
            }
            None => false,
        }
    }

    /// O painel Histórico: volta (ou avança) até ficarem `posicao` passos
    /// aplicados. Devolve se andou.
    pub fn ir_para(&mut self, posicao: usize) -> bool {
        self.fechar_o_que_esta_aberto();
        let alvo = posicao.min(self.hist.passos().len());
        let mut andou = false;
        while self.hist.posicao() > alvo && self.desfazer() {
            andou = true;
        }
        while self.hist.posicao() < alvo && self.refazer() {
            andou = true;
        }
        andou
    }

    pub fn refazer(&mut self) -> bool {
        self.fechar_o_que_esta_aberto();
        let passo = self.hist.a_refazer().cloned();
        match self.hist.refazer(&mut self.doc) {
            Some(sujo) => {
                if let Some(p) = passo {
                    self.seguir_o_passo(&p, true);
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
        self.hist.alterado()
            || self.traco.is_some()
            || self.opacidade_antes.is_some()
            || self.ajuste_antes.is_some()
            || self.movendo.is_some()
            || self.flutuante.is_some()
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
    fn o_carimbo_alinhado_copia_a_mesma_distancia() {
        let mut s = sessao();
        s.nova_camada();
        s.pincel.ferramenta = Ferramenta::Carimbo;
        s.pincel.dureza = 1.0;
        s.pincel.raio = 10.0;
        assert!(!s.apertar(300.0, 300.0), "sem origem não copia");
        s.definir_origem(100.0, 100.0);
        assert_eq!(s.mira_do_carimbo(300.0, 300.0), Some((100.0, 100.0)));
        assert!(s.apertar(300.0, 300.0));
        s.soltar();
        assert_eq!(s.compor().get_pixel(300, 300).0, [100, 100, 50]);
        // O segundo traço, em outro lugar, copia à mesma distância.
        assert_eq!(s.mira_do_carimbo(500.0, 400.0), Some((300.0, 200.0)));
        s.apertar(500.0, 400.0);
        s.soltar();
        assert_eq!(s.compor().get_pixel(500, 400).0, [44, 200, 50]);
        assert!(
            s.documento().camadas[0].pixels.vazia(),
            "a base e a de baixo intactas"
        );
        assert_eq!(s.cor_em(500.0, 400.0), Some([44, 200, 50]));
        assert_eq!(s.cor_em(-1.0, 0.0), None);
    }

    #[test]
    fn mover_desloca_ao_vivo_e_e_um_passo_so() {
        let mut s = sessao();
        s.pincel.cor = [255, 0, 0];
        s.pincel.dureza = 1.0;
        s.apertar(100.0, 100.0);
        s.soltar();
        assert!(s.comecar_a_mover());
        for d in [10, 50, 120] {
            s.mover_por(d, d / 2);
        }
        assert_eq!(s.compor().get_pixel(220, 160).0, [255, 0, 0]);
        assert_eq!(s.compor().get_pixel(100, 100).0, [100, 100, 50]);
        assert!(s.terminar_de_mover());
        assert_eq!(s.historico().passos().len(), 2);
        s.desfazer();
        assert_eq!(s.compor().get_pixel(100, 100).0, [255, 0, 0]);
    }

    #[test]
    fn transformar_aplica_cancela_e_e_um_passo_so() {
        let mut s = sessao();
        s.pincel.cor = [255, 0, 0];
        s.pincel.dureza = 1.0;
        s.pincel.raio = 20.0;
        s.apertar(200.0, 200.0);
        s.soltar();
        assert!(s.comecar_a_transformar());
        s.definir_transformacao(Transformacao {
            escala_x: 2.0,
            escala_y: 2.0,
            ..Default::default()
        });
        assert_eq!(s.compor().get_pixel(200, 236).0, [255, 0, 0], "ampliado");
        s.cancelar_transformacao();
        assert_eq!(s.compor().get_pixel(200, 236).0, [200, 236, 50], "voltou");
        assert_eq!(s.historico().passos().len(), 1);

        s.comecar_a_transformar();
        s.definir_transformacao(Transformacao::deslocamento(100.0, 0.0));
        assert!(s.aplicar_transformacao());
        assert_eq!(s.historico().passos().len(), 2);
        assert_eq!(s.compor().get_pixel(300, 200).0, [255, 0, 0]);
        s.desfazer();
        assert_eq!(s.compor().get_pixel(200, 200).0, [255, 0, 0]);
    }

    #[test]
    fn mover_com_selecao_leva_so_o_pedaco_e_a_selecao() {
        let mut s = sessao();
        s.pincel.cor = [0, 255, 0];
        s.preencher_selecao();
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(100, 100, 50, 50)),
            Operacao::Nova,
        );
        assert!(s.comecar_a_mover());
        s.mover_por(200, 0);
        assert!(s.terminar_de_mover());
        let c = &s.documento().camadas[0].pixels;
        assert_eq!(c.pixel(120, 120)[3], 0, "o buraco");
        assert_eq!(c.pixel(320, 120), [0, 255, 0, 255], "o pedaço");
        assert_eq!(c.pixel(500, 500), [0, 255, 0, 255], "o resto ficou");
        let sel = s.selecao().unwrap();
        assert_eq!((sel.valor(320, 120), sel.valor(120, 120)), (255, 0));
    }

    #[test]
    fn camada_via_copia_e_via_recorte() {
        let mut s = sessao();
        s.pincel.cor = [0, 0, 255];
        s.preencher_selecao();
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(0, 0, 100, 100)),
            Operacao::Nova,
        );
        assert!(s.camada_via_copia(false));
        assert_eq!(s.documento().camadas.len(), 2);
        assert_eq!(s.ativa(), 1);
        let nova = &s.documento().camadas[1].pixels;
        assert_eq!((nova.pixel(50, 50)[3], nova.pixel(150, 50)[3]), (255, 0));
        s.escolher_camada(0);
        assert!(s.camada_via_copia(true));
        assert_eq!(
            s.documento().camadas[0].pixels.pixel(50, 50)[3],
            0,
            "recortado"
        );
        assert_eq!(s.documento().camadas.len(), 3);
    }

    #[test]
    fn colar_um_remendo_e_um_passo_na_camada_pedida() {
        let mut s = sessao();
        s.nova_camada();
        let ret = Retangulo::novo(100, 100, 10, 10);
        let foto = s.foto_ate_a_ativa(&ret);
        assert_eq!(foto.get_pixel(0, 0).0, [100, 100, 50]);
        let rgba = vec![255u8; 10 * 10 * 4];
        assert!(s.colar_remendo(1, &ret, &rgba, &|_, _| 255));
        assert_eq!(s.compor().get_pixel(105, 105).0, [255, 255, 255]);
        assert!(s.documento().camadas[0].pixels.vazia());
        s.desfazer();
        assert_eq!(s.compor().get_pixel(105, 105).0, [105, 105, 50]);
        assert!(!s.colar_remendo(9, &ret, &rgba, &|_, _| 255));
    }

    #[test]
    fn subexposicao_e_desfoque_escrevem_na_camada_vazia_e_o_historico_anda() {
        use crate::pincel::Faixa;
        let mut s = sessao();
        s.nova_camada();
        s.pincel.ferramenta = Ferramenta::Subexposicao(Faixa::MeiosTons);
        s.pincel.dureza = 1.0;
        s.pincel.raio = 10.0;
        let antes = s.compor().get_pixel(400, 128).0;
        assert!(s.apertar(400.0, 128.0));
        s.soltar();
        let depois = s.compor().get_pixel(400, 128).0;
        assert!(depois[1] > antes[1], "clareou: {antes:?} → {depois:?}");
        assert!(s.documento().camadas[0].pixels.vazia(), "a de baixo fica");

        s.pincel.ferramenta = Ferramenta::Desfoque;
        assert!(s.apertar(100.0, 100.0));
        s.soltar();
        assert_eq!(s.historico().posicao(), 3);
        assert!(s.ir_para(1));
        assert_eq!(s.historico().posicao(), 1);
        assert_eq!(s.compor().get_pixel(400, 128).0, antes);
        assert!(s.ir_para(3));
        assert_eq!(s.compor().get_pixel(400, 128).0, depois);
        assert!(!s.ir_para(3), "já está lá");
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

    /// Uma camada de vermelho cheio por cima da foto, numa sessão nova.
    fn vermelha() -> Sessao {
        let mut s = sessao();
        s.selecionar_tudo();
        s.pincel.cor = [255, 0, 0];
        assert!(s.preencher_selecao());
        s.desmarcar();
        s
    }

    #[test]
    fn a_mascara_esconde_onde_e_preta_e_volta_ao_desligar() {
        let mut s = vermelha();
        let base = s.base().clone();
        assert!(s.adicionar_mascara(false));
        assert!(s.na_mascara(), "a máscara nova recebe o pincel");
        assert!(!s.adicionar_mascara(false), "uma só por camada");
        assert_eq!(
            s.cor_em(10.0, 10.0),
            Some([255, 0, 0]),
            "branca revela tudo"
        );
        // Preto na máscara: a foto aparece ali.
        s.pincel.cor = [0, 0, 0];
        s.pincel.raio = 20.0;
        s.pincel.dureza = 1.0;
        assert!(s.apertar(100.0, 100.0));
        s.soltar();
        assert_eq!(s.cor_em(100.0, 100.0), Some(base.get_pixel(100, 100).0));
        assert_eq!(s.cor_em(400.0, 400.0), Some([255, 0, 0]));
        assert!(
            s.documento().camadas[0].pixels.pixel(100, 100) == [255, 0, 0, 255],
            "a camada ficou intacta"
        );
        assert_eq!(
            s.historico().a_desfazer().unwrap().descricao(s.documento()),
            "Pincel na máscara"
        );
        // ⇧ + clique: desligada, a camada aparece inteira.
        assert!(s.alternar_mascara_de(0));
        assert_eq!(s.cor_em(100.0, 100.0), Some([255, 0, 0]));
        assert!(s.desfazer());
        assert_eq!(s.cor_em(100.0, 100.0), Some(base.get_pixel(100, 100).0));
        // Pintar nos pixels de novo, e a borracha na máscara devolve o fundo.
        s.escolher_camada(0);
        assert!(!s.na_mascara());
        assert!(s.escolher_mascara(0));
        s.pincel.ferramenta = Ferramenta::Borracha;
        s.apertar(100.0, 100.0);
        s.soltar();
        assert_eq!(s.cor_em(100.0, 100.0), Some([255, 0, 0]));
        // O carimbo não pinta na máscara.
        s.pincel.ferramenta = Ferramenta::Carimbo;
        s.definir_origem(10.0, 10.0);
        assert!(!s.apertar(200.0, 200.0));
        // Excluir a máscara, e desfazer devolve tudo, com o pincel nela.
        assert!(s.excluir_mascara());
        assert!(!s.na_mascara());
        assert!(s.desfazer());
        assert!(s.documento().camadas[0].mascara.is_some());
        // Desfazer até antes da máscara: ela sai e o alvo volta aos pixels.
        while s.documento().camadas[0].mascara.is_some() {
            assert!(s.desfazer());
        }
        assert!(!s.na_mascara());
        assert_eq!(s.cor_em(100.0, 100.0), Some([255, 0, 0]));
    }

    #[test]
    fn a_mascara_com_alt_esconde_tudo_e_a_selecao_vira_mascara() {
        let mut s = vermelha();
        let base = s.base().clone();
        assert!(s.adicionar_mascara(true));
        assert!(s.documento().neutro(), "escondendo tudo, a foto é a base");
        assert_eq!(s.cor_em(10.0, 10.0), Some(base.get_pixel(10, 10).0));
        s.desfazer();

        // Com seleção, só o selecionado aparece (com ⌥, só ele some).
        s.escolher_camada(0);
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(300, 200, 100, 100)),
            Operacao::Nova,
        );
        assert!(s.adicionar_mascara(false));
        assert_eq!(s.cor_em(350.0, 250.0), Some([255, 0, 0]));
        assert_eq!(s.cor_em(10.0, 10.0), Some(base.get_pixel(10, 10).0));
        s.desfazer();
        assert!(s.adicionar_mascara(true));
        assert_eq!(s.cor_em(350.0, 250.0), Some(base.get_pixel(350, 250).0));
        assert_eq!(s.cor_em(10.0, 10.0), Some([255, 0, 0]));
    }

    #[test]
    fn o_degrade_na_mascara_esmaece_a_camada_e_a_lata_preenche() {
        let mut s = vermelha();
        let base = s.base().clone();
        s.adicionar_mascara(false);
        s.pincel.cor = [0, 0, 0];
        assert!(s.degrade((100.0, 0.0), (700.0, 0.0)));
        assert_eq!(
            s.cor_em(50.0, 300.0),
            Some(base.get_pixel(50, 300).0),
            "começo escondido"
        );
        assert_eq!(s.cor_em(750.0, 300.0), Some([255, 0, 0]), "fim revelado");
        let meio = s.cor_em(400.0, 300.0).unwrap();
        assert!(
            meio[0] > base.get_pixel(400, 300).0[0] && meio[0] < 255,
            "meio a meio"
        );
        assert_eq!(
            s.historico().a_desfazer().unwrap().descricao(s.documento()),
            "Pincel na máscara"
        );

        // A lata na máscara: a área preta toda (antes do degradê) volta a branco.
        s.pincel.cor = [255, 255, 255];
        assert!(s.lata_de_tinta(10.0, 10.0));
        assert_eq!(s.cor_em(10.0, 300.0), Some([255, 0, 0]));

        // Na camada: a lata pinta o vermelho todo (é tudo igual) de azul.
        s.escolher_camada(0);
        s.pincel.cor = [0, 0, 255];
        assert!(s.lata_de_tinta(10.0, 10.0));
        assert_eq!(
            s.documento().camadas[0].pixels.pixel(799, 599),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn o_mover_e_o_delete_na_mascara_mexem_so_nela() {
        let mut s = vermelha();
        let base = s.base().clone();
        s.adicionar_mascara(true);
        // Revela um quadrado e o leva para o lado com o Mover.
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(0, 0, 100, 100)),
            Operacao::Nova,
        );
        s.pincel.cor = [255, 255, 255];
        assert!(s.preencher_selecao());
        s.desmarcar();
        assert_eq!(s.cor_em(50.0, 50.0), Some([255, 0, 0]));
        assert!(s.comecar_a_mover());
        s.mover_por(300, 0);
        assert!(s.terminar_de_mover());
        assert_eq!(s.cor_em(50.0, 50.0), Some(base.get_pixel(50, 50).0));
        assert_eq!(s.cor_em(350.0, 50.0), Some([255, 0, 0]));
        assert_eq!(
            s.documento().camadas[0].pixels.pixel(50, 50),
            [255, 0, 0, 255]
        );
        // Delete na máscara devolve o fundo (esconde, aqui).
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(300, 0, 100, 100)),
            Operacao::Nova,
        );
        assert!(s.apagar_selecao());
        assert_eq!(s.cor_em(350.0, 50.0), Some(base.get_pixel(350, 50).0));
    }

    #[test]
    fn a_camada_de_ajuste_muda_o_de_baixo_sem_tocar_em_pixel() {
        let mut s = sessao();
        let base = s.base().clone();
        let niveis = Ajuste::Niveis {
            preto: 0.0,
            gama: 1.0,
            branco: 255.0,
        };
        s.nova_camada_de_ajuste(niveis);
        assert_eq!(s.ativa(), 1);
        assert_eq!(s.camada_ativa().nome, "Níveis 1");
        assert!(s.na_mascara(), "o pincel vai para a máscara do ajuste");
        assert!(s.documento().neutro(), "recém-criado, não muda nada");
        // O arrasto do slider: a foto muda a cada valor, o desfazer é um passo.
        let passos = s.historico().passos().len();
        for branco in [240.0, 200.0, 128.0] {
            s.mover_ajuste(Ajuste::Niveis {
                preto: 0.0,
                gama: 1.0,
                branco,
            });
        }
        s.confirmar_ajuste();
        assert_eq!(s.historico().passos().len(), passos + 1);
        let p = base.get_pixel(100, 100).0;
        let esperado = p.map(|v| ((v as f32 / 128.0).min(1.0) * 255.0).round() as u8);
        assert_eq!(s.cor_em(100.0, 100.0), Some(esperado));
        assert!(
            s.documento().camadas[1].pixels.vazia(),
            "nenhum pixel na camada"
        );
        // Pintar de preto na máscara tira o ajuste dali.
        s.pincel.cor = [0; 3];
        s.pincel.raio = 30.0;
        s.pincel.dureza = 1.0;
        assert!(s.apertar(100.0, 100.0));
        s.soltar();
        assert_eq!(s.cor_em(100.0, 100.0), Some(p));
        assert_eq!(s.cor_em(500.0, 400.0).unwrap(), {
            let q = base.get_pixel(500, 400).0;
            q.map(|v| ((v as f32 / 128.0).min(1.0) * 255.0).round() as u8)
        });
        // Desfazer volta o traço e depois o ajuste inteiro.
        assert!(s.desfazer());
        assert!(s.desfazer());
        assert!(s.documento().neutro());
        // Esconder a camada tira o efeito.
        s.refazer();
        s.alternar_visibilidade();
        assert_eq!(s.cor_em(100.0, 100.0), Some(p));
    }

    #[test]
    fn o_ajuste_com_selecao_e_o_mesclar_para_baixo() {
        let mut s = vermelha();
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(0, 0, 400, 600)),
            Operacao::Nova,
        );
        s.nova_camada_de_ajuste(Ajuste::Inverter);
        s.desmarcar();
        assert_eq!(
            s.cor_em(10.0, 10.0),
            Some([0, 255, 255]),
            "inverteu na seleção"
        );
        assert_eq!(s.cor_em(700.0, 10.0), Some([255, 0, 0]), "e só nela");
        // Sem máscara não há onde pintar.
        assert!(s.excluir_mascara());
        assert!(!s.apertar(10.0, 10.0));
        assert_eq!(
            s.cor_em(700.0, 10.0),
            Some([0, 255, 255]),
            "sem máscara vale tudo"
        );
        // ⌘E: o ajuste entra nos pixels da de baixo.
        s.mesclar_para_baixo().unwrap();
        assert_eq!(s.documento().camadas.len(), 1);
        assert_eq!(
            s.documento().camadas[0].pixels.pixel(10, 10),
            [0, 255, 255, 255]
        );
        assert!(s.desfazer());
        // Mesclar numa de ajuste é recusado.
        s.nova_camada_de_ajuste(Ajuste::Inverter);
        s.nova_camada();
        assert!(s.mesclar_para_baixo().is_err());
    }

    #[test]
    fn o_rascunho_do_arrasto_vira_a_vista_exata_ao_soltar() {
        let mut s = vermelha();
        s.nova_camada_de_ajuste(Ajuste::Inverter);
        s.mover_ajuste(Ajuste::MatizSaturacao {
            matiz: 0.0,
            saturacao: 0.0,
            luminosidade: 0.0,
        });
        // Trocar o tipo não vale: o slider é do ajuste que a camada tem.
        assert_eq!(s.camada_ativa().ajuste, Some(Ajuste::Inverter));
        s.desfazer();
        s.nova_camada_de_ajuste(Ajuste::Niveis {
            preto: 0.0,
            gama: 1.0,
            branco: 255.0,
        });
        for gama in [1.2, 1.6, 2.0] {
            s.mover_ajuste(Ajuste::Niveis {
                preto: 10.0,
                gama,
                branco: 240.0,
            });
        }
        s.confirmar_ajuste();
        let exata = Vista::nova(s.base(), s.documento(), 400);
        assert_eq!(s.vista().imagem().as_raw(), exata.imagem().as_raw());
    }

    #[test]
    fn a_varinha_le_a_foto_como_aparece_e_o_cmd_clique_le_a_camada() {
        // A sessão de teste: x%256 no vermelho. Uma camada azul cobrindo um
        // retângulo vira a "cor" que a varinha pega.
        let mut s = sessao();
        s.selecionar(
            &Forma::Retangulo(Retangulo::novo(100, 100, 200, 100)),
            Operacao::Nova,
        );
        s.pincel.cor = [0, 0, 255];
        s.preencher_selecao();
        s.desmarcar();
        assert!(s.varinha(150.0, 150.0, 32, true, Operacao::Nova));
        let sel = s.selecao().unwrap();
        assert_eq!(sel.valor(299, 199), 255);
        assert_eq!(sel.valor(300, 150), 0);
        // ⇧: soma a varinha num ponto da base.
        assert!(s.varinha(5.0, 500.0, 2, true, Operacao::Somar));
        assert_eq!(s.selecao().unwrap().valor(150, 150), 255);
        assert_eq!(s.selecao().unwrap().valor(5, 500), 255);
        // Um clique fora da foto não muda nada.
        assert!(!s.varinha(-1.0, 5.0, 32, true, Operacao::Nova));

        // ⌘ + clique na miniatura: o alfa da camada.
        s.desmarcar();
        assert!(s.selecionar_da_camada(0, false, Operacao::Nova));
        let sel = s.selecao().unwrap();
        assert_eq!((sel.valor(150, 150), sel.valor(50, 50)), (255, 0));
        // ⌥: tira a máscara (que revela tudo) — não sobra nada.
        s.adicionar_mascara(false);
        s.selecionar_da_camada(0, true, Operacao::Subtrair);
        assert!(s.selecao().is_none());
        // Difusão e expandir sem seleção não fazem nada.
        assert!(!s.difundir_selecao(5));
        assert!(!s.expandir_selecao(5));
    }
}
