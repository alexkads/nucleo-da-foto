//! 🎞️ **A vinheta do darktable, na CPU** — o gabarito de
//! `vinheta_do_darktable` (`shaders/corpo.wgsl`).
//!
//! É a `vignette_no_quadro` do estágio darktable que saiu do motor em
//! 2/out/2026 (`git show 2cccd47^:crates/revelacao-core/src/darktable.rs`), a
//! conta de `src/iop/vignette.c:694–841` do darktable 5.6.1 medida contra o
//! `darktable-cli` (máximo de 1 nível). Voltou sozinha, sem o resto do estágio,
//! porque as duas vinhetas do Lightroom não imitam a do `RecordarFotos P&B`:
//!
//! - 🔑 **brilho positivo SOMA luz linear, negativo multiplica**
//!   (`vignette.c:816–827`) — a borda branca;
//! - a forma é a superelipse de expoente `2/forma`, que com a proporção
//!   automática acompanha o quadro.
//!
//! Lá a conta era no Rec.2020 linear do darktable; aqui é no sRGB linear do
//! motor. Brilho e pesos somam o mesmo nos três canais, então numa foto cinza
//! (o P&B) os dois dão o mesmo nível; na colorida a saturação difere no
//! terceiro algarismo.
//!
//! A matização (o *dithering* do módulo) entra como no `vignette.c` de hoje —
//! o cosseno no peso da faixa de transição e o ruído triangular do `tea.h` —,
//! com o sorteio partindo do `(x, y)` de cada pixel, como a GPU consegue.
//! Sempre *unbound* (o 0–255 do fim do shader prende).

use crate::ajustes::Ajustes;
use crate::transformacao::Quadro;

fn srgb_para_linear(v255: f32) -> f32 {
    let x = v255 / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_para_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92 * 255.0
    } else {
        (1.055 * v.powf(1.0 / 2.4) - 0.055) * 255.0
    }
}

/// O `encrypt_tea` do darktable (`src/common/tea.h`, 8 voltas), a partir do
/// `(x, y)` do pixel — o `dt_tea` do shader.
fn tea(x: u32, y: u32) -> u32 {
    let (mut v0, mut v1, mut soma) = (x, y, 0u32);
    for _ in 0..8 {
        soma = soma.wrapping_add(0x9e3779b9);
        v0 = v0.wrapping_add(
            (v1 << 4).wrapping_add(0xa341316c)
                ^ v1.wrapping_add(soma)
                ^ (v1 >> 5).wrapping_add(0xc8013ea4),
        );
        v1 = v1.wrapping_add(
            (v0 << 4).wrapping_add(0xad90777d)
                ^ v0.wrapping_add(soma)
                ^ (v0 >> 5).wrapping_add(0x7e95761e),
        );
    }
    v0
}

/// O `tpdf` do darktable: o sorteio em −1..1, triangular.
fn tpdf(sorteio: u32) -> f32 {
    let f = sorteio as f32 / 4294967295.0;
    if f < 0.5 {
        (2.0 * f).sqrt() - 1.0
    } else {
        1.0 - (2.0 * (1.0 - f)).sqrt()
    }
}

/// Aplica a vinheta do darktable de `a` em `pixels` (RGB 0–255, sRGB), uma foto
/// `largura` × `altura`, medida no `quadro` do arquivo que sai.
pub fn aplicar(
    pixels: &mut [[f32; 3]],
    largura: usize,
    altura: usize,
    a: &Ajustes,
    quadro: &Quadro,
) {
    if !a.vinheta_do_darktable_ligada() {
        return;
    }
    let (w, h) = (quadro.largura.max(1.0), quadro.altura.max(1.0));
    let centro = [
        w * 0.5 + a.darktable_vignette_center_x * w / 2.0,
        h * 0.5 + a.darktable_vignette_center_y * h / 2.0,
    ];
    let (xscale, yscale) = if a.darktable_vignette_autoratio >= 0.5 {
        (2.0 / w, 2.0 / h)
    } else {
        let base = 2.0 / w.max(h);
        let proporcao = a.darktable_vignette_whratio.clamp(0.001, 1.999);
        if proporcao <= 1.0 {
            (base / proporcao, base)
        } else {
            (base, base / (2.0 - proporcao))
        }
    };
    let dscale = a.darktable_vignette_scale / 100.0;
    let min_falloff = 100.0 / w.min(h);
    let fscale = a.darktable_vignette_falloff_scale.max(min_falloff) / 100.0;
    let forma = a.darktable_vignette_shape.max(0.001);
    let (exp1, exp2) = (2.0 / forma, forma / 2.0);
    let brilho = a.darktable_vignette_brightness;
    let saturacao = a.darktable_vignette_saturation;
    let degrau = match a.darktable_vignette_dithering.round() as i32 {
        1 => 1.0 / 256.0,
        2 => 1.0 / 65536.0,
        _ => 0.0,
    };

    for j in 0..altura {
        for i in 0..largura {
            let (x, y) = quadro.no_quadro(i as f32, j as f32);
            let pv = [
                (x * xscale - centro[0] * xscale).abs().max(1e-12),
                (y * yscale - centro[1] * yscale).abs().max(1e-12),
            ];
            let cplen = (pv[0].powf(exp1) + pv[1].powf(exp1)).powf(exp2);
            let (mut peso, mut ruido) = (0.0f32, 0.0f32);
            if cplen >= dscale {
                peso = (cplen - dscale) / fscale;
                if peso >= 1.0 {
                    peso = 1.0;
                } else if peso <= 0.0 {
                    peso = 0.0;
                } else if degrau != 0.0 {
                    peso = 0.5 - (std::f32::consts::PI * peso).cos() / 2.0;
                    ruido = degrau * tpdf(tea(i as u32, j as u32));
                }
            }
            if peso <= 0.0 {
                continue;
            }
            let px = &mut pixels[j * largura + i];
            let mut col = px.map(|c| srgb_para_linear(c.clamp(0.0, 255.0)));
            if brilho < 0.0 {
                col = col.map(|c| c * (1.0 + peso * brilho) + ruido);
            } else {
                col = col.map(|c| c + peso * brilho + ruido);
            }
            let mv = (col[0] + col[1] + col[2]) / 3.0;
            col = col.map(|c| (c - (mv - c) * peso * saturacao).max(0.0));
            *px = col.map(linear_para_srgb);
        }
    }
}
