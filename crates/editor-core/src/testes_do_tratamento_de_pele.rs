//! 🧴 O tratamento de pele de ponta a ponta, na sessão: separação de
//! frequências com prévia e OK num passo, retoque de textura na alta (carimbo
//! e recuperação), de tom na baixa (Suavizar tons), intensidade, original,
//! regenerar, Dodge & Burn, salvar e reabrir.
//!
//! A foto de teste é um "rosto" sintético: pele com degradê de luz (o
//! volume), poros (grão fino), uma **mancha** de tom (escura e larga) e uma
//! faixa de **cabelo** escuro à esquerda.

use std::path::Path;
use std::sync::Arc;

use image::RgbImage;

use crate::carimbo::AmostraDoCarimbo;
use crate::composicao::{compor_recorte_exibindo, Exibicao};
use crate::documento::{BaseRef, Documento, Retoque};
use crate::filtros::{filtrada_com, Filtro};
use crate::frequencias::{separar, NOME_DA_ALTA, NOME_DA_BAIXA};
use crate::historico::Historico;
use crate::mesclagem::Modo;
use crate::pincel::Ferramenta;
use crate::projeto::{DiscoReal, Projeto};
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::{Frequencia, OrigemDaSeparacao, Sessao, VistaDaSeparacao};
use crate::tiles::CamadaDePixels;

const L: u32 = 640;
const A: u32 = 520;
/// O centro e o raio da mancha.
const MANCHA: (f32, f32, f32) = (430.0, 300.0, 14.0);
/// O cabelo: x < 120.
const CABELO: u32 = 120;

fn poro(x: u32, y: u32) -> i32 {
    let h = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(2246822519)).rotate_left(13);
    (h % 25) as i32 - 12
}

fn rosto() -> Arc<RgbImage> {
    Arc::new(RgbImage::from_fn(L, A, |x, y| {
        if x < CABELO {
            let v = 40 + poro(x, y) / 3;
            return image::Rgb([(v + 10) as u8, v as u8, (v - 8) as u8]);
        }
        // A luz do volume: mais clara no meio.
        let luz = 30.0 * (1.0 - ((x as f32 - 380.0) / 300.0).powi(2));
        let (mx, my, mr) = MANCHA;
        let d2 = (x as f32 - mx).powi(2) + (y as f32 - my).powi(2);
        let mancha = 40.0 * (-d2 / (2.0 * mr * mr)).exp();
        let p = poro(x, y) as f32;
        let c = |base: f32, k: f32| (base + luz - mancha * k + p).clamp(0.0, 255.0) as u8;
        image::Rgb([c(200.0, 1.0), c(150.0, 1.1), c(125.0, 1.2)])
    }))
}

fn sessao() -> Sessao {
    let base = rosto();
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 320);
    s.pincel.suavizacao = 0.0;
    s
}

/// Separação pela origem, com prévia e OK, como a janela faz.
fn separar_na_sessao(s: &mut Sessao, origem: OrigemDaSeparacao, raio: f32) {
    let pedido = s.pedido_de_separacao(origem);
    let sep = separar(&pedido.fonte(), raio);
    assert!(s.mostrar_separacao(&pedido, raio, sep));
    assert!(s.confirmar_separacao());
}

fn media(img: &RgbImage, r: &Retangulo, c: usize) -> f32 {
    let mut soma = 0.0;
    for y in r.y..r.baixo() {
        for x in r.x..r.direita() {
            soma += img.get_pixel(x, y).0[c] as f32;
        }
    }
    soma / (r.largura * r.altura) as f32
}

fn desvio(img: &RgbImage, r: &Retangulo, c: usize) -> f32 {
    let m = media(img, r, c);
    let mut soma = 0.0;
    for y in r.y..r.baixo() {
        for x in r.x..r.direita() {
            soma += (img.get_pixel(x, y).0[c] as f32 - m).powi(2);
        }
    }
    (soma / (r.largura * r.altura) as f32).sqrt()
}

fn projeto(pasta: &Path) -> Projeto {
    Projeto::novo(pasta.join("e1"), Arc::new(DiscoReal))
}

// ------------------------------------------------------- separação

#[test]
fn a_previa_nao_e_passo_e_cancelar_devolve_o_documento() {
    let mut s = sessao();
    let doc_antes = s.documento().clone();
    let pedido = s.pedido_de_separacao(OrigemDaSeparacao::Visivel);
    assert_eq!(pedido.indice, 1, "no topo");
    assert!(s.mostrar_separacao(&pedido, 4.0, separar(&pedido.fonte(), 4.0)));
    assert!(s.separando());
    assert_eq!(s.documento().camadas.len(), 3);
    assert_eq!(s.historico().posicao(), 0, "a prévia não é passo");
    assert_eq!(
        s.compor().as_raw(),
        s.base().as_raw(),
        "recomposta = a foto"
    );
    // Outro raio troca os pixels no lugar.
    assert!(s.mostrar_separacao(&pedido, 9.0, separar(&pedido.fonte(), 9.0)));
    assert_eq!(s.documento().camadas.len(), 3);
    assert_eq!(
        s.documento().camadas[1].retoque,
        Some(Retoque::Baixa { raio: 9.0 })
    );
    assert!(s.exibir_separacao(VistaDaSeparacao::Alta));
    assert_eq!(s.exibicao(), Exibicao::SoACamada(2));
    s.cancelar_separacao();
    assert!(!s.separando());
    assert_eq!(s.documento(), &doc_antes);
    assert_eq!(s.exibicao(), Exibicao::Foto);
    assert!(!s.alterado());
}

#[test]
fn o_ok_e_um_passo_que_desfaz_e_refaz_e_a_composicao_e_a_foto() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 5.0);
    let doc = s.documento();
    assert_eq!(doc.camadas.len(), 3);
    assert_eq!(doc.camadas[1].nome, NOME_DA_BAIXA);
    assert_eq!(doc.camadas[2].nome, NOME_DA_ALTA);
    assert_eq!(doc.camadas[2].modo, Modo::LuzLinear);
    assert!(doc.camadas[2].recortada);
    assert_eq!(s.historico().posicao(), 1, "um passo");
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(doc),
        "Separação de frequências"
    );
    assert_eq!(s.ativa(), 2, "a alta fica escolhida");
    assert_eq!(s.frequencia_escolhida(), Some(Frequencia::Alta));
    assert_eq!(s.carimbo.amostra, AmostraDoCarimbo::CamadaAtual);
    assert_eq!(s.compor().as_raw(), s.base().as_raw());

    assert!(s.desfazer());
    assert_eq!(s.documento().camadas.len(), 1);
    assert!(s.refazer());
    assert_eq!(s.documento().camadas.len(), 3);
    assert_eq!(s.compor().as_raw(), s.base().as_raw());
    assert_eq!(s.conjunto_de_pele(), Some((1, 2)));
}

#[test]
fn a_previa_e_recusada_se_a_foto_mudou_depois_do_pedido() {
    let mut s = sessao();
    let pedido = s.pedido_de_separacao(OrigemDaSeparacao::Visivel);
    s.pincel.cor = [255, 0, 0];
    s.apertar(300.0, 100.0);
    s.arrastar(340.0, 100.0);
    s.soltar();
    assert!(!s.mostrar_separacao(&pedido, 4.0, separar(&pedido.fonte(), 4.0)));
    assert!(!s.separando());
}

#[test]
fn a_camada_selecionada_entra_acima_dela_e_as_de_cima_continuam_por_cima() {
    let mut s = sessao();
    assert!(s.criar_camada_da_fotografia(CamadaDePixels::da_imagem(s.base())));
    // A "Pintura" (índice 1) tem um traço vermelho por cima de tudo.
    s.escolher_camada(1);
    s.pincel.cor = [255, 0, 0];
    s.pincel.dureza = 1.0;
    s.apertar(300.0, 100.0);
    s.arrastar(360.0, 100.0);
    s.soltar();
    let antes = s.compor();
    s.escolher_camada(0);
    let pedido = s.pedido_de_separacao(OrigemDaSeparacao::CamadaSelecionada);
    assert_eq!(pedido.indice, 1);
    assert_eq!(
        pedido.fonte().pixel(330, 100)[..3],
        s.base().get_pixel(330, 100).0,
        "a origem é a Fotografia, sem o traço de cima"
    );
    assert!(s.mostrar_separacao(&pedido, 4.0, separar(&pedido.fonte(), 4.0)));
    assert!(s.confirmar_separacao());
    let nomes: Vec<_> = s
        .documento()
        .camadas
        .iter()
        .map(|c| c.nome.as_str())
        .collect();
    assert_eq!(
        nomes,
        ["Fotografia", NOME_DA_BAIXA, NOME_DA_ALTA, "Pintura"]
    );
    assert_eq!(s.compor().as_raw(), antes.as_raw());
}

// ------------------------------------------------- textura na alta

/// Copia a textura do lado esquerdo da pele (x ≈ 180) para cima da mancha.
fn retocar_a_mancha_na_alta(s: &mut Sessao, ferramenta: Ferramenta) {
    s.pincel.ferramenta = ferramenta;
    s.pincel.raio = 10.0;
    s.pincel.dureza = 1.0;
    s.pincel.opacidade = 1.0;
    s.pincel.fluxo = 1.0;
    let (mx, my, _) = MANCHA;
    s.definir_origem(mx - 240.0, my);
    s.apertar(mx - 20.0, my);
    s.arrastar(mx + 20.0, my);
    s.soltar();
}

#[test]
fn o_carimbo_na_alta_copia_so_textura_e_a_baixa_nao_muda() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    let baixa_antes = s.documento().camadas[1].pixels.clone();
    let foto = s.compor();
    retocar_a_mancha_na_alta(&mut s, Ferramenta::Carimbo);
    assert_eq!(s.documento().camadas[1].pixels, baixa_antes, "a baixa fica");
    let depois = s.compor();
    // No caminho do traço, o tom (média) é o do lugar — a mancha continua,
    // porque ela mora na baixa — e o grão continua grão.
    let (mx, my, _) = MANCHA;
    let r = Retangulo::novo(mx as u32 - 15, my as u32 - 4, 30, 8);
    for c in 0..3 {
        let (m0, m1) = (media(&foto, &r, c), media(&depois, &r, c));
        assert!((m0 - m1).abs() < 3.0, "canal {c}: tom {m0} → {m1}");
        assert!(desvio(&depois, &r, c) > 4.0, "canal {c}: sem textura");
    }
    // A composição é a baixa do lugar mais o detalhe da origem, exata.
    let alta = &s.documento().camadas[2].pixels;
    for x in [mx as u32 - 10, mx as u32, mx as u32 + 10] {
        let y = my as u32;
        let d = (alta.pixel(x, y)[0] as i32) * 2 - 255;
        let l = baixa_antes.pixel(x, y)[0] as i32;
        assert_eq!(depois.get_pixel(x, y).0[0] as i32, (l + d).clamp(0, 255));
        // Alinhado: a distância é a da origem ao primeiro clique (−220).
        assert_eq!(
            alta.pixel(x, y),
            s.documento().camadas[2].pixels.pixel(x - 220, y),
            "copiou a alta da origem"
        );
    }
}

#[test]
fn o_carimbo_amostrando_a_composta_na_alta_estragaria_o_tom() {
    // A razão da amostra padrão: na alta, "atual e abaixo" copia a foto
    // composta (cor) para dentro do resíduo, e o tom explode.
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    let foto = s.compor();
    s.carimbo.amostra = AmostraDoCarimbo::AtualEAbaixo;
    retocar_a_mancha_na_alta(&mut s, Ferramenta::Carimbo);
    let (mx, my, _) = MANCHA;
    let r = Retangulo::novo(mx as u32 - 15, my as u32 - 4, 30, 8);
    let m = (media(&foto, &r, 0), media(&s.compor(), &r, 0));
    assert!((m.0 - m.1).abs() > 30.0, "{m:?}");
}

#[test]
fn a_recuperacao_na_alta_e_aditiva_e_guarda_o_tom() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    let baixa_antes = s.documento().camadas[1].pixels.clone();
    let foto = s.compor();
    retocar_a_mancha_na_alta(&mut s, Ferramenta::Recuperacao);
    assert_eq!(s.documento().camadas[1].pixels, baixa_antes);
    let (mx, my, _) = MANCHA;
    let r = Retangulo::novo(mx as u32 - 15, my as u32 - 4, 30, 8);
    let alta = s.documento().camadas[2].pixels.clone();
    let alta_img = RgbImage::from_fn(L, A, |x, y| {
        let p = alta.pixel(x, y);
        image::Rgb([p[0], p[1], p[2]])
    });
    let depois = s.compor();
    for c in 0..3 {
        // O resíduo fica em volta de 127,5 (nenhum tom entrou na alta)…
        let m = media(&alta_img, &r, c);
        assert!((m - 127.5).abs() < 2.5, "canal {c}: média da alta {m}");
        // …e a foto fica com o tom do lugar e o grão da origem.
        let (m0, m1) = (media(&foto, &r, c), media(&depois, &r, c));
        assert!((m0 - m1).abs() < 3.0, "canal {c}: {m0} → {m1}");
        assert!(desvio(&depois, &r, c) > 4.0);
    }
}

// ------------------------------------------------------ tom na baixa

#[test]
fn suavizar_tons_na_baixa_tira_a_mancha_dentro_da_selecao_sem_puxar_o_cabelo() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    assert!(s.escolher_frequencia(Frequencia::Baixa));
    assert_eq!(s.ativa(), 1);
    let alta_antes = s.documento().camadas[2].pixels.clone();
    let baixa_antes = s.documento().camadas[1].pixels.clone();
    // A pele, sem o cabelo (x < 120) e sem a parte de baixo da foto.
    let pele = Retangulo::novo(CABELO, 0, L - CABELO, 440);
    s.selecionar(&Forma::Retangulo(pele), Operacao::Nova);
    let (original, selecao) = s.comecar_filtro().unwrap();
    let filtro = Filtro::SuavizarTons { raio: 30.0 };
    // A prévia em dois valores não acumula: a segunda parte da original.
    s.mostrar_filtro(filtrada_com(&original, filtro, selecao.as_deref(), 0.3));
    s.mostrar_filtro(filtrada_com(&original, filtro, selecao.as_deref(), 1.0));
    assert!(s.aplicar_filtro("Suavizar tons"));
    assert_eq!(s.historico().posicao(), 3, "separação, seleção, filtro");
    let doc = s.documento();
    assert_eq!(doc.camadas[2].pixels, alta_antes, "a textura não mudou");
    let baixa = &doc.camadas[1].pixels;
    // A mancha some da baixa: o centro chega perto da pele em volta.
    let (mx, my, _) = MANCHA;
    let centro = |p: &CamadaDePixels| p.pixel(mx as u32, my as u32)[0] as i32;
    let volta = baixa_antes.pixel(mx as u32 + 60, my as u32)[0] as i32;
    assert!(volta - centro(&baixa_antes) > 25, "a mancha existia");
    assert!(
        volta - centro(baixa) < 12,
        "a mancha saiu: {}",
        centro(baixa)
    );
    // Fora da seleção, nada.
    assert_eq!(baixa.pixel(60, 200), baixa_antes.pixel(60, 200));
    assert_eq!(baixa.pixel(300, 480), baixa_antes.pixel(300, 480));
    // Na borda com o cabelo, a pele não escurece (o gaussiano comum puxaria
    // o cabelo para dentro).
    let borda = baixa.pixel(CABELO + 2, 200)[0] as i32;
    let pele_perto = baixa_antes.pixel(CABELO + 40, 200)[0] as i32;
    assert!(
        borda > pele_perto - 15,
        "a borda escureceu: {borda} × {pele_perto}"
    );
    let gaussiano = crate::filtros::filtrada(
        &baixa_antes,
        Filtro::DesfoqueGaussiano { raio: 30.0 },
        selecao.as_deref(),
    );
    assert!(
        (gaussiano.pixel(CABELO + 2, 200)[0] as i32) < borda - 20,
        "o gaussiano comum escureceria"
    );
    // O poro da foto continua: o detalhe da composição é o de antes.
    let foto = s.base().clone();
    let agora = s.compor();
    for (x, y) in [(300u32, 100u32), (430, 300), (500, 200)] {
        let det =
            |img: &RgbImage| img.get_pixel(x, y).0[1] as i32 - img.get_pixel(x + 1, y).0[1] as i32;
        assert!((det(&foto) - det(&agora)).abs() <= 2, "poro em ({x},{y})");
    }
}

// --------------------------------------------- intensidade e original

#[test]
fn a_intensidade_mistura_o_tratado_com_a_referencia_e_o_intocado_fica() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    assert!(s.escolher_frequencia(Frequencia::Baixa));
    s.pincel.ferramenta = Ferramenta::Pincel;
    s.pincel.cor = [230, 190, 170];
    s.pincel.dureza = 1.0;
    s.pincel.raio = 20.0;
    s.apertar(250.0, 200.0);
    s.soltar();
    let tratado = s.compor();
    assert!(s.mover_intensidade(0.5));
    s.confirmar_opacidade();
    assert_eq!(s.ativa(), 1, "a escolhida não muda");
    assert_eq!(s.intensidade_do_tratamento(), Some(0.5));
    let meio = s.compor();
    let foto = s.base();
    let p = (250, 200);
    for c in 0..3 {
        let esperado =
            (foto.get_pixel(p.0, p.1).0[c] as f32 + tratado.get_pixel(p.0, p.1).0[c] as f32) / 2.0;
        assert!((meio.get_pixel(p.0, p.1).0[c] as f32 - esperado).abs() <= 1.0);
    }
    // Onde nada foi retocado, a foto exata em qualquer intensidade.
    for q in [(500u32, 100u32), (430, 300), (10, 10)] {
        assert_eq!(meio.get_pixel(q.0, q.1), foto.get_pixel(q.0, q.1), "{q:?}");
    }
    assert!(s
        .historico()
        .a_desfazer()
        .unwrap()
        .descricao(s.documento())
        .starts_with("Opacidade de Baixa frequência"));
    assert!(s.desfazer());
    assert_eq!(s.intensidade_do_tratamento(), Some(1.0));
}

#[test]
fn isolar_e_ver_o_original_so_mudam_a_tela() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    retocar_a_mancha_na_alta(&mut s, Ferramenta::Carimbo);
    let (doc, posicao) = (s.documento().clone(), s.historico().posicao());
    let tudo = Retangulo::inteiro(L, A);
    assert!(s.exibir_separacao(VistaDaSeparacao::Baixa));
    assert_eq!(s.vista_da_separacao(), VistaDaSeparacao::Baixa);
    let so_baixa = compor_recorte_exibindo(s.base(), s.documento(), &tudo, s.exibicao());
    let p = s.documento().camadas[1].pixels.pixel(200, 200);
    assert_eq!(so_baixa.get_pixel(200, 200).0, [p[0], p[1], p[2]]);
    assert!(s.exibir_separacao(VistaDaSeparacao::Original));
    let original = compor_recorte_exibindo(s.base(), s.documento(), &tudo, s.exibicao());
    assert_eq!(original.as_raw(), s.base().as_raw(), "o original é a foto");
    assert!(s.exibir_separacao(VistaDaSeparacao::Recomposta));
    assert_eq!(s.exibicao(), Exibicao::Foto);
    assert_eq!(s.documento(), &doc);
    assert_eq!(s.historico().posicao(), posicao);
}

#[test]
fn regenerar_com_outro_raio_guarda_o_retoque_num_passo() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    retocar_a_mancha_na_alta(&mut s, Ferramenta::Carimbo);
    let retocada = s.compor();
    let pedido = s.pedido_de_regeneracao().unwrap();
    assert_eq!(pedido.regenera, Some((1, 2)));
    assert!(s.mostrar_separacao(&pedido, 9.0, separar(&pedido.fonte(), 9.0)));
    assert_eq!(s.documento().camadas.len(), 3, "as mesmas duas camadas");
    assert!(s.confirmar_separacao());
    let doc = s.documento();
    assert_eq!(doc.camadas[1].retoque, Some(Retoque::Baixa { raio: 9.0 }));
    assert_eq!(doc.camadas[2].retoque, Some(Retoque::Alta { raio: 9.0 }));
    assert_eq!(s.compor().as_raw(), retocada.as_raw(), "o retoque ficou");
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(doc),
        "Regenerar separação"
    );
    assert!(s.desfazer());
    assert_eq!(
        s.documento().camadas[1].retoque,
        Some(Retoque::Baixa { raio: 4.0 })
    );
    assert_eq!(s.compor().as_raw(), retocada.as_raw());
}

// --------------------------------------------------- Dodge & Burn

fn matiz(p: [u8; 3]) -> f32 {
    let [r, g, b] = p.map(|v| v as f32);
    (3f32.sqrt() * (g - b)).atan2(2.0 * r - g - b).to_degrees()
}

#[test]
fn dodge_and_burn_clareia_e_escurece_a_luz_sem_mudar_a_cor() {
    let mut s = sessao();
    let foto = s.compor();
    assert!(s.criar_dodge_and_burn());
    assert_eq!(s.historico().posicao(), 1);
    assert_eq!(s.compor().as_raw(), foto.as_raw(), "neutro até pintar");
    let clarear = s.camada_do_dodge_and_burn(true).unwrap();
    let escurecer = s.camada_do_dodge_and_burn(false).unwrap();
    assert_eq!(s.ativa(), clarear);
    assert!(s.na_mascara());
    assert_eq!(s.pincel.cor, [255; 3]);
    assert!(s.pincel.fluxo <= 0.05 && s.pincel.dureza == 0.0);
    assert_eq!(s.documento().camadas[clarear].modo, Modo::Luminosidade);
    // Algumas passadas de fluxo baixo, cada uma um passo.
    s.pincel.raio = 40.0;
    for _ in 0..6 {
        s.apertar(300.0, 150.0);
        s.arrastar(360.0, 150.0);
        s.soltar();
    }
    assert!(s.escolher_dodge_and_burn(false));
    assert_eq!(s.ativa(), escurecer);
    for _ in 0..6 {
        s.apertar(300.0, 350.0);
        s.arrastar(360.0, 350.0);
        s.soltar();
    }
    assert_eq!(s.historico().posicao(), 13);
    let depois = s.compor();
    let lum = |p: [u8; 3]| 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32;
    let (c0, c1) = (foto.get_pixel(330, 150).0, depois.get_pixel(330, 150).0);
    assert!(lum(c1) > lum(c0) + 3.0, "clareou: {c0:?} → {c1:?}");
    assert!(
        (matiz(c1) - matiz(c0)).abs() < 4.0,
        "a cor ficou: {c0:?} → {c1:?}"
    );
    let (e0, e1) = (foto.get_pixel(330, 350).0, depois.get_pixel(330, 350).0);
    assert!(lum(e1) < lum(e0) - 3.0, "escureceu: {e0:?} → {e1:?}");
    assert!(
        (matiz(e1) - matiz(e0)).abs() < 4.0,
        "a cor ficou: {e0:?} → {e1:?}"
    );
    // Longe dos traços, a foto exata.
    assert_eq!(depois.get_pixel(550, 470), foto.get_pixel(550, 470));
    // A máscara apagada devolve tudo: é reversível.
    while s.historico().posicao() > 1 {
        s.desfazer();
    }
    assert_eq!(s.compor().as_raw(), foto.as_raw());
}

// --------------------------------------------- salvar e reabrir

#[test]
fn o_tratamento_inteiro_grava_reabre_e_continua_editavel() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = sessao();
    let base = s.base().clone();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    retocar_a_mancha_na_alta(&mut s, Ferramenta::Carimbo);
    assert!(s.mover_intensidade(0.8));
    s.confirmar_opacidade();
    assert!(s.criar_dodge_and_burn());
    s.apertar(300.0, 150.0);
    s.soltar();
    let (doc, hist) = s.instantaneo();
    let p = projeto(dir.path());
    p.salvar("e1", &base, &doc, &hist, 1).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("e1/projeto.json")).unwrap())
            .unwrap();
    assert_eq!(json["formato"], crate::projeto::FORMATO);
    assert_eq!(json["camadas"][2]["modo"], "luz_linear");
    assert_eq!(json["camadas"][2]["retoque"]["papel"], "alta");
    assert_eq!(json["camadas"][1]["retoque"]["raio"], 4.0);
    p.coletar(1).unwrap();

    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    let mut s2 = Sessao::nova(base.clone(), aberto.documento, aberto.historico, 320);
    assert_eq!(s2.compor().as_raw(), s.compor().as_raw());
    assert_eq!(s2.conjunto_de_pele(), Some((1, 2)));
    assert_eq!(s2.intensidade_do_tratamento(), Some(0.8));
    assert!(s2.camada_do_dodge_and_burn(true).is_some());
    // Continua: mais um retoque na alta, e o desfazer anda até antes da
    // separação.
    assert!(s2.escolher_frequencia(Frequencia::Alta));
    assert_eq!(s2.carimbo.amostra, AmostraDoCarimbo::CamadaAtual);
    s2.definir_origem(200.0, 200.0);
    s2.pincel.ferramenta = Ferramenta::Carimbo;
    s2.apertar(260.0, 260.0);
    s2.soltar();
    while s2.desfazer() {}
    assert_eq!(s2.documento().camadas.len(), 1);
    assert_eq!(s2.compor().as_raw(), base.as_raw());
}

#[test]
fn o_projeto_do_formato_10_abre_igual_e_sem_papel() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = sessao();
    let base = s.base().clone();
    s.pincel.cor = [1, 2, 3];
    s.apertar(100.0, 100.0);
    s.arrastar(300.0, 100.0);
    s.soltar();
    let (doc, hist) = s.instantaneo();
    projeto(dir.path())
        .salvar("e1", &base, &doc, &hist, 1)
        .unwrap();
    let caminho = dir.path().join("e1/projeto.json");
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&caminho).unwrap()).unwrap();
    assert!(
        json["camadas"][0].get("retoque").is_none(),
        "sem papel, sem campo"
    );
    json["formato"] = 10.into();
    std::fs::write(&caminho, serde_json::to_vec(&json).unwrap()).unwrap();
    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    assert!(aberto.documento.camadas.iter().all(|c| c.retoque.is_none()));
}

// ------------------------------------------------- Pincel misturador

#[test]
fn o_misturador_na_baixa_borra_a_mancha_num_passo_e_respeita_a_selecao() {
    let mut s = sessao();
    separar_na_sessao(&mut s, OrigemDaSeparacao::Visivel, 4.0);
    assert!(s.escolher_frequencia(Frequencia::Baixa));
    let alta = s.documento().camadas[2].pixels.clone();
    let baixa = s.documento().camadas[1].pixels.clone();
    let (mx, my, _) = MANCHA;
    // Só a metade de cima da mancha está selecionada.
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(300, 0, 340, my as u32)),
        Operacao::Nova,
    );
    let posicao = s.historico().posicao();
    s.pincel.ferramenta = Ferramenta::Misturador;
    s.pincel.raio = 18.0;
    s.pincel.dureza = 0.0;
    s.pincel.fluxo = 0.6;
    // Carregado com a cor da pele limpa (o conta-gotas), meio a meio com a
    // tela — o uso do retoque.
    let pele = baixa.pixel(mx as u32 + 60, my as u32);
    s.pincel.cor = [pele[0], pele[1], pele[2]];
    s.misturador = crate::misturador::OpcoesDoMisturador::default();
    // Da pele clara para dentro da mancha, num traço só.
    s.apertar(mx + 50.0, my - 8.0);
    for i in 0..30 {
        let t = i as f32 / 29.0;
        s.arrastar(mx + 50.0 - 50.0 * t, my - 8.0);
    }
    s.soltar();
    assert_eq!(s.historico().posicao(), posicao + 1, "um gesto, um passo");
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Pincel misturador"
    );
    let depois = &s.documento().camadas[1].pixels;
    assert_eq!(s.documento().camadas[2].pixels, alta, "a alta não muda");
    let (cx, cy) = (mx as u32, my as u32 - 8);
    assert!(
        depois.pixel(cx, cy)[0] >= baixa.pixel(cx, cy)[0] + 8,
        "a mancha clareou com a pele arrastada: {:?} → {:?}",
        baixa.pixel(cx, cy),
        depois.pixel(cx, cy)
    );
    // Fora da seleção (abaixo de my), nada.
    for y in [my as u32 + 1, my as u32 + 6] {
        assert_eq!(depois.pixel(cx, y), baixa.pixel(cx, y));
    }
    assert!(s.desfazer());
    assert_eq!(s.documento().camadas[1].pixels, baixa);
}

#[test]
fn o_misturador_amostrando_todas_as_camadas_pinta_numa_camada_vazia() {
    let mut s = sessao();
    // A "Pintura" vazia é a escolhida.
    assert!(s.documento().camadas[0].pixels.vazia());
    s.pincel.ferramenta = Ferramenta::Misturador;
    s.pincel.raio = 10.0;
    s.pincel.dureza = 1.0;
    s.pincel.fluxo = 1.0;
    s.misturador = crate::misturador::OpcoesDoMisturador {
        umidade: 1.0,
        mistura: 1.0,
        carregar_apos: false,
        todas_as_camadas: true,
        ..Default::default()
    };
    let foto = s.compor();
    s.apertar(200.0, 200.0);
    s.arrastar(260.0, 200.0);
    s.soltar();
    let camada = &s.documento().camadas[0].pixels;
    assert!(!camada.vazia());
    // A camada ganhou a foto recolhida, opaca no caminho: a composição é a
    // foto arrastada — o mesmo tom em média.
    assert_eq!(camada.pixel(230, 200)[3], 255);
    let r = Retangulo::novo(215, 196, 30, 8);
    let agora = s.compor();
    for c in 0..3 {
        let (m0, m1) = (media(&foto, &r, c), media(&agora, &r, c));
        assert!((m0 - m1).abs() < 4.0, "canal {c}: {m0} → {m1}");
    }
    // Só a camada atual, sem "todas": numa camada vazia não há o que borrar.
    let mut s = sessao();
    s.pincel.ferramenta = Ferramenta::Misturador;
    s.misturador.carregar_apos = false;
    s.apertar(200.0, 200.0);
    s.arrastar(260.0, 200.0);
    s.soltar();
    assert!(s.documento().camadas[0].pixels.vazia());
}

#[test]
fn o_pincel_sujo_segue_para_o_proximo_traco_sem_limpar() {
    let mut s = sessao();
    s.pincel.ferramenta = Ferramenta::Misturador;
    s.pincel.cor = [0, 200, 0];
    s.misturador.limpar_apos = false;
    s.misturador.carregar_apos = false;
    s.tinta.carregar([0, 200, 0]);
    s.misturador.carga = 1.0;
    s.misturador.mistura = 0.0;
    s.apertar(100.0, 100.0);
    s.soltar();
    assert_eq!(s.tinta.cor, Some([0, 200, 0]), "o reservatório ficou");
    s.apertar(300.0, 300.0);
    s.soltar();
    let p = s.documento().camadas[0].pixels.pixel(300, 300);
    assert_eq!(p, [0, 200, 0, 255], "o segundo traço ainda tinha tinta");
    s.tinta.limpar();
    assert!(s.tinta.limpa());
}
