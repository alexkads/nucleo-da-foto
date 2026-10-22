//! Os modos de mesclagem de uma camada — 17 do Photoshop, com as contas da
//! especificação de composição do W3C (*Compositing and Blending Level 1*),
//! que é a mesma família de fórmulas.
//!
//! ```text
//! a   = alfa_do_pixel / 255 · opacidade_da_camada
//! out = baixo + (B(baixo, cima) − baixo) · a
//! ```
//!
//! `baixo` é o que já está composto embaixo (a base e as camadas de baixo), e
//! `B` é o modo. No **Normal** `B(baixo, cima) = cima`, e a conta é a mesma da
//! etapa 1 — bit a bit: projeto antigo compõe igual.
//!
//! 🔑 **Com `a = 0` o pixel de baixo sai exato**, em qualquer modo (C30).

use serde::{Deserialize, Serialize};

/// O modo de mesclagem de uma camada: o `B(baixo, cima)` da conta do topo do
/// módulo. Nas fórmulas abaixo `b` é o canal de baixo e `c` o de cima, ambos em
/// `0..=1`; os separáveis valem canal a canal, os quatro últimos (Matiz,
/// Saturação, Cor, Luminosidade) trabalham com a cor inteira, pela luminância
/// `0,3·R + 0,59·G + 0,11·B` do W3C.
///
/// No manifesto e no roteiro vai pela [`chave`](Modo::chave) em `snake_case`
/// (`"luz_suave"`); o padrão é o [`Normal`](Modo::Normal), e projeto sem o
/// campo abre em Normal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modo {
    /// *Normal*: `c` — a de cima tapa a de baixo na proporção do alfa. O padrão.
    #[default]
    Normal,
    /// *Darken*: `min(b, c)`, canal a canal.
    Escurecer,
    /// *Multiply*: `b·c`. Branco não muda nada; sempre escurece.
    Multiplicacao,
    /// *Color Burn*: `1 − min(1, (1 − b)/c)` (branco embaixo fica branco, `c = 0`
    /// dá preto). Escurece aumentando o contraste.
    SuperexposicaoDeCores,
    /// *Lighten*: `max(b, c)`, canal a canal.
    Clarear,
    /// *Screen*: `b + c − b·c`. Preto não muda nada; sempre clareia.
    Tela,
    /// *Color Dodge*: `min(1, b/(1 − c))` (preto embaixo fica preto, `c = 1` dá
    /// branco). Clareia diminuindo o contraste.
    SubexposicaoDeCores,
    /// *Overlay*: a Luz Direta com os papéis trocados — `2·b·c` onde `b ≤ 0,5`,
    /// tela de `b` com `2c − 1` onde é mais claro. Cinza 50% é neutro.
    Sobrepor,
    /// *Soft Light*, na fórmula do W3C: escurece de leve onde `c ≤ 0,5`
    /// (`b − (1 − 2c)·b·(1 − b)`) e clareia de leve acima, rumo a `√b`.
    LuzSuave,
    /// *Hard Light*: multiplicação dobrada (`2·b·c`) onde `c ≤ 0,5`, tela com
    /// `2c − 1` acima.
    LuzDireta,
    /// *Linear Light*: `b + 2c − 1` — a recomposição da separação de
    /// frequências (`frequencias.rs`). Não é a Luz Direta (*Hard Light*).
    LuzLinear,
    /// *Difference*: `|b − c|`. Preto não muda nada; branco inverte.
    Diferenca,
    /// *Exclusion*: `b + c − 2·b·c` — a Diferença com menos contraste.
    Exclusao,
    /// *Hue*: o matiz da de cima com a saturação e a luminância da de baixo.
    Matiz,
    /// *Saturation*: a saturação da de cima com o matiz e a luminância da de
    /// baixo. Camada cinza tira a cor da foto.
    Saturacao,
    /// *Color*: matiz e saturação da de cima, luminância da de baixo — o de
    /// colorir foto em preto e branco.
    Cor,
    /// *Luminosity*: a luminância da de cima com a cor (matiz e saturação) da de
    /// baixo. O inverso da [`Cor`](Modo::Cor).
    Luminosidade,
}

impl Modo {
    /// Na ordem do menu do Photoshop, com os grupos dele.
    pub const TODOS: [Modo; 17] = [
        Modo::Normal,
        Modo::Escurecer,
        Modo::Multiplicacao,
        Modo::SuperexposicaoDeCores,
        Modo::Clarear,
        Modo::Tela,
        Modo::SubexposicaoDeCores,
        Modo::Sobrepor,
        Modo::LuzSuave,
        Modo::LuzDireta,
        Modo::LuzLinear,
        Modo::Diferenca,
        Modo::Exclusao,
        Modo::Matiz,
        Modo::Saturacao,
        Modo::Cor,
        Modo::Luminosidade,
    ];

    /// O nome do Photoshop em português.
    pub fn nome(self) -> &'static str {
        match self {
            Modo::Normal => "Normal",
            Modo::Escurecer => "Escurecer",
            Modo::Multiplicacao => "Multiplicação",
            Modo::SuperexposicaoDeCores => "Superexposição de Cores",
            Modo::Clarear => "Clarear",
            Modo::Tela => "Tela",
            Modo::SubexposicaoDeCores => "Subexposição de Cores",
            Modo::Sobrepor => "Sobrepor",
            Modo::LuzSuave => "Luz Suave",
            Modo::LuzDireta => "Luz Direta",
            Modo::LuzLinear => "Luz Linear",
            Modo::Diferenca => "Diferença",
            Modo::Exclusao => "Exclusão",
            Modo::Matiz => "Matiz",
            Modo::Saturacao => "Saturação",
            Modo::Cor => "Cor",
            Modo::Luminosidade => "Luminosidade",
        }
    }

    /// A chave estável (a do manifesto), para o roteiro e a tela.
    pub fn chave(self) -> &'static str {
        match self {
            Modo::Normal => "normal",
            Modo::Escurecer => "escurecer",
            Modo::Multiplicacao => "multiplicacao",
            Modo::SuperexposicaoDeCores => "superexposicao_de_cores",
            Modo::Clarear => "clarear",
            Modo::Tela => "tela",
            Modo::SubexposicaoDeCores => "subexposicao_de_cores",
            Modo::Sobrepor => "sobrepor",
            Modo::LuzSuave => "luz_suave",
            Modo::LuzDireta => "luz_direta",
            Modo::LuzLinear => "luz_linear",
            Modo::Diferenca => "diferenca",
            Modo::Exclusao => "exclusao",
            Modo::Matiz => "matiz",
            Modo::Saturacao => "saturacao",
            Modo::Cor => "cor",
            Modo::Luminosidade => "luminosidade",
        }
    }

    /// O caminho de volta da [`chave`](Modo::chave): `None` se a chave não é de
    /// nenhum modo (manifesto de versão futura, roteiro com erro de digitação).
    pub fn da_chave(chave: &str) -> Option<Modo> {
        Modo::TODOS.into_iter().find(|m| m.chave() == chave)
    }

    /// `B(baixo, cima)`, com os canais em `0..=1`. Só a fórmula do modo, sem
    /// alfa nem opacidade: quem pondera é [`mesclar`] ou [`mesclar_em_camada`].
    ///
    /// 🚨 Não corta o resultado — a [`LuzLinear`](Modo::LuzLinear) sai de
    /// `0..=1`, e quem chama corta.
    ///
    /// ```rust
    /// use editor_core::Modo;
    ///
    /// let b = [0.5, 0.25, 0.0];
    /// // Multiplicação por branco e Tela com preto devolvem o de baixo.
    /// assert_eq!(Modo::Multiplicacao.misturar(b, [1.0; 3]), b);
    /// assert_eq!(Modo::Tela.misturar(b, [0.0; 3]), b);
    /// // Diferença com branco inverte.
    /// assert_eq!(Modo::Diferenca.misturar(b, [1.0; 3]), [0.5, 0.75, 1.0]);
    /// ```
    pub fn misturar(self, b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
        let canal = |f: fn(f32, f32) -> f32| [f(b[0], c[0]), f(b[1], c[1]), f(b[2], c[2])];
        match self {
            Modo::Normal => c,
            Modo::Escurecer => canal(f32::min),
            Modo::Multiplicacao => canal(|b, c| b * c),
            Modo::SuperexposicaoDeCores => canal(|b, c| {
                if b >= 1.0 {
                    1.0
                } else if c <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - b) / c).min(1.0)
                }
            }),
            Modo::Clarear => canal(f32::max),
            Modo::Tela => canal(tela),
            Modo::SubexposicaoDeCores => canal(|b, c| {
                if b <= 0.0 {
                    0.0
                } else if c >= 1.0 {
                    1.0
                } else {
                    (b / (1.0 - c)).min(1.0)
                }
            }),
            Modo::Sobrepor => canal(|b, c| luz_direta(c, b)),
            Modo::LuzSuave => canal(|b, c| {
                if c <= 0.5 {
                    b - (1.0 - 2.0 * c) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 {
                        ((16.0 * b - 12.0) * b + 4.0) * b
                    } else {
                        b.sqrt()
                    };
                    b + (2.0 * c - 1.0) * (d - b)
                }
            }),
            Modo::LuzDireta => canal(luz_direta),
            // Sem corte aqui: quem compõe corta em 0..=1 (o W3C não tem este
            // modo; é o do Photoshop e o do PDF 2.0 de fora da lista).
            Modo::LuzLinear => canal(|b, c| b + 2.0 * c - 1.0),
            Modo::Diferenca => canal(|b, c| (b - c).abs()),
            Modo::Exclusao => canal(|b, c| b + c - 2.0 * b * c),
            Modo::Matiz => com_lum(com_sat(c, sat(b)), lum(b)),
            Modo::Saturacao => com_lum(com_sat(b, sat(c)), lum(b)),
            Modo::Cor => com_lum(c, lum(b)),
            Modo::Luminosidade => com_lum(b, lum(c)),
        }
    }
}

fn tela(b: f32, c: f32) -> f32 {
    b + c - b * c
}

/// `HardLight(b, c)`: multiplica onde a de cima é escura, tela onde é clara.
fn luz_direta(b: f32, c: f32) -> f32 {
    if c <= 0.5 {
        b * 2.0 * c
    } else {
        tela(b, 2.0 * c - 1.0)
    }
}

// Os modos não separáveis: luminância e saturação, do W3C.

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn cortar(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        for v in &mut c {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut c {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    c
}

fn com_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    cortar([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn com_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let maior = c[0].max(c[1]).max(c[2]);
    let menor = c[0].min(c[1]).min(c[2]);
    if maior <= menor {
        return [0.0; 3];
    }
    c.map(|v| (v - menor) * s / (maior - menor))
}

/// Um pixel da camada sobre um pixel de baixo, no modo da camada — a conta do
/// topo do módulo, a que a composição faz em cada pixel.
///
/// `baixo` é o já composto (opaco, RGB8); `cima` é o pixel da camada, RGBA8 com
/// o alfa não pré-multiplicado; `opacidade` é a da camada, em `0..=1` (acima de
/// 1 conta como 1). O resultado é arredondado e cortado em `0..=255`.
///
/// 🔑 Alfa 0 ou opacidade `≤ 0` devolvem `baixo` exato, em qualquer modo; e o
/// Normal não passa por `0..=1`, para compor bit a bit igual ao projeto antigo.
///
/// ```rust
/// use editor_core::Modo;
/// use editor_core::mesclagem::mesclar;
///
/// let baixo = [200, 100, 30];
/// // Normal a 50%: metade do caminho até a cor de cima.
/// assert_eq!(mesclar(baixo, [0, 0, 0, 255], 0.5, Modo::Normal), [100, 50, 15]);
/// // Multiplicação: 200·60/255 ≈ 47, 100·180/255 ≈ 71, 30·240/255 ≈ 28.
/// assert_eq!(mesclar(baixo, [60, 180, 240, 255], 1.0, Modo::Multiplicacao), [47, 71, 28]);
/// // Transparente não mexe em nada.
/// assert_eq!(mesclar(baixo, [9, 9, 9, 0], 1.0, Modo::Diferenca), baixo);
/// ```
#[inline]
pub fn mesclar(baixo: [u8; 3], cima: [u8; 4], opacidade: f32, modo: Modo) -> [u8; 3] {
    if cima[3] == 0 || opacidade <= 0.0 {
        return baixo;
    }
    // 🔑 **Normal opaco é a cor de cima, exata** — o caso de quase todo pixel
    // pintado. Com `a = 1` a conta abaixo dá `b + (c − b)·1 = c` sem erro de
    // arredondamento (inteiros até 255 são exatos em `f32`); sair antes poupa
    // três `round` por pixel e camada em toda composição.
    if modo == Modo::Normal && cima[3] == 255 && opacidade >= 1.0 {
        return [cima[0], cima[1], cima[2]];
    }
    let a = cima[3] as f32 / 255.0 * opacidade.min(1.0);
    let alvo: [f32; 3] = match modo {
        // O Normal sem passar por `0..=1`: a conta da etapa 1, bit a bit.
        Modo::Normal => [cima[0] as f32, cima[1] as f32, cima[2] as f32],
        _ => {
            let n = |v: u8| v as f32 / 255.0;
            modo.misturar(
                [n(baixo[0]), n(baixo[1]), n(baixo[2])],
                [n(cima[0]), n(cima[1]), n(cima[2])],
            )
            .map(|v| v.clamp(0.0, 1.0) * 255.0)
        }
    };
    let mistura = |b: u8, c: f32| -> u8 {
        let b = b as f32;
        (b + (c - b) * a).round().clamp(0.0, 255.0) as u8
    };
    [
        mistura(baixo[0], alvo[0]),
        mistura(baixo[1], alvo[1]),
        mistura(baixo[2], alvo[2]),
    ]
}

/// Um pixel da camada de cima sobre um pixel **de camada** (com alfa) — o
/// "Mesclar para baixo". A conta geral do W3C para fundo com alfa:
///
/// ```text
/// cima' = (1 − αb)·cima + αb·B(baixo, cima)
/// αo    = αs + αb·(1 − αs)
/// cor   = (αs·cima' + αb·(1 − αs)·baixo) / αo
/// ```
///
/// Sobre fundo opaco (αb = 1) é a mesma conta de [`mesclar`]; sobre fundo
/// transparente, o modo não tem com o que misturar e a camada entra como está.
pub fn mesclar_em_camada(baixo: [u8; 4], cima: [u8; 4], opacidade: f32, modo: Modo) -> [u8; 4] {
    let a_s = cima[3] as f32 / 255.0 * opacidade.clamp(0.0, 1.0);
    if a_s <= 0.0 {
        return baixo;
    }
    // 🔑 **Normal com opacidade cheia: cima opaco, ou nada por baixo, é a
    // cor de cima, exata** — o ⌘T e o colar quase sempre caem aqui. A conta
    // abaixo devolve `cima` nesses dois casos para todo par de bytes
    // (`os_atalhos_da_camada_sao_a_conta_de_sempre` confere os 65 536), e
    // sair antes poupa as divisões e os arredondamentos por pixel.
    if modo == Modo::Normal && opacidade >= 1.0 && (cima[3] == 255 || baixo[3] == 0) {
        return cima;
    }
    let a_b = baixo[3] as f32 / 255.0;
    let n = |v: u8| v as f32 / 255.0;
    let b = [n(baixo[0]), n(baixo[1]), n(baixo[2])];
    let c = [n(cima[0]), n(cima[1]), n(cima[2])];
    let misturada = modo.misturar(b, c);
    let a_o = a_s + a_b * (1.0 - a_s);
    let canal = |i: usize| -> u8 {
        let c_linha = (1.0 - a_b) * c[i] + a_b * misturada[i].clamp(0.0, 1.0);
        let v = (a_s * c_linha + a_b * (1.0 - a_s) * b[i]) / a_o;
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [
        canal(0),
        canal(1),
        canal(2),
        (a_o * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod testes {
    use super::*;

    /// A conta de `mesclar_em_camada` sem os atalhos — a de antes de
    /// 2026-10-22, para conferir que eles dão o mesmo byte.
    fn em_camada_pela_conta(baixo: [u8; 4], cima: [u8; 4]) -> [u8; 4] {
        let a_s = cima[3] as f32 / 255.0 * 1.0;
        if a_s <= 0.0 {
            return baixo;
        }
        let a_b = baixo[3] as f32 / 255.0;
        let n = |v: u8| v as f32 / 255.0;
        let b = [n(baixo[0]), n(baixo[1]), n(baixo[2])];
        let c = [n(cima[0]), n(cima[1]), n(cima[2])];
        let misturada = Modo::Normal.misturar(b, c);
        let a_o = a_s + a_b * (1.0 - a_s);
        let canal = |i: usize| -> u8 {
            let c_linha = (1.0 - a_b) * c[i] + a_b * misturada[i].clamp(0.0, 1.0);
            let v = (a_s * c_linha + a_b * (1.0 - a_s) * b[i]) / a_o;
            (v * 255.0).round().clamp(0.0, 255.0) as u8
        };
        [
            canal(0),
            canal(1),
            canal(2),
            (a_o * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }

    /// 🚨 **Os atalhos do Normal em camada são a conta de sempre**, byte a
    /// byte: cima opaco sobre todo alfa de baixo, e todo alfa de cima sobre
    /// nada — com todo valor de canal (o resultado de cada canal só depende
    /// dele e dos dois alfas), e a cor de baixo variando junto.
    #[test]
    fn os_atalhos_da_camada_sao_a_conta_de_sempre() {
        for alfa in 0..=255u8 {
            for v in 0..=255u8 {
                let w = v.wrapping_mul(37).wrapping_add(11);
                // Cima opaco, baixo com qualquer alfa.
                let (baixo, cima) = ([w, v, 255 - v, alfa], [v, w, 255 - w, 255]);
                assert_eq!(
                    mesclar_em_camada(baixo, cima, 1.0, Modo::Normal),
                    em_camada_pela_conta(baixo, cima),
                    "opaco sobre alfa {alfa}, canal {v}"
                );
                // Nada por baixo, cima com qualquer alfa.
                let (baixo, cima) = ([w, 255 - v, v, 0], [v, w, 255 - w, alfa]);
                assert_eq!(
                    mesclar_em_camada(baixo, cima, 1.0, Modo::Normal),
                    em_camada_pela_conta(baixo, cima),
                    "alfa {alfa} sobre nada, canal {v}"
                );
                // E acima de 1 a opacidade conta como 1.
                assert_eq!(
                    mesclar_em_camada(baixo, cima, 1.5, Modo::Normal),
                    em_camada_pela_conta(baixo, cima)
                );
            }
        }
    }

    /// 🚨 **O atalho do Normal opaco devolve o que a conta devolvia**, para
    /// todo par de valores e para opacidade acima de 1.
    #[test]
    fn o_normal_opaco_e_a_conta_de_sempre() {
        let conta = |b: u8, c: u8| -> u8 {
            let a = 255.0f32 / 255.0 * 1.0;
            let b = b as f32;
            (b + (c as f32 - b) * a).round().clamp(0.0, 255.0) as u8
        };
        for b in 0..=255u8 {
            for c in 0..=255u8 {
                for opacidade in [1.0, 1.5] {
                    assert_eq!(
                        mesclar([b, b, b], [c, c, c, 255], opacidade, Modo::Normal),
                        [conta(b, c); 3],
                        "{b} sob {c}"
                    );
                }
            }
        }
    }

    const B: [u8; 3] = [200, 100, 30];
    const C: [u8; 4] = [60, 180, 240, 255];

    fn em(modo: Modo) -> [u8; 3] {
        mesclar(B, C, 1.0, modo)
    }

    #[test]
    fn as_contas_de_cada_modo() {
        assert_eq!(em(Modo::Normal), [60, 180, 240]);
        assert_eq!(em(Modo::Escurecer), [60, 100, 30]);
        assert_eq!(em(Modo::Clarear), [200, 180, 240]);
        // 200·60/255 = 47,06 · 100·180/255 = 70,59 · 30·240/255 = 28,24
        assert_eq!(em(Modo::Multiplicacao), [47, 71, 28]);
        // b + c − b·c
        assert_eq!(em(Modo::Tela), [213, 209, 242]);
        assert_eq!(em(Modo::Diferenca), [140, 80, 210]);
        // b + c − 2bc: 200+60−94,1 = 165,9 · 100+180−141,2 = 138,8 · 30+240−56,5 = 213,5
        assert_eq!(em(Modo::Exclusao), [166, 139, 214]);
        // Sobrepor: luz direta com os papéis trocados. b=200 é claro → tela;
        // b=100 e b=30 são escuros → multiplicação dobrada.
        let s = em(Modo::Sobrepor);
        assert_eq!(s[1], (2.0 * 100.0 * 180.0 / 255.0f32).round() as u8);
        assert_eq!(s[2], (2.0 * 30.0 * 240.0 / 255.0f32).round() as u8);
    }

    #[test]
    fn preto_e_branco_nos_modos_neutros_nao_mudam_a_foto() {
        let preto = [0, 0, 0, 255];
        let branco = [255, 255, 255, 255];
        assert_eq!(mesclar(B, branco, 1.0, Modo::Multiplicacao), B);
        assert_eq!(mesclar(B, preto, 1.0, Modo::Tela), B);
        assert_eq!(mesclar(B, preto, 1.0, Modo::Diferenca), B);
        assert_eq!(mesclar(B, preto, 1.0, Modo::SubexposicaoDeCores), B);
        assert_eq!(mesclar(B, branco, 1.0, Modo::SuperexposicaoDeCores), B);
        let cinza = [128, 128, 128, 255];
        let quase = mesclar(B, cinza, 1.0, Modo::Sobrepor);
        for i in 0..3 {
            assert!((quase[i] as i32 - B[i] as i32).abs() <= 1, "{quase:?}");
        }
    }

    #[test]
    fn transparente_ou_sem_opacidade_devolve_o_de_baixo_em_todo_modo() {
        for modo in Modo::TODOS {
            assert_eq!(mesclar(B, [9, 9, 9, 0], 1.0, modo), B, "{modo:?}");
            assert_eq!(mesclar(B, C, 0.0, modo), B, "{modo:?}");
        }
    }

    #[test]
    fn cor_e_luminosidade_trocam_o_que_prometem() {
        // Luminosidade: a luminância da de cima, a cor da de baixo.
        let l = em(Modo::Luminosidade);
        let lum8 = |p: [u8; 3]| 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32;
        assert!((lum8(l) - lum8([C[0], C[1], C[2]])).abs() < 1.5);
        // Cor: a luminância da de baixo.
        let c = em(Modo::Cor);
        assert!((lum8(c) - lum8(B)).abs() < 1.5);
        // Cinza neutro em Cor tira a cor da foto.
        let cinza = mesclar(B, [128, 128, 128, 255], 1.0, Modo::Cor);
        assert!(cinza[0] == cinza[1] && cinza[1] == cinza[2], "{cinza:?}");
        // Saturação de uma camada cinza também tira a cor.
        let s = mesclar(B, [90, 90, 90, 255], 1.0, Modo::Saturacao);
        assert!(s[0] == s[1] && s[1] == s[2], "{s:?}");
    }

    #[test]
    fn mesclar_em_camada_opaca_e_a_mesma_conta_da_composicao() {
        for modo in Modo::TODOS {
            let em_camada = mesclar_em_camada([B[0], B[1], B[2], 255], C, 0.7, modo);
            let na_foto = mesclar(B, C, 0.7, modo);
            assert_eq!(em_camada[3], 255);
            for i in 0..3 {
                assert!(
                    (em_camada[i] as i32 - na_foto[i] as i32).abs() <= 1,
                    "{modo:?}: {em_camada:?} × {na_foto:?}"
                );
            }
        }
        // Sobre transparente, entra a de cima como está, com a opacidade no alfa.
        assert_eq!(
            mesclar_em_camada([0; 4], C, 0.5, Modo::Multiplicacao),
            [60, 180, 240, 128]
        );
    }

    #[test]
    fn luz_linear_e_b_mais_2c_menos_1_e_nao_e_a_luz_direta() {
        // c = 60/255: 200 + 120 − 255 = 65 · 100 + 120 − 255 < 0 → 0 · 30 + 120 − 255 → 0.
        assert_eq!(
            mesclar(B, [60, 60, 60, 255], 1.0, Modo::LuzLinear),
            [65, 0, 0]
        );
        // c = 240/255: 200 + 480 − 255 > 255 → 255 · 100 + 225 = 325 → 255 · 30 + 225 = 255.
        assert_eq!(
            mesclar(B, [240, 240, 240, 255], 1.0, Modo::LuzLinear),
            [255, 255, 255]
        );
        // c = 140: b + 25.
        assert_eq!(
            mesclar(B, [140, 140, 140, 255], 1.0, Modo::LuzLinear),
            [225, 125, 55]
        );
        // A Luz Direta no mesmo par dá outra coisa.
        assert_ne!(
            mesclar(B, [140, 140, 140, 255], 1.0, Modo::LuzDireta),
            [225, 125, 55]
        );
        // Opacidade e alfa entram como em todo modo: metade do caminho.
        assert_eq!(
            mesclar(B, [140, 140, 140, 255], 0.5, Modo::LuzLinear),
            [213, 113, 43]
        );
        assert_eq!(
            mesclar(B, [140, 140, 140, 128], 1.0, Modo::LuzLinear),
            [213, 113, 43]
        );
        // Sobre camada transparente entra a de cima como está.
        assert_eq!(
            mesclar_em_camada([0; 4], [140, 140, 140, 255], 1.0, Modo::LuzLinear),
            [140, 140, 140, 255]
        );
        assert_eq!(Modo::da_chave("luz_linear"), Some(Modo::LuzLinear));
        assert_eq!(Modo::LuzLinear.nome(), "Luz Linear");
    }

    #[test]
    fn luz_linear_recompoe_a_separacao_exata_em_todo_par_de_8_bits() {
        // A conta da separação (`frequencias.rs`): com I − L ímpar,
        // h = (I − L + 255)/2 é inteiro e L + 2h − 255 = I.
        for l in 0..=255i32 {
            for i in 0..=255i32 {
                if (i - l).rem_euclid(2) == 0 {
                    continue;
                }
                let h = ((i - l + 255) / 2) as u8;
                let r = mesclar([l as u8; 3], [h, h, h, 255], 1.0, Modo::LuzLinear);
                assert_eq!(r, [i as u8; 3], "L {l} I {i} h {h}");
            }
        }
    }

    #[test]
    fn a_chave_vai_e_volta() {
        for modo in Modo::TODOS {
            assert_eq!(Modo::da_chave(modo.chave()), Some(modo));
            let json = serde_json::to_string(&modo).unwrap();
            assert_eq!(json, format!("\"{}\"", modo.chave()));
        }
    }
}
