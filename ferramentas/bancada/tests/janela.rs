//! A janela de verdade, numa janela de teste do GPUI (sem tela): os botões,
//! a tabela e o fim de cada caso.

use bancada::janela::{Bancada, Estado};
use bancada::Foto;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{px, size, AppContext as _, TestAppContext};

/// Sem foto, só "Rodar" fica desligado; com a foto, "Rodar" preenche a tabela
/// inteira — e toda linha diz que a foto saiu igual.
#[gpui_kit::test]
fn rodar_preenche_a_tabela_e_a_foto_sai_igual(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let janela = cx.update(|cx| {
        gpui_kit::open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                    origin: Default::default(),
                    size: size(px(980.), px(760.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| Bancada::nova()),
        )
        .expect("a janela abre")
    });
    let (handle, bancada) = janela;

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window
            .find("mensagem")
            .label()
            .is_some_and(|m| m.starts_with("Abra uma foto")));
        // Sem foto não há o que rodar: o clique não faz nada.
        window.click("rodar", cx);
    })
    .unwrap();
    bancada.read_with(cx, |b, _| assert!(b.linhas().is_empty()));

    // Uma foto pequena, para o teste não demorar.
    bancada.update(cx, |b, cx| b.usar(Foto::de_exemplo(240, 180), cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("mensagem").label(),
            Some("exemplo — 240 × 180 (0,0 MP)")
        );
        window.click("rodar", cx);
    })
    .unwrap();
    bancada.read_with(cx, |b, _| assert!(b.ocupada(), "o clique pôs para rodar"));

    cx.run_until_parked();

    bancada.read_with(cx, |b, _| {
        assert!(!b.ocupada());
        assert!(!b.linhas().is_empty());
        for linha in b.linhas() {
            match linha.estado {
                Estado::Feito(m) => assert!(m.mesma_saida, "{}: a foto mudou", linha.caso.nome),
                outro => panic!("{} ficou em {outro:?}", linha.caso.nome),
            }
        }
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let primeira = window
            .find(("linha", 0usize))
            .label()
            .unwrap_or_default()
            .to_string();
        assert!(primeira.ends_with("✓ igual"), "{primeira}");
        assert!(primeira.contains(" ms | "), "{primeira}");
    })
    .unwrap();
}
