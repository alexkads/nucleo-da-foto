//! A janela da bancada (GPUI Kit) e o mesmo relatório no terminal.
//!
//! A janela abre uma foto (ou usa a de exemplo), roda os casos um a um **fora
//! da thread da tela** e preenche a tabela conforme cada um termina.

use std::path::Path;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _};
use gpui_kit::{
    div, prelude::*, px, size, App, Bounds, Context, Hsla, PathPromptOptions, SharedString, Task,
    TestSupportExt as _, Window, WindowBounds, WindowOptions,
};

use crate::casos::{casos_para, Caso, Medida};
use crate::foto::Foto;

/// O lado da foto de exemplo: 4000 × 3000, 12 MP — uma câmera de entrada.
pub const EXEMPLO: (u32, u32) = (4000, 3000);

/// Como está cada linha da tabela.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Estado {
    /// Ainda não rodou.
    Esperando,
    /// Rodando agora.
    Rodando,
    /// Terminou.
    Feito(Medida),
}

/// Uma linha da tabela: o caso e como ele está.
#[derive(Clone, Copy)]
pub struct Linha {
    /// O caso.
    pub caso: Caso,
    /// O estado dele.
    pub estado: Estado,
}

/// A janela: a foto, a tabela e o que está rodando.
pub struct Bancada {
    foto: Option<Arc<Foto>>,
    linhas: Vec<Linha>,
    mensagem: SharedString,
    ocupada: bool,
    _tarefa: Option<Task<()>>,
}

impl Bancada {
    /// A janela vazia, esperando uma foto.
    pub fn nova() -> Self {
        Bancada {
            foto: None,
            linhas: Vec::new(),
            mensagem: "Abra uma foto sua (JPEG, PNG, WebP, TIFF ou RAW) ou use a de exemplo."
                .into(),
            ocupada: false,
            _tarefa: None,
        }
    }

    /// As linhas da tabela, na ordem.
    pub fn linhas(&self) -> &[Linha] {
        &self.linhas
    }

    /// A janela está abrindo uma foto ou rodando os casos.
    pub fn ocupada(&self) -> bool {
        self.ocupada
    }

    /// Troca a foto e zera a tabela.
    pub fn usar(&mut self, foto: Foto, cx: &mut Context<Self>) {
        self.mensagem = foto.descricao().into();
        self.linhas = casos_para(&foto)
            .into_iter()
            .map(|caso| Linha {
                caso,
                estado: Estado::Esperando,
            })
            .collect();
        self.foto = Some(Arc::new(foto));
        cx.notify();
    }

    /// Abre a foto de `caminho` fora da thread da tela (um RAW de 24 MP leva
    /// segundos para revelar).
    pub fn abrir_arquivo(&mut self, caminho: std::path::PathBuf, cx: &mut Context<Self>) {
        self.ocupada = true;
        self.mensagem = format!("Abrindo {}…", caminho.display()).into();
        cx.notify();
        self._tarefa = Some(cx.spawn(async move |este, cx| {
            let aberta = cx
                .background_spawn(async move { Foto::abrir(&caminho) })
                .await;
            este.update(cx, |este, cx| {
                este.ocupada = false;
                match aberta {
                    Ok(foto) => este.usar(foto, cx),
                    Err(e) => {
                        este.mensagem = e.into();
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
    }

    /// Pede o arquivo ao sistema e abre.
    fn escolher_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pedido = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Abrir a foto".into()),
        });
        let este = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let caminho = pedido.await.ok()?.ok()??.into_iter().next()?;
            window
                .update(|_, cx| este.update(cx, |este, cx| este.abrir_arquivo(caminho, cx)))
                .ok()
        })
        .detach();
    }

    /// Roda os casos, um a um, fora da thread da tela; cada linha se preenche
    /// quando o caso dela termina.
    pub fn rodar(&mut self, cx: &mut Context<Self>) {
        let Some(foto) = self.foto.clone() else {
            return;
        };
        if self.ocupada {
            return;
        }
        self.ocupada = true;
        for linha in &mut self.linhas {
            linha.estado = Estado::Esperando;
        }
        cx.notify();
        let casos: Vec<Caso> = self.linhas.iter().map(|l| l.caso).collect();
        self._tarefa = Some(cx.spawn(async move |este, cx| {
            for (i, caso) in casos.into_iter().enumerate() {
                let marcou = este.update(cx, |este, cx| {
                    este.linhas[i].estado = Estado::Rodando;
                    cx.notify();
                });
                if marcou.is_err() {
                    return; // a janela fechou
                }
                let foto = foto.clone();
                let medida = cx
                    .background_spawn(async move { (caso.rodar)(&foto) })
                    .await;
                este.update(cx, |este, cx| {
                    este.linhas[i].estado = Estado::Feito(medida);
                    cx.notify();
                })
                .ok();
            }
            este.update(cx, |este, cx| {
                este.ocupada = false;
                cx.notify();
            })
            .ok();
        }));
    }

    fn cabecalho(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tem_foto = self.foto.is_some();
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Bancada do núcleo da foto"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Cada caso roda na última tag (antes) e no código desta pasta (depois), \
                         alternados, com a mesma foto — e confere se a saída é a mesma byte a byte.",
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("abrir")
                            .label("Abrir foto…")
                            .disabled(self.ocupada)
                            .on_click(cx.listener(|este, _, window, cx| {
                                este.escolher_arquivo(window, cx)
                            })),
                    )
                    .child(
                        Button::new("exemplo")
                            .label("Foto de exemplo (12 MP)")
                            .disabled(self.ocupada)
                            .on_click(cx.listener(|este, _, _, cx| {
                                este.usar(Foto::de_exemplo(EXEMPLO.0, EXEMPLO.1), cx)
                            })),
                    )
                    .child(
                        Button::new("rodar")
                            .primary()
                            .label("Rodar")
                            .loading(self.ocupada && tem_foto)
                            .disabled(self.ocupada || !tem_foto)
                            .on_click(cx.listener(|este, _, _, cx| este.rodar(cx))),
                    ),
            )
            .child(
                div()
                    .id("mensagem")
                    .test_support()
                    .aria_label(self.mensagem.clone())
                    .text_sm()
                    .child(self.mensagem.clone()),
            )
    }

    fn tabela(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tema = cx.theme();
        let (fraco, verde, vermelho, borda) = (
            tema.muted_foreground,
            tema.success,
            tema.danger,
            tema.border,
        );
        let celula = |largura: f32, texto: SharedString, cor: Option<Hsla>| {
            let c = div().w(px(largura)).flex_shrink_0().child(texto);
            match cor {
                Some(cor) => c.text_color(cor),
                None => c,
            }
        };
        let cabecalho = h_flex()
            .gap_3()
            .py_1()
            .text_sm()
            .text_color(fraco)
            .border_b_1()
            .border_color(borda)
            .child(celula(90., "Grupo".into(), None))
            .child(div().flex_1().child("Caso"))
            .child(celula(90., "Antes".into(), None))
            .child(celula(90., "Depois".into(), None))
            .child(celula(70., "Ganho".into(), None))
            .child(celula(110., "Mesma foto".into(), None));
        let linhas = self.linhas.iter().enumerate().map(|(i, linha)| {
            let (antes, depois, ganho, igual) = textos(linha.estado);
            let cor_do_ganho = match linha.estado {
                Estado::Feito(m) if m.ganho() >= 1.1 => Some(verde),
                Estado::Feito(m) if m.ganho() <= 0.9 => Some(vermelho),
                _ => None,
            };
            let cor_da_foto = match linha.estado {
                Estado::Feito(m) if m.mesma_saida => Some(verde),
                Estado::Feito(_) => Some(vermelho),
                _ => Some(fraco),
            };
            h_flex()
                .id(("linha", i))
                .test_support()
                .aria_label(SharedString::from(format!(
                    "{} | {} | {antes} | {depois} | {ganho} | {igual}",
                    linha.caso.grupo, linha.caso.nome
                )))
                .gap_3()
                .py_1()
                .text_sm()
                .border_b_1()
                .border_color(borda)
                .child(celula(90., linha.caso.grupo.into(), Some(fraco)))
                .child(div().flex_1().child(linha.caso.nome))
                .child(celula(90., antes, None))
                .child(celula(90., depois, None))
                .child(celula(70., ganho, cor_do_ganho))
                .child(celula(110., igual, cor_da_foto))
        });
        v_flex().child(cabecalho).children(linhas)
    }
}

impl Render for Bancada {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("bancada")
            .size_full()
            .p_5()
            .gap_5()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .overflow_y_scroll()
            .child(self.cabecalho(cx))
            .child(self.tabela(cx))
    }
}

/// Os textos de uma linha: antes, depois, ganho e a conferência da foto.
pub fn textos(estado: Estado) -> (SharedString, SharedString, SharedString, SharedString) {
    match estado {
        Estado::Esperando => ("—".into(), "—".into(), "".into(), "".into()),
        Estado::Rodando => ("medindo…".into(), "".into(), "".into(), "".into()),
        Estado::Feito(m) => (
            milissegundos(m.antes_ms).into(),
            milissegundos(m.depois_ms).into(),
            ganho(m.ganho()).into(),
            if m.mesma_saida {
                "✓ igual".into()
            } else {
                "✗ diferente".into()
            },
        ),
    }
}

/// "3,3×"; acima de 999× (o caminho que deixou de ter custo), "> 999×".
fn ganho(vezes: f64) -> String {
    if vezes > 999.0 {
        return "> 999×".into();
    }
    format!("{vezes:.1}×").replace('.', ",")
}

/// "< 0,01 ms", "0,09 ms", "12,3 ms", "1234 ms".
fn milissegundos(ms: f64) -> String {
    if ms < 0.01 {
        return "< 0,01 ms".into();
    }
    let texto = if ms < 1.0 {
        format!("{ms:.2}")
    } else if ms < 100.0 {
        format!("{ms:.1}")
    } else {
        format!("{ms:.0}")
    };
    format!("{} ms", texto.replace('.', ","))
}

/// Abre a janela da bancada.
pub fn abrir() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            let limites = Bounds::centered(None, size(px(980.), px(760.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(limites)),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Bancada::nova()),
            )
            .expect("a janela da bancada não abriu");
            cx.activate(true);
        });
}

/// A mesma tabela no terminal (`--sem-janela [foto]`). Devolve o código de
/// saída: 1 se algum caso mudou a foto.
#[allow(clippy::print_stdout)]
pub fn sem_janela(caminho: Option<&Path>) -> i32 {
    let foto = match caminho {
        Some(c) => match Foto::abrir(c) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        },
        None => Foto::de_exemplo(EXEMPLO.0, EXEMPLO.1),
    };
    println!("{}\n", foto.descricao());
    println!(
        "{:<10} {:<52} {:>11} {:>11} {:>7}  Mesma foto",
        "Grupo", "Caso", "Antes", "Depois", "Ganho"
    );
    let mut alguma_mudou = false;
    for caso in casos_para(&foto) {
        let m = (caso.rodar)(&foto);
        alguma_mudou |= !m.mesma_saida;
        let (antes, depois, ganho, igual) = textos(Estado::Feito(m));
        // `&str`: o `SharedString` não respeita a largura do `{:>11}`.
        println!(
            "{:<10} {:<52} {:>11} {:>11} {:>7}  {igual}",
            caso.grupo,
            caso.nome,
            antes.as_ref(),
            depois.as_ref(),
            ganho.as_ref()
        );
    }
    i32::from(alguma_mudou)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn os_numeros_saem_em_portugues_e_sem_exagero() {
        assert_eq!(milissegundos(0.004), "< 0,01 ms");
        assert_eq!(milissegundos(0.09), "0,09 ms");
        assert_eq!(milissegundos(18.14), "18,1 ms");
        assert_eq!(milissegundos(593.4), "593 ms");
        assert_eq!(ganho(3.26), "3,3×");
        assert_eq!(ganho(23059.3), "> 999×");
    }
}
