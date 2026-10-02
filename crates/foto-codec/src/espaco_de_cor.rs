//! O espaço de cor que o arquivo declara — e a foto convertida para sRGB.
//!
//! 🚨 **A câmera do estúdio grava em Adobe RGB, e o app lia como sRGB.** As
//! Nikon no modo Adobe RGB não embutem perfil ICC: marcam no EXIF o índice de
//! interoperabilidade DCF **`R03`**. O Lightroom e o darktable obedecem; o
//! `image` do Rust e os navegadores, não. Lida como sRGB, a foto sai mais
//! apagada e mais fria do que o fotógrafo viu na câmera e no Lightroom: na
//! régua de 1/out/2026, o neutro ficou a ΔE2000 3,08 do Lightroom (croma 0,81);
//! convertido, 1,27 (croma 1,00) — `docs/REGUA-DO-LIGHTROOM.md`, seção 9.
//!
//! 🔑 **Uma leitura só, a do arquivo** (dono, 2/out/2026). O `RecordarFotos
//! P&B` do darktable lia sRGB de propósito — o `colorin` do `.dtstyle` fixa
//! sRGB, e o motor lendo sRGB batia com o `darktable-cli` a ΔE 0,7 — mas manter
//! as duas leituras complicaria toda medição; o estilo é que se refaz sobre esta.
//!
//! Mora aqui pelo mesmo motivo da orientação: é o que a etiqueta do arquivo diz,
//! aplicado ao que se *deriva* dele (C1: o arquivo da câmera não é tocado), e
//! todo destino que decodifica por este crate tem de ver a mesma foto.
//!
//! | O arquivo diz | Espaço | Quem grava assim |
//! |---|---|---|
//! | ICC com "Adobe RGB" na descrição | Adobe RGB (1998) | Lightroom, Photoshop |
//! | ICC com "Display P3" | Display P3 | iPhone, Mac |
//! | outro ICC | tratado como sRGB | — |
//! | sem ICC, EXIF `InteropIndex = R03` | Adobe RGB (1998) | câmeras no modo Adobe RGB |
//! | nada disso | sRGB | — |
//!
//! O ICC vence o EXIF: é a declaração mais específica. Um ICC que não se
//! reconhece não é convertido — sem um gerenciador de cor completo, chutar
//! seria pior que deixar.
//!
//! A conta é a de qualquer gerenciador de cor entre espaços RGB de mesmo branco
//! (D65): tira a curva do espaço de origem, troca os primários por uma matriz
//! 3×3 em luz linear, e põe a curva do sRGB. O que sai da gama do sRGB é
//! cortado, como o Lightroom faz ao exportar em sRGB.

use image::DynamicImage;

/// O espaço de cor de uma foto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EspacoDeCor {
    Srgb,
    /// Adobe RGB (1998): primários mais largos no verde, curva de gama 563/256.
    AdobeRgb,
    /// Display P3: primários do DCI-P3 com branco D65 e a curva do sRGB.
    DisplayP3,
}

impl EspacoDeCor {
    /// O que o arquivo declara: o perfil ICC, se houver; senão o EXIF.
    pub fn declarado(icc: Option<&[u8]>, exif: Option<&[u8]>) -> Self {
        if let Some(icc) = icc {
            return Self::do_icc(icc);
        }
        exif.and_then(Self::do_exif).unwrap_or(Self::Srgb)
    }

    /// Pela descrição do perfil, em ASCII (ICC v2) ou UTF-16BE (v4, `mluc`).
    fn do_icc(icc: &[u8]) -> Self {
        let tem = |texto: &str| {
            let ascii = texto.as_bytes();
            let utf16: Vec<u8> = texto.bytes().flat_map(|c| [0, c]).collect();
            icc.windows(ascii.len()).any(|j| j == ascii)
                || icc.windows(utf16.len()).any(|j| j == utf16.as_slice())
        };
        if tem("Adobe RGB") {
            Self::AdobeRgb
        } else if tem("Display P3") {
            Self::DisplayP3
        } else {
            Self::Srgb
        }
    }

    /// O índice de interoperabilidade DCF do EXIF: `R03` é Adobe RGB, `R98`
    /// é sRGB. O caminho é IFD0 → Exif (0x8769) → Interop (0xA005) → 0x0001.
    fn do_exif(exif: &[u8]) -> Option<Self> {
        let tiff = exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
        let grande = match tiff.get(..4)? {
            [b'I', b'I', 42, 0] => false,
            [b'M', b'M', 0, 42] => true,
            _ => return None,
        };
        let u16_em = |i: usize| -> Option<u16> {
            let b: [u8; 2] = tiff.get(i..i + 2)?.try_into().ok()?;
            Some(if grande {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            })
        };
        let u32_em = |i: usize| -> Option<u32> {
            let b: [u8; 4] = tiff.get(i..i + 4)?.try_into().ok()?;
            Some(if grande {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            })
        };
        // O começo da entrada `etiqueta` numa IFD, se houver.
        let entrada = |ifd: usize, etiqueta: u16| -> Option<usize> {
            let n = u16_em(ifd)? as usize;
            (0..n)
                .map(|k| ifd + 2 + k * 12)
                .find(|&e| u16_em(e) == Some(etiqueta))
        };
        let ifd0 = u32_em(4)? as usize;
        let exif_ifd = u32_em(entrada(ifd0, 0x8769)? + 8)? as usize;
        let interop = u32_em(entrada(exif_ifd, 0xA005)? + 8)? as usize;
        let indice = entrada(interop, 0x0001)?;
        // ASCII de até 4 bytes ("R03\0") cabe na própria entrada.
        match tiff.get(indice + 8..indice + 11)? {
            b"R03" => Some(Self::AdobeRgb),
            b"R98" => Some(Self::Srgb),
            _ => None,
        }
    }

    /// A matriz de luz linear deste espaço para o sRGB (os dois em D65).
    fn para_srgb(self) -> [[f32; 3]; 3] {
        match self {
            Self::Srgb => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Self::AdobeRgb => [
                [1.398_283, -0.398_283, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -0.042_938, 1.042_938],
            ],
            Self::DisplayP3 => [
                [1.224_94, -0.224_94, 0.0],
                [-0.042_057, 1.042_057, 0.0],
                [-0.019_638, -0.078_636, 1.098_274],
            ],
        }
    }

    /// Do valor de 8 bits deste espaço para a luz linear.
    fn linear(self, v: u8) -> f32 {
        let v = v as f32 / 255.0;
        match self {
            Self::AdobeRgb => v.powf(563.0 / 256.0),
            Self::Srgb | Self::DisplayP3 => srgb_para_linear(v),
        }
    }
}

fn srgb_para_linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_para_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Passos da tabela que volta da luz linear para o sRGB de 8 bits: com 65 536,
/// o passo no escuro (onde a curva é mais íngreme) fica abaixo de 0,05 nível.
const PASSOS: usize = 1 << 16;

/// A foto em sRGB. Em sRGB, devolve como veio, sem tocar num pixel.
///
/// Em 8 bits por canal: 16 bits (um PNG raro) descem a 8 antes — o motor de
/// revelação recebe 8 de todo jeito. O alfa passa intacto.
pub fn para_srgb(foto: DynamicImage, de: EspacoDeCor) -> DynamicImage {
    if de == EspacoDeCor::Srgb {
        return foto;
    }
    let linear: Vec<f32> = (0..=255u8).map(|v| de.linear(v)).collect();
    let volta: Vec<u8> = (0..PASSOS)
        .map(|i| {
            let v = linear_para_srgb(i as f32 / (PASSOS - 1) as f32);
            (v * 255.0).round().clamp(0.0, 255.0) as u8
        })
        .collect();
    let m = de.para_srgb();
    let converter = |p: &mut [u8]| {
        let c = [
            linear[p[0] as usize],
            linear[p[1] as usize],
            linear[p[2] as usize],
        ];
        for (saida, linha) in p.iter_mut().zip(m) {
            let v = linha[0] * c[0] + linha[1] * c[1] + linha[2] * c[2];
            *saida = volta[(v.clamp(0.0, 1.0) * (PASSOS - 1) as f32).round() as usize];
        }
    };
    match foto {
        DynamicImage::ImageRgb8(mut rgb) => {
            rgb.as_chunks_mut::<3>()
                .0
                .iter_mut()
                .for_each(|p| converter(p));
            DynamicImage::ImageRgb8(rgb)
        }
        DynamicImage::ImageRgba8(mut rgba) => {
            rgba.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| converter(p));
            DynamicImage::ImageRgba8(rgba)
        }
        // Cinza não tem primários para trocar; a curva difere pouco e fica.
        cinza @ (DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_)) => cinza,
        outra => para_srgb(DynamicImage::ImageRgba8(outra.to_rgba8()), de),
    }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;

    /// O EXIF mínimo de uma câmera no modo Adobe RGB: IFD0 → Exif → Interop,
    /// com `InteropIndex = "R03"`. Em little-endian, como as Nikon.
    pub(crate) fn exif_com_indice(indice: &[u8; 3]) -> Vec<u8> {
        let mut t = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        let mut ifd = |etiqueta: u16, tipo: u16, valor: [u8; 4]| {
            t.extend_from_slice(&1u16.to_le_bytes());
            t.extend_from_slice(&etiqueta.to_le_bytes());
            t.extend_from_slice(&tipo.to_le_bytes());
            t.extend_from_slice(&if tipo == 2 { 4u32 } else { 1u32 }.to_le_bytes());
            t.extend_from_slice(&valor);
            t.extend_from_slice(&0u32.to_le_bytes());
        };
        // Cada IFD de uma entrada ocupa 18 bytes: 8 → 26 → 44.
        ifd(0x8769, 4, 26u32.to_le_bytes());
        ifd(0xA005, 4, 44u32.to_le_bytes());
        ifd(0x0001, 2, [indice[0], indice[1], indice[2], 0]);
        t
    }

    #[test]
    fn o_r03_do_exif_e_adobe_rgb() {
        let exif = exif_com_indice(b"R03");
        assert_eq!(
            EspacoDeCor::declarado(None, Some(&exif)),
            EspacoDeCor::AdobeRgb
        );
        // Com o cabeçalho do APP1 junto, também.
        let mut com_cabecalho = b"Exif\0\0".to_vec();
        com_cabecalho.extend_from_slice(&exif);
        assert_eq!(
            EspacoDeCor::declarado(None, Some(&com_cabecalho)),
            EspacoDeCor::AdobeRgb
        );
    }

    #[test]
    fn o_r98_e_o_exif_sem_indice_sao_srgb() {
        let exif = exif_com_indice(b"R98");
        assert_eq!(EspacoDeCor::declarado(None, Some(&exif)), EspacoDeCor::Srgb);
        assert_eq!(EspacoDeCor::declarado(None, None), EspacoDeCor::Srgb);
        assert_eq!(
            EspacoDeCor::declarado(None, Some(b"lixo")),
            EspacoDeCor::Srgb
        );
        // Truncado no meio de uma IFD: sem pânico, sRGB.
        let cortado = &exif_com_indice(b"R03")[..30];
        assert_eq!(
            EspacoDeCor::declarado(None, Some(cortado)),
            EspacoDeCor::Srgb
        );
    }

    #[test]
    fn o_icc_vence_o_exif() {
        let r03 = exif_com_indice(b"R03");
        let srgb = b"....desc....sRGB IEC61966-2.1....";
        assert_eq!(
            EspacoDeCor::declarado(Some(srgb), Some(&r03)),
            EspacoDeCor::Srgb
        );
        let adobe = b"....desc....Adobe RGB (1998)....";
        assert_eq!(
            EspacoDeCor::declarado(Some(adobe), None),
            EspacoDeCor::AdobeRgb
        );
        // ICC v4: a descrição em UTF-16BE.
        let p3: Vec<u8> = b"mluc...."
            .iter()
            .copied()
            .chain("Display P3".bytes().flat_map(|c| [0, c]))
            .collect();
        assert_eq!(
            EspacoDeCor::declarado(Some(&p3), None),
            EspacoDeCor::DisplayP3
        );
    }

    fn um_pixel(rgb: [u8; 3], de: EspacoDeCor) -> [u8; 3] {
        let foto = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, image::Rgb(rgb)));
        para_srgb(foto, de).to_rgb8().get_pixel(0, 0).0
    }

    /// Um laranja de pele em Adobe RGB, convertido à mão (curva 563/256,
    /// matriz dos primários, curva do sRGB): fica mais vermelho e mais
    /// saturado — é o que o Lightroom mostra e o app apagava.
    #[test]
    fn o_laranja_em_adobe_rgb_sai_mais_saturado() {
        let [r, g, b] = um_pixel([200, 120, 60], EspacoDeCor::AdobeRgb);
        for (canal, valor, esperado) in [("R", r, 224), ("G", g, 121), ("B", b, 53)] {
            assert!(
                (valor as i32 - esperado).abs() <= 1,
                "{canal} = {valor}, esperado {esperado}"
            );
        }
    }

    /// O cinza continua cinza. Em Adobe RGB ele muda um pouco de claro nas
    /// sombras — a curva 2,2 não é a do sRGB (40 vira 35) —, mas o preto, o
    /// branco e o meio ficam. O Display P3 tem a curva do sRGB: não muda.
    #[test]
    fn o_cinza_continua_cinza_e_o_srgb_nao_muda() {
        let mut antes = 0;
        for v in [0u8, 1, 40, 128, 200, 255] {
            let [r, g, b] = um_pixel([v, v, v], EspacoDeCor::AdobeRgb);
            assert!(r == g && g == b, "cinza {v} virou {r}/{g}/{b}");
            assert!(r >= antes, "a escala de cinza desceu em {v}");
            antes = r;
            let [r, g, b] = um_pixel([v, v, v], EspacoDeCor::DisplayP3);
            assert!(r.abs_diff(v) <= 1 && g.abs_diff(v) <= 1 && b.abs_diff(v) <= 1);
        }
        assert_eq!(um_pixel([0, 0, 0], EspacoDeCor::AdobeRgb), [0, 0, 0]);
        assert_eq!(um_pixel([255, 255, 255], EspacoDeCor::AdobeRgb), [255; 3]);
        assert!(um_pixel([128; 3], EspacoDeCor::AdobeRgb)[0].abs_diff(128) <= 2);
        assert_eq!(um_pixel([200, 120, 60], EspacoDeCor::Srgb), [200, 120, 60]);
    }

    #[test]
    fn o_alfa_passa_intacto() {
        let foto = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([200, 120, 60, 77]),
        ));
        let p = para_srgb(foto, EspacoDeCor::AdobeRgb)
            .to_rgba8()
            .get_pixel(0, 0)
            .0;
        assert_eq!(p[3], 77);
        assert_ne!(&p[..3], &[200, 120, 60]);
    }
}
