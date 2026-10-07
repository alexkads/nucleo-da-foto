//! 🎭 A máscara de camada ponta a ponta, no contrato do Photoshop: pintar de
//! preto na máscara da camada de cima revela, **exatamente**, o que está
//! embaixo — as camadas de baixo e a base — sem tocar nos pixels de ninguém.
//!
//! ```text
//! alfa efetivo = alfa do pixel × opacidade da camada × valor da máscara / 255
//! ```
//!
//! O cenário: uma foto em degradê (a base), a camada de baixo com um
//! retângulo azul, e a de cima toda vermelha — com a máscara.

use std::sync::Arc;

use image::RgbImage;

use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::mesclagem::{mesclar, Modo};
use crate::pincel::Ferramenta;
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::Sessao;
use crate::{composicao, Camada};

pub(crate) fn cenario() -> Sessao {
    let base = Arc::new(RgbImage::from_fn(800, 600, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 97) as u8])
    }));
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 400);
    // A de baixo: um retângulo azul.
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(100, 100, 600, 400)),
        Operacao::Nova,
    );
    s.pincel.cor = [0, 0, 255];
    assert!(s.preencher_selecao());
    // A de cima: vermelha, cheia.
    s.nova_camada();
    s.selecionar_tudo();
    s.pincel.cor = [255, 0, 0];
    assert!(s.preencher_selecao());
    s.desmarcar();
    s
}

/// A foto como seria sem a camada de cima — "o que está embaixo".
fn sem_a_de_cima(s: &Sessao) -> RgbImage {
    let mut doc = s.documento().clone();
    doc.camadas.truncate(1);
    composicao::compor(s.base(), &doc)
}

/// Um traço reto de pincel duro, de `de` a `ate`.
fn tracar(s: &mut Sessao, cor: [u8; 3], opacidade: f32, de: (f32, f32), ate: (f32, f32)) {
    s.pincel.ferramenta = Ferramenta::Pincel;
    s.pincel.cor = cor;
    s.pincel.opacidade = opacidade;
    s.pincel.dureza = 1.0;
    s.pincel.raio = 30.0;
    assert!(s.apertar(de.0, de.1));
    s.arrastar(ate.0, ate.1);
    assert!(s.soltar());
}

fn pixel(img: &RgbImage, x: u32, y: u32) -> [u8; 3] {
    img.get_pixel(x, y).0
}

#[test]
fn preto_revela_o_de_baixo_exatamente_e_branco_restaura_sem_mexer_nos_pixels() {
    let mut s = cenario();
    let pixels_antes = s.documento().camadas[1].pixels.clone();
    assert!(s.adicionar_mascara(false));
    assert!(s.na_mascara(), "a máscara nova recebe o pincel");
    assert_eq!(
        s.compor().get_pixel(400, 300).0,
        [255, 0, 0],
        "branca: tudo visível"
    );

    // Preto de (150, 300) a (650, 300): passa pelo azul e pela base.
    tracar(&mut s, [0; 3], 1.0, (150.0, 300.0), (650.0, 300.0));
    let feita = s.compor();
    let embaixo = sem_a_de_cima(&s);
    for x in [150, 300, 512, 650] {
        assert_eq!(pixel(&feita, x, 300), pixel(&embaixo, x, 300), "x = {x}");
    }
    assert_eq!(pixel(&feita, 400, 300), [0, 0, 255], "o azul da de baixo");
    assert_eq!(
        pixel(&feita, 400, 100),
        [255, 0, 0],
        "fora do traço, a de cima"
    );
    // Nem os pixels da camada nem os da de baixo mudaram: só a máscara.
    assert_eq!(s.documento().camadas[1].pixels, pixels_antes);
    assert!(
        s.documento().camadas[1]
            .mascara
            .as_ref()
            .unwrap()
            .pixels
            .quantos()
            > 0
    );

    // Branco por cima: a de cima volta exatamente.
    tracar(&mut s, [255; 3], 1.0, (150.0, 300.0), (650.0, 300.0));
    assert_eq!(pixel(&s.compor(), 400, 300), [255, 0, 0]);
    assert_eq!(s.documento().camadas[1].pixels, pixels_antes);
}

#[test]
fn cinza_mistura_e_a_mascara_se_multiplica_com_a_opacidade_e_o_modo() {
    let mut s = cenario();
    s.adicionar_mascara(false);
    tracar(&mut s, [128; 3], 1.0, (300.0, 300.0), (500.0, 300.0));
    let embaixo = sem_a_de_cima(&s);
    let esperado = |s: &Sessao, x: u32, modo: Modo| {
        let opacidade = s.documento().camadas[1].opacidade;
        mesclar(
            pixel(&embaixo, x, 300),
            [255, 0, 0, 255],
            opacidade * 128.0 / 255.0,
            modo,
        )
    };
    assert_eq!(
        pixel(&s.compor(), 400, 300),
        esperado(&s, 400, Modo::Normal)
    );

    // Opacidade da camada a 50%: os dois se multiplicam.
    s.escolher_camada(1);
    s.mover_opacidade(0.5);
    s.confirmar_opacidade();
    assert_eq!(
        pixel(&s.compor(), 400, 300),
        esperado(&s, 400, Modo::Normal)
    );
    // E o modo de mesclagem vale do mesmo jeito.
    s.mudar_modo(Modo::Multiplicacao);
    assert_eq!(
        pixel(&s.compor(), 400, 300),
        esperado(&s, 400, Modo::Multiplicacao)
    );
    assert_eq!(pixel(&s.compor(), 250, 300), {
        let o = 0.5;
        mesclar(
            pixel(&embaixo, 250, 300),
            [255, 0, 0, 255],
            o,
            Modo::Multiplicacao,
        )
    });
}

#[test]
fn desligar_a_mascara_mostra_a_camada_inteira_e_ligar_volta() {
    let mut s = cenario();
    s.adicionar_mascara(false);
    tracar(&mut s, [0; 3], 1.0, (150.0, 300.0), (650.0, 300.0));
    let com = s.compor();
    assert!(s.alternar_mascara_de(1));
    assert_eq!(pixel(&s.compor(), 400, 300), [255, 0, 0]);
    let mut sem_mascara = s.documento().clone();
    sem_mascara.camadas[1].mascara = None;
    assert_eq!(
        s.compor().as_raw(),
        composicao::compor(s.base(), &sem_mascara).as_raw()
    );
    assert!(s.alternar_mascara_de(1));
    assert_eq!(s.compor().as_raw(), com.as_raw());
}

#[test]
fn um_traco_e_um_passo_do_desfazer_e_vai_e_volta_exato() {
    let mut s = cenario();
    s.adicionar_mascara(false);
    let antes = s.compor();
    let passos = s.historico().passos().len();
    tracar(&mut s, [0; 3], 1.0, (150.0, 300.0), (650.0, 300.0));
    assert_eq!(s.historico().passos().len(), passos + 1);
    let depois = s.compor();
    assert!(s.desfazer());
    assert_eq!(s.compor().as_raw(), antes.as_raw());
    assert!(
        s.na_mascara(),
        "desfazer o traço da máscara deixa o pincel nela"
    );
    assert!(s.refazer());
    assert_eq!(s.compor().as_raw(), depois.as_raw());
}

#[test]
fn ir_e_voltar_no_mesmo_traco_nao_acumula_mas_dois_tracos_sim() {
    // O contrato do pincel: dentro de um traço vale a cobertura máxima — o
    // resultado não depende de quantos eventos o ponteiro mandou.
    let mut s = cenario();
    s.adicionar_mascara(false);
    s.pincel = crate::pincel::Pincel {
        cor: [0; 3],
        opacidade: 0.5,
        dureza: 1.0,
        raio: 30.0,
        ..Default::default()
    };
    assert!(s.apertar(300.0, 300.0));
    for _ in 0..6 {
        s.arrastar(500.0, 300.0);
        s.arrastar(300.0, 300.0);
    }
    s.soltar();
    let valor = |s: &Sessao| {
        s.documento().camadas[1]
            .mascara
            .as_ref()
            .unwrap()
            .valor(400, 300)
    };
    let uma = valor(&s);
    assert!(
        (126..=129).contains(&uma),
        "meia cobertura numa passada ({uma})"
    );
    // Um segundo traço acumula, como no Photoshop.
    s.apertar(300.0, 300.0);
    s.arrastar(500.0, 300.0);
    s.soltar();
    let duas = valor(&s);
    assert!((62..=66).contains(&duas), "dois traços: um quarto ({duas})");
}

#[test]
fn o_traco_atravessa_a_emenda_dos_tiles_sem_degrau() {
    let mut s = cenario();
    s.adicionar_mascara(false);
    // O tile vai de 0 a 255: o traço passa por x = 255 | 256 e y = 255 | 256.
    tracar(&mut s, [0; 3], 1.0, (200.0, 256.0), (320.0, 256.0));
    let m = s.documento().camadas[1].mascara.as_ref().unwrap().clone();
    for y in 240..272 {
        assert_eq!(m.valor(255, y), m.valor(256, y), "y = {y}");
    }
    for x in 220..300 {
        assert_eq!(m.valor(x, 255), m.valor(x, 256), "x = {x}");
    }
    let feita = s.compor();
    let embaixo = sem_a_de_cima(&s);
    for (x, y) in [(255, 256), (256, 256), (255, 255), (256, 255)] {
        assert_eq!(pixel(&feita, x, y), pixel(&embaixo, x, y));
    }
}

#[test]
fn a_vista_acompanha_o_traco_na_mascara() {
    // A prévia (a vista reduzida) refeita pelo traço é a mesma de uma vista
    // nova montada do documento.
    let mut s = cenario();
    s.adicionar_mascara(false);
    tracar(&mut s, [0; 3], 1.0, (150.0, 300.0), (650.0, 300.0));
    let nova = crate::vista::Vista::nova(s.base(), s.documento(), 400);
    assert_eq!(s.vista().imagem().as_raw(), nova.imagem().as_raw());
}

#[test]
fn a_camada_de_baixo_continua_pintavel_e_a_mascara_so_vale_para_a_de_cima() {
    let mut s = cenario();
    s.adicionar_mascara(false);
    tracar(&mut s, [0; 3], 1.0, (150.0, 300.0), (650.0, 300.0));
    // Na de baixo, o pincel pinta pixels (não máscara).
    s.escolher_camada(0);
    assert!(!s.na_mascara());
    let de_baixo: &Camada = &s.documento().camadas[0];
    assert!(de_baixo.mascara.is_none());
    tracar(&mut s, [0, 255, 0], 1.0, (400.0, 300.0), (401.0, 300.0));
    assert_eq!(
        pixel(&s.compor(), 400, 300),
        [0, 255, 0],
        "a de baixo aparece pelo buraco"
    );
}

#[test]
fn x_e_d_trocam_as_cores_e_a_borracha_na_mascara_pinta_a_cor_de_fundo() {
    let mut s = cenario();
    // D: preto e branco; X: trocam.
    s.pincel.cor = [10, 200, 30];
    s.cores_padrao();
    assert_eq!((s.pincel.cor, s.pincel.cor_de_fundo), ([0; 3], [255; 3]));
    s.trocar_cores();
    assert_eq!((s.pincel.cor, s.pincel.cor_de_fundo), ([255; 3], [0; 3]));
    s.trocar_cores();

    // ⌥ + máscara (esconde tudo): a borracha nela revela, como no Photoshop,
    // porque pinta o fundo branco.
    assert!(s.adicionar_mascara(true));
    assert_eq!(
        pixel(&s.compor(), 400, 300),
        [0, 0, 255],
        "escondida: o azul de baixo"
    );
    s.pincel.ferramenta = Ferramenta::Borracha;
    s.pincel.dureza = 1.0;
    s.pincel.raio = 30.0;
    assert!(s.apertar(400.0, 300.0));
    s.soltar();
    assert_eq!(
        pixel(&s.compor(), 400, 300),
        [255, 0, 0],
        "a borracha revelou"
    );
    assert_eq!(pixel(&s.compor(), 600, 300), [0, 0, 255]);
}

#[test]
fn ao_ir_para_a_mascara_as_cores_viram_cinza() {
    let mut s = cenario();
    s.pincel.cor = [255, 0, 0];
    s.pincel.cor_de_fundo = [0, 0, 255];
    s.adicionar_mascara(false);
    assert_eq!(s.pincel.cor, [77; 3], "o cinza do vermelho");
    assert_eq!(s.pincel.cor_de_fundo, [29; 3], "o cinza do azul");
    // Nos pixels da camada, a cor escolhida depois fica como está.
    s.escolher_camada(1);
    s.pincel.cor = [255, 0, 0];
    assert_eq!(s.pincel.cor, [255, 0, 0]);
    assert!(s.escolher_mascara(1));
    assert_eq!(s.pincel.cor, [77; 3]);
}
