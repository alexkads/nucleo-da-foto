//! Os módulos do darktable, reescritos a partir do código-fonte do `release-5.6.1`.
//!
//! # Por que existe
//!
//! 🚨 **Em 2026-09-12 o dono pediu para importar os estilos do darktable "sem
//! faltar nenhum ajuste"**, e a conferência (`docs/DARKTABLE_CONFERENCIA.md` no
//! e-commerce) mostrou que nenhum dos módulos do estilo `RecordarFotos P&B` tinha
//! equivalente no nosso motor com a mesma conta. Converter os números para os
//! nossos controles não serve, por três motivos medidos: o darktable opera em RGB
//! **linear** Rec.2020 e em Lab, nós em sRGB com gama; dois dos módulos são
//! **locais**; e a melhor escala das nossas curvas ainda errava ~8 níveis.
//!
//! Este arquivo é a **referência de CPU**: cada módulo é a tradução linha a linha
//! do C do darktable, e cada função cita o arquivo e a linha de onde saiu. Ele
//! serve a duas coisas, e nenhuma é a revelação do dia a dia:
//!
//! 1. ser comparado com a saída do `darktable-cli` 5.6.1 sobre uma carta de
//!    teste, módulo a módulo — é o que prova que a tradução está certa;
//! 2. ser o oráculo dos testes do porte em WGSL, como o [`crate::transformacao`]
//!    já é para o enquadramento.
//!
//! # A tubulação de cor
//!
//! Tudo aqui trabalha no espaço em que o darktable trabalha: **RGB linear
//! Rec.2020**, com as primárias adaptadas para o branco D50 por Bradford, que é o
//! que o lcms faz em `cmsCreateRGBProfile` (`src/common/colorspaces.c:285–333`).
//! Entra sRGB de 8 bits, sai sRGB de 8 bits.

// 🔑 **As constantes são copiadas do código do darktable como estão no fonte**
// (matrizes CAT16, LMS, Filmlight, coeficientes do dt UCS). O clippy acusa
// "precisão excessiva" porque um `f32` não guarda todos aqueles dígitos — e não
// guarda no C também: o compilador de lá arredonda o mesmo literal para o mesmo
// `float`. Encurtar os números aqui seria escolher um arredondamento diferente do
// deles, e poder conferir cada número contra o arquivo citado vale mais.
#![allow(clippy::excessive_precision)]

use std::f32::consts::PI;

use crate::Ajustes;

/// Um pixel no espaço de trabalho: RGB linear Rec.2020, branco D50.
pub type Rgb = [f32; 3];
type Matriz = [[f32; 3]; 3];

// ------------------------------------------------------------------ constantes

/// O branco D65 do darktable, em xy (`src/common/colorspaces.h:38`).
const D65_XY: [f64; 2] = [0.31271, 0.32902];
/// O D50 do PCS do lcms, em XYZ — e o do Lab do darktable
/// (`src/common/colorspaces_inline_conversions.h:144`).
const D50_XYZ: [f64; 3] = [0.9642, 1.0, 0.8249];

/// Primárias sRGB (`src/common/colorspaces.c:57`).
const PRIMARIAS_SRGB: [[f64; 2]; 3] = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]];
/// Primárias Rec.2020 (`src/common/colorspaces.c:64`).
const PRIMARIAS_REC2020: [[f64; 2]; 3] = [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]];

/// `src/common/chromatic_adaptation.h:375`.
const XYZ_D50_PARA_D65_CAT16: Matriz = [
    [9.89466254e-01, -4.00304626e-02, 4.40530317e-02],
    [-5.40518733e-03, 1.00666069e+00, -1.75551955e-03],
    [-4.03920992e-04, 1.50768030e-02, 1.30210211e+00],
];

/// `src/common/chromatic_adaptation.h:390`. Constante própria do darktable, e
/// não a inversa da de cima: as duas foram arredondadas separadamente.
const XYZ_D65_PARA_D50_CAT16: Matriz = [
    [1.01085433e+00, 4.07086103e-02, -3.41445825e-02],
    [5.42814201e-03, 9.93581926e-01, 1.15592039e-03],
    [2.50722468e-04, -1.14918759e-02, 7.67964947e-01],
];

/// `src/common/colorspaces_inline_conversions.h:984`.
const XYZ_D65_PARA_LMS_2006: Matriz = [
    [0.257085, 0.859943, -0.031061],
    [-0.394427, 1.175800, 0.106423],
    [0.064856, -0.076250, 0.559067],
];
/// `src/common/colorspaces_inline_conversions.h:994`.
const LMS_2006_PARA_XYZ_D65: Matriz = [
    [1.80794659, -1.29971660, 0.34785879],
    [0.61783960, 0.39595453, -0.04104687],
    [-0.12546960, 0.20478038, 1.74274183],
];
/// A transposta de `filmlightRGB_D65_to_LMS_D65_trans`
/// (`src/common/colorspaces_inline_conversions.h:1024`).
const FILMLIGHT_PARA_LMS: Matriz = [[0.95, 0.38, 0.00], [0.05, 0.62, 0.03], [0.00, 0.00, 0.97]];
/// A transposta de `LMS_D65_to_filmlightRGB_D65_trans` (`:1029`).
const LMS_PARA_FILMLIGHT: Matriz = [
    [1.08771930, -0.66666667, 0.02061856],
    [-0.0877193, 1.66666667, -0.05154639],
    [0.0, 0.0, 1.03092784],
];

const DT_UCS_L_STAR_RANGE: f32 = 2.098883786377;
const DT_UCS_L_STAR_UPPER_LIMIT: f32 = 2.09885;
/// `src/common/math.h:26`.
const LUT_ELEM: usize = 512;
/// O deslocamento de matiz entre a roda da tela e o Filmlight Yrg
/// (`src/iop/colorbalancergb.c:48`): lá o vermelho fica em 330°.
const ANGLE_SHIFT: f32 = -30.0;

// ------------------------------------------------------------- álgebra pequena

fn mul(m: &Matriz, v: Rgb) -> Rgb {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn mul_mat(a: &Matriz, b: &Matriz) -> Matriz {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

type Matriz64 = [[f64; 3]; 3];

fn mul_mat64(a: &Matriz64, b: &Matriz64) -> Matriz64 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn inversa64(m: &Matriz64) -> Matriz64 {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let c =
        |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    [
        [
            c(1, 1, 2, 2) / det,
            -c(0, 1, 2, 2) / det,
            c(0, 1, 1, 2) / det,
        ],
        [
            -c(1, 0, 2, 2) / det,
            c(0, 0, 2, 2) / det,
            -c(0, 0, 1, 2) / det,
        ],
        [
            c(1, 0, 2, 1) / det,
            -c(0, 0, 2, 1) / det,
            c(0, 0, 1, 1) / det,
        ],
    ]
}

fn para_f32(m: &Matriz64) -> Matriz {
    let mut r = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = m[i][j] as f32;
        }
    }
    r
}

// --------------------------------------------------------- perfis e tubulação

/// RGB → XYZ **adaptado para D50 por Bradford**, como o `cmsCreateRGBProfile`
/// do lcms grava os colorantes de um perfil de matriz.
fn rgb_para_xyz_d50(primarias: &[[f64; 2]; 3]) -> Matriz64 {
    let branco = {
        let [x, y] = D65_XY;
        [x / y, 1.0, (1.0 - x - y) / y]
    };
    // As colunas são as primárias em XYZ, escaladas para somar o branco.
    let p: Matriz64 = {
        let col = |[x, y]: [f64; 2]| [x / y, 1.0, (1.0 - x - y) / y];
        let (r, g, b) = (col(primarias[0]), col(primarias[1]), col(primarias[2]));
        [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]]
    };
    let s = {
        let inv = inversa64(&p);
        [
            inv[0][0] * branco[0] + inv[0][1] * branco[1] + inv[0][2] * branco[2],
            inv[1][0] * branco[0] + inv[1][1] * branco[1] + inv[1][2] * branco[2],
            inv[2][0] * branco[0] + inv[2][1] * branco[1] + inv[2][2] * branco[2],
        ]
    };
    let rgb_para_xyz_d65: Matriz64 = [
        [p[0][0] * s[0], p[0][1] * s[1], p[0][2] * s[2]],
        [p[1][0] * s[0], p[1][1] * s[1], p[1][2] * s[2]],
        [p[2][0] * s[0], p[2][1] * s[1], p[2][2] * s[2]],
    ];
    // Bradford, a adaptação padrão do lcms.
    let bradford: Matriz64 = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let cone = |w: [f64; 3]| {
        [
            bradford[0][0] * w[0] + bradford[0][1] * w[1] + bradford[0][2] * w[2],
            bradford[1][0] * w[0] + bradford[1][1] * w[1] + bradford[1][2] * w[2],
            bradford[2][0] * w[0] + bradford[2][1] * w[1] + bradford[2][2] * w[2],
        ]
    };
    let (origem, destino) = (cone(branco), cone(D50_XYZ));
    let escala: Matriz64 = [
        [destino[0] / origem[0], 0.0, 0.0],
        [0.0, destino[1] / origem[1], 0.0],
        [0.0, 0.0, destino[2] / origem[2]],
    ];
    let adaptacao = mul_mat64(&inversa64(&bradford), &mul_mat64(&escala, &bradford));
    mul_mat64(&adaptacao, &rgb_para_xyz_d65)
}

/// As matrizes da tubulação, calculadas uma vez.
pub struct Tubulacao {
    srgb_para_trabalho: Matriz,
    trabalho_para_srgb: Matriz,
    /// `work_profile->matrix_in`: Rec.2020 linear → XYZ D50.
    trabalho_para_xyz_d50: Matriz,
    /// `work_profile->matrix_out`.
    xyz_d50_para_trabalho: Matriz,
}

impl Tubulacao {
    pub fn nova() -> Self {
        let srgb = rgb_para_xyz_d50(&PRIMARIAS_SRGB);
        let rec2020 = rgb_para_xyz_d50(&PRIMARIAS_REC2020);
        let rec2020_inv = inversa64(&rec2020);
        Self {
            srgb_para_trabalho: para_f32(&mul_mat64(&rec2020_inv, &srgb)),
            trabalho_para_srgb: para_f32(&mul_mat64(&inversa64(&srgb), &rec2020)),
            trabalho_para_xyz_d50: para_f32(&rec2020),
            xyz_d50_para_trabalho: para_f32(&rec2020_inv),
        }
    }

    /// 8 bits sRGB → trabalho. A curva é a paramétrica tipo 4 do lcms com os
    /// parâmetros do sRGB (`src/common/colorspaces.c:405`).
    pub fn entrar(&self, rgb8: [u8; 3]) -> Rgb {
        let lin = |v: u8| {
            let x = v as f32 / 255.0;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        mul(
            &self.srgb_para_trabalho,
            [lin(rgb8[0]), lin(rgb8[1]), lin(rgb8[2])],
        )
    }

    /// Trabalho → 8 bits sRGB, recortado e arredondado.
    pub fn sair(&self, rgb: Rgb) -> [u8; 3] {
        let s = mul(&self.trabalho_para_srgb, rgb);
        let cod = |v: f32| {
            let v = v.max(0.0);
            let e = if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            (e * 255.0).round().clamp(0.0, 255.0) as u8
        };
        [cod(s[0]), cod(s[1]), cod(s[2])]
    }

    /// Trabalho → Lab D50 (`dt_XYZ_to_Lab`, `colorspaces_inline_conversions.h:148`).
    pub fn para_lab(&self, rgb: Rgb) -> Rgb {
        let xyz = mul(&self.trabalho_para_xyz_d50, rgb);
        let (eps, kappa) = (216.0f32 / 24389.0, 24389.0f32 / 27.0);
        let f = |v: f32, w: f64| {
            let x = v / w as f32;
            if x > eps {
                x.cbrt()
            } else {
                (kappa * x + 16.0) / 116.0
            }
        };
        let (fx, fy, fz) = (
            f(xyz[0], D50_XYZ[0]),
            f(xyz[1], D50_XYZ[1]),
            f(xyz[2], D50_XYZ[2]),
        );
        [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
    }

    /// Só o L do Lab — o que a grade do `shadows and highlights` espalha. A mesma
    /// conta de [`Tubulacao::para_lab`], com uma raiz cúbica em vez de três.
    pub fn luminancia_lab(&self, rgb: Rgb) -> f32 {
        let y = mul(&self.trabalho_para_xyz_d50, rgb)[1] / D50_XYZ[1] as f32;
        let (eps, kappa) = (216.0f32 / 24389.0, 24389.0f32 / 27.0);
        let fy = if y > eps {
            y.cbrt()
        } else {
            (kappa * y + 16.0) / 116.0
        };
        116.0 * fy - 16.0
    }

    /// Lab D50 → trabalho (`dt_Lab_to_XYZ`, `colorspaces_inline_conversions.h:192`).
    pub fn de_lab(&self, lab: Rgb) -> Rgb {
        let fy = (lab[0] + 16.0) / 116.0;
        let fx = fy + lab[1] / 500.0;
        let fz = fy - lab[2] / 200.0;
        let inv = |x: f32| {
            let eps = 0.20689655172413796f32;
            if x > eps {
                x * x * x
            } else {
                (116.0 * x - 16.0) / (24389.0 / 27.0)
            }
        };
        let xyz = [
            D50_XYZ[0] as f32 * inv(fx),
            D50_XYZ[1] as f32 * inv(fy),
            D50_XYZ[2] as f32 * inv(fz),
        ];
        mul(&self.xyz_d50_para_trabalho, xyz)
    }
}

// ------------------------------------------------------------------- exposure

/// `dt_iop_exposure_params_t` v7 — só o que age fora de RAW.
#[derive(Debug, Clone, Copy)]
pub struct Exposure {
    pub black: f32,
    pub exposure: f32,
}

/// `src/iop/exposure.c:44, 508, 562`: `out = (in − black) · 1/(2^−e − black)`.
///
/// ⚠️ `compensate_hilite_pres` e `compensate_exposure_bias` somam valores do
/// EXIF de RAW Nikon/Fuji (`exposure.c:569–627`); fora deles valem zero, e é o
/// caso de toda foto que o site recebe.
pub fn exposure(pixels: &mut [Rgb], p: Exposure) {
    let white = 2f32.powf(-p.exposure);
    let scale = 1.0 / (white - p.black);
    for px in pixels.iter_mut() {
        for c in px.iter_mut() {
            *c = (*c - p.black) * scale;
        }
    }
}

// ------------------------------------------------------------------- vignette

/// `dt_iop_vignette_params_t` v4.
#[derive(Debug, Clone, Copy)]
pub struct Vignette {
    pub scale: f32,
    pub falloff_scale: f32,
    pub brightness: f32,
    pub saturation: f32,
    pub center: [f32; 2],
    pub autoratio: bool,
    pub whratio: f32,
    pub shape: f32,
    pub unbound: bool,
}

/// `src/iop/vignette.c:694–841`, sem pontilhamento (desligado no estilo).
///
/// 🔑 **Brilho positivo SOMA, negativo multiplica** (`vignette.c:816–827`). É o
/// que permite a vinheta branca do `RecordarFotos P&B`, e o que o nosso controle
/// de vinheta não faz.
pub fn vignette(pixels: &mut [Rgb], largura: usize, altura: usize, p: Vignette) {
    let (w, h) = (largura as f32, altura as f32);
    let centro = [
        w * 0.5 + p.center[0] * w / 2.0,
        h * 0.5 + p.center[1] * h / 2.0,
    ];
    let (xscale, yscale) = if p.autoratio {
        (2.0 / w, 2.0 / h)
    } else {
        let base = 2.0 / w.max(h);
        if p.whratio <= 1.0 {
            (base / p.whratio, base)
        } else {
            (base, base / (2.0 - p.whratio))
        }
    };
    let dscale = p.scale / 100.0;
    let min_falloff = 100.0 / w.min(h);
    let fscale = p.falloff_scale.max(min_falloff) / 100.0;
    let shape = p.shape.max(0.001);
    let (exp1, exp2) = (2.0 / shape, shape / 2.0);
    let centro_escalado = [centro[0] * xscale, centro[1] * yscale];

    for j in 0..altura {
        for i in 0..largura {
            let pv = [
                (i as f32 * xscale - centro_escalado[0]).abs(),
                (j as f32 * yscale - centro_escalado[1]).abs(),
            ];
            let cplen = (pv[0].powf(exp1) + pv[1].powf(exp1)).powf(exp2);
            let mut peso = 0.0;
            if cplen >= dscale {
                peso = ((cplen - dscale) / fscale).clamp(0.0, 1.0);
            }
            if peso <= 0.0 {
                continue;
            }
            let col = &mut pixels[j * largura + i];
            if p.brightness < 0.0 {
                let f = 1.0 + peso * p.brightness;
                for c in col.iter_mut() {
                    *c *= f;
                }
            } else {
                let f = peso * p.brightness;
                for c in col.iter_mut() {
                    *c += f;
                }
            }
            if !p.unbound {
                for c in col.iter_mut() {
                    *c = c.clamp(0.0, 1.0);
                }
            }
            let mv = (col[0] + col[1] + col[2]) / 3.0;
            let wss = peso * p.saturation;
            for c in col.iter_mut() {
                *c -= (mv - *c) * wss;
                if !p.unbound {
                    *c = c.clamp(0.0, 1.0);
                }
            }
        }
    }
}

// ------------------------------------------------------------ color balance rgb

/// `dt_iop_colorbalancergb_params_t` v5, com a fórmula de saturação dt UCS.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorBalanceRgb {
    pub shadows_y: f32,
    pub shadows_c: f32,
    pub shadows_h: f32,
    pub midtones_y: f32,
    pub midtones_c: f32,
    pub midtones_h: f32,
    pub highlights_y: f32,
    pub highlights_c: f32,
    pub highlights_h: f32,
    pub global_y: f32,
    pub global_c: f32,
    pub global_h: f32,
    pub shadows_weight: f32,
    pub white_fulcrum: f32,
    pub highlights_weight: f32,
    pub chroma_shadows: f32,
    pub chroma_highlights: f32,
    pub chroma_global: f32,
    pub chroma_midtones: f32,
    pub saturation_global: f32,
    pub saturation_highlights: f32,
    pub saturation_midtones: f32,
    pub saturation_shadows: f32,
    pub hue_angle: f32,
    pub brilliance_global: f32,
    pub brilliance_highlights: f32,
    pub brilliance_midtones: f32,
    pub brilliance_shadows: f32,
    pub mask_grey_fulcrum: f32,
    pub vibrance: f32,
    pub grey_fulcrum: f32,
    pub contrast: f32,
}

fn lms_para_yrg(lms: Rgb) -> Rgb {
    let y = 0.68990272 * lms[0] + 0.34832189 * lms[1];
    let a = lms[0] + lms[1] + lms[2];
    let n = if a == 0.0 {
        [0.0; 3]
    } else {
        [lms[0] / a, lms[1] / a, lms[2] / a]
    };
    let rgb = mul(&LMS_PARA_FILMLIGHT, n);
    [y, rgb[0], rgb[1]]
}

fn yrg_para_lms(yrg: Rgb) -> Rgb {
    let rgb = [yrg[1], yrg[2], 1.0 - yrg[1] - yrg[2]];
    let lms = mul(&FILMLIGHT_PARA_LMS, rgb);
    let den = 0.68990272 * lms[0] + 0.34832189 * lms[1];
    let a = if den == 0.0 { 0.0 } else { yrg[0] / den };
    [lms[0] * a, lms[1] * a, lms[2] * a]
}

/// `[Y, c, cos h, sin h]`.
fn yrg_para_ych(yrg: Rgb) -> [f32; 4] {
    let r = yrg[1] - 0.21902143;
    let g = yrg[2] - 0.54371398;
    let c = (g * g + r * r).sqrt();
    let (cos_h, sin_h) = if c != 0.0 { (r / c, g / c) } else { (1.0, 0.0) };
    [yrg[0], c, cos_h, sin_h]
}

fn ych_para_yrg(ych: [f32; 4]) -> Rgb {
    [
        ych[0],
        ych[1] * ych[2] + 0.21902143,
        ych[1] * ych[3] + 0.54371398,
    ]
}

fn ych_para_grading(ych: [f32; 4]) -> Rgb {
    mul(&LMS_PARA_FILMLIGHT, yrg_para_lms(ych_para_yrg(ych)))
}

fn gamut_check_yrg(ych: &mut [f32; 4]) {
    let yrg = ych_para_yrg(*ych);
    let (d65_r, d65_g) = (0.21902143f32, 0.54371398f32);
    let mut max_c = ych[1];
    let (cos_h, sin_h) = (ych[2], ych[3]);
    if yrg[1] < 0.0 {
        max_c = (-d65_r / cos_h).min(max_c);
    }
    if yrg[2] < 0.0 {
        max_c = (-d65_g / sin_h).min(max_c);
    }
    if yrg[1] + yrg[2] > 1.0 {
        max_c = ((1.0 - d65_r - d65_g) / (cos_h + sin_h)).min(max_c);
    }
    ych[1] = max_c;
}

fn soft_clip(x: f32, soft: f32, hard: f32) -> f32 {
    let norm = hard - soft;
    if x > soft {
        soft + (1.0 - (-(x - soft) / norm).exp()) * norm
    } else {
        x
    }
}

fn y_para_l_star(y: f32) -> f32 {
    let y_hat = y.powf(0.631651345306265);
    DT_UCS_L_STAR_RANGE * y_hat / (y_hat + 1.12426773749357)
}

fn l_star_para_y(l: f32) -> f32 {
    (1.12426773749357 * l / (DT_UCS_L_STAR_RANGE - l)).powf(1.5831518565279648)
}

fn xyy_para_uv(xyy: Rgb) -> [f32; 2] {
    let xf = [-0.783941002840055f32, 0.745273540913283, 0.318707282433486];
    let yf = [0.277512987809202f32, -0.205375866083878, 2.16743692732158];
    let of = [0.153836578598858f32, -0.165478376301988, 0.291320554395942];
    let mut uvd = [0.0f32; 3];
    for c in 0..3 {
        uvd[c] = xf[c] * xyy[0] + yf[c] * xyy[1] + of[c];
    }
    let div = if uvd[2] >= 0.0 {
        uvd[2].max(f32::MIN_POSITIVE)
    } else {
        uvd[2].min(-f32::MIN_POSITIVE)
    };
    uvd[0] /= div;
    uvd[1] /= div;
    let factors = [1.39656225667f32, 1.4513954287];
    let half = [1.49217352929f32, 1.52488637914];
    let uv_star = [
        factors[0] * uvd[0] / (uvd[0].abs() + half[0]),
        factors[1] * uvd[1] / (uvd[1].abs() + half[1]),
    ];
    [
        -1.124983854323892 * uv_star[0] - 0.980483721769325 * uv_star[1],
        1.86323315098672 * uv_star[0] + 1.971853092390862 * uv_star[1],
    ]
}

fn xyy_para_jch(xyy: Rgb, l_white: f32) -> Rgb {
    let uv = xyy_para_uv(xyy);
    let l_star = y_para_l_star(xyy[2]);
    let m2 = uv[0] * uv[0] + uv[1] * uv[1];
    [
        l_star / l_white,
        15.932993652962535 * l_star.powf(0.6523997524738018) * m2.powf(0.6007557017508491)
            / l_white,
        uv[1].atan2(uv[0]),
    ]
}

fn jch_para_xyy(jch: Rgb, l_white: f32) -> Rgb {
    let l_star = (jch[0] * l_white).clamp(0.0, DT_UCS_L_STAR_UPPER_LIMIT);
    let m = if l_star != 0.0 {
        (jch[1] * l_white / (15.932993652962535 * l_star.powf(0.6523997524738018)))
            .powf(0.8322850678616855)
    } else {
        0.0
    };
    let (u, v) = (m * jch[2].cos(), m * jch[2].sin());
    let uv_star = [
        -5.037522385190711 * u - 2.504856328185843 * v,
        4.760029407436461 * u + 2.874012963239247 * v,
    ];
    let factors = [1.39656225667f32, 1.4513954287];
    let half = [1.49217352929f32, 1.52488637914];
    let uvx = [
        -half[0] * uv_star[0] / (uv_star[0].abs() - factors[0]),
        -half[1] * uv_star[1] / (uv_star[1].abs() - factors[1]),
    ];
    let uf = [0.167171472114775f32, -0.150959086409163, 0.940254742367256];
    let vf = [0.141299802443708f32, -0.155185060382272, 1.0];
    let of = [
        -0.00801531300850582f32,
        -0.00843312433578007,
        -0.0256325967652889,
    ];
    let mut xyd = [0.0f32; 3];
    for c in 0..3 {
        xyd[c] = uf[c] * uvx[0] + vf[c] * uvx[1] + of[c];
    }
    let div = if xyd[2] >= 0.0 {
        xyd[2].max(f32::MIN_POSITIVE)
    } else {
        xyd[2].min(-f32::MIN_POSITIVE)
    };
    [xyd[0] / div, xyd[1] / div, l_star_para_y(l_star)]
}

fn xyz_para_xyy(xyz: Rgb) -> Rgb {
    let xyz = [xyz[0].max(0.0), xyz[1].max(0.0), xyz[2].max(0.0)];
    let soma = xyz[0] + xyz[1] + xyz[2];
    if soma > 0.0 {
        [xyz[0] / soma, xyz[1] / soma, xyz[1]]
    } else {
        [D65_XY[0] as f32, D65_XY[1] as f32, xyz[1]]
    }
}

fn xyy_para_xyz(xyy: Rgb) -> Rgb {
    if xyy[1] == 0.0 {
        return [0.0; 3];
    }
    [
        xyy[2] * xyy[0] / xyy[1],
        xyy[2],
        xyy[2] * (1.0 - xyy[0] - xyy[1]) / xyy[1],
    ]
}

fn delta_h(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    if d < -PI {
        d += 2.0 * PI;
    }
    if d > PI {
        d -= 2.0 * PI;
    }
    d
}

/// `dt_UCS_22_build_gamut_LUT` (`src/common/darktable_ucs_22_helpers.h:18–113`).
fn tabela_de_gamut(rgb_para_xyz_d65: &Matriz) -> Vec<f32> {
    let mut lut = vec![0.0f32; LUT_ELEM];
    let mut amostras = vec![0.0f32; LUT_ELEM];
    let d65 = [D65_XY[0] as f32, D65_XY[1] as f32];
    let prim = |col: usize| {
        xyz_para_xyy([
            rgb_para_xyz_d65[0][col],
            rgb_para_xyz_d65[1][col],
            rgb_para_xyz_d65[2][col],
        ])
    };
    let (vermelho, verde, azul) = (prim(0), prim(1), prim(2));
    let ang = |p: Rgb| (p[1] - d65[1]).atan2(p[0] - d65[0]);
    let (h_r, h_g, h_b) = (ang(vermelho), ang(verde), ang(azul));
    let n = 50 * LUT_ELEM;
    for i in 0..n {
        let angulo = -PI + i as f32 / n as f32 * 2.0 * PI;
        let tan = angulo.tan();
        let t1 = delta_h(angulo, h_b) / delta_h(h_r, h_b);
        let t2 = delta_h(angulo, h_r) / delta_h(h_g, h_r);
        let t3 = delta_h(angulo, h_g) / delta_h(h_b, h_g);
        let (mut xt, mut yt) = (0.0f32, 0.0f32);
        let dentro = |t: f32| t == t.clamp(0.0, 1.0);
        let segmento = |a: Rgb, b: Rgb| {
            let t = (d65[1] - a[1] + tan * (a[0] - d65[0])) / (b[1] - a[1] + tan * (a[0] - b[0]));
            (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]))
        };
        if dentro(t1) {
            (xt, yt) = segmento(azul, vermelho);
        } else if dentro(t2) {
            (xt, yt) = segmento(vermelho, verde);
        } else if dentro(t3) {
            (xt, yt) = segmento(verde, azul);
        }
        let uv = xyy_para_uv([xt, yt, 1.0]);
        let hue = uv[1].atan2(uv[0]);
        let mut idx = ((LUT_ELEM - 1) as f32 * (hue + PI) / (2.0 * PI)).round() as i32;
        if idx < 0 {
            idx += LUT_ELEM as i32;
        }
        if idx >= LUT_ELEM as i32 {
            idx -= LUT_ELEM as i32;
        }
        lut[idx as usize] += uv[0] * uv[0] + uv[1] * uv[1];
        amostras[idx as usize] += 1.0;
    }
    for k in 0..LUT_ELEM {
        lut[k] /= amostras[k].max(1.0);
    }
    lut
}

fn consultar_gamut(lut: &[f32], hue: f32) -> f32 {
    let x = LUT_ELEM as f32 * (hue + PI) / (2.0 * PI);
    let (xp, xn) = (x.floor(), x.ceil());
    let xi = (xp as i32 & (LUT_ELEM as i32 - 1)) as usize;
    let xii = (xn as i32 & (LUT_ELEM as i32 - 1)) as usize;
    let yp = lut[xi];
    yp + if xi != xii {
        (x - xp) * (lut[xii] - yp)
    } else {
        0.0
    }
}

/// `src/iop/colorbalancergb.c:551–577`: sombras, meios-tons, altas luzes.
fn mascaras(x: f32, sw: f32, hw: f32, mw: f32, fulcro: f32) -> ([f32; 3], [f32; 3]) {
    let off = x - fulcro;
    let norm = off / fulcro;
    let alpha = 1.0 / (1.0 + (norm * sw).exp());
    let beta = 1.0 / (1.0 + (-norm * hw).exp());
    let (ac, bc) = (1.0 - alpha, 1.0 - beta);
    let gamma = (-(off * off) * mw / 4.0).exp() * ac * ac * bc * bc * 8.0;
    ([alpha, gamma, beta], [ac, 1.0 - gamma, bc])
}

/// `src/iop/colorbalancergb.c` — `commit_params` (1087–1168) e `process`
/// (579–901), no ramo de saturação **dt UCS** (`saturation_formula = 1`).
pub fn color_balance_rgb(pixels: &mut [Rgb], tub: &Tubulacao, p: &ColorBalanceRgb) {
    // commit_params
    let ych_norm = [1.0f32, 0.0, 1.0, 0.0];
    let rgb_norm = ych_para_grading(ych_norm);
    let faixa = |c: f32, h_graus: f32| {
        let h = (h_graus + ANGLE_SHIFT).to_radians();
        ych_para_grading([1.0, c, h.cos(), h.sin()])
    };
    let g = faixa(p.global_c, p.global_h);
    let global = [0, 1, 2].map(|c| (g[c] - rgb_norm[c]) + rgb_norm[c] * p.global_y);
    let s = faixa(p.shadows_c, p.shadows_h);
    let shadows = [0, 1, 2].map(|c| 1.0 + (s[c] - rgb_norm[c]) + p.shadows_y);
    let hi = faixa(p.highlights_c, p.highlights_h);
    let highlights = [0, 1, 2].map(|c| 1.0 + (hi[c] - rgb_norm[c]) + p.highlights_y);
    let m = faixa(p.midtones_c, p.midtones_h);
    let midtones = [0, 1, 2].map(|c| 1.0 / (1.0 + (m[c] - rgb_norm[c])));
    let midtones_y = 1.0 / (1.0 + p.midtones_y);
    let sw = 2.0 + p.shadows_weight * 2.0;
    let hw = 2.0 + p.highlights_weight * 2.0;
    let mw = sw * sw * hw * hw / (sw * sw + hw * hw);
    let white_fulcrum = 2f32.powf(p.white_fulcrum);
    let mask_fulcro = p.mask_grey_fulcrum.powf(0.4101205819200422);
    let contrast = 1.0 + p.contrast;
    let hue = p.hue_angle.to_radians();
    let chroma = [p.chroma_shadows, p.chroma_midtones, p.chroma_highlights];
    let saturation = [
        p.saturation_shadows,
        p.saturation_midtones,
        p.saturation_highlights,
    ];
    let brilliance = [
        p.brilliance_shadows,
        p.brilliance_midtones,
        p.brilliance_highlights,
    ];

    // As matrizes: trabalho → XYZ D65 (CAT16) → LMS, e a volta (`process`, 612–629).
    let para_xyz_d65 = mul_mat(&XYZ_D50_PARA_D65_CAT16, &tub.trabalho_para_xyz_d50);
    let entrada = mul_mat(&XYZ_D65_PARA_LMS_2006, &para_xyz_d65);
    let saida = mul_mat(&tub.xyz_d50_para_trabalho, &XYZ_D65_PARA_D50_CAT16);
    let lut = tabela_de_gamut(&para_xyz_d65);
    let l_white = y_para_l_star(white_fulcrum);
    let (cos_hue, sin_hue) = (hue.cos(), hue.sin());

    for px in pixels.iter_mut() {
        let rgb = [px[0].max(0.0), px[1].max(0.0), px[2].max(0.0)];
        let lms = mul(&entrada, rgb);
        let mut ych = yrg_para_ych(lms_para_yrg(lms));
        ych[0] = ych[0].max(0.0);

        let (op, opc) = mascaras(ych[0].powf(0.4101205819200422), sw, hw, mw, mask_fulcro);

        let (ch, sh) = (ych[2], ych[3]);
        ych[2] = cos_hue * ch - sin_hue * sh;
        ych[3] = sin_hue * ch + cos_hue * sh;

        let boost = p.chroma_global + op[0] * chroma[0] + op[1] * chroma[1] + op[2] * chroma[2];
        let vib = p.vibrance * (1.0 - ych[1].powf(p.vibrance.abs()));
        ych[1] *= (1.0 + boost + vib).max(0.0);
        gamut_check_yrg(&mut ych);

        let mut rgb = mul(&LMS_PARA_FILMLIGHT, yrg_para_lms(ych_para_yrg(ych)));
        for c in 0..3 {
            rgb[c] += global[c];
        }
        for c in 0..3 {
            rgb[c] *= opc[2] * (opc[0] + op[0] * shadows[c]) + op[2] * highlights[c];
        }
        for c in 0..3 {
            let sinal = if rgb[c] < 0.0 { -1.0 } else { 1.0 };
            rgb[c] = (rgb[c].abs() / white_fulcrum).powf(midtones[c]) * sinal * white_fulcrum;
        }

        let mut yrg = lms_para_yrg(mul(&FILMLIGHT_PARA_LMS, rgb));
        yrg[0] = (yrg[0] / white_fulcrum).max(0.0).powf(midtones_y) * white_fulcrum;
        yrg[0] = p.grey_fulcrum * (yrg[0] / p.grey_fulcrum).powf(contrast);

        let xyz_d65 = mul(&LMS_2006_PARA_XYZ_D65, yrg_para_lms(yrg));

        // Ramo dt UCS (852–900).
        let xyy = xyz_para_xyy(xyz_d65);
        let jch = xyy_para_jch(xyy, l_white);
        let mut hcb = [
            jch[2],
            jch[1],
            jch[0] * (jch[1].powf(1.33654221029386) + 1.0),
        ];
        let raio = (hcb[1] * hcb[1] + hcb[2] * hcb[2]).sqrt();
        let (sin_t, cos_t) = if raio > 0.0 {
            (hcb[1] / raio, hcb[2] / raio)
        } else {
            (0.0, 0.0)
        };
        let pp = hcb[1].max(f32::MIN_POSITIVE);
        let ww = sin_t * hcb[1] + cos_t * hcb[2];
        let mut a = (1.0
            + p.saturation_global
            + op[0] * saturation[0]
            + op[1] * saturation[1]
            + op[2] * saturation[2])
            .max(0.0);
        let b = (1.0
            + p.brilliance_global
            + op[0] * brilliance[0]
            + op[1] * brilliance[1]
            + op[2] * brilliance[2])
            .max(0.0);
        let max_a = (pp * pp + ww * ww).sqrt() / pp;
        a = soft_clip(a, 0.5 * max_a, max_a);
        let p_linha = (a - 1.0) * pp;
        let w_linha = (pp * pp * (1.0 - a * a) + ww * ww).sqrt() * b;
        hcb[1] = (cos_t * p_linha + sin_t * w_linha).max(0.0);
        hcb[2] = (-sin_t * p_linha + cos_t * w_linha).max(0.0);
        let mut jch = [
            hcb[2] / (hcb[1].powf(1.33654221029386) + 1.0),
            hcb[1],
            hcb[0],
        ];

        let max_m2 = consultar_gamut(&lut, jch[2]);
        let max_chroma = 15.932993652962535
            * (jch[0] * l_white).powf(0.6523997524738018)
            * max_m2.powf(0.6007557017508491)
            / l_white;
        let borda = {
            let b = jch[0] * (max_chroma.powf(1.33654221029386) + 1.0);
            let s = if b > 0.0 { max_chroma / b } else { 0.0 };
            [jch[2], s, b]
        };
        let mut hsb = [
            hcb[0],
            if hcb[2] > 0.0 { hcb[1] / hcb[2] } else { 0.0 },
            hcb[2],
        ];
        hsb[1] = soft_clip(hsb[1], 0.8 * borda[1], borda[1]);
        jch[2] = hsb[0];
        jch[1] = hsb[1] * hsb[2];
        jch[0] = hsb[2] / (jch[1].powf(1.33654221029386) + 1.0);
        let xyz_d65 = xyy_para_xyz(jch_para_xyy(jch, l_white));

        let out = mul(&saida, xyz_d65);
        *px = [out[0].max(0.0), out[1].max(0.0), out[2].max(0.0)];
    }
}

// ------------------------------------------------------- o que o shader recebe

impl Exposure {
    /// Os campos `dt_exposure_*` do [`Ajustes`].
    pub fn de(a: &Ajustes) -> Self {
        Self {
            black: a.dt_exposure_black,
            exposure: a.dt_exposure_exposure,
        }
    }
}

impl Vignette {
    /// Os campos `dt_vignette_*` do [`Ajustes`].
    pub fn de(a: &Ajustes) -> Self {
        Self {
            scale: a.dt_vignette_scale,
            falloff_scale: a.dt_vignette_falloff_scale,
            brightness: a.dt_vignette_brightness,
            saturation: a.dt_vignette_saturation,
            center: [a.dt_vignette_center_x, a.dt_vignette_center_y],
            autoratio: a.dt_vignette_autoratio != 0.0,
            whratio: a.dt_vignette_whratio,
            shape: a.dt_vignette_shape,
            unbound: a.dt_vignette_unbound != 0.0,
        }
    }
}

impl ColorBalanceRgb {
    /// Os campos `dt_cb_*` do [`Ajustes`].
    pub fn de(a: &Ajustes) -> Self {
        Self {
            shadows_y: a.dt_cb_shadows_y,
            shadows_c: a.dt_cb_shadows_c,
            shadows_h: a.dt_cb_shadows_h,
            midtones_y: a.dt_cb_midtones_y,
            midtones_c: a.dt_cb_midtones_c,
            midtones_h: a.dt_cb_midtones_h,
            highlights_y: a.dt_cb_highlights_y,
            highlights_c: a.dt_cb_highlights_c,
            highlights_h: a.dt_cb_highlights_h,
            global_y: a.dt_cb_global_y,
            global_c: a.dt_cb_global_c,
            global_h: a.dt_cb_global_h,
            shadows_weight: a.dt_cb_shadows_weight,
            white_fulcrum: a.dt_cb_white_fulcrum,
            highlights_weight: a.dt_cb_highlights_weight,
            chroma_shadows: a.dt_cb_chroma_shadows,
            chroma_highlights: a.dt_cb_chroma_highlights,
            chroma_global: a.dt_cb_chroma_global,
            chroma_midtones: a.dt_cb_chroma_midtones,
            saturation_global: a.dt_cb_saturation_global,
            saturation_highlights: a.dt_cb_saturation_highlights,
            saturation_midtones: a.dt_cb_saturation_midtones,
            saturation_shadows: a.dt_cb_saturation_shadows,
            hue_angle: a.dt_cb_hue_angle,
            brilliance_global: a.dt_cb_brilliance_global,
            brilliance_highlights: a.dt_cb_brilliance_highlights,
            brilliance_midtones: a.dt_cb_brilliance_midtones,
            brilliance_shadows: a.dt_cb_brilliance_shadows,
            mask_grey_fulcrum: a.dt_cb_mask_grey_fulcrum,
            vibrance: a.dt_cb_vibrance,
            grey_fulcrum: a.dt_cb_grey_fulcrum,
            contrast: a.dt_cb_contrast,
        }
    }
}

/// O texto de `shaders/darktable_constantes.wgsl`: as matrizes da tubulação e a
/// tabela de gamut do Rec.2020, calculadas aqui e escritas como literais.
///
/// # Por que gerado, e não escrito à mão
///
/// 🚨 **São 512 números da tabela de gamut e 63 das matrizes**, e todos saem de
/// contas que já foram medidas contra o darktable (Bradford do lcms, CAT16,
/// dt UCS). Copiá-los à mão para o WGSL seria a forma mais certa de o shader
/// divergir do gabarito por um dígito — e ninguém acharia. O teste
/// `as_constantes_do_wgsl_estao_em_dia` falha se o arquivo não for exatamente
/// o que esta função devolve.
pub fn constantes_wgsl() -> String {
    let tub = Tubulacao::nova();
    let mut s = String::from(
        "// Gerado por `cargo run -q -p revelacao-core --example darktable-constantes-wgsl` — não editar.\n\
         // As matrizes e a tabela de gamut do estágio darktable, calculadas em `src/darktable.rs`.\n\n",
    );
    let matriz = |nome: &str, m: &Matriz| {
        let linha = |l: [f32; 3]| format!("    vec3<f32>({:e}, {:e}, {:e}),\n", l[0], l[1], l[2]);
        format!(
            "const {nome}: array<vec3<f32>, 3> = array<vec3<f32>, 3>(\n{}{}{});\n",
            linha(m[0]),
            linha(m[1]),
            linha(m[2])
        )
    };
    let para_xyz_d65 = mul_mat(&XYZ_D50_PARA_D65_CAT16, &tub.trabalho_para_xyz_d50);
    s += &matriz("DT_SRGB_PARA_TRABALHO", &tub.srgb_para_trabalho);
    s += &matriz("DT_TRABALHO_PARA_SRGB", &tub.trabalho_para_srgb);
    s += &matriz(
        "DT_CB_ENTRADA",
        &mul_mat(&XYZ_D65_PARA_LMS_2006, &para_xyz_d65),
    );
    s += &matriz(
        "DT_CB_SAIDA",
        &mul_mat(&tub.xyz_d50_para_trabalho, &XYZ_D65_PARA_D50_CAT16),
    );
    s += &matriz("DT_TRABALHO_PARA_XYZ_D50", &tub.trabalho_para_xyz_d50);
    s += &matriz("DT_XYZ_D50_PARA_TRABALHO", &tub.xyz_d50_para_trabalho);
    s += &matriz("DT_LMS_PARA_FILMLIGHT", &LMS_PARA_FILMLIGHT);
    s += &matriz("DT_FILMLIGHT_PARA_LMS", &FILMLIGHT_PARA_LMS);
    s += &matriz("DT_LMS_2006_PARA_XYZ_D65", &LMS_2006_PARA_XYZ_D65);
    let lut = tabela_de_gamut(&para_xyz_d65);
    s += "\n// `dt_UCS_22_build_gamut_LUT` sobre o Rec.2020 linear: M² da borda, por matiz.\n";
    s += &format!("var<private> DT_GAMUT: array<f32, {LUT_ELEM}> = array<f32, {LUT_ELEM}>(\n");
    for bloco in lut.chunks(6) {
        s += "    ";
        s += &bloco
            .iter()
            .map(|v| format!("{v:e},"))
            .collect::<Vec<_>>()
            .join(" ");
        s += "\n";
    }
    s += ");\n";
    s
}

// ------------------------------------------------------------------ bilateral

/// A grade bilateral do darktable (`src/common/bilateral.c`).
///
/// # Por que não é um borrão gaussiano qualquer
///
/// 🚨 **O resultado dela é o que o `shadhi` e o `monochrome` enxergam como
/// "vizinhança"**, e o formato importa: a grade é amostrada a cada `σs` pixels
/// e `σr` níveis de L, borrada com `[1 4 6 4 1]/16` em x e y, e no eixo de L com
/// a **derivada** (`blur_line_z`) — o fatiamento soma essa derivada ao L do
/// pixel com peso `σr · 0,04`, que é o truque do darktable para devolver a base
/// sem dividir pelo peso. Um gaussiano "equivalente" daria outra base, e a base
/// é o que decide onde é sombra.
///
/// Uma thread só: o darktable fatia a grade por thread e soma no fim
/// (`bilateral.c:266–290`), e a soma é a mesma.
pub struct Bilateral {
    size_x: usize,
    size_y: usize,
    size_z: usize,
    sigma_s: f32,
    sigma_r: f32,
    largura: usize,
    altura: usize,
    buf: Vec<f32>,
}

impl Bilateral {
    /// `dt_bilateral_grid_size` (`bilateral.c:41–82`), com `L_range = 100`.
    pub fn nova(largura: usize, altura: usize, sigma_s: f32, sigma_r: f32) -> Self {
        let sigma_s = sigma_s.max(0.5);
        let (w, h) = (largura as f32, altura as f32);
        let gx = ((w / sigma_s).round() as i32).clamp(4, 3000) as f32;
        let gy = ((h / sigma_s).round() as i32).clamp(4, 3000) as f32;
        let gz = ((100.0 / sigma_r).round() as i32).clamp(4, 50) as f32;
        let sigma_s = (h / gy).max(w / gx);
        let sigma_r = 100.0 / gz;
        let size_x = (w * (1.0 / sigma_s)).ceil() as usize + 1;
        let size_y = (h * (1.0 / sigma_s)).ceil() as usize + 1;
        let size_z = (100.0 * (1.0 / sigma_r)).ceil() as usize + 1;
        Self {
            size_x,
            size_y,
            size_z,
            sigma_s,
            sigma_r,
            largura,
            altura,
            buf: vec![0.0; size_x * size_y * size_z],
        }
    }

    /// `dt_bilateral_splat` (`bilateral.c:202–291`), a partir do canal L.
    pub fn espalhar(&mut self, l: &[f32]) {
        let (ox, oy, oz) = (self.size_z, self.size_x * self.size_z, 1usize);
        let s_inv = 1.0 / self.sigma_s;
        let r_inv = 1.0 / self.sigma_r;
        let norma = 100.0 / (self.sigma_s * self.sigma_s);
        let offsets = [0, ox, oy, ox + oy, oz, oz + ox, oz + oy, oz + oy + ox];
        for j in 0..self.altura {
            let y = (j as f32 * s_inv).clamp(0.0, (self.size_y - 1) as f32);
            let yi = (y as usize).min(self.size_y - 2);
            let yf = y - yi as f32;
            let base = yi * oy;
            for i in 0..self.largura {
                let lv = l[j * self.largura + i];
                let x = (i as f32 * s_inv).clamp(0.0, (self.size_x - 1) as f32);
                let z = (lv * r_inv).clamp(0.0, (self.size_z - 1) as f32);
                let xi = (x as usize).min(self.size_x - 2);
                let zi = (z as usize).min(self.size_z - 2);
                let (xf, zf) = (x - xi as f32, z - zi as f32);
                let gi = base + xi * self.size_z + zi;
                let contrib = [
                    (1.0 - xf) * (1.0 - yf) * norma,
                    xf * (1.0 - yf) * norma,
                    (1.0 - xf) * yf * norma,
                    xf * yf * norma,
                ];
                for k in 0..4 {
                    self.buf[gi + offsets[k]] += contrib[k] * (1.0 - zf);
                    self.buf[gi + offsets[k + 4]] += contrib[k] * zf;
                }
            }
        }
    }

    /// `blur_line` (`bilateral.c:335–379`): `[1 4 6 4 1]/16`, em lugar, com borda zero.
    fn borrar_linha(
        buf: &mut [f32],
        o1: usize,
        o2: usize,
        o3: usize,
        s1: usize,
        s2: usize,
        s3: usize,
    ) {
        let (w0, w1, w2) = (6.0f32 / 16.0, 4.0f32 / 16.0, 1.0f32 / 16.0);
        for k in 0..s1 {
            let mut idx = k * o1;
            for _ in 0..s2 {
                let mut tmp1 = buf[idx];
                buf[idx] = buf[idx] * w0 + w1 * buf[idx + o3] + w2 * buf[idx + 2 * o3];
                idx += o3;
                let mut tmp2 = buf[idx];
                buf[idx] = buf[idx] * w0 + w1 * (buf[idx + o3] + tmp1) + w2 * buf[idx + 2 * o3];
                idx += o3;
                for _ in 2..s3 - 2 {
                    let tmp3 = buf[idx];
                    buf[idx] = buf[idx] * w0
                        + w1 * (buf[idx + o3] + tmp2)
                        + w2 * (buf[idx + 2 * o3] + tmp1);
                    idx += o3;
                    tmp1 = tmp2;
                    tmp2 = tmp3;
                }
                let tmp3 = buf[idx];
                buf[idx] = buf[idx] * w0 + w1 * (buf[idx + o3] + tmp2) + w2 * tmp1;
                idx += o3;
                buf[idx] = buf[idx] * w0 + w1 * tmp3 + w2 * tmp2;
                idx += o3;
                idx = idx + o2 - o3 * s3;
            }
        }
    }

    /// `blur_line_z` (`bilateral.c:293–333`): a derivada, com borda zero.
    fn borrar_linha_z(
        buf: &mut [f32],
        o1: usize,
        o2: usize,
        o3: usize,
        s1: usize,
        s2: usize,
        s3: usize,
    ) {
        let (w1, w2) = (4.0f32 / 16.0, 2.0f32 / 16.0);
        for k in 0..s1 {
            let mut idx = k * o1;
            for _ in 0..s2 {
                let mut tmp1 = buf[idx];
                buf[idx] = w1 * buf[idx + o3] + w2 * buf[idx + 2 * o3];
                idx += o3;
                let mut tmp2 = buf[idx];
                buf[idx] = w1 * (buf[idx + o3] - tmp1) + w2 * buf[idx + 2 * o3];
                idx += o3;
                for _ in 2..s3 - 2 {
                    let tmp3 = buf[idx];
                    buf[idx] = w1 * (buf[idx + o3] - tmp2) + w2 * (buf[idx + 2 * o3] - tmp1);
                    idx += o3;
                    tmp1 = tmp2;
                    tmp2 = tmp3;
                }
                let tmp3 = buf[idx];
                buf[idx] = w1 * (buf[idx + o3] - tmp2) - w2 * tmp1;
                idx += o3;
                buf[idx] = -w1 * tmp3 - w2 * tmp2;
                idx += o3;
                idx = idx + o2 - o3 * s3;
            }
        }
    }

    /// `dt_bilateral_blur` (`bilateral.c:381–396`).
    pub fn borrar(&mut self) {
        let (ox, oy, oz) = (self.size_z, self.size_x * self.size_z, 1usize);
        let (sx, sy, sz) = (self.size_x, self.size_y, self.size_z);
        Self::borrar_linha(&mut self.buf, oz, oy, ox, sz, sy, sx);
        Self::borrar_linha(&mut self.buf, oz, ox, oy, sz, sx, sy);
        Self::borrar_linha_z(&mut self.buf, ox, oy, oz, sx, sy, sz);
    }

    /// `dt_bilateral_slice` (`bilateral.c:398–439`): devolve o L de cada pixel.
    pub fn fatiar(&self, l: &[f32], detalhe: f32) -> Vec<f32> {
        let norm = -detalhe * self.sigma_r * 0.04;
        let (ox, oy, oz) = (self.size_z, self.size_x * self.size_z, 1usize);
        let s_inv = 1.0 / self.sigma_s;
        let r_inv = 1.0 / self.sigma_r;
        let mut saida = vec![0.0f32; l.len()];
        for j in 0..self.altura {
            for i in 0..self.largura {
                let lv = l[j * self.largura + i];
                let x = (i as f32 * s_inv).clamp(0.0, (self.size_x - 1) as f32);
                let y = (j as f32 * s_inv).clamp(0.0, (self.size_y - 1) as f32);
                let z = (lv * r_inv).clamp(0.0, (self.size_z - 1) as f32);
                let xi = (x as usize).min(self.size_x - 2);
                let yi = (y as usize).min(self.size_y - 2);
                let zi = (z as usize).min(self.size_z - 2);
                let (xf, yf, zf) = (x - xi as f32, y - yi as f32, z - zi as f32);
                let gi = (xi + yi * self.size_x) * self.size_z + zi;
                let b = &self.buf;
                let soma = b[gi] * (1.0 - xf) * (1.0 - yf) * (1.0 - zf)
                    + b[gi + ox] * xf * (1.0 - yf) * (1.0 - zf)
                    + b[gi + oy] * (1.0 - xf) * yf * (1.0 - zf)
                    + b[gi + ox + oy] * xf * yf * (1.0 - zf)
                    + b[gi + oz] * (1.0 - xf) * (1.0 - yf) * zf
                    + b[gi + ox + oz] * xf * (1.0 - yf) * zf
                    + b[gi + oy + oz] * (1.0 - xf) * yf * zf
                    + b[gi + ox + oy + oz] * xf * yf * zf;
                saida[j * self.largura + i] = (lv + norm * soma).max(0.0);
            }
        }
        saida
    }
}

// ----------------------------------------------------- shadows and highlights

const UNBOUND_SHADOWS_L: u32 = 1;
const UNBOUND_SHADOWS_A: u32 = 2;
const UNBOUND_SHADOWS_B: u32 = 4;
const UNBOUND_HIGHLIGHTS_L: u32 = 8;
const UNBOUND_HIGHLIGHTS_A: u32 = 16;
const UNBOUND_HIGHLIGHTS_B: u32 = 32;
/// `shadhi.c:53`, e o próprio darktable anota "not implemented yet".
const UNBOUND_BILATERAL: u32 = 128;

/// `dt_iop_shadhi_params_t` v5, no algoritmo bilateral (o do estilo).
#[derive(Debug, Clone, Copy)]
pub struct ShadowsHighlights {
    pub radius: f32,
    pub shadows: f32,
    pub whitepoint: f32,
    pub highlights: f32,
    pub compress: f32,
    pub shadows_ccorrect: f32,
    pub highlights_ccorrect: f32,
    pub flags: u32,
    pub low_approximation: f32,
}

/// `shadhi.c:330`: zero conta como positivo.
fn sinal(x: f32) -> f32 {
    if x < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// `src/iop/shadhi.c:336–491`, algoritmo **bilateral**.
///
/// # O que ele faz, e por que não é a nossa curva de sombras
///
/// 🔑 **A decisão de "isto é sombra" é da vizinhança, e não do pixel.** A base
/// é o L borrado pelo bilateral (raio `radius`, σr = 100), **invertida**; a
/// sombra clareia por uma sobreposição (overlay) com essa base, em até duas
/// passadas, e só onde a base é escura o bastante para passar do `compress`. Um
/// botão preto num casaco claro não é "sombra" aqui; na nossa curva global, é.
///
/// `escala` é `roi_in->scale / piece->iscale`: 1 na exportação em tamanho
/// cheio. Na cópia de trabalho do navegador ela é a razão entre a cópia e o
/// original — senão o raio de 100 px viraria, numa foto de 1400 px, um raio
/// de 430 px da foto original.
///
/// ⚠️ **As flags são lidas como o darktable as lê**, inclusive o `la` da
/// passada de sombras usando `UNBOUND_HIGHLIGHTS_L` (`shadhi.c:462`). Parece
/// descuido lá; aqui é cópia fiel, porque o objetivo é o mesmo pixel.
pub fn shadhi(
    pixels: &mut [Rgb],
    largura: usize,
    altura: usize,
    tub: &Tubulacao,
    p: ShadowsHighlights,
    escala: f32,
) {
    let lab: Vec<Rgb> = pixels.iter().map(|px| tub.para_lab(*px)).collect();
    let l: Vec<f32> = lab.iter().map(|x| x[0]).collect();
    let mut grade = Bilateral::nova(largura, altura, p.radius.max(0.1) * escala, 100.0);
    grade.espalhar(&l);
    grade.borrar();
    let base = grade.fatiar(&l, -1.0);
    for (px, saida) in pixels.iter_mut().zip(shadhi_em_lab(&lab, &base, p)) {
        *px = tub.de_lab(saida);
    }
}

/// O miolo do [`shadhi`]: do Lab e da base borrada ao Lab de saída.
///
/// Separado para [`grades_do_estagio`] reaproveitar o Lab e a grade que já
/// calculou, e tirar o filtro do `monochrome` direto do Lab que sai daqui — sem
/// a ida ao RGB e a volta ao Lab, que custavam mais que a conta inteira.
fn shadhi_em_lab(lab: &[Rgb], base: &[f32], p: ShadowsHighlights) -> Vec<Rgb> {
    let shadows = 2.0 * (p.shadows / 100.0).clamp(-1.0, 1.0);
    let highlights = 2.0 * (p.highlights / 100.0).clamp(-1.0, 1.0);
    let whitepoint = (1.0 - p.whitepoint / 100.0).max(0.01);
    let compress = (p.compress / 100.0).clamp(0.0, 0.99);
    let shadows_ccorrect =
        ((p.shadows_ccorrect / 100.0).clamp(0.0, 1.0) - 0.5) * sinal(shadows) + 0.5;
    let highlights_ccorrect =
        ((p.highlights_ccorrect / 100.0).clamp(0.0, 1.0) - 0.5) * sinal(-highlights) + 0.5;
    let flags = p.flags;
    let unbound_mask = flags & UNBOUND_BILATERAL != 0;
    let low = p.low_approximation;

    let refs = |la: f32| {
        let lref = (if la.abs() > low {
            1.0 / la.abs()
        } else {
            1.0 / low
        })
        .copysign(la);
        let href = (if (1.0 - la).abs() > low {
            1.0 / (1.0 - la).abs()
        } else {
            1.0 / low
        })
        .copysign(1.0 - la);
        (lref, href)
    };
    let overlay = |la: f32, lb: f32| {
        if la > 0.5 {
            1.0 - (1.0 - 2.0 * (la - 0.5)) * (1.0 - lb)
        } else {
            2.0 * la * lb
        }
    };

    lab.iter()
        .zip(base)
        .map(|(c, b)| {
            let mut ta = [c[0] / 100.0, c[1] / 128.0, c[2] / 128.0];
            let mut tb = [(100.0 - b) / 100.0, 0.0f32, 0.0f32];
            if ta[0] > 0.0 {
                ta[0] /= whitepoint;
            }
            if tb[0] > 0.0 {
                tb[0] /= whitepoint;
            }

            // Altas luzes (shadhi.c:424–454).
            let mut h2 = highlights * highlights;
            let hx = (1.0 - tb[0] / (1.0 - compress)).clamp(0.0, 1.0);
            while h2 > 0.0 {
                let la = if flags & UNBOUND_HIGHLIGHTS_L != 0 {
                    ta[0]
                } else {
                    ta[0].clamp(0.0, 1.0)
                };
                let mut lb = (tb[0] - 0.5) * sinal(-highlights) * sinal(1.0 - la) + 0.5;
                if !unbound_mask {
                    lb = lb.clamp(0.0, 1.0);
                }
                let (lref, href) = refs(la);
                let op = h2.min(1.0) * hx;
                h2 -= 1.0;
                ta[0] = la * (1.0 - op) + overlay(la, lb) * op;
                if flags & UNBOUND_HIGHLIGHTS_L == 0 {
                    ta[0] = ta[0].clamp(0.0, 1.0);
                }
                let cf = ta[0] * lref * (1.0 - highlights_ccorrect)
                    + (1.0 - ta[0]) * href * highlights_ccorrect;
                ta[1] = ta[1] * (1.0 - op) + (ta[1] + tb[1]) * cf * op;
                if flags & UNBOUND_HIGHLIGHTS_A == 0 {
                    ta[1] = ta[1].clamp(-1.0, 1.0);
                }
                ta[2] = ta[2] * (1.0 - op) + (ta[2] + tb[2]) * cf * op;
                if flags & UNBOUND_HIGHLIGHTS_B == 0 {
                    ta[2] = ta[2].clamp(-1.0, 1.0);
                }
            }

            // Sombras (shadhi.c:456–487).
            let mut s2 = shadows * shadows;
            let sx = (tb[0] / (1.0 - compress) - compress / (1.0 - compress)).clamp(0.0, 1.0);
            while s2 > 0.0 {
                let la = if flags & UNBOUND_HIGHLIGHTS_L != 0 {
                    ta[0]
                } else {
                    ta[0].clamp(0.0, 1.0)
                };
                let mut lb = (tb[0] - 0.5) * sinal(shadows) * sinal(1.0 - la) + 0.5;
                if !unbound_mask {
                    lb = lb.clamp(0.0, 1.0);
                }
                let (lref, href) = refs(la);
                let op = s2.min(1.0) * sx;
                s2 -= 1.0;
                ta[0] = la * (1.0 - op) + overlay(la, lb) * op;
                if flags & UNBOUND_SHADOWS_L == 0 {
                    ta[0] = ta[0].clamp(0.0, 1.0);
                }
                let cf = ta[0] * lref * shadows_ccorrect
                    + (1.0 - ta[0]) * href * (1.0 - shadows_ccorrect);
                ta[1] = ta[1] * (1.0 - op) + (ta[1] + tb[1]) * cf * op;
                if flags & UNBOUND_SHADOWS_A == 0 {
                    ta[1] = ta[1].clamp(-1.0, 1.0);
                }
                ta[2] = ta[2] * (1.0 - op) + (ta[2] + tb[2]) * cf * op;
                if flags & UNBOUND_SHADOWS_B == 0 {
                    ta[2] = ta[2].clamp(-1.0, 1.0);
                }
            }

            [ta[0] * 100.0, ta[1] * 128.0, ta[2] * 128.0]
        })
        .collect()
}

// ----------------------------------------------------------------- monochrome

/// `dt_iop_monochrome_params_t` v2.
#[derive(Debug, Clone, Copy)]
pub struct Monochrome {
    pub a: f32,
    pub b: f32,
    pub size: f32,
    pub highlights: f32,
}

/// `dt_fast_expf` (`src/common/math.h:418–431`).
///
/// 🚨 **Não é o `exp`.** É uma interpolação na representação binária do
/// ponto flutuante, com erro de até 0,06 perto de zero — e o `monochrome` a usa
/// no filtro de cor. Trocar pelo `exp` de verdade daria um P&B ligeiramente
/// diferente do que o darktable dá, que é justamente o que não se quer aqui.
fn dt_fast_expf(x: f32) -> f32 {
    let i1: i32 = 0x3f80_0000;
    let i2: i32 = 0x402D_F854;
    let k0 = (i1 as f32 + x * (i2 - i1) as f32) as i32;
    f32::from_bits(k0.max(0) as u32)
}

/// `monochrome.c:176–195`.
fn envelope(l: f32) -> f32 {
    let x = (l / 100.0).clamp(0.0, 1.0);
    let beta = 0.6f32;
    if x < beta {
        let t = x / beta - 1.0;
        1.0 - t * t
    } else {
        let t1 = (1.0 - x) / (1.0 - beta);
        let t2 = t1 * t1;
        3.0 * t2 - 2.0 * t2 * t1
    }
}

/// O peso do filtro de cor de um pixel Lab, em 0–100 (`monochrome.c:167–173, 214`).
fn filtro_monochrome(lab: Rgb, p: Monochrome, sigma2: f32) -> f32 {
    let d = ((lab[1] - p.a) * (lab[1] - p.a) + (lab[2] - p.b) * (lab[2] - p.b)) / sigma2;
    100.0 * dt_fast_expf(-d.clamp(0.0, 1.0))
}

/// `src/iop/monochrome.c:196–249`.
///
/// # O que ele faz
///
/// 🔑 **O P&B aqui é um filtro de cor na frente da lente**, e não uma mistura
/// por canal: cada pixel mede a distância da sua cor (Lab a, b) ao centro do
/// filtro, e fica mais escuro quanto mais longe. Com `a = b = 0` — o do estilo
/// — o filtro é neutro e só escurece de leve o que é muito saturado.
///
/// O peso do filtro é **borrado** (bilateral σs = 20 px, σr = 250) antes de
/// multiplicar o L: sem isso, a textura de cor de um tecido viraria textura de
/// luminância no P&B. `escala_inversa` é `piece->iscale / roi_in->scale`: 1 na
/// exportação cheia.
pub fn monochrome(
    pixels: &mut [Rgb],
    largura: usize,
    altura: usize,
    tub: &Tubulacao,
    p: Monochrome,
    escala_inversa: f32,
) {
    let lab: Vec<Rgb> = pixels.iter().map(|px| tub.para_lab(*px)).collect();
    let sigma2 = 2.0 * (p.size * 128.0) * (p.size * 128.0);
    let filtro: Vec<f32> = lab
        .iter()
        .map(|c| filtro_monochrome(*c, p, sigma2))
        .collect();

    let sigma_s = 20.0 / escala_inversa.max(1.0);
    let mut grade = Bilateral::nova(largura, altura, sigma_s, 250.0);
    grade.espalhar(&filtro);
    grade.borrar();
    let borrado = grade.fatiar(&filtro, -1.0);

    for (k, px) in pixels.iter_mut().enumerate() {
        let l = lab[k][0];
        let tt = envelope(l);
        let t = tt + (1.0 - tt) * (1.0 - p.highlights);
        let saida = (1.0 - t) * l + t * borrado[k] * (1.0 / 100.0) * l;
        *px = tub.de_lab([saida, 0.0, 0.0]);
    }
}

impl ShadowsHighlights {
    /// Os campos `dt_shadhi_*` do [`Ajustes`]. `low_approximation` é constante
    /// no darktable (`$DEFAULT: 0.000001`) e não vira campo.
    pub fn de(a: &Ajustes) -> Self {
        Self {
            radius: a.dt_shadhi_radius,
            shadows: a.dt_shadhi_shadows,
            whitepoint: a.dt_shadhi_whitepoint,
            highlights: a.dt_shadhi_highlights,
            compress: a.dt_shadhi_compress,
            shadows_ccorrect: a.dt_shadhi_shadows_ccorrect,
            highlights_ccorrect: a.dt_shadhi_highlights_ccorrect,
            flags: a.dt_shadhi_flags as u32,
            low_approximation: 1e-6,
        }
    }
}

impl Monochrome {
    /// Os campos `dt_monochrome_*` do [`Ajustes`].
    pub fn de(a: &Ajustes) -> Self {
        Self {
            a: a.dt_monochrome_a,
            b: a.dt_monochrome_b,
            size: a.dt_monochrome_size,
            highlights: a.dt_monochrome_highlights,
        }
    }
}

/// Uma grade bilateral pronta para subir como textura `R32Float`.
///
/// 🔑 **As fatias de L ficam lado a lado**: o texel `(x + z·size_x, y)` é a
/// célula `(x, y, z)`. Uma textura 2D comum, lida com `textureLoad`, funciona
/// igual no WebGPU e no WebGL2 — textura 3D de ponto flutuante tem suporte
/// irregular no segundo.
pub struct GradeParaGpu {
    pub size_x: u32,
    pub size_y: u32,
    pub size_z: u32,
    pub sigma_s: f32,
    pub sigma_r: f32,
    pub dados: Vec<f32>,
}

impl GradeParaGpu {
    /// A grade vazia de 1×1×1, para o bind group ter o que ligar enquanto o
    /// módulo está desligado.
    pub fn vazia() -> Self {
        Self {
            size_x: 1,
            size_y: 1,
            size_z: 1,
            sigma_s: 1.0,
            sigma_r: 1.0,
            dados: vec![0.0],
        }
    }

    fn de(g: &Bilateral) -> Self {
        let (sx, sy, sz) = (g.size_x, g.size_y, g.size_z);
        let mut dados = vec![0.0f32; sx * sz * sy];
        for y in 0..sy {
            for x in 0..sx {
                for z in 0..sz {
                    dados[y * sx * sz + x + z * sx] = g.buf[(x + y * sx) * sz + z];
                }
            }
        }
        Self {
            size_x: sx as u32,
            size_y: sy as u32,
            size_z: sz as u32,
            sigma_s: g.sigma_s,
            sigma_r: g.sigma_r,
            dados,
        }
    }

    pub fn largura_do_atlas(&self) -> u32 {
        self.size_x * self.size_z
    }
}

/// As grades que o estágio darktable precisa para esta foto e estes ajustes.
pub struct GradesDoEstagio {
    pub shadhi: Option<GradeParaGpu>,
    pub monochrome: Option<GradeParaGpu>,
}

/// As grades bilaterais do `shadhi` e do `monochrome`, a partir dos pixels.
///
/// # Por que em CPU
///
/// 🔑 **Espalhar numa grade é juntar muitos pixels em poucas células**, e um
/// shader de fragmento não escreve fora do próprio pixel. A grade é pequena —
/// dezenas a centenas de milhares de células —, e o que custa é o que a
/// alimenta: o L depois da exposição (para o `shadhi`) e o filtro de cor
/// depois do `shadhi` (para o `monochrome`). Os dois saem das funções deste
/// arquivo, que são o gabarito medido contra o darktable.
///
/// Quem chama guarda o resultado: ele só muda quando mudam os pixels, a
/// `escala` ou os parâmetros de `exposure`, `shadhi` e `monochrome`.
///
/// `escala` é a razão entre esta imagem e a foto original — 1 na exportação em
/// tamanho cheio. É ela que faz o raio de 100 px do `shadhi` significar a mesma
/// região da foto numa cópia de trabalho de 2048 px.
pub fn grades_do_estagio(
    rgba: &[u8],
    largura: usize,
    altura: usize,
    a: &Ajustes,
    escala: f32,
) -> GradesDoEstagio {
    let tub = Tubulacao::nova();
    // 🔑 **O sRGB por tabela**: são 256 valores de byte, e o `powf` por canal de
    // [`Tubulacao::entrar`] era um décimo do custo. Mesma conta, mesmos bits.
    let linear: [f32; 256] = std::array::from_fn(|v| {
        let x = v as f32 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    });
    let mut px: Vec<Rgb> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| {
            mul(
                &tub.srgb_para_trabalho,
                [
                    linear[c[0] as usize],
                    linear[c[1] as usize],
                    linear[c[2] as usize],
                ],
            )
        })
        .collect();
    if a.dt_exposure_ativo != 0.0 {
        exposure(&mut px, Exposure::de(a));
    }

    let com_monochrome = a.dt_monochrome_ativo != 0.0;
    // O Lab que sai do `shadhi`, quando o `monochrome` precisa dele.
    let mut lab_depois_do_shadhi: Option<Vec<Rgb>> = None;
    let mut shadhi_grade = None;
    if a.dt_shadhi_ativo != 0.0 {
        let p = ShadowsHighlights::de(a);
        let mut g = Bilateral::nova(largura, altura, p.radius.max(0.1) * escala, 100.0);
        if com_monochrome {
            // O filtro do `monochrome` é tirado da imagem **depois** do `shadhi`:
            // a conta roda aqui, sobre o Lab e a grade que já estão prontos.
            let lab: Vec<Rgb> = px.iter().map(|c| tub.para_lab(*c)).collect();
            let l: Vec<f32> = lab.iter().map(|c| c[0]).collect();
            g.espalhar(&l);
            g.borrar();
            let base = g.fatiar(&l, -1.0);
            lab_depois_do_shadhi = Some(shadhi_em_lab(&lab, &base, p));
        } else {
            let l: Vec<f32> = px.iter().map(|c| tub.luminancia_lab(*c)).collect();
            g.espalhar(&l);
            g.borrar();
        }
        shadhi_grade = Some(GradeParaGpu::de(&g));
    }

    let mut monochrome_grade = None;
    if com_monochrome {
        let p = Monochrome::de(a);
        let sigma2 = 2.0 * (p.size * 128.0) * (p.size * 128.0);
        let filtro: Vec<f32> = match &lab_depois_do_shadhi {
            Some(lab) => lab
                .iter()
                .map(|c| filtro_monochrome(*c, p, sigma2))
                .collect(),
            None => px
                .iter()
                .map(|c| filtro_monochrome(tub.para_lab(*c), p, sigma2))
                .collect(),
        };
        let mut g = Bilateral::nova(largura, altura, 20.0 / (1.0 / escala).max(1.0), 250.0);
        g.espalhar(&filtro);
        g.borrar();
        monochrome_grade = Some(GradeParaGpu::de(&g));
    }
    GradesDoEstagio {
        shadhi: shadhi_grade,
        monochrome: monochrome_grade,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// 🔑 **As grades do caminho rápido são as do gabarito.** O caminho rápido
    /// decodifica o sRGB por tabela, espalha só o L e tira o filtro do
    /// `monochrome` direto do Lab do `shadhi`; o gabarito é a composição das
    /// funções medidas contra o darktable — `entrar`, `para_lab`, `shadhi` e o
    /// Lab da imagem de saída.
    #[test]
    fn as_grades_rapidas_sao_as_do_gabarito() {
        let (w, h) = (64usize, 40usize);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[
                    (x * 4) as u8,
                    (y * 6) as u8,
                    ((x + y) * 3 % 256) as u8,
                    255,
                ]);
            }
        }
        let a = Ajustes {
            dt_exposure_ativo: 1.0,
            dt_exposure_black: -0.002,
            dt_exposure_exposure: 0.4,
            dt_shadhi_ativo: 1.0,
            dt_shadhi_radius: 8.0,
            dt_shadhi_shadows: 65.0,
            dt_shadhi_highlights: -20.0,
            dt_monochrome_ativo: 1.0,
            dt_monochrome_a: 12.0,
            dt_monochrome_b: -6.0,
            ..Default::default()
        };
        let tub = Tubulacao::nova();
        let mut px: Vec<Rgb> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| tub.entrar([c[0], c[1], c[2]]))
            .collect();
        exposure(&mut px, Exposure::de(&a));
        let l: Vec<f32> = px.iter().map(|c| tub.para_lab(*c)[0]).collect();
        let mut gabarito_sh = Bilateral::nova(w, h, 8.0, 100.0);
        gabarito_sh.espalhar(&l);
        gabarito_sh.borrar();
        shadhi(&mut px, w, h, &tub, ShadowsHighlights::de(&a), 1.0);
        let p = Monochrome::de(&a);
        let sigma2 = 2.0 * (p.size * 128.0) * (p.size * 128.0);
        let filtro: Vec<f32> = px
            .iter()
            .map(|c| filtro_monochrome(tub.para_lab(*c), p, sigma2))
            .collect();
        let mut gabarito_mo = Bilateral::nova(w, h, 20.0, 250.0);
        gabarito_mo.espalhar(&filtro);
        gabarito_mo.borrar();

        let rapidas = grades_do_estagio(&rgba, w, h, &a, 1.0);
        for (nome, rapida, gabarito) in [
            (
                "shadhi",
                rapidas.shadhi.unwrap(),
                GradeParaGpu::de(&gabarito_sh),
            ),
            (
                "monochrome",
                rapidas.monochrome.unwrap(),
                GradeParaGpu::de(&gabarito_mo),
            ),
        ] {
            assert_eq!(rapida.dados.len(), gabarito.dados.len(), "{nome}");
            for (k, (r, g)) in rapida.dados.iter().zip(&gabarito.dados).enumerate() {
                assert!(
                    (r - g).abs() <= 1e-3 * (1.0 + g.abs()),
                    "{nome}[{k}]: {r} × {g}"
                );
            }
        }
    }

    #[test]
    fn a_grade_vira_atlas_com_as_fatias_de_l_lado_a_lado() {
        // 🔑 O shader lê o texel `(x + z·size_x, y)` como a célula `(x, y, z)`.
        // Se esta disposição mudar, o fatiamento da GPU lê a célula do vizinho e
        // nenhum teste de pixel isolado aponta onde.
        let mut g = Bilateral::nova(64, 40, 16.0, 25.0);
        for (k, v) in g.buf.iter_mut().enumerate() {
            *v = k as f32;
        }
        let atlas = GradeParaGpu::de(&g);
        let (sx, sy, sz) = (g.size_x, g.size_y, g.size_z);
        for (x, y, z) in [(0, 0, 0), (sx - 1, 0, 2), (2, sy - 1, sz - 1), (1, 1, 1)] {
            assert_eq!(
                atlas.dados[y * sx * sz + x + z * sx],
                g.buf[(x + y * sx) * sz + z]
            );
        }
        assert_eq!(atlas.largura_do_atlas() as usize, sx * sz);
    }

    #[test]
    fn as_constantes_do_wgsl_estao_em_dia() {
        let caminho = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/shaders/darktable_constantes.wgsl"
        );
        let no_disco = std::fs::read_to_string(caminho).unwrap_or_default();
        assert!(
            no_disco == constantes_wgsl(),
            "darktable_constantes.wgsl envelheceu — regere com: cargo run -q -p revelacao-core \
             --example darktable-constantes-wgsl > crates/revelacao-core/src/shaders/darktable_constantes.wgsl"
        );
    }

    #[test]
    fn o_dt_fast_expf_e_o_do_darktable_e_nao_o_exp() {
        // Em zero ele devolve exatamente 1 (os bits de 0x3f800000).
        assert_eq!(dt_fast_expf(0.0), 1.0);
        // Longe de zero ele se afasta do exp de verdade — é a aproximação que o
        // monochrome usa, e é ela que tem de sair aqui.
        let erro = (dt_fast_expf(-1.0) - (-1.0f32).exp()).abs();
        assert!(erro > 1e-4 && erro < 0.07, "erro contra o exp: {erro}");
    }

    #[test]
    fn o_monochrome_neutro_tira_a_cor_e_mantem_o_cinza() {
        let t = Tubulacao::nova();
        let (w, h) = (48usize, 32usize);
        let mut px = vec![t.entrar([128, 128, 128]); w * h];
        px[5] = t.entrar([200, 40, 40]);
        monochrome(
            &mut px,
            w,
            h,
            &t,
            Monochrome {
                a: 0.0,
                b: 0.0,
                size: 2.0,
                highlights: 0.0,
            },
            1.0,
        );
        let lab = t.para_lab(px[5]);
        assert!(
            lab[1].abs() < 1e-3 && lab[2].abs() < 1e-3,
            "a e b zerados: {lab:?}"
        );
        let cinza = t.sair(px[(h / 2) * w + w / 2]);
        assert!(
            cinza[0].abs_diff(128) <= 1 && cinza[0] == cinza[1] && cinza[1] == cinza[2],
            "{cinza:?}"
        );
    }

    #[test]
    fn o_shadhi_clareia_a_sombra_e_nao_mexe_no_meio_tom_protegido() {
        let t = Tubulacao::nova();
        let (w, h) = (64usize, 64usize);
        // Metade escura, metade clara: a base borrada decide quem é sombra.
        let mut px: Vec<Rgb> = (0..w * h)
            .map(|k| {
                if k % w < w / 2 {
                    t.entrar([30, 30, 30])
                } else {
                    t.entrar([220, 220, 220])
                }
            })
            .collect();
        let antes_escuro = t.sair(px[(h / 2) * w + 4])[0];
        shadhi(
            &mut px,
            w,
            h,
            &t,
            ShadowsHighlights {
                radius: 100.0,
                shadows: 65.38,
                whitepoint: 0.0,
                highlights: -20.51,
                compress: 50.0,
                shadows_ccorrect: 100.0,
                highlights_ccorrect: 50.0,
                flags: 127,
                low_approximation: 1e-6,
            },
            1.0,
        );
        let depois_escuro = t.sair(px[(h / 2) * w + 4])[0];
        assert!(
            depois_escuro > antes_escuro,
            "a sombra clareou: {antes_escuro} → {depois_escuro}"
        );
    }

    #[test]
    fn a_tubulacao_de_ida_e_volta_devolve_o_mesmo_nivel() {
        // 🔑 É a primeira régua de tudo: se sRGB → Rec.2020 → sRGB não fecha,
        // nenhuma comparação com o darktable diz alguma coisa sobre os módulos.
        let t = Tubulacao::nova();
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(51) {
                for b in (0..=255).step_by(85) {
                    let px = [r as u8, g as u8, b as u8];
                    assert_eq!(t.sair(t.entrar(px)), px, "ida e volta de {px:?}");
                }
            }
        }
    }

    #[test]
    fn o_lab_de_ida_e_volta_fecha() {
        let t = Tubulacao::nova();
        for px in [
            [200u8, 120, 80],
            [30, 60, 200],
            [128, 128, 128],
            [250, 250, 10],
        ] {
            let rgb = t.entrar(px);
            let volta = t.de_lab(t.para_lab(rgb));
            for c in 0..3 {
                assert!(
                    (volta[c] - rgb[c]).abs() < 1e-4,
                    "{px:?}: {volta:?} contra {rgb:?}"
                );
            }
        }
        // O cinza médio sRGB tem L* perto de 53,6 — conta conhecida.
        let l = t.para_lab(t.entrar([128, 128, 128]))[0];
        assert!((l - 53.585).abs() < 0.05, "L* do cinza 128 = {l}");
    }

    #[test]
    fn o_dt_ucs_de_ida_e_volta_fecha() {
        let l_white = y_para_l_star(1.0);
        for xyy in [
            [0.3127f32, 0.3290, 0.18],
            [0.40, 0.35, 0.5],
            [0.25, 0.30, 0.05],
        ] {
            let volta = jch_para_xyy(xyy_para_jch(xyy, l_white), l_white);
            for c in 0..3 {
                assert!((volta[c] - xyy[c]).abs() < 1e-3, "{xyy:?} voltou {volta:?}");
            }
        }
    }

    #[test]
    fn a_exposicao_e_linear_e_respeita_o_preto() {
        let mut px = vec![[0.18f32, 0.18, 0.18]];
        exposure(
            &mut px,
            Exposure {
                black: 0.0,
                exposure: 1.0,
            },
        );
        assert!((px[0][0] - 0.36).abs() < 1e-6);
        let mut px = vec![[0.01f32, 0.5, 1.0]];
        exposure(
            &mut px,
            Exposure {
                black: 0.01,
                exposure: 0.0,
            },
        );
        assert!(px[0][0].abs() < 1e-6, "o preto volta a zero: {:?}", px[0]);
    }

    #[test]
    fn a_vinheta_nao_toca_o_centro_e_com_brilho_positivo_clareia_o_canto() {
        let (w, h) = (64usize, 40usize);
        let mut px = vec![[0.2f32; 3]; w * h];
        vignette(
            &mut px,
            w,
            h,
            Vignette {
                scale: 87.82,
                falloff_scale: 45.51,
                brightness: 1.0,
                saturation: 0.147,
                center: [0.0, 0.0],
                autoratio: true,
                whratio: 1.0,
                shape: 0.48,
                unbound: true,
            },
        );
        assert_eq!(px[(h / 2) * w + w / 2], [0.2; 3], "o centro ficou intacto");
        // A conta do darktable no canto (0,0): `pv = (1, 1)`, a superelipse de
        // forma 0,48 dá `2^0,24 = 1,181`, e o peso é `(1,181 − 0,8782) / 0,4551`.
        // Com brilho +1 isso SOMA: 0,2 + peso. Num cinza a saturação não mexe.
        let peso = ((2f32.powf(0.24) - 0.8782) / 0.4551).clamp(0.0, 1.0);
        assert!(
            (px[0][0] - (0.2 + peso)).abs() < 1e-5,
            "canto {:?}, esperado {}",
            px[0],
            0.2 + peso
        );
        assert!(
            px[0][0] > 0.8,
            "o canto foi em direção ao branco: {:?}",
            px[0]
        );
    }

    #[test]
    fn a_tabela_de_gamut_do_rec2020_e_positiva_em_toda_a_roda() {
        let t = Tubulacao::nova();
        let lut = tabela_de_gamut(&mul_mat(&XYZ_D50_PARA_D65_CAT16, &t.trabalho_para_xyz_d50));
        assert!(lut.iter().all(|v| *v > 0.0), "há matiz sem borda de gamut");
    }
}
