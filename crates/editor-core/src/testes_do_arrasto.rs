//! 🔑 **Todo arrasto mostra a vista em rascunho e volta à exata quando acaba.**
//!
//! É o acordo do Lightroom: durante o gesto, só o que a tela mostra (um pixel
//! da foto por pixel da vista); ao soltar, aplicar, cancelar ou fechar, a vista
//! exata (a média de cada caixa). O documento, a composição e o histórico não
//! mudam em nada — só a tela durante o gesto.

use std::sync::Arc;

use image::RgbImage;

use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::Sessao;
use crate::tiles::CamadaDePixels;
use crate::transformar::Transformacao;
use crate::vista::Vista;

const LARGURA: u32 = 800;
const ALTURA: u32 = 600;
/// 800 px em 300 de vista: fator 3, onde rascunho e exata diferem.
const LADO: u32 = 300;

fn tudo() -> Retangulo {
    Retangulo::inteiro(LARGURA, ALTURA)
}

/// A base com textura (para a média da caixa diferir do pixel do meio) e uma
/// camada pintada por cima de tudo, a 60%: o que ela faz aparece na foto
/// inteira, e a textura da base continua aparecendo.
fn sessao() -> Sessao {
    let base = Arc::new(RgbImage::from_fn(LARGURA, ALTURA, |x, y| {
        image::Rgb([
            (x * 7 % 256) as u8,
            (y * 5 % 256) as u8,
            ((x ^ y) % 256) as u8,
        ])
    }));
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), LADO);
    s.nova_camada();
    s.selecionar(&Forma::Retangulo(tudo()), Operacao::Nova);
    s.pincel.cor = [200, 40, 90];
    assert!(s.preencher_selecao());
    s.desmarcar();
    s.mover_opacidade(0.6);
    s.confirmar_opacidade();
    s
}

fn exata(s: &Sessao) -> Vec<u8> {
    Vista::nova(s.base(), s.documento(), LADO)
        .imagem()
        .as_raw()
        .clone()
}

fn rascunho(s: &Sessao) -> Vec<u8> {
    let mut v = Vista::nova(s.base(), s.documento(), LADO);
    v.rascunhar(s.base(), s.documento(), &tudo());
    v.imagem().as_raw().clone()
}

/// A vista é o rascunho do documento de agora — e o rascunho não é a exata
/// (senão o teste não provaria nada).
#[track_caller]
fn em_rascunho(s: &Sessao) {
    let esperado = rascunho(s);
    assert_ne!(esperado, exata(s), "a cena precisa de textura");
    assert!(
        s.vista().imagem().as_raw() == &esperado,
        "a vista não está em rascunho"
    );
}

#[track_caller]
fn exata_de_novo(s: &Sessao) {
    assert!(
        s.vista().imagem().as_raw() == &exata(s),
        "a vista não voltou à exata"
    );
}

#[test]
fn a_cena_comeca_exata() {
    exata_de_novo(&sessao());
}

#[test]
fn mover_rascunha_no_arrasto_e_firma_ao_soltar() {
    let mut s = sessao();
    assert!(s.comecar_a_mover());
    s.mover_por(13, 7);
    em_rascunho(&s);
    s.mover_por(40, -5);
    em_rascunho(&s);
    let composta = s.compor();
    assert!(s.terminar_de_mover());
    exata_de_novo(&s);
    assert_eq!(
        s.compor().as_raw(),
        composta.as_raw(),
        "soltar não muda a foto"
    );
}

#[test]
fn transformar_rascunha_e_firma_ao_aplicar_e_ao_cancelar() {
    let mut s = sessao();
    let antes = s.compor();
    assert!(s.comecar_a_transformar());
    s.definir_transformacao(Transformacao::deslocamento(20.0, 10.0));
    em_rascunho(&s);
    s.cancelar_transformacao();
    exata_de_novo(&s);
    assert_eq!(
        s.compor().as_raw(),
        antes.as_raw(),
        "cancelar devolve a foto"
    );

    assert!(s.comecar_a_transformar());
    s.definir_transformacao(Transformacao::deslocamento(-30.0, 5.0));
    em_rascunho(&s);
    assert!(s.aplicar_transformacao());
    exata_de_novo(&s);
}

#[test]
fn a_previa_do_filtro_rascunha_e_firma_no_ok_e_no_cancelar() {
    let mut s = sessao();
    let filtrada = || {
        CamadaDePixels::da_imagem(&RgbImage::from_fn(LARGURA, ALTURA, |x, y| {
            image::Rgb([(x % 97) as u8 * 2, (y % 89) as u8 * 2, 30])
        }))
    };
    assert!(s.comecar_filtro().is_some());
    s.mostrar_filtro(filtrada());
    em_rascunho(&s);
    s.cancelar_filtro();
    exata_de_novo(&s);

    assert!(s.comecar_filtro().is_some());
    s.mostrar_filtro(filtrada());
    em_rascunho(&s);
    assert!(s.aplicar_filtro("Teste"));
    exata_de_novo(&s);
}

#[test]
fn a_opacidade_rascunha_e_firma_ao_confirmar() {
    let mut s = sessao();
    s.mover_opacidade(0.3);
    em_rascunho(&s);
    s.mover_opacidade(0.8);
    em_rascunho(&s);
    s.confirmar_opacidade();
    exata_de_novo(&s);
}

/// O app pode trocar de ferramenta no meio do arrasto (sem o "soltar"): o
/// gesto seguinte fecha o aberto, e a tela não fica em rascunho.
#[test]
fn outro_gesto_no_meio_do_arrasto_firma_a_vista() {
    let mut s = sessao();
    s.mover_opacidade(0.3);
    em_rascunho(&s);
    s.nova_camada();
    exata_de_novo(&s);

    let mut s = sessao();
    assert!(s.comecar_a_mover());
    s.mover_por(9, 9);
    em_rascunho(&s);
    s.desfazer();
    exata_de_novo(&s);
}
