//! Uma edição aberta: a base, o documento, o histórico, a vista e o traço em
//! curso — tudo o que a janela precisa, sem a janela.
//!
//! 🔑 **Todo gesto passa por aqui e devolve o que sujou**; a vista é refeita no
//! mesmo passo. A janela só pergunta quais ladrilhos subir para a GPU. É o que
//! deixa a sessão inteira ser testada sem GPUI.

use std::sync::Arc;

use image::RgbImage;

use crate::composicao;
use crate::documento::Documento;
use crate::historico::{Comando, Historico};
use crate::pincel::{Pincel, Traco};
use crate::retangulo::Retangulo;
use crate::vista::Vista;

pub struct Sessao {
    base: Arc<RgbImage>,
    doc: Documento,
    hist: Historico,
    vista: Vista,
    pub pincel: Pincel,
    traco: Option<Traco>,
    /// A opacidade da camada quando o arrasto do slider começou — o passo do
    /// desfazer é o arrasto inteiro, e não cada valor do caminho.
    opacidade_antes: Option<f32>,
}

impl Sessao {
    /// `lado_da_vista` é o maior lado da vista reduzida, em pixels.
    pub fn nova(base: Arc<RgbImage>, doc: Documento, hist: Historico, lado_da_vista: u32) -> Self {
        debug_assert_eq!((base.width(), base.height()), (doc.largura(), doc.altura()));
        let vista = Vista::nova(&base, &doc, lado_da_vista);
        Self {
            base,
            doc,
            hist,
            vista,
            pincel: Pincel::default(),
            traco: None,
            opacidade_antes: None,
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

    fn refazer_a_vista(&mut self, sujo: &Retangulo) {
        self.vista.refazer(&self.base, &self.doc, sujo);
    }

    // --------------------------------------------------------------- traço

    /// O ponteiro desceu em `(x, y)`, em pixels da foto.
    pub fn apertar(&mut self, x: f32, y: f32) {
        // Um traço que ficou aberto (o ponteiro saiu da janela sem soltar) fecha
        // antes: ele é um passo próprio do desfazer.
        self.soltar();
        let mut traco = Traco::novo(self.pincel);
        let sujo = traco.ate(&mut self.doc.camadas[0].pixels, x, y);
        self.traco = Some(traco);
        self.refazer_a_vista(&sujo);
    }

    /// O ponteiro andou, apertado.
    pub fn arrastar(&mut self, x: f32, y: f32) {
        let Some(traco) = self.traco.as_mut() else {
            return;
        };
        let sujo = traco.ate(&mut self.doc.camadas[0].pixels, x, y);
        self.refazer_a_vista(&sujo);
    }

    /// O ponteiro subiu: o traço vira um passo do desfazer. Devolve se houve
    /// mudança.
    pub fn soltar(&mut self) -> bool {
        let Some(traco) = self.traco.take() else {
            return false;
        };
        match traco.terminar(&mut self.doc.camadas[0].pixels) {
            Some(mudanca) => {
                self.hist.registrar(Comando::Traco { camada: 0, mudanca });
                true
            }
            None => false,
        }
    }

    pub fn tracando(&self) -> bool {
        self.traco.is_some()
    }

    // -------------------------------------------------------- a camada

    pub fn alternar_visibilidade(&mut self) {
        self.soltar();
        let camada = &mut self.doc.camadas[0];
        let antes = camada.visivel;
        camada.visivel = !antes;
        self.hist.registrar(Comando::Visibilidade {
            camada: 0,
            antes,
            depois: !antes,
        });
        self.refazer_tudo();
    }

    /// O slider da opacidade andou: a camada muda na hora, o histórico só no
    /// [`Self::confirmar_opacidade`].
    pub fn mover_opacidade(&mut self, valor: f32) {
        self.soltar();
        let valor = valor.clamp(0.0, 1.0);
        let camada = &mut self.doc.camadas[0];
        self.opacidade_antes.get_or_insert(camada.opacidade);
        if camada.opacidade == valor {
            return;
        }
        camada.opacidade = valor;
        self.refazer_tudo();
    }

    /// O arrasto do slider acabou.
    pub fn confirmar_opacidade(&mut self) {
        let Some(antes) = self.opacidade_antes.take() else {
            return;
        };
        let depois = self.doc.camadas[0].opacidade;
        if antes != depois {
            self.hist.registrar(Comando::Opacidade {
                camada: 0,
                antes,
                depois,
            });
        }
    }

    fn refazer_tudo(&mut self) {
        let tudo = Retangulo::inteiro(self.doc.largura(), self.doc.altura());
        self.refazer_a_vista(&tudo);
    }

    // ------------------------------------------------------ desfazer

    pub fn desfazer(&mut self) -> bool {
        self.soltar();
        self.confirmar_opacidade();
        match self.hist.desfazer(&mut self.doc) {
            Some(sujo) => {
                self.refazer_a_vista(&sujo);
                true
            }
            None => false,
        }
    }

    pub fn refazer(&mut self) -> bool {
        self.soltar();
        self.confirmar_opacidade();
        match self.hist.refazer(&mut self.doc) {
            Some(sujo) => {
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
        self.soltar();
        self.confirmar_opacidade();
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
    use crate::documento::BaseRef;
    use crate::pincel::Ferramenta;

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
