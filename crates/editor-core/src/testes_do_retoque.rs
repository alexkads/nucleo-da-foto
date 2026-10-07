//! ✂️ O retoque manual de queixo e pescoço do Photoshop, peça por peça, na
//! sessão: a camada da fotografia base, a camada via cópia com borda difusa e
//! a duplicação exata (etapa 15, `docs/editor-em-camadas/21-ETAPA-15.md`).

use std::path::Path;
use std::sync::Arc;

use image::RgbImage;

use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::projeto::{DiscoReal, Projeto};
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::Sessao;
use crate::tiles::CamadaDePixels;

/// 700 × 520 (três colunas de tiles, a última cortada), com textura.
fn base() -> Arc<RgbImage> {
    Arc::new(RgbImage::from_fn(700, 520, |x, y| {
        image::Rgb([
            (x * 7 % 256) as u8,
            (y * 5 % 256) as u8,
            ((x + 3 * y) % 200) as u8 + 30,
        ])
    }))
}

fn sessao() -> Sessao {
    let base = base();
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 350);
    s.pincel.suavizacao = 0.0;
    s
}

fn com_fotografia() -> Sessao {
    let mut s = sessao();
    let pixels = CamadaDePixels::da_imagem(s.base());
    assert!(s.criar_camada_da_fotografia(pixels));
    s
}

fn projeto(pasta: &Path) -> Projeto {
    Projeto::novo(pasta.join("e1"), Arc::new(DiscoReal))
}

// ------------------------------------------------- camada da fotografia

#[test]
fn a_camada_da_fotografia_e_a_base_opaca_logo_acima_dela() {
    let mut s = sessao();
    let base = s.base().clone();
    let camadas_antes = s.documento().camadas.len();
    assert!(s.criar_camada_da_fotografia(CamadaDePixels::da_imagem(&base)));
    let doc = s.documento();
    assert_eq!(doc.camadas.len(), camadas_antes + 1);
    assert_eq!(doc.camadas[0].nome, "Fotografia", "embaixo das outras");
    assert_eq!(s.ativa(), 0, "e escolhida");
    let p = &doc.camadas[0].pixels;
    for (x, y) in [(0, 0), (699, 519), (256, 300), (511, 13)] {
        let b = base.get_pixel(x, y).0;
        assert_eq!(p.pixel(x, y), [b[0], b[1], b[2], 255], "({x}, {y})");
    }
    assert_eq!(p.pixel_em(700, 10)[3], 0, "o resto do tile da borda é vazio");
    assert_eq!(p.quantos(), 3 * 3, "um tile por posição da foto");
    assert_eq!(doc.base, BaseRef::da_imagem(&base), "a referência da base fica");
    assert_eq!(s.compor().as_raw(), base.as_raw(), "compõe a base exata");
    assert!(s.alterado());

    assert!(s.desfazer());
    assert_eq!(s.documento().camadas.len(), camadas_antes);
    assert!(!s.alterado());
    assert!(s.refazer());
    assert_eq!(s.documento().camadas[0].nome, "Fotografia");
    assert_eq!(s.ativa(), 0);
}

#[test]
fn camada_de_outra_dimensao_e_recusada() {
    let mut s = sessao();
    assert!(!s.criar_camada_da_fotografia(CamadaDePixels::nova(10, 10)));
    assert_eq!(s.documento().camadas.len(), 1);
}

#[test]
fn a_camada_da_fotografia_sozinha_nao_publica_versao_editada() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = com_fotografia();
    let base = s.base().clone();
    let (doc, hist) = s.instantaneo();
    assert!(!doc.neutro(), "a camada tem pixels…");
    let salvo = projeto(dir.path()).salvar("e1", &base, &doc, &hist, 1).unwrap();
    assert!(salvo.versao.is_none(), "…mas compõe a base: sem versão (C30)");

    // Um retoque nela já é edição — e reabre igual.
    s.pincel.cor = [255, 0, 0];
    s.pincel.raio = 6.0;
    s.apertar(100.0, 100.0);
    s.arrastar(140.0, 100.0);
    s.soltar();
    let (doc, hist) = s.instantaneo();
    let salvo = projeto(dir.path()).salvar("e1", &base, &doc, &hist, 2).unwrap();
    assert!(salvo.versao.is_some());
    projeto(dir.path()).coletar(2).unwrap();
    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    assert_eq!(aberto.documento.camadas[0].nome, "Fotografia");
}

// --------------------------------------------- via cópia e duplicação

/// Seleção do retângulo `(200, 150, 220, 160)` difusa de 10 px — o trecho do
/// queixo.
fn selecao_difusa(s: &mut Sessao) {
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(200, 150, 220, 160)),
        Operacao::Nova,
    );
    assert!(s.difundir_selecao(10));
}

#[test]
fn via_copia_da_fotografia_leva_a_borda_difusa_e_tira_a_selecao() {
    let mut s = com_fotografia();
    selecao_difusa(&mut s);
    let sel = Arc::new(s.selecao().unwrap().clone());
    let borda = (200u32..230)
        .find(|&x| (1..255).contains(&sel.valor(x, 230)))
        .expect("a difusão faz borda parcial");
    assert!(s.camada_via_copia(false));
    assert!(s.selecao().is_none(), "a seleção sai com a cópia");
    assert_eq!(s.ativa(), 1, "a cópia, logo acima da fotografia");
    let copia = &s.documento().camadas[1].pixels;
    let b = s.base().get_pixel(borda, 230).0;
    assert_eq!(
        copia.pixel(borda, 230),
        [b[0], b[1], b[2], sel.valor(borda, 230)],
        "alfa = 255 × cobertura, cor da foto"
    );
    assert_eq!(copia.pixel(310, 230)[3], 255, "o miolo cheio");
    assert_eq!(copia.pixel(100, 100)[3], 0, "fora da seleção");

    // Um desfazer devolve a seleção e tira a camada.
    let passos = s.historico().passos().len();
    assert!(s.desfazer());
    assert_eq!(s.documento().camadas.len(), 2);
    assert_eq!(s.selecao().map(|x| x.valor(borda, 230)), Some(sel.valor(borda, 230)));
    assert!(s.refazer());
    assert_eq!(s.historico().passos().len(), passos);
}

#[test]
fn dois_ctrl_j_dao_duas_copias_identicas_sem_atenuar_a_borda() {
    let mut s = com_fotografia();
    selecao_difusa(&mut s);
    assert!(s.camada_via_copia(false));
    // O segundo ⌘J, sem seleção (ela saiu com a cópia), duplica exato.
    assert!(s.camada_via_copia(false));
    let doc = s.documento();
    assert_eq!(doc.camadas.len(), 4);
    assert_eq!(s.ativa(), 2);
    assert_eq!(
        doc.camadas[2].pixels, doc.camadas[1].pixels,
        "cópia exata, borda inclusive"
    );
    // "Duplicar camada" também é exato.
    s.duplicar_camada();
    assert_eq!(s.documento().camadas[3].pixels, s.documento().camadas[1].pixels);
}

// ------------------------------------------------------ máscara de corte

use crate::documento::{Camada, Mascara};
use crate::mesclagem::{mesclar, Modo};

/// Pinta `cor` com alfa `a` no retângulo `(x0, y0)–(x1, y1)` da camada.
fn pintar(c: &mut CamadaDePixels, x0: u32, y0: u32, x1: u32, y1: u32, cor: [u8; 3], a: u8) {
    for y in y0..y1 {
        for x in x0..x1 {
            let i = crate::tiles::indice(x % 256, y % 256);
            c.tile_mut(crate::tiles::tile_de(x, y))[i..i + 4]
                .copy_from_slice(&[cor[0], cor[1], cor[2], a]);
        }
    }
}

/// Fotografia + base (vermelha, borda em degraus de alfa) + recortada (azul),
/// sobre uma camada vazia: [vazia, base, recortada].
fn com_conjunto() -> Sessao {
    let mut s = sessao();
    let (l, a) = (700, 520);
    let mut base = Camada::nova("Base", l, a);
    pintar(&mut base.pixels, 100, 100, 300, 300, [220, 30, 30], 255);
    pintar(&mut base.pixels, 300, 100, 320, 300, [220, 30, 30], 128);
    let mut cima = Camada::nova("Cima", l, a);
    pintar(&mut cima.pixels, 150, 150, 400, 250, [20, 40, 230], 255);
    pintar(&mut cima.pixels, 150, 250, 400, 260, [20, 40, 230], 77);
    s.escolher_camada(0);
    s.nova_camada();
    s.nova_camada();
    let mut doc = s.documento().clone();
    doc.camadas[1] = base;
    doc.camadas[2] = cima;
    let base_img = s.base().clone();
    let mut s = Sessao::nova(base_img, doc, Historico::novo(), 350);
    s.pincel.suavizacao = 0.0;
    s
}

fn px(s: &Sessao, x: u32, y: u32) -> [u8; 3] {
    s.compor().get_pixel(x, y).0
}

#[test]
fn a_recortada_so_aparece_onde_a_base_tem_pixels() {
    let mut s = com_conjunto();
    let solto = s.compor();
    assert!(s.criar_mascara_de_corte(2));
    assert!(s.documento().camadas[2].recortada);
    assert_eq!(s.documento().base_do_recorte(2), Some(1));
    let foto = s.base().get_pixel(350, 200).0;
    assert_eq!(px(&s, 350, 200), foto, "fora da base a azul não aparece");
    assert_eq!(px(&s, 200, 200), [20, 40, 230], "dentro, a azul");
    assert_eq!(px(&s, 120, 120), [220, 30, 30], "a base fora da azul");
    assert_eq!(solto.get_pixel(350, 200).0, [20, 40, 230], "solta, aparecia");
    assert_eq!(s.historico().passos().len(), 1);
    assert!(s.desfazer());
    assert_eq!(s.compor(), solto);
    assert!(s.refazer());
    assert_eq!(px(&s, 350, 200), foto);
}

#[test]
fn borda_semitransparente_da_base_entra_uma_vez() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    let foto = s.base().get_pixel(310, 200).0;
    // Base com α 128, recortada opaca: a cor do conjunto é a azul, com o
    // alfa da base — uma vez.
    assert_eq!(
        px(&s, 310, 200),
        mesclar(foto, [20, 40, 230, 128], 1.0, Modo::Normal)
    );
    // Recortada com α 77 na base cheia: a azul mistura na vermelha por 77.
    let g = mesclar([220, 30, 30], [20, 40, 230, 77], 1.0, Modo::Normal);
    assert_eq!(px(&s, 200, 255), g);
    // Opacidade da base 50%: o conjunto inteiro a 50%, sem dobrar.
    let mut doc = s.documento().clone();
    doc.camadas[1].opacidade = 0.5;
    let s2 = Sessao::nova(s.base().clone(), doc, Historico::novo(), 350);
    let f = s.base().get_pixel(200, 200).0;
    assert_eq!(
        s2.compor().get_pixel(200, 200).0,
        mesclar(f, [20, 40, 230, 255], 0.5, Modo::Normal)
    );
}

#[test]
fn mascara_da_base_e_da_recortada_e_visibilidade() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    let foto = |x, y| s.base().get_pixel(x, y).0;
    let (f200, f120) = (foto(200, 200), foto(120, 120));
    let mut doc = s.documento().clone();
    // A máscara da base esconde a metade esquerda: o conjunto some ali.
    let mut m = Mascara::nova(255, 700, 520);
    pintar(&mut m.pixels, 0, 0, 220, 520, [0, 0, 0], 255);
    doc.camadas[1].mascara = Some(m);
    let s2 = Sessao::nova(s.base().clone(), doc.clone(), Historico::novo(), 350);
    assert_eq!(s2.compor().get_pixel(200, 200).0, f200, "a base esconde a recortada");
    assert_eq!(s2.compor().get_pixel(250, 200).0, [20, 40, 230]);
    // A máscara da recortada só esconde ela.
    doc.camadas[1].mascara = None;
    let mut m = Mascara::nova(255, 700, 520);
    pintar(&mut m.pixels, 0, 0, 220, 520, [0, 0, 0], 255);
    doc.camadas[2].mascara = Some(m);
    let s3 = Sessao::nova(s.base().clone(), doc.clone(), Historico::novo(), 350);
    assert_eq!(s3.compor().get_pixel(200, 200).0, [220, 30, 30]);
    // Recortada escondida: só a base.
    doc.camadas[2].mascara = None;
    doc.camadas[2].visivel = false;
    let s4 = Sessao::nova(s.base().clone(), doc.clone(), Historico::novo(), 350);
    assert_eq!(s4.compor().get_pixel(200, 200).0, [220, 30, 30]);
    // Base escondida: o conjunto inteiro some.
    doc.camadas[2].visivel = true;
    doc.camadas[1].visivel = false;
    let s5 = Sessao::nova(s.base().clone(), doc.clone(), Historico::novo(), 350);
    assert_eq!(s5.compor().get_pixel(200, 200).0, f200);
    assert_eq!(s5.compor().get_pixel(120, 120).0, f120);
    assert!(s5.documento().neutro(), "recortada sobre base escondida não é edição");
}

#[test]
fn base_sem_recortada_com_efeito_compoe_igual_a_solta() {
    let mut s = com_conjunto();
    let mut doc = s.documento().clone();
    doc.camadas[1].opacidade = 0.37;
    doc.camadas[2].visivel = false;
    let solta = Sessao::nova(s.base().clone(), doc.clone(), Historico::novo(), 350).compor();
    doc.camadas[2].recortada = true;
    s = Sessao::nova(s.base().clone(), doc, Historico::novo(), 350);
    assert_eq!(s.compor(), solta, "byte a byte");
}

#[test]
fn mesclar_a_recortada_na_base_nao_muda_a_foto() {
    let mut s = com_conjunto();
    let mut doc = s.documento().clone();
    // Recortada com opacidade, máscara cinza e Multiplicação; base a 80%.
    doc.camadas[2].opacidade = 0.6;
    doc.camadas[2].modo = Modo::Multiplicacao;
    let mut m = Mascara::nova(255, 700, 520);
    pintar(&mut m.pixels, 180, 0, 260, 520, [120, 120, 120], 255);
    doc.camadas[2].mascara = Some(m);
    doc.camadas[1].opacidade = 0.8;
    doc.camadas[2].recortada = true;
    s = Sessao::nova(s.base().clone(), doc, Historico::novo(), 350);
    s.escolher_camada(2);
    let antes = s.compor();
    let doc_antes = s.documento().clone();
    s.mesclar_para_baixo().unwrap();
    assert_eq!(s.documento().camadas.len(), 2);
    assert_eq!(s.ativa(), 1);
    assert_eq!(s.compor(), antes, "a mesma foto, byte a byte");
    assert_eq!(
        s.documento().camadas[1].pixels.pixel(330, 200)[3],
        0,
        "a base não cresce"
    );
    assert!(s.desfazer());
    assert_eq!(s.documento(), &doc_antes, "camadas, máscaras e recorte voltam");
}

#[test]
fn ctrl_e_na_base_mescla_o_conjunto_num_passo() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    s.escolher_camada(2);
    s.duplicar_camada();
    assert!(s.documento().camadas[3].recortada, "a cópia fica no conjunto");
    let mut doc = s.documento().clone();
    doc.camadas[3].opacidade = 0.4;
    doc.camadas[3].pixels = CamadaDePixels::nova(700, 520);
    pintar(&mut doc.camadas[3].pixels, 250, 120, 330, 280, [250, 250, 0], 200);
    let mut s = Sessao::nova(s.base().clone(), doc, Historico::novo(), 350);
    let antes = s.compor();
    let doc_antes = s.documento().clone();
    s.escolher_camada(1);
    s.mesclar_para_baixo().unwrap();
    assert_eq!(s.documento().camadas.len(), 2);
    assert_eq!(s.historico().passos().len(), 1);
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Mesclar máscara de corte"
    );
    assert_eq!(s.compor(), antes);
    assert!(s.desfazer());
    assert_eq!(s.documento(), &doc_antes);
}

#[test]
fn mesclar_uma_solta_numa_recortada_e_recusado() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    s.escolher_camada(2);
    s.nova_camada();
    // A nova entrou no topo do conjunto? Não: acima dela não há recortada.
    assert!(!s.documento().camadas[3].recortada);
    assert!(s.mesclar_para_baixo().is_err());
}

#[test]
fn camada_nova_no_meio_do_conjunto_entra_nele() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    s.escolher_camada(1);
    s.nova_camada();
    assert_eq!(s.ativa(), 2);
    assert!(s.documento().camadas[2].recortada);
    assert_eq!(s.documento().base_do_recorte(3), Some(1), "a azul segue na base");
}

#[test]
fn excluir_a_base_libera_as_recortadas_num_passo() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    let doc_antes = s.documento().clone();
    s.escolher_camada(1);
    assert!(s.excluir_camada());
    assert_eq!(s.documento().camadas.len(), 2);
    assert!(!s.documento().camadas[1].recortada, "a azul ficou solta");
    assert!(s.desfazer());
    assert_eq!(s.documento(), &doc_antes);
}

#[test]
fn mover_leva_o_conjunto_inteiro() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    s.nova_camada(); // [vazia, base, azul, nova] — escolhida a nova (3)
    assert!(!s.documento().camadas[3].recortada);
    s.escolher_camada(1);
    let foto_antes = s.compor();
    // A base sobe com a azul por cima da nova.
    assert!(s.mover_camada(1));
    let nomes: Vec<_> = s.documento().camadas.iter().map(|c| c.nome.clone()).collect();
    assert_eq!(nomes[2..], ["Base".to_string(), "Cima".to_string()]);
    assert_eq!(s.ativa(), 2);
    assert!(s.documento().camadas[3].recortada);
    assert_eq!(s.compor(), foto_antes, "a nova é vazia: a foto é a mesma");
    assert_eq!(s.historico().passos().len(), 3);
    // E desce de novo, inteira.
    assert!(s.mover_camada(-1));
    assert_eq!(s.documento().camadas[1].nome, "Base");
    assert_eq!(s.documento().camadas[2].nome, "Cima");
    assert_eq!(s.ativa(), 1);
    // A recortada desce para baixo da base e sai do conjunto.
    s.escolher_camada(2);
    assert!(s.mover_camada(-1));
    assert_eq!(s.documento().camadas[1].nome, "Cima");
    assert!(!s.documento().camadas[1].recortada);
    assert!(s.desfazer());
    assert!(s.documento().camadas[2].recortada);
}

#[test]
fn nao_recorta_a_de_baixo_de_todas_nem_por_ajuste() {
    let mut s = com_conjunto();
    assert!(!s.pode_recortar(0));
    assert!(!s.criar_mascara_de_corte(0));
    s.escolher_camada(1);
    s.nova_camada_de_ajuste(crate::ajuste::Ajuste::Inverter);
    s.nova_camada();
    let i = s.ativa();
    assert!(!s.pode_recortar(i), "a base seria a de ajuste");
}

#[test]
fn o_recorte_grava_e_reabre_no_formato_8() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = com_conjunto();
    let base = s.base().clone();
    s.criar_mascara_de_corte(2);
    let (doc, hist) = s.instantaneo();
    projeto(dir.path()).salvar("e1", &base, &doc, &hist, 1).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("e1/projeto.json")).unwrap())
            .unwrap();
    assert_eq!(json["formato"], 8);
    assert_eq!(json["camadas"][2]["recortada"], true);
    assert!(json["camadas"][1].get("recortada").is_none());
    projeto(dir.path()).coletar(1).unwrap();
    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    let mut s2 = Sessao::nova(base, aberto.documento, aberto.historico, 350);
    assert_eq!(s2.compor(), s.compor());
    assert!(s2.desfazer());
    assert!(!s2.documento().camadas[2].recortada);
}
