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
    assert_eq!(
        p.pixel_em(700, 10)[3],
        0,
        "o resto do tile da borda é vazio"
    );
    assert_eq!(p.quantos(), 3 * 3, "um tile por posição da foto");
    assert_eq!(
        doc.base,
        BaseRef::da_imagem(&base),
        "a referência da base fica"
    );
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
    let salvo = projeto(dir.path())
        .salvar("e1", &base, &doc, &hist, 1)
        .unwrap();
    assert!(
        salvo.versao.is_none(),
        "…mas compõe a base: sem versão (C30)"
    );

    // Um retoque nela já é edição — e reabre igual.
    s.pincel.cor = [255, 0, 0];
    s.pincel.raio = 6.0;
    s.apertar(100.0, 100.0);
    s.arrastar(140.0, 100.0);
    s.soltar();
    let (doc, hist) = s.instantaneo();
    let salvo = projeto(dir.path())
        .salvar("e1", &base, &doc, &hist, 2)
        .unwrap();
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
    assert_eq!(
        s.selecao().map(|x| x.valor(borda, 230)),
        Some(sel.valor(borda, 230))
    );
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
    assert_eq!(
        s.documento().camadas[3].pixels,
        s.documento().camadas[1].pixels
    );
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
    assert_eq!(
        solto.get_pixel(350, 200).0,
        [20, 40, 230],
        "solta, aparecia"
    );
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
    assert_eq!(
        s2.compor().get_pixel(200, 200).0,
        f200,
        "a base esconde a recortada"
    );
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
    assert!(
        s5.documento().neutro(),
        "recortada sobre base escondida não é edição"
    );
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
    assert_eq!(
        s.documento(),
        &doc_antes,
        "camadas, máscaras e recorte voltam"
    );
}

#[test]
fn ctrl_e_na_base_mescla_o_conjunto_num_passo() {
    let mut s = com_conjunto();
    s.criar_mascara_de_corte(2);
    s.escolher_camada(2);
    s.duplicar_camada();
    assert!(
        s.documento().camadas[3].recortada,
        "a cópia fica no conjunto"
    );
    let mut doc = s.documento().clone();
    doc.camadas[3].opacidade = 0.4;
    doc.camadas[3].pixels = CamadaDePixels::nova(700, 520);
    pintar(
        &mut doc.camadas[3].pixels,
        250,
        120,
        330,
        280,
        [250, 250, 0],
        200,
    );
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
    assert_eq!(
        s.documento().base_do_recorte(3),
        Some(1),
        "a azul segue na base"
    );
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
    let nomes: Vec<_> = s
        .documento()
        .camadas
        .iter()
        .map(|c| c.nome.clone())
        .collect();
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
    projeto(dir.path())
        .salvar("e1", &base, &doc, &hist, 1)
        .unwrap();
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

// ------------------------------------------------------------- deformar

use crate::deformar::{self, Malha};
use crate::transformar::{Caixa, Conteudo, Transformacao};

/// Um bloco opaco com textura em (200..320, 150..260), borda difusa de 6 px.
fn camada_com_bloco() -> CamadaDePixels {
    let mut c = CamadaDePixels::nova(700, 520);
    for y in 144..266u32 {
        for x in 194..326u32 {
            let dentro = |v: u32, a: u32, b: u32| -> f32 {
                let d = (v as f32 + 0.5 - a as f32).min(b as f32 - v as f32 - 0.5);
                ((d + 6.0) / 12.0).clamp(0.0, 1.0)
            };
            let a = (dentro(x, 200, 320) * dentro(y, 150, 260) * 255.0).round() as u8;
            if a == 0 {
                continue;
            }
            let i = crate::tiles::indice(x % 256, y % 256);
            c.tile_mut(crate::tiles::tile_de(x, y))[i..i + 4].copy_from_slice(&[
                (x * 3 % 256) as u8,
                (y * 7 % 256) as u8,
                ((x ^ y) % 256) as u8,
                a,
            ]);
        }
    }
    c
}

fn max_dif(a: [u8; 4], b: [u8; 4]) -> u8 {
    (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap()
}

#[test]
fn a_grade_parada_e_a_identidade_e_o_ctrl_t_vira_malha_exata() {
    let caixa = Caixa::nova(100, 50, 300, 120);
    let m = Malha::da_caixa(&caixa);
    for (u, v) in [(0.0, 0.0), (1.0, 1.0), (0.3, 0.71), (0.5, 0.5)] {
        let (x, y) = m.ponto(u, v);
        assert!((x - (100.0 + 300.0 * u)).abs() < 1e-3 && (y - (50.0 + 120.0 * v)).abs() < 1e-3);
    }
    let t = Transformacao {
        dx: 13.0,
        dy: -7.0,
        escala_x: 1.3,
        escala_y: 0.8,
        angulo: 0.4,
    };
    let mt = Malha::da_transformacao(&caixa, &t);
    for (u, v) in [(0.0, 0.0), (0.25, 0.8), (1.0, 0.5)] {
        let (x, y) = mt.ponto(u, v);
        let (ex, ey) = t.aplicar(&caixa, 100.0 + 300.0 * u, 50.0 + 120.0 * v);
        assert!(
            (x - ex).abs() < 1e-2 && (y - ey).abs() < 1e-2,
            "afim é exata"
        );
    }
}

#[test]
fn malha_parada_e_deslocada_reproduzem_o_conteudo_com_alfa() {
    let c = camada_com_bloco();
    let conteudo = Conteudo::da_camada(&c, None).unwrap();
    let m = Malha::da_caixa(&conteudo.caixa);
    let d = deformar::desenhar(&conteudo, &m, 700, 520);
    let mut desloc = m;
    for p in desloc.pontos.iter_mut().flatten() {
        p.0 += 37.0;
        p.1 -= 20.0;
    }
    let dd = deformar::desenhar(&conteudo, &desloc, 700, 520);
    for y in 140..270 {
        for x in 190..330 {
            assert!(
                max_dif(d.pixel(x, y), c.pixel(x, y)) <= 1,
                "identidade em ({x}, {y})"
            );
            assert!(
                max_dif(dd.pixel(x + 37, y - 20), c.pixel(x, y)) <= 1,
                "deslocada em ({x}, {y})"
            );
        }
    }
}

#[test]
fn levantar_a_mandibula_deforma_sem_buracos_e_so_dentro_da_caixa() {
    let c = camada_com_bloco();
    let conteudo = Conteudo::da_camada(&c, None).unwrap();
    let mut m = Malha::da_caixa(&conteudo.caixa);
    // Os dois pontos internos de baixo sobem 25 px, e a borda de baixo 10.
    m.mover_ponto(2, 1, 0.0, -25.0);
    m.mover_ponto(2, 2, 0.0, -25.0);
    m.mover_ponto(3, 1, 0.0, -10.0);
    m.mover_ponto(3, 2, 0.0, -10.0);
    let d = deformar::desenhar(&conteudo, &m, 700, 520);
    // Sem buracos: todo ponto do miolo levado pela malha cai num pixel opaco.
    let (cx, cy) = (conteudo.caixa.x as f32, conteudo.caixa.y as f32);
    let (l, a) = (conteudo.caixa.largura as f32, conteudo.caixa.altura as f32);
    let mut opacos = 0;
    for j in 0..60 {
        for i in 0..60 {
            let (u, v) = (0.15 + 0.7 * i as f32 / 59.0, 0.15 + 0.7 * j as f32 / 59.0);
            // Só onde o original é opaco.
            let (ox, oy) = (cx + u * l, cy + v * a);
            if c.pixel(ox as u32, oy as u32)[3] < 255 {
                continue;
            }
            let (x, y) = m.ponto(u, v);
            assert!(
                d.pixel(x as u32, y as u32)[3] >= 250,
                "buraco em ({x}, {y})"
            );
            opacos += 1;
        }
    }
    assert!(opacos > 1000);
    // O meio de baixo subiu; os cantos de cima ficaram.
    let (_, y_meio) = m.ponto(0.5, 1.0);
    assert!(y_meio < cy + a - 7.0, "¾ dos 10 px da borda");
    assert!(
        max_dif(d.pixel(201, 151), c.pixel(201, 151)) <= 2,
        "canto de cima parado"
    );
    // Nada fora da caixa da malha.
    assert_eq!(d.pixel(100, 100)[3], 0);
    assert_eq!(d.pixel(400, 300)[3], 0);
}

#[test]
fn malha_dobrada_degenerada_ou_absurda_nao_quebra() {
    let c = camada_com_bloco();
    let conteudo = Conteudo::da_camada(&c, None).unwrap();
    let mut m = Malha::da_caixa(&conteudo.caixa);
    // Dobra: o canto de cima à esquerda passa do de baixo à direita.
    m.mover_ponto(0, 0, 300.0, 250.0);
    let d = deformar::desenhar(&conteudo, &m, 700, 520);
    assert!(d
        .todos()
        .all(|(_, t)| t.len() == crate::tiles::BYTES_DO_TILE));
    // Tudo num ponto só: nada a desenhar.
    let mut ponto = m;
    for p in ponto.pontos.iter_mut().flatten() {
        *p = (10.0, 10.0);
    }
    assert!(deformar::desenhar(&conteudo, &ponto, 700, 520).vazia());
    // Não finito: recusado.
    let mut nan = m;
    nan.pontos[1][1] = (f32::NAN, 0.0);
    assert!(!nan.valida(700, 520));
    assert!(deformar::desenhar(&conteudo, &nan, 700, 520).vazia());
    let mut longe = m;
    longe.pontos[0][0] = (1e9, 0.0);
    assert!(!longe.valida(700, 520));
}

#[test]
fn puxar_por_dentro_leva_o_ponto_agarrado() {
    let caixa = Caixa::nova(0, 0, 300, 300);
    let mut m = Malha::da_caixa(&caixa);
    let antes = m.ponto(0.5, 0.8);
    m.puxar(0.5, 0.8, 0.0, -30.0);
    let depois = m.ponto(0.5, 0.8);
    assert!(
        (depois.1 - (antes.1 - 30.0)).abs() < 1e-2,
        "o ponto agarrado anda o arrasto"
    );
    assert!(
        (m.ponto(0.0, 0.0).1).abs() < 1.0,
        "o canto longe quase não anda"
    );
    let (u, v) = m.onde(depois.0, depois.1).unwrap();
    assert!((u - 0.5).abs() < 0.02 && (v - 0.8).abs() < 0.02);
    assert_eq!(m.pegar(0.4, 0.3, 5.0), Some(deformar::Pega::Ponto(0, 0)));
}

/// Fotografia, o trecho via cópia e a cópia dele, a de cima recortada pela de
/// baixo — o começo do retoque do queixo.
fn retoque_pronto_para_deformar() -> Sessao {
    let mut s = com_fotografia();
    selecao_difusa(&mut s);
    assert!(s.camada_via_copia(false));
    assert!(s.camada_via_copia(false));
    assert!(s.criar_mascara_de_corte(2));
    assert_eq!(s.ativa(), 2);
    s
}

#[test]
fn deformar_cancelar_redefinir_e_confirmar_num_passo() {
    let mut s = retoque_pronto_para_deformar();
    let doc0 = s.documento().clone();
    let passos0 = s.historico().passos().len();
    assert!(s.comecar_a_deformar());
    let (caixa, m0) = s.malha().unwrap();
    assert_eq!(m0, Malha::da_caixa(&caixa));
    let mut m = m0;
    m.mover_ponto(3, 1, 0.0, -18.0);
    m.mover_ponto(3, 2, 0.0, -18.0);
    s.definir_malha(m);
    assert_ne!(s.documento().camadas[2].pixels, doc0.camadas[2].pixels);
    // Esc devolve tudo.
    s.cancelar_transformacao();
    assert_eq!(s.documento(), &doc0);
    assert_eq!(s.historico().passos().len(), passos0);
    // Redefinir volta à grade sem confirmar; Enter sem mudança não faz passo.
    assert!(s.comecar_a_deformar());
    s.definir_malha(m);
    s.redefinir_malha();
    assert_eq!(s.documento(), &doc0);
    assert!(!s.aplicar_transformacao());
    assert_eq!(s.historico().passos().len(), passos0);
    // De novo, e Enter: um passo "Deformar".
    assert!(s.comecar_a_deformar());
    s.definir_malha(m);
    let deformado = s.documento().clone();
    assert!(s.aplicar_transformacao());
    assert_eq!(s.historico().passos().len(), passos0 + 1);
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Deformar"
    );
    assert!(s.desfazer());
    assert_eq!(s.documento(), &doc0);
    assert!(s.refazer());
    assert_eq!(s.documento(), &deformado);
    // A recortada não passa da de baixo: fora do trecho a foto é a base.
    let foto = s.compor();
    assert_eq!(foto.get_pixel(150, 100), s.base().get_pixel(150, 100));
}

#[test]
fn do_ctrl_t_ao_deformar_e_de_volta_sem_mexer() {
    let mut s = retoque_pronto_para_deformar();
    assert!(s.comecar_a_transformar());
    let (caixa, _) = s.transformacao().unwrap();
    s.definir_transformacao(Transformacao::deslocamento(12.0, 0.0));
    let movido = s.documento().camadas[2].pixels.clone();
    assert!(s.comecar_a_deformar());
    let (_, m) = s.malha().unwrap();
    assert_eq!(m.pontos[0][0], (caixa.x as f32 + 12.0, caixa.y as f32));
    assert_eq!(
        s.documento().camadas[2].pixels,
        movido,
        "a passagem não muda pixel"
    );
    assert!(s.malha_intocada());
    assert!(s.voltar_a_transformacao_livre());
    let mut m2 = m;
    m2.mover_ponto(1, 1, 5.0, 5.0);
    assert!(s.comecar_a_deformar());
    s.definir_malha(m2);
    assert!(!s.voltar_a_transformacao_livre(), "deformada não volta");
    s.cancelar_transformacao();
}

#[test]
fn deformar_para_fora_da_foto_guarda_e_traz_de_volta() {
    let mut s = retoque_pronto_para_deformar();
    assert!(s.comecar_a_deformar());
    let (_, mut m) = s.malha().unwrap();
    for p in m.pontos.iter_mut().flatten() {
        p.0 -= 400.0; // o trecho começa em x = 190: sai inteiro pela esquerda
    }
    s.definir_malha(m);
    assert!(s.aplicar_transformacao());
    let c = &s.documento().camadas[2].pixels;
    assert!(c.tem_fora());
    assert_eq!(s.compor().get_pixel(5, 230), s.base().get_pixel(5, 230));
    // Volta com o Mover, inteiro.
    assert!(s.comecar_a_mover());
    s.mover_por(400, 0);
    assert!(s.terminar_de_mover());
    let volta = &s.documento().camadas[2].pixels;
    let copia = &s.documento().camadas[1].pixels;
    let mut diferentes = 0;
    for y in 140..320 {
        for x in 180..440 {
            if max_dif(volta.pixel(x, y), copia.pixel(x, y)) > 1 {
                diferentes += 1;
            }
        }
    }
    assert_eq!(diferentes, 0, "deslocamento inteiro de ida e volta");
}

#[test]
fn deformado_grava_reabre_e_desfaz() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = retoque_pronto_para_deformar();
    let base = s.base().clone();
    assert!(s.comecar_a_deformar());
    let (_, mut m) = s.malha().unwrap();
    m.mover_ponto(3, 1, 0.0, -18.0);
    s.definir_malha(m);
    s.aplicar_transformacao();
    let (doc, hist) = s.instantaneo();
    projeto(dir.path())
        .salvar("e1", &base, &doc, &hist, 1)
        .unwrap();
    projeto(dir.path()).coletar(1).unwrap();
    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    let mut s2 = Sessao::nova(base, aberto.documento, aberto.historico, 350);
    assert_eq!(s2.compor(), s.compor());
    assert!(s2.desfazer());
    assert!(s.desfazer());
    assert_eq!(s2.documento(), s.documento());
}

// ------------------------------------------------ pincel de recuperação

use crate::pincel::Ferramenta;

/// "Pele" com poros (textura de ±12): clara (175) à esquerda de x = 350 e
/// mais escura (115) à direita.
fn pele() -> Sessao {
    let base = Arc::new(RgbImage::from_fn(700, 520, |x, y| {
        let poro = if (x / 3 + y / 3) % 2 == 0 { 12i32 } else { -12 };
        let tom = if x < 350 { 175 } else { 115 };
        let v = |d: i32| (tom + d + poro).clamp(0, 255) as u8;
        image::Rgb([v(20), v(0), v(-15)])
    }));
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 350);
    s.pincel.suavizacao = 0.0;
    s
}

fn media_e_desvio(img: &RgbImage, x0: u32, y0: u32, lado: u32) -> (f32, f32) {
    let v: Vec<f32> = (y0..y0 + lado)
        .flat_map(|y| (x0..x0 + lado).map(move |x| (x, y)))
        .map(|(x, y)| img.get_pixel(x, y).0[1] as f32)
        .collect();
    let m = v.iter().sum::<f32>() / v.len() as f32;
    let d = (v.iter().map(|a| (a - m).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
    (m, d)
}

fn pincelada(s: &mut Sessao, ferramenta: Ferramenta) -> RgbImage {
    s.pincel.ferramenta = ferramenta;
    s.pincel.raio = 25.0;
    s.pincel.dureza = 0.6;
    s.definir_origem(150.0, 200.0); // pele clara
    assert!(s.apertar(500.0, 200.0)); // pele escura
    for k in 1..=40 {
        s.arrastar(500.0, 200.0 + k as f32 * 2.0);
    }
    assert!(s.soltar());
    s.compor()
}

#[test]
fn a_recuperacao_leva_a_textura_e_adapta_a_luz_ao_destino() {
    let mut carimbo = pele();
    carimbo.nova_camada();
    let clonado = pincelada(&mut carimbo, Ferramenta::Carimbo);
    let mut s = pele();
    s.nova_camada();
    let curado = pincelada(&mut s, Ferramenta::Recuperacao);
    let (m_carimbo, _) = media_e_desvio(&clonado, 490, 230, 20);
    let (m_curado, d_curado) = media_e_desvio(&curado, 490, 230, 20);
    let (m_destino, d_destino) = media_e_desvio(s.base(), 490, 230, 20);
    assert!(
        (m_carimbo - 175.0).abs() < 6.0,
        "o carimbo traz a luz da origem: {m_carimbo}"
    );
    assert!(
        (m_curado - m_destino).abs() < 8.0,
        "a recuperação, a do destino: {m_curado} × {m_destino}"
    );
    assert!(
        d_curado > 0.6 * d_destino,
        "a textura vem junto: {d_curado} × {d_destino}"
    );
    // Só a camada de cima mudou; a base e a fotografia não.
    assert!(s.documento().camadas[0].pixels.vazia());
    assert!(!s.documento().camadas[1].pixels.vazia());
    assert_eq!(s.historico().passos().len(), 2, "nova camada + um traço");
    assert!(s.desfazer());
    assert!(s.documento().camadas[1].pixels.vazia());
}

#[test]
fn a_recuperacao_respeita_a_selecao_e_a_borda_some() {
    let mut s = pele();
    s.nova_camada();
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(480, 180, 60, 60)),
        Operacao::Nova,
    );
    let curado = pincelada(&mut s, Ferramenta::Recuperacao);
    let base = s.base().clone();
    assert_eq!(
        curado.get_pixel(500, 260),
        base.get_pixel(500, 260),
        "fora da seleção"
    );
    assert_ne!(curado.get_pixel(500, 210), base.get_pixel(500, 210));
    // Sem emenda: na borda da seleção, a média de um lado e do outro casa.
    let (dentro, _) = media_e_desvio(&curado, 500, 232, 6);
    let (fora, _) = media_e_desvio(&curado, 500, 242, 6);
    assert!((dentro - fora).abs() < 10.0, "{dentro} × {fora}");
}

#[test]
fn a_recuperacao_sem_origem_nao_pinta() {
    let mut s = pele();
    s.nova_camada();
    s.pincel.ferramenta = Ferramenta::Recuperacao;
    assert!(!s.apertar(500.0, 200.0), "⌥ + clique na origem antes");
}
