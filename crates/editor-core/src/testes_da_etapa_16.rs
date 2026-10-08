//! 🎭 Etapa 16 (`docs/editor-em-camadas/22-ETAPA-16.md`): as Propriedades da
//! máscara (densidade, difusão, inverter, vínculo, aplicar), a máscara à vista
//! (sozinha e em rubi), os cadeados da camada e a área de transferência.

use std::path::Path;
use std::sync::Arc;

use image::RgbImage;

use crate::composicao::{self, Exibicao};
use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::pincel::Ferramenta;
use crate::projeto::{DiscoReal, Projeto};
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::{Cadeado, Sessao};
use crate::tiles::CamadaDePixels;
use crate::transformar::Transformacao;
use crate::vista::Vista;

const VERMELHO: [u8; 3] = [230, 20, 20];

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

fn retangulo(s: &mut Sessao, x: u32, y: u32, l: u32, a: u32) {
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(x, y, l, a)),
        Operacao::Nova,
    );
}

/// A camada "Pintura" com um quadrado vermelho em (100..400, 100..400) e uma
/// máscara que esconde a metade da esquerda (x < 250) — a máscara escolhida.
fn quadrado_com_mascara() -> Sessao {
    let mut s = sessao();
    retangulo(&mut s, 100, 100, 300, 300);
    s.pincel.cor = VERMELHO;
    assert!(s.preencher_selecao());
    s.desmarcar();
    assert!(s.adicionar_mascara(false));
    retangulo(&mut s, 0, 0, 250, 520);
    s.pincel.cor = [0; 3];
    assert!(s.preencher_selecao(), "preto na máscara");
    s.desmarcar();
    s
}

fn pixel(img: &RgbImage, x: u32, y: u32) -> [u8; 3] {
    img.get_pixel(x, y).0
}

/// A vista refeita do zero — o que a vista incremental tem de ser.
fn vista_do_zero(s: &Sessao) -> Vista {
    Vista::nova(s.base(), s.documento(), 350)
}

fn projeto(pasta: &Path) -> Projeto {
    Projeto::novo(pasta.join("e1"), Arc::new(DiscoReal))
}

// --------------------------------------------------- densidade e difusão

#[test]
fn a_densidade_deixa_o_preto_passar_em_parte_e_e_um_passo() {
    let mut s = quadrado_com_mascara();
    let base = s.base().clone();
    let antes = s.compor();
    assert_eq!(pixel(&antes, 200, 200), pixel(&base, 200, 200), "escondido");
    assert_eq!(pixel(&antes, 300, 200), VERMELHO);

    let passos = s.historico().passos().len();
    for v in [0.9, 0.7, 0.5] {
        s.mover_densidade(v);
    }
    s.confirmar_mascara();
    assert_eq!(
        s.historico().passos().len(),
        passos + 1,
        "o arrasto é um passo"
    );
    let doc = s.documento();
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(doc),
        "Densidade da máscara de Pintura (50%)"
    );
    let meio = s.compor();
    let b = pixel(&base, 200, 200);
    let esperado: Vec<u8> = (0..3)
        .map(|c| (b[c] as f32 + (VERMELHO[c] as f32 - b[c] as f32) * (128.0 / 255.0)).round() as u8)
        .collect();
    let p = pixel(&meio, 200, 200);
    for c in 0..3 {
        assert!(p[c].abs_diff(esperado[c]) <= 1, "{p:?} × {esperado:?}");
    }
    assert_eq!(pixel(&meio, 300, 200), VERMELHO, "o branco segue revelando");
    assert_eq!(
        pixel(&meio, 50, 50),
        pixel(&base, 50, 50),
        "fora do quadrado nada"
    );
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());

    assert!(s.desfazer());
    assert_eq!(s.compor(), antes);
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());
}

#[test]
fn a_difusao_amacia_a_borda_sem_mexer_nos_pixels_e_volta_exata() {
    let mut s = quadrado_com_mascara();
    let antes = s.compor();
    let pixels_da_mascara = s.documento().camadas[0].mascara.clone().unwrap().pixels;

    s.mover_difusao(12.0);
    s.confirmar_mascara();
    let m = s.documento().camadas[0].mascara.as_ref().unwrap();
    assert_eq!(m.pixels, pixels_da_mascara, "os pixels da máscara ficam");
    assert_eq!(m.difusao, 12.0);
    let difusa = s.compor();
    // Na divisa (x = 250) o vermelho entra pela metade; longe dela, igual.
    let base = s.base().clone();
    let na_divisa = pixel(&difusa, 250, 250);
    assert_ne!(na_divisa, pixel(&antes, 250, 250));
    assert_ne!(na_divisa, pixel(&base, 250, 250));
    assert_eq!(pixel(&difusa, 150, 250), pixel(&antes, 150, 250));
    assert_eq!(pixel(&difusa, 380, 250), pixel(&antes, 380, 250));
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());

    s.mover_difusao(0.0);
    s.confirmar_mascara();
    assert_eq!(s.compor(), antes, "difusão 0 devolve a borda pintada");
    assert!(s.desfazer());
    assert_eq!(s.compor(), difusa);
}

#[test]
fn o_pincel_na_mascara_difusa_refaz_a_vista_alem_do_traco() {
    let mut s = quadrado_com_mascara();
    s.mover_difusao(20.0);
    s.confirmar_mascara();
    assert!(s.na_mascara());
    s.pincel.cor = [0; 3];
    s.pincel.raio = 15.0;
    s.pincel.dureza = 1.0;
    assert!(s.apertar(330.0, 200.0));
    s.arrastar(330.0, 300.0);
    // Durante o traço, a vista já é a da foto com o desfoque novo.
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());
    assert!(s.soltar());
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());
    assert!(s.desfazer());
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());
}

// ------------------------------------------------- inverter e aplicar

#[test]
fn inverter_a_mascara_troca_o_que_aparece_e_duas_vezes_volta() {
    let mut s = quadrado_com_mascara();
    let base = s.base().clone();
    let antes_doc = s.documento().clone();
    assert!(s.inverter(), "⌘I com a máscara escolhida");
    let foto = s.compor();
    assert_eq!(pixel(&foto, 200, 200), VERMELHO);
    assert_eq!(pixel(&foto, 300, 200), pixel(&base, 300, 200));
    let m = s.documento().camadas[0].mascara.as_ref().unwrap();
    assert_eq!(m.valor(200, 200), 255);
    assert_eq!(m.valor(300, 200), 0);
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Inverter a máscara de Pintura"
    );
    assert!(s.inverter());
    assert_eq!(s.documento(), &antes_doc);
}

#[test]
fn inverter_uma_camada_de_pixels_faz_o_negativo_na_selecao() {
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    retangulo(&mut s, 100, 100, 100, 300);
    assert!(s.inverter());
    let p = &s.documento().camadas[0].pixels;
    let [r, g, b] = VERMELHO;
    assert_eq!(p.pixel(150, 150), [255 - r, 255 - g, 255 - b, 255]);
    assert_eq!(p.pixel(300, 150), [r, g, b, 255], "fora da seleção fica");
    assert_eq!(p.pixel(50, 50)[3], 0, "o transparente fica transparente");
    assert!(s.desfazer());
    assert_eq!(
        s.documento().camadas[0].pixels.pixel(150, 150),
        [r, g, b, 255]
    );
}

#[test]
fn aplicar_a_mascara_poe_ela_no_alfa_sem_mudar_a_foto() {
    let mut s = quadrado_com_mascara();
    s.mover_difusao(6.0);
    s.mover_densidade(0.8);
    s.confirmar_mascara();
    let foto = s.compor();
    assert!(s.aplicar_mascara());
    assert!(s.documento().camadas[0].mascara.is_none());
    let depois = s.compor();
    let maior = foto
        .as_raw()
        .iter()
        .zip(depois.as_raw())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(maior <= 2, "a foto fica (arredondamento do alfa): {maior}");
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Aplicar máscara"
    );
    assert!(s.desfazer());
    assert!(s.documento().camadas[0].mascara.is_some());
    assert_eq!(s.compor(), foto);
}

// ------------------------------------------------------------- vínculo

#[test]
fn o_mover_leva_a_mascara_vinculada_e_a_solta_fica() {
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    let base = s.base().clone();
    assert!(s.comecar_a_mover());
    s.mover_por(100, 0);
    assert!(s.terminar_de_mover());
    let foto = s.compor();
    // O quadrado foi para 200..500, a máscara junto: esconde x < 350.
    assert_eq!(pixel(&foto, 300, 200), pixel(&base, 300, 200));
    assert_eq!(pixel(&foto, 400, 200), VERMELHO);
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Mover"
    );
    assert!(s.desfazer());

    assert!(s.alternar_vinculo_de(0));
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Desvincular a máscara de Pintura"
    );
    assert!(s.comecar_a_mover());
    s.mover_por(100, 0);
    assert!(s.terminar_de_mover());
    let foto = s.compor();
    // Solta: a máscara ficou escondendo x < 250.
    assert_eq!(pixel(&foto, 300, 200), VERMELHO);
    assert_eq!(pixel(&foto, 200, 200), pixel(&base, 200, 200));
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());
}

#[test]
fn o_ctrl_t_transforma_a_mascara_vinculada_pela_mesma_conta() {
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    let base = s.base().clone();
    assert!(s.comecar_a_transformar());
    // Metade da largura em volta do centro do quadrado (250, 250): ele vai
    // para 175..325, e a divisa da máscara (250) fica no mesmo lugar.
    s.definir_transformacao(Transformacao {
        escala_x: 0.5,
        ..Transformacao::default()
    });
    assert!(s.aplicar_transformacao());
    let foto = s.compor();
    assert_eq!(
        pixel(&foto, 200, 250),
        pixel(&base, 200, 250),
        "metade escondida"
    );
    assert_eq!(pixel(&foto, 300, 250), VERMELHO);
    assert_eq!(
        pixel(&foto, 350, 250),
        pixel(&base, 350, 250),
        "fora do quadrado"
    );
    // A máscara encolheu junto: o preto que ia até 250 agora vai até 250 em
    // volta do centro — e o que estava em x < 100 (fora do quadrado) veio
    // para perto dele.
    let m = s.documento().camadas[0].mascara.as_ref().unwrap();
    assert_eq!(m.valor(240, 250), 0);
    assert_eq!(m.valor(260, 250), 255);
    assert!(s.desfazer());
    assert_eq!(
        s.compor().as_raw(),
        quadrado_com_mascara().compor().as_raw()
    );
}

#[test]
fn o_deformar_leva_a_mascara_vinculada_dentro_da_caixa() {
    use crate::deformar::Malha;
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    let base = s.base().clone();
    assert!(s.comecar_a_deformar());
    let (caixa, malha) = s.malha().unwrap();
    assert_eq!((caixa.x, caixa.largura), (100, 300));
    // Todos os pontos 60 px para baixo: o Deformar vira um deslocamento.
    let mut m: Malha = malha;
    for linha in m.pontos.iter_mut() {
        for p in linha.iter_mut() {
            p.1 += 60.0;
        }
    }
    s.definir_malha(m);
    assert!(s.aplicar_transformacao());
    let foto = s.compor();
    assert_eq!(pixel(&foto, 300, 440), VERMELHO, "desceu");
    assert_eq!(
        pixel(&foto, 200, 440),
        pixel(&base, 200, 440),
        "a máscara desceu junto"
    );
    assert_eq!(pixel(&foto, 300, 130), pixel(&base, 300, 130));
}

// ------------------------------------------------------------ cadeados

#[test]
fn bloquear_pixels_tranca_a_tinta_e_deixa_a_mascara() {
    let mut s = quadrado_com_mascara();
    let antes = s.documento().camadas[0].pixels.clone();
    assert!(s.alternar_bloqueio_de(0, Cadeado::Pixels));
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Bloquear Pintura"
    );
    s.escolher_camada(0);
    assert!(s.pixels_bloqueados());
    assert!(!s.apertar(300.0, 300.0), "o pincel recusa");
    retangulo(&mut s, 120, 120, 50, 50);
    assert!(!s.apagar_selecao());
    assert!(!s.preencher_selecao());
    assert!(!s.inverter());
    assert!(!s.comecar_a_transformar());
    assert!(s.recortar().is_none());
    assert_eq!(s.documento().camadas[0].pixels, antes);
    // A máscara continua aberta.
    assert!(s.escolher_mascara(0));
    assert!(!s.pixels_bloqueados());
    s.pincel.cor = [255; 3];
    assert!(s.preencher_selecao(), "branco na máscara");
    // Desbloquear volta.
    assert!(s.alternar_bloqueio_de(0, Cadeado::Pixels));
    s.escolher_camada(0);
    assert!(s.preencher_selecao());
}

#[test]
fn bloquear_posicao_tranca_o_mover() {
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    assert!(s.alternar_bloqueio_de(0, Cadeado::Posicao));
    assert!(!s.comecar_a_mover());
    assert!(!s.comecar_a_transformar());
    assert!(!s.comecar_a_deformar());
    assert!(s.apertar(300.0, 300.0), "pintar pode");
    s.soltar();
}

#[test]
fn bloquear_a_transparencia_pinta_so_a_cor_do_que_existe() {
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    assert!(s.alternar_bloqueio_de(0, Cadeado::Transparencia));
    s.pincel.ferramenta = Ferramenta::Pincel;
    s.pincel.cor = [0, 0, 255];
    s.pincel.raio = 30.0;
    s.pincel.dureza = 1.0;
    // Um traço que cruza a borda do quadrado (x = 400).
    assert!(s.apertar(380.0, 200.0));
    s.arrastar(430.0, 200.0);
    assert!(s.soltar());
    let p = &s.documento().camadas[0].pixels;
    assert_eq!(
        p.pixel(390, 200),
        [0, 0, 255, 255],
        "azul onde havia vermelho"
    );
    assert_eq!(
        p.pixel(420, 200)[3],
        0,
        "fora do quadrado continua transparente"
    );

    // A borracha pinta a cor de fundo, sem tirar alfa.
    s.pincel.ferramenta = Ferramenta::Borracha;
    s.pincel.cor_de_fundo = [255, 255, 255];
    assert!(s.apertar(300.0, 300.0));
    assert!(s.soltar());
    assert_eq!(
        s.documento().camadas[0].pixels.pixel(300, 300),
        [255, 255, 255, 255]
    );

    // O Delete pinta a cor de fundo também.
    retangulo(&mut s, 120, 300, 40, 40);
    s.pincel.cor_de_fundo = [0, 255, 0];
    assert!(s.apagar_selecao());
    assert_eq!(
        s.documento().camadas[0].pixels.pixel(130, 310),
        [0, 255, 0, 255]
    );
    // Preencher a seleção que passa da borda só pinta dentro.
    retangulo(&mut s, 380, 380, 60, 60);
    s.pincel.cor = [9, 9, 9];
    assert!(s.preencher_selecao());
    let p = &s.documento().camadas[0].pixels;
    assert_eq!(p.pixel(390, 390), [9, 9, 9, 255]);
    assert_eq!(p.pixel(420, 420)[3], 0);
}

#[test]
fn bloquear_tudo_tranca_tambem_a_opacidade_e_o_modo() {
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    assert!(s.alternar_bloqueio_de(0, Cadeado::Tudo));
    assert!(s.documento().camadas[0].bloqueio.tudo());
    s.mover_opacidade(0.3);
    s.confirmar_opacidade();
    assert_eq!(s.documento().camadas[0].opacidade, 1.0);
    s.mudar_modo(crate::mesclagem::Modo::Multiplicacao);
    assert_eq!(
        s.documento().camadas[0].modo,
        crate::mesclagem::Modo::Normal
    );
    assert!(
        s.alternar_bloqueio_de(0, Cadeado::Tudo),
        "de novo, solta os três"
    );
    assert!(!s.documento().camadas[0].bloqueio.algum());
    assert!(s.desfazer());
    assert!(s.documento().camadas[0].bloqueio.tudo());
}

// ------------------------------------------------------------- exibição

#[test]
fn a_mascara_sozinha_e_o_rubi_so_mudam_a_tela() {
    let mut s = quadrado_com_mascara();
    let foto = s.compor();
    let normal = s.vista().imagem().clone();
    assert!(s.alternar_so_a_mascara(0));
    assert_eq!(s.exibicao(), Exibicao::SoAMascara(0));
    assert!(s.na_mascara(), "o pincel vai para a máscara");
    let v = s.vista();
    let f = v.fator();
    assert_eq!(v.imagem().get_pixel(200 / f, 200 / f).0, [0, 0, 0]);
    assert_eq!(v.imagem().get_pixel(300 / f, 200 / f).0, [255, 255, 255]);
    assert_eq!(s.compor(), foto, "a imagem editada é a foto");
    assert!(s.alternar_so_a_mascara(0), "de novo volta à foto");
    assert_eq!(s.vista().imagem(), &normal);

    assert!(s.alternar_rubi());
    let rubi = composicao::compor_recorte_exibindo(
        s.base(),
        s.documento(),
        &Retangulo::inteiro(700, 520),
        Exibicao::Rubi(0),
    );
    let base = s.base().clone();
    let b = pixel(&base, 200, 200);
    let r = pixel(&rubi, 200, 200);
    assert_eq!(
        r[0],
        ((b[0] as f32 + (255.0 - b[0] as f32) * 0.5).round()) as u8
    );
    assert_eq!(pixel(&rubi, 300, 200), VERMELHO, "onde revela, a foto");
    assert_eq!(
        s.vista().imagem(),
        &Vista::da_regiao_exibindo(
            s.base(),
            s.documento(),
            &Retangulo::inteiro(700, 520),
            s.vista().fator(),
            Exibicao::Rubi(0),
        )
        .imagem()
        .clone()
    );
    // Outra camada escolhida: volta à foto.
    s.nova_camada();
    assert_eq!(s.exibicao(), Exibicao::Foto);
    assert_eq!(s.vista().imagem(), vista_do_zero(&s).imagem());
}

// -------------------------------------------------- área de transferência

#[test]
fn copiar_e_colar_no_lugar_da_os_mesmos_pixels_numa_camada_nova() {
    let mut s = quadrado_com_mascara();
    s.escolher_camada(0);
    retangulo(&mut s, 150, 150, 100, 80);
    let copiado = s.copiar(false).unwrap();
    assert_eq!(copiado.caixa.x, 150);
    assert_eq!((copiado.caixa.largura, copiado.caixa.altura), (100, 80));
    let n = s.documento().camadas.len();
    assert!(s.colar(&copiado, None));
    assert_eq!(s.documento().camadas.len(), n + 1);
    assert_eq!(s.ativa(), 1, "acima da escolhida, e escolhida");
    assert!(s.selecao().is_none(), "a seleção sai ao colar");
    let p = &s.documento().camadas[1].pixels;
    assert_eq!(
        p.pixel(160, 160),
        [VERMELHO[0], VERMELHO[1], VERMELHO[2], 255]
    );
    assert_eq!(p.pixel(260, 160)[3], 0);
    assert_eq!(s.documento().camadas[1].nome, "Camada 1");
    // Um desfazer tira a camada e devolve a seleção.
    assert!(s.desfazer());
    assert_eq!(s.documento().camadas.len(), n);
    assert!(s.selecao().is_some());

    // ⌘V com centro: o meio da caixa vai para lá.
    assert!(s.colar(&copiado, Some((500.0, 300.0))));
    let p = &s.documento().camadas[1].pixels;
    assert_eq!(p.pixel(500, 300)[3], 255);
    assert_eq!(p.pixel(451, 261)[3], 255);
    assert_eq!(p.pixel(449, 300)[3], 0);
}

#[test]
fn copiar_mesclado_e_a_foto_como_aparece_e_recortar_apaga() {
    let mut s = quadrado_com_mascara();
    let foto = s.compor();
    retangulo(&mut s, 200, 200, 100, 100);
    let mesclado = s.copiar(true).unwrap();
    let img = mesclado.imagem();
    for (x, y) in [(0, 0), (49, 10), (51, 10), (99, 99)] {
        let p = pixel(&foto, 200 + x, 200 + y);
        assert_eq!(img.get_pixel(x, y).0, [p[0], p[1], p[2], 255]);
    }
    s.escolher_camada(0);
    retangulo(&mut s, 300, 300, 50, 50);
    let recortado = s.recortar().unwrap();
    assert_eq!(recortado.caixa.largura, 50);
    assert_eq!(s.documento().camadas[0].pixels.pixel(320, 320)[3], 0);
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Recortar"
    );
    assert!(s.desfazer());
    assert_eq!(s.documento().camadas[0].pixels.pixel(320, 320)[3], 255);
}

#[test]
fn carimbar_visivel_poe_a_foto_numa_camada_e_recusa_se_ela_mudou() {
    let mut s = quadrado_com_mascara();
    let foto = s.compor();
    let (versao, base, doc) = s.pedido_de_carimbo();
    let pixels = CamadaDePixels::da_imagem(&composicao::compor(&base, &doc));
    assert!(s.carimbar_visivel(versao, pixels.clone()));
    assert_eq!(s.compor(), foto, "por cima, a mesma foto");
    let c = &s.documento().camadas[s.ativa()];
    assert_eq!(c.pixels.pixel(200, 200)[..3], pixel(&foto, 200, 200));
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Carimbar visível"
    );
    assert!(
        !s.carimbar_visivel(versao, pixels),
        "a foto mudou desde o pedido"
    );
}

// --------------------------------------------------------------- projeto

#[test]
fn as_propriedades_e_os_cadeados_gravam_e_reabrem_no_formato_9() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = quadrado_com_mascara();
    let base = s.base().clone();
    s.mover_densidade(0.6);
    s.mover_difusao(8.0);
    s.confirmar_mascara();
    s.alternar_vinculo_de(0);
    s.alternar_bloqueio_de(0, Cadeado::Posicao);
    let (doc, hist) = s.instantaneo();
    projeto(dir.path())
        .salvar("e1", &base, &doc, &hist, 1)
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("e1/projeto.json")).unwrap())
            .unwrap();
    assert_eq!(json["formato"], 9);
    let m = &json["camadas"][0]["mascara"];
    assert_eq!(m["vinculada"], false);
    assert!((m["densidade"].as_f64().unwrap() - 0.6).abs() < 1e-6);
    assert_eq!(m["difusao"], 8.0);
    assert_eq!(json["camadas"][0]["bloqueio"]["posicao"], true);
    let aberto = projeto(dir.path()).abrir(&base).unwrap().unwrap();
    assert_eq!(aberto.documento, doc);
    let mut s2 = Sessao::nova(base, aberto.documento, aberto.historico, 350);
    assert_eq!(s2.compor(), s.compor());
    assert!(s2.desfazer(), "o cadeado no desfazer");
    assert!(!s2.documento().camadas[0].bloqueio.posicao);
    assert!(s2.desfazer());
    assert!(
        s2.documento().camadas[0]
            .mascara
            .as_ref()
            .unwrap()
            .vinculada
    );
}

#[test]
fn a_camada_sem_as_propriedades_novas_nao_as_grava() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = quadrado_com_mascara();
    let base = s.base().clone();
    let (doc, hist) = s.instantaneo();
    projeto(dir.path())
        .salvar("e1", &base, &doc, &hist, 1)
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("e1/projeto.json")).unwrap())
            .unwrap();
    let m = &json["camadas"][0]["mascara"];
    assert!(m.get("vinculada").is_none() && m.get("densidade").is_none());
    assert!(m.get("difusao").is_none());
    assert!(json["camadas"][0].get("bloqueio").is_none());
}

// ------------------------------------------------------ arrastar a camada

#[test]
fn arrastar_a_camada_por_varias_posicoes_e_um_passo() {
    let mut s = sessao();
    for _ in 0..3 {
        s.nova_camada();
    }
    let nomes = |s: &Sessao| -> Vec<String> {
        s.documento()
            .camadas
            .iter()
            .map(|c| c.nome.clone())
            .collect()
    };
    let antes = nomes(&s);
    s.escolher_camada(3);
    let passos = s.historico().passos().len();
    assert!(s.mover_camada_para(0));
    assert_eq!(s.ativa(), 0);
    assert_eq!(nomes(&s)[0], "Camada 3");
    assert_eq!(s.historico().passos().len(), passos + 1, "um passo só");
    assert_eq!(
        s.historico().a_desfazer().unwrap().descricao(s.documento()),
        "Mover camada"
    );
    assert!(s.desfazer());
    assert_eq!(nomes(&s), antes);
    assert!(!s.mover_camada_para(3), "já está lá");
}

#[test]
fn o_arrasto_ao_vivo_por_varias_linhas_e_um_passo() {
    let mut s = sessao();
    for _ in 0..3 {
        s.nova_camada();
    }
    s.escolher_camada(3);
    let desde = s.historico().posicao();
    let passos = s.historico().passos().len();
    assert!(s.mover_camada_arrastando(2, Some(desde)));
    assert!(s.mover_camada_arrastando(1, Some(desde)));
    assert!(s.mover_camada_arrastando(0, Some(desde)));
    assert_eq!(s.documento().camadas[0].nome, "Camada 3");
    assert_eq!(s.historico().passos().len(), passos + 1, "um passo só");
    assert!(s.desfazer());
    assert_eq!(s.documento().camadas[3].nome, "Camada 3");
}
