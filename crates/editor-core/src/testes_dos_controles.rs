//! 🎛️ Os controles da etapa 13 (compatibilidade com o Photoshop) de ponta a
//! ponta na sessão: um traço é um passo, ⇧ + clique liga com uma reta, a
//! seleção entra no desfazer (com a interseção ⇧⌥), o contorno anda sem os
//! pixels, a camada via recorte é um passo só — e o projeto grava e reabre
//! tudo isso sem levar a seleção.

use std::path::Path;
use std::sync::Arc;

use image::RgbImage;

use crate::documento::{BaseRef, Documento};
use crate::historico::{Comando, Historico};
use crate::projeto::{DiscoReal, Projeto};
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::Sessao;

fn sessao() -> Sessao {
    let base = Arc::new(RgbImage::from_fn(800, 600, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 70])
    }));
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 400);
    s.pincel.suavizacao = 0.0;
    s
}

fn retangulo(x: u32, y: u32, l: u32, a: u32) -> Forma {
    Forma::Retangulo(Retangulo::novo(x, y, l, a))
}

fn selecionado(s: &Sessao, x: u32, y: u32) -> u8 {
    s.selecao().map_or(0, |sel| sel.valor(x, y))
}

#[test]
fn um_traco_com_muitos_eventos_e_um_passo_do_desfazer() {
    let mut s = sessao();
    s.pincel.cor = [255, 0, 0];
    s.apertar(100.0, 100.0);
    for i in 0..500 {
        s.arrastar(100.0 + i as f32, 100.0 + (i as f32 * 0.3).sin() * 40.0);
    }
    assert!(s.soltar());
    assert_eq!(s.historico().passos().len(), 1);
    assert!(s.desfazer());
    assert!(
        s.documento().camadas[0].pixels.vazia(),
        "um desfazer volta tudo"
    );
}

#[test]
fn shift_clique_liga_com_uma_reta_num_passo_proprio() {
    let mut s = sessao();
    s.pincel.cor = [0, 0, 255];
    s.pincel.dureza = 1.0;
    s.pincel.raio = 5.0;
    s.apertar(100.0, 100.0);
    s.soltar();
    assert_eq!(s.fim_do_ultimo_traco(), Some((100.0, 100.0)));
    assert!(s.apertar_em_reta(500.0, 300.0));
    s.soltar();
    let meio = s.compor().get_pixel(300, 200).0;
    assert_eq!(meio, [0, 0, 255], "o meio da reta");
    assert_eq!(s.historico().passos().len(), 2);
    // Desfazer tira só a reta; o primeiro clique fica.
    s.desfazer();
    assert_ne!(s.compor().get_pixel(300, 200).0, [0, 0, 255]);
    assert_eq!(s.compor().get_pixel(100, 100).0, [0, 0, 255]);
}

#[test]
fn shift_clique_sem_traco_anterior_e_um_clique() {
    let mut s = sessao();
    s.pincel.cor = [0, 255, 0];
    s.pincel.dureza = 1.0;
    assert!(s.apertar_em_reta(200.0, 200.0));
    s.soltar();
    assert_eq!(s.compor().get_pixel(200, 200).0, [0, 255, 0]);
    assert_eq!(s.compor().get_pixel(100, 100).0, [100, 100, 70]);
}

#[test]
fn a_selecao_entra_no_desfazer_com_a_intersecao() {
    let mut s = sessao();
    s.selecionar(&retangulo(100, 100, 300, 300), Operacao::Nova);
    s.selecionar(&retangulo(250, 250, 300, 300), Operacao::Intersecao);
    // Fica só o quadrado comum, de 250 a 400.
    assert_eq!(selecionado(&s, 300, 300), 255);
    assert_eq!(selecionado(&s, 150, 150), 0, "só no primeiro");
    assert_eq!(selecionado(&s, 500, 500), 0, "só no segundo");
    assert_eq!(s.historico().passos().len(), 2);
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Seleção retangular"
    );

    assert!(s.desfazer());
    assert_eq!(selecionado(&s, 150, 150), 255, "volta o primeiro retângulo");
    assert!(s.desfazer());
    assert!(s.selecao().is_none());
    assert!(s.refazer() && s.refazer());
    assert_eq!(selecionado(&s, 300, 300), 255);
    assert_eq!(selecionado(&s, 150, 150), 0);

    // ⌘D, ⌘A e inverter também são passos.
    s.desmarcar();
    assert!(s.selecao().is_none());
    s.desfazer();
    assert_eq!(selecionado(&s, 300, 300), 255);
    s.selecionar_tudo();
    s.inverter_selecao();
    assert!(s.selecao().is_none(), "o inverso de tudo é nada");
    s.desfazer();
    assert_eq!(selecionado(&s, 10, 10), 255);
}

#[test]
fn tirar_ou_cruzar_sem_selecao_nao_seleciona_nada() {
    let mut s = sessao();
    s.selecionar(&retangulo(100, 100, 50, 50), Operacao::Subtrair);
    s.selecionar(&retangulo(100, 100, 50, 50), Operacao::Intersecao);
    assert!(s.selecao().is_none());
    assert!(
        s.historico().passos().is_empty(),
        "nada mudou, nada a desfazer"
    );
}

#[test]
fn os_modificadores_sao_os_mesmos_em_toda_ferramenta() {
    assert_eq!(Operacao::dos_modificadores(false, false), Operacao::Nova);
    assert_eq!(Operacao::dos_modificadores(true, false), Operacao::Somar);
    assert_eq!(Operacao::dos_modificadores(false, true), Operacao::Subtrair);
    assert_eq!(
        Operacao::dos_modificadores(true, true),
        Operacao::Intersecao
    );
}

#[test]
fn passos_so_de_selecao_nao_pedem_para_salvar() {
    let mut s = sessao();
    assert!(!s.alterado());
    s.selecionar(&retangulo(10, 10, 100, 100), Operacao::Nova);
    s.desmarcar();
    assert!(!s.alterado(), "a seleção não é salva");
    s.pincel.cor = [9, 9, 9];
    s.apertar(50.0, 50.0);
    s.soltar();
    assert!(s.alterado());
}

#[test]
fn mover_o_contorno_nao_mexe_nos_pixels() {
    let mut s = sessao();
    s.selecionar(&retangulo(100, 100, 50, 50), Operacao::Nova);
    s.pincel.cor = [200, 0, 0];
    assert!(s.preencher_selecao());
    let pixels = s.documento().camadas[0].pixels.clone();
    assert!(s.comecar_a_mover_o_contorno());
    s.mover_o_contorno_por(5, 5);
    s.mover_o_contorno_por(200, 30);
    assert!(s.terminar_de_mover_o_contorno());
    assert_eq!(selecionado(&s, 320, 150), 255, "o contorno andou");
    assert_eq!(selecionado(&s, 120, 120), 0);
    assert_eq!(s.documento().camadas[0].pixels, pixels, "os pixels ficaram");
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Mover seleção"
    );
    s.desfazer();
    assert_eq!(selecionado(&s, 120, 120), 255);
}

#[test]
fn a_camada_via_recorte_e_um_passo_so() {
    let mut s = sessao();
    s.selecionar_tudo();
    s.pincel.cor = [10, 200, 30];
    assert!(s.preencher_selecao());
    let original = s.documento().camadas[0].pixels.clone();
    let antes = s.historico().passos().len();
    s.selecionar(&retangulo(200, 200, 100, 80), Operacao::Nova);
    let passos = s.historico().passos().len();
    assert!(s.camada_via_copia(true));
    assert_eq!(s.documento().camadas.len(), 2);
    assert_eq!(
        s.documento().camadas[0].pixels.pixel(250, 240)[3],
        0,
        "o pedaço saiu"
    );
    assert_eq!(
        s.documento().camadas[1].pixels.pixel(250, 240),
        [10, 200, 30, 255]
    );
    assert_eq!(s.historico().passos().len(), passos + 1, "um passo só");
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Camada via recorte"
    );

    assert!(s.desfazer());
    assert_eq!(s.documento().camadas.len(), 1, "a camada nova saiu");
    assert_eq!(
        s.documento().camadas[0].pixels,
        original,
        "e o pedaço voltou"
    );
    assert_eq!(s.ativa(), 0);
    assert!(s.refazer());
    assert_eq!(s.documento().camadas.len(), 2);
    assert_eq!(s.documento().camadas[0].pixels.pixel(250, 240)[3], 0);
    assert_eq!(s.ativa(), 1, "escolhida a camada nova, como depois do ⇧⌘J");
    assert!(antes < passos);
}

#[test]
fn o_mover_com_selecao_leva_pixels_e_selecao_num_passo() {
    let mut s = sessao();
    s.selecionar(&retangulo(100, 100, 40, 40), Operacao::Nova);
    s.pincel.cor = [255, 255, 0];
    assert!(s.preencher_selecao());
    let pixels = s.documento().camadas[0].pixels.clone();
    let passos = s.historico().passos().len();
    assert!(s.comecar_a_mover());
    s.mover_por(100, 0);
    assert!(s.terminar_de_mover());
    assert_eq!(s.historico().passos().len(), passos + 1);
    assert_eq!(selecionado(&s, 220, 120), 255, "a seleção foi junto");
    assert_eq!(
        s.documento().camadas[0].pixels.pixel(220, 120),
        [255, 255, 0, 255]
    );
    s.desfazer();
    assert_eq!(selecionado(&s, 120, 120), 255, "e volta junto");
    assert_eq!(s.documento().camadas[0].pixels, pixels);
}

fn projeto(pasta: &Path) -> Projeto {
    Projeto::novo(pasta.join("e1"), Arc::new(DiscoReal))
}

#[test]
fn o_recorte_atomico_e_a_selecao_gravam_e_reabrem() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = sessao();
    let base = s.base().clone();
    s.selecionar_tudo();
    s.pincel.cor = [40, 90, 200];
    s.preencher_selecao();
    s.selecionar(&retangulo(50, 60, 120, 90), Operacao::Nova);
    s.camada_via_copia(true);
    s.selecionar(&retangulo(0, 0, 10, 10), Operacao::Somar);
    let (doc, hist) = s.instantaneo();
    let (gravados, posicao) = hist.para_gravar();
    assert!(gravados.iter().all(|p| !p.so_selecao()));
    assert!(matches!(gravados.last(), Some(Comando::Varios { .. })));
    assert_eq!(posicao, gravados.len());

    let p = projeto(dir.path());
    p.salvar("e1", &base, &doc, &hist, 1).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("e1/projeto.json")).unwrap())
            .unwrap();
    assert_eq!(json["formato"], 6);
    // A coleta não pode apagar os tiles que só o passo composto cita.
    p.coletar(1).unwrap();

    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    assert_eq!(aberto.historico.passos(), gravados.as_slice());
    let mut s2 = Sessao::nova(base, aberto.documento, aberto.historico, 300);
    assert!(s2.selecao().is_none(), "a seleção não vem do projeto");
    assert!(s2.desfazer(), "o recorte se desfaz depois de reabrir");
    assert_eq!(s2.documento().camadas.len(), 1);
    assert_eq!(
        s2.documento().camadas[0].pixels.pixel(100, 100),
        [40, 90, 200, 255],
        "o pedaço voltou dos tiles do histórico"
    );
}

#[test]
fn o_projeto_do_formato_5_abre_igual() {
    // Um projeto gravado antes da etapa 13 (formato 5, sem `varios`) abre com
    // a mesma foto e o mesmo histórico: o formato só sobe ao gravar de novo.
    let dir = tempfile::tempdir().unwrap();
    let mut s = sessao();
    let base = s.base().clone();
    s.pincel.cor = [1, 2, 3];
    s.apertar(100.0, 100.0);
    s.arrastar(300.0, 100.0);
    s.soltar();
    let (doc, hist) = s.instantaneo();
    let p = projeto(dir.path());
    p.salvar("e1", &base, &doc, &hist, 1).unwrap();
    let caminho = dir.path().join("e1/projeto.json");
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&caminho).unwrap()).unwrap();
    json["formato"] = 5.into();
    std::fs::write(&caminho, serde_json::to_vec(&json).unwrap()).unwrap();

    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    assert_eq!(aberto.historico.passos(), hist.passos());
    let s2 = Sessao::nova(base, aberto.documento, aberto.historico, 300);
    assert_eq!(
        s2.compor().as_raw(),
        s.compor().as_raw(),
        "a mesma aparência"
    );
}
