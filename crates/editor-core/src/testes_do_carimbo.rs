//! 🖃 O carimbo com as opções do Photoshop (etapa 14): alinhado ligável, a
//! amostra (camada atual, atual e abaixo, todas), o modo da ferramenta
//! separado do modo da camada e a prévia da origem.

use std::sync::Arc;

use image::RgbImage;

use crate::carimbo::AmostraDoCarimbo;
use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::mesclagem::Modo;
use crate::pincel::Ferramenta;
use crate::sessao::Sessao;

/// 600×400: a base cinza (100) com um quadrado verde em (0..100, 0..100).
fn sessao() -> Sessao {
    let base = Arc::new(RgbImage::from_fn(600, 400, |x, y| {
        if x < 100 && y < 100 {
            image::Rgb([0, 200, 0])
        } else {
            image::Rgb([100, 100, 100])
        }
    }));
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 300);
    s.pincel.ferramenta = Ferramenta::Carimbo;
    s.pincel.raio = 6.0;
    s.pincel.dureza = 1.0;
    s.pincel.suavizacao = 0.0;
    s
}

fn pixel(s: &Sessao, camada: usize, x: u32, y: u32) -> [u8; 4] {
    s.documento().camadas[camada].pixels.pixel(x, y)
}

fn clique(s: &mut Sessao, x: f32, y: f32) {
    assert!(s.apertar(x, y));
    s.soltar();
}

#[test]
fn alinhado_mantem_a_distancia_e_sem_alinhar_volta_a_origem() {
    let mut s = sessao();
    s.definir_origem(50.0, 50.0); // no verde
    clique(&mut s, 300.0, 300.0); // copia o verde para (300, 300)
    assert_eq!(pixel(&s, 0, 300, 300), [0, 200, 0, 255]);
    // Alinhado: o segundo traço em (400, 300) copia de (150, 50) — cinza.
    clique(&mut s, 400.0, 300.0);
    assert_eq!(pixel(&s, 0, 400, 300), [100, 100, 100, 255]);
    assert_eq!(s.mira_do_carimbo(400.0, 300.0), Some((150.0, 50.0)));

    // Sem alinhar: cada traço volta a copiar da origem — verde de novo.
    s.carimbo.alinhado = false;
    assert_eq!(s.mira_do_carimbo(500.0, 200.0), Some((50.0, 50.0)));
    clique(&mut s, 500.0, 200.0);
    assert_eq!(pixel(&s, 0, 500, 200), [0, 200, 0, 255]);
    clique(&mut s, 200.0, 300.0);
    assert_eq!(pixel(&s, 0, 200, 300), [0, 200, 0, 255]);
}

#[test]
fn a_amostra_escolhe_de_onde_copiar() {
    let mut s = sessao();
    // Na camada 0 (a escolhida), um vermelho em (200..220, 50..70); uma
    // camada 1 por cima com azul em (400..420, 50..70).
    s.pincel.ferramenta = Ferramenta::Pincel;
    s.pincel.cor = [255, 0, 0];
    clique(&mut s, 210.0, 60.0);
    s.nova_camada();
    s.pincel.cor = [0, 0, 255];
    clique(&mut s, 410.0, 60.0);
    s.escolher_camada(0);
    s.pincel.ferramenta = Ferramenta::Carimbo;

    // Atual e abaixo (o padrão): o verde da base vem, o azul de cima não.
    s.definir_origem(50.0, 50.0);
    clique(&mut s, 300.0, 300.0);
    assert_eq!(pixel(&s, 0, 300, 300), [0, 200, 0, 255]);
    s.definir_origem(410.0, 60.0);
    clique(&mut s, 300.0, 200.0);
    assert_eq!(
        pixel(&s, 0, 300, 200),
        [100, 100, 100, 255],
        "a camada de cima não entra"
    );

    // Todas: o azul de cima vem.
    s.carimbo.amostra = AmostraDoCarimbo::Todas;
    s.definir_origem(410.0, 60.0);
    clique(&mut s, 500.0, 300.0);
    assert_eq!(pixel(&s, 0, 500, 300), [0, 0, 255, 255]);

    // Camada atual: do verde da base não vem nada (a camada é transparente
    // ali); do vermelho da camada, vem.
    s.carimbo.amostra = AmostraDoCarimbo::CamadaAtual;
    s.definir_origem(50.0, 50.0);
    clique(&mut s, 100.0, 300.0);
    assert_eq!(pixel(&s, 0, 100, 300)[3], 0, "copiar do vazio não pinta");
    s.definir_origem(210.0, 60.0);
    clique(&mut s, 100.0, 200.0);
    assert_eq!(pixel(&s, 0, 100, 200), [255, 0, 0, 255]);
}

#[test]
fn o_modo_da_ferramenta_e_separado_do_da_camada() {
    let mut s = sessao();
    // Uma área cinza-claro pintada na camada.
    s.pincel.ferramenta = Ferramenta::Pincel;
    s.pincel.cor = [200, 200, 200];
    s.pincel.raio = 30.0;
    clique(&mut s, 300.0, 300.0);
    // O carimbo em Multiplicação copia o verde (0, 200, 0) por cima.
    s.pincel.ferramenta = Ferramenta::Carimbo;
    s.pincel.raio = 6.0;
    s.pincel.modo = Modo::Multiplicacao;
    s.definir_origem(50.0, 50.0);
    clique(&mut s, 300.0, 300.0);
    // 200 × 200 / 255 ≈ 157 no verde; 0 nos outros.
    let p = pixel(&s, 0, 300, 300);
    assert_eq!((p[0], p[2], p[3]), (0, 0, 255));
    assert!((155..=158).contains(&p[1]), "{p:?}");
    assert_eq!(
        s.documento().camadas[0].modo,
        Modo::Normal,
        "a camada não mudou de modo"
    );
    // Sobre transparente, não há com o que multiplicar: entra o verde.
    s.definir_origem(50.0, 50.0);
    clique(&mut s, 500.0, 100.0);
    assert_eq!(pixel(&s, 0, 500, 100), [0, 200, 0, 255]);
}

#[test]
fn a_previa_mostra_o_que_a_origem_daria() {
    let mut s = sessao();
    assert!(
        s.previa_do_carimbo(300.0, 300.0, 10.0).is_none(),
        "sem origem"
    );
    s.definir_origem(50.0, 50.0);
    let (ret, img) = s.previa_do_carimbo(300.0, 300.0, 10.0).unwrap();
    assert_eq!((ret.x, ret.y, ret.largura, ret.altura), (290, 290, 20, 20));
    assert_eq!(img.get_pixel(10, 10).0, [0, 200, 0, 255]);
    // Camada atual, vazia: a prévia é transparente.
    s.carimbo.amostra = AmostraDoCarimbo::CamadaAtual;
    let (_, img) = s.previa_do_carimbo(300.0, 300.0, 10.0).unwrap();
    assert_eq!(img.get_pixel(10, 10).0[3], 0);
}
