//! Os modos de mesclagem de uma camada — os 16 do Photoshop, com as contas da
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modo {
    #[default]
    Normal,
    Escurecer,
    Multiplicacao,
    SuperexposicaoDeCores,
    Clarear,
    Tela,
    SubexposicaoDeCores,
    Sobrepor,
    LuzSuave,
    LuzDireta,
    Diferenca,
    Exclusao,
    Matiz,
    Saturacao,
    Cor,
    Luminosidade,
}

impl Modo {
    /// Na ordem do menu do Photoshop, com os grupos dele.
    pub const TODOS: [Modo; 16] = [
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
            Modo::Diferenca => "diferenca",
            Modo::Exclusao => "exclusao",
            Modo::Matiz => "matiz",
            Modo::Saturacao => "saturacao",
            Modo::Cor => "cor",
            Modo::Luminosidade => "luminosidade",
        }
    }

    pub fn da_chave(chave: &str) -> Option<Modo> {
        Modo::TODOS.into_iter().find(|m| m.chave() == chave)
    }

    /// `B(baixo, cima)`, com os canais em `0..=1`.
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

/// Um pixel da camada sobre um pixel de baixo, no modo da camada.
#[inline]
pub fn mesclar(baixo: [u8; 3], cima: [u8; 4], opacidade: f32, modo: Modo) -> [u8; 3] {
    if cima[3] == 0 || opacidade <= 0.0 {
        return baixo;
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

#[cfg(test)]
mod testes {
    use super::*;

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
    fn a_chave_vai_e_volta() {
        for modo in Modo::TODOS {
            assert_eq!(Modo::da_chave(modo.chave()), Some(modo));
            let json = serde_json::to_string(&modo).unwrap();
            assert_eq!(json, format!("\"{}\"", modo.chave()));
        }
    }
}
