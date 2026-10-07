//! 🧱 O conteúdo fora da foto (etapa 14, formato 7): o que o Mover e o ⌘T levam
//! para fora da foto continua na camada, volta inteiro, se desfaz, e grava e
//! reabre — sem aparecer na composta.

use std::path::Path;
use std::sync::Arc;

use image::RgbImage;

use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::projeto::{DiscoReal, Projeto, FORMATO};
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::Sessao;
use crate::transformar::Transformacao;

const VERMELHO: [u8; 4] = [220, 30, 30, 255];

/// 600×400 com um quadrado vermelho na camada em (20..80, 20..80).
fn sessao() -> Sessao {
    let base = Arc::new(RgbImage::from_pixel(600, 400, image::Rgb([90, 90, 90])));
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 300);
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(20, 20, 60, 60)),
        Operacao::Nova,
    );
    s.pincel.cor = [220, 30, 30];
    s.preencher_selecao();
    s.desmarcar();
    s
}

fn camada(s: &Sessao) -> &crate::tiles::CamadaDePixels {
    &s.documento().camadas[0].pixels
}

#[test]
fn o_mover_leva_para_fora_e_traz_de_volta_inteiro() {
    let mut s = sessao();
    // Para fora, à esquerda e acima.
    assert!(s.comecar_a_mover());
    s.mover_por(-300, -200);
    assert!(s.terminar_de_mover());
    assert!(camada(&s)
        .existentes()
        .all(|(_, t)| t.iter().skip(3).step_by(4).all(|a| *a == 0)));
    assert!(camada(&s).tem_fora(), "o quadrado mora fora da foto");
    assert_eq!(camada(&s).pixel_em(-260, -160), VERMELHO);
    assert_eq!(s.compor().get_pixel(50, 50).0, [90, 90, 90], "não aparece");
    assert_eq!(
        s.historico()
            .a_desfazer()
            .map(|p| p.descricao(s.documento())),
        Some("Mover".to_string())
    );
    // Num gesto novo, de volta.
    assert!(s.comecar_a_mover());
    s.mover_por(300, 200);
    assert!(s.terminar_de_mover());
    assert_eq!(camada(&s).pixel(20, 20), VERMELHO);
    assert_eq!(camada(&s).pixel(79, 79), VERMELHO, "inteiro, sem corte");
    assert!(!camada(&s).tem_fora());
    // O desfazer anda pelos dois gestos.
    assert!(s.desfazer());
    assert_eq!(camada(&s).pixel_em(-221, -121), VERMELHO);
    assert!(s.desfazer());
    assert_eq!(camada(&s).pixel(50, 50), VERMELHO);
}

#[test]
fn o_mover_para_alem_da_direita_tambem_guarda() {
    let mut s = sessao();
    assert!(s.comecar_a_mover());
    s.mover_por(560, 0); // metade passa da borda direita (x ≥ 600)
    assert!(s.terminar_de_mover());
    assert_eq!(camada(&s).pixel(590, 50), VERMELHO);
    assert_eq!(camada(&s).pixel_em(620, 50), VERMELHO);
    assert!(s.comecar_a_mover());
    s.mover_por(-560, 0);
    assert!(s.terminar_de_mover());
    assert_eq!(camada(&s).pixel(79, 50), VERMELHO);
}

#[test]
fn a_transformacao_livre_guarda_o_que_sai_e_traz_de_volta() {
    let mut s = sessao();
    assert!(s.comecar_a_transformar());
    s.definir_transformacao(Transformacao::deslocamento(-70.0, 0.0));
    assert!(s.aplicar_transformacao());
    // Metade fora: (−50..10) — o que ficou dentro é (0..10).
    assert_eq!(camada(&s).pixel(5, 50), VERMELHO);
    assert_eq!(camada(&s).pixel_em(-40, 50), VERMELHO);
    // O ⌘T seguinte enxerga a caixa inteira, com o pedaço de fora.
    assert!(s.comecar_a_transformar());
    let (caixa, _) = s.transformacao().unwrap();
    assert_eq!((caixa.x, caixa.largura), (-50, 60));
    s.definir_transformacao(Transformacao::deslocamento(70.0, 0.0));
    assert!(s.aplicar_transformacao());
    assert_eq!(camada(&s).pixel(20, 50), VERMELHO);
    assert_eq!(camada(&s).pixel(79, 50), VERMELHO);
    assert!(!camada(&s).tem_fora());
}

fn projeto(pasta: &Path) -> Projeto {
    Projeto::novo(pasta.join("e1"), Arc::new(DiscoReal))
}

#[test]
fn o_de_fora_grava_e_reabre_no_formato_7() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = sessao();
    let base = s.base().clone();
    assert!(s.comecar_a_mover());
    s.mover_por(-300, 0);
    assert!(s.terminar_de_mover());
    let (doc, hist) = s.instantaneo();
    let p = projeto(dir.path());
    p.salvar("e1", &base, &doc, &hist, 1).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("e1/projeto.json")).unwrap())
            .unwrap();
    assert_eq!(json["formato"], FORMATO);
    assert_eq!(FORMATO, 7);
    p.coletar(1).unwrap();
    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    let mut s2 = Sessao::nova(base, aberto.documento, aberto.historico, 300);
    assert_eq!(
        s2.documento().camadas[0].pixels.pixel_em(-250, 50),
        VERMELHO
    );
    // E volta para dentro depois de reabrir.
    assert!(s2.comecar_a_mover());
    s2.mover_por(300, 0);
    assert!(s2.terminar_de_mover());
    assert_eq!(s2.documento().camadas[0].pixels.pixel(50, 50), VERMELHO);
    // O desfazer gravado também sabe dos tiles de fora.
    assert!(s2.desfazer());
    assert!(s2.desfazer());
    assert_eq!(s2.documento().camadas[0].pixels.pixel(50, 50), VERMELHO);
}
