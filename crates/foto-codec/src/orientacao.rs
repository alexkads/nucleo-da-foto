//! Abrir a foto **de pé** — com a orientação do EXIF aplicada aos pixels.
//!
//! 🚨 **Câmera e celular quase nunca giram os pixels quando o fotógrafo vira o
//! aparelho**: gravam a foto deitada e anotam `Orientation` no EXIF, e quem
//! desenha é que roda. O navegador roda sozinho; o `image` do Rust **não** — ele
//! devolve os pixels como estão no arquivo e deixa a etiqueta de lado.
//!
//! O resultado foi a foto em retrato deitada em tudo que o desktop mostrava —
//! grade, tira, revelação — e, pior, **subindo deitada e sem etiqueta**: o JPEG
//! que o desktop gera não leva EXIF, então o servidor (que já aplica a
//! orientação, `marca_dagua::decodificar_de_pe`) não tinha mais o que ler
//! (dono, 21/set/2026, com as fotos da NIKON D3100 dele).
//!
//! Mora no `foto-codec` porque é regra de **todo destino**: o desktop abre o
//! arquivo da câmera com ela, e os dois wasm do site (a grade e o motor de
//! revelação) decodificam com ela quando o `createImageBitmap` do navegador não
//! é quem decodifica.
//!
//! 🔑 **O arquivo da câmera não é tocado** (contrato C1): a orientação entra no
//! que se *deriva* dele, que é o que o navegador faria ao abrir o mesmo arquivo.
//! Etiqueta ilegível não recusa a foto: sem orientação que se leia, ela segue
//! como veio.

use std::io::{BufRead, Cursor, Seek};
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, ImageResult};

use crate::espaco_de_cor::{self, EspacoDeCor};

/// Decodifica bytes de imagem já **de pé e em sRGB**: a orientação do EXIF
/// aplicada aos pixels e o espaço de cor declarado (ver
/// [`espaco_de_cor`]) convertido.
///
/// É a porta de quem decodifica sem o navegador: os wasm do site e o desktop.
/// Etiqueta ilegível não recusa a foto — sem orientação que se leia, ela sai
/// como veio.
///
/// ```rust
/// use image::{DynamicImage, ImageFormat, RgbImage};
///
/// // Um JPEG sem EXIF: sai como entrou, 6×4.
/// let mut jpeg = Vec::new();
/// DynamicImage::ImageRgb8(RgbImage::new(6, 4))
///     .write_to(&mut std::io::Cursor::new(&mut jpeg), ImageFormat::Jpeg)
///     .unwrap();
///
/// let foto = foto_codec::orientacao::decodificar_de_pe(&jpeg).unwrap();
/// assert_eq!((foto.width(), foto.height()), (6, 4));
/// ```
///
/// # Erros
///
/// [`image::ImageError`] se o formato não for reconhecido ou os pixels não
/// decodificarem.
pub fn decodificar_de_pe(bytes: &[u8]) -> ImageResult<DynamicImage> {
    de_pe(ImageReader::new(Cursor::new(bytes)).with_guessed_format()?)
}

/// Abre um arquivo do disco já de pé e em sRGB — o mesmo que
/// [`decodificar_de_pe`], lendo do caminho. O arquivo não é tocado.
///
/// # Erros
///
/// [`image::ImageError`] se o arquivo não abrir (inclusive erro de E/S) ou
/// não decodificar.
pub fn abrir_de_pe(caminho: impl AsRef<Path>) -> ImageResult<DynamicImage> {
    de_pe(ImageReader::open(caminho)?.with_guessed_format()?)
}

/// De pé **e em sRGB**: a outra etiqueta que o `image` deixa de lado é o
/// espaço de cor (`espaco_de_cor`) — a câmera do estúdio grava Adobe RGB.
fn de_pe<R: BufRead + Seek>(leitor: ImageReader<R>) -> ImageResult<DynamicImage> {
    let mut decodificador = leitor.into_decoder()?;
    let orientacao = decodificador
        .orientation()
        .unwrap_or(Orientation::NoTransforms);
    let icc = decodificador.icc_profile().ok().flatten();
    let exif = decodificador.exif_metadata().ok().flatten();
    let espaco = EspacoDeCor::declarado(icc.as_deref(), exif.as_deref());
    let mut foto = espaco_de_cor::para_srgb(DynamicImage::from_decoder(decodificador)?, espaco);
    aplicar(&mut foto, orientacao);
    Ok(foto)
}

/// A orientação do EXIF aplicada aos pixels — o mesmo que
/// [`DynamicImage::apply_orientation`], pixel a pixel, mais rápido.
///
/// 🔑 O `image` gira chamando `get_pixel` e `put_pixel` em cada pixel, com
/// conferência de limite nos dois; numa foto de 24 MP deitada eram dezenas de
/// milissegundos a cada abertura. Aqui, em RGB e RGBA de 8 bits, cada pixel
/// de destino sai de uma conta afim da posição (a orientação só troca pixels
/// de lugar), em blocos de 64 × 64 que cabem no cache, com as faixas de linhas
/// divididas entre os núcleos (no `wasm32`, uma thread só). Os outros tipos
/// vão ao `image`. O espaço de cor que a foto carrega vai junto.
pub fn aplicar(foto: &mut DynamicImage, orientacao: Orientation) {
    if orientacao == Orientation::NoTransforms {
        return;
    }
    match foto {
        DynamicImage::ImageRgb8(rgb) => {
            if let Some(nova) = reorientada::<image::Rgb<u8>, 3>(rgb, orientacao) {
                *rgb = nova;
                return;
            }
        }
        DynamicImage::ImageRgba8(rgba) => {
            if let Some(nova) = reorientada::<image::Rgba<u8>, 4>(rgba, orientacao) {
                *rgba = nova;
                return;
            }
        }
        _ => {}
    }
    foto.apply_orientation(orientacao);
}

/// A foto na orientação pedida, ou `None` se ela não couber na conta.
fn reorientada<P, const N: usize>(
    foto: &image::ImageBuffer<P, Vec<u8>>,
    orientacao: Orientation,
) -> Option<image::ImageBuffer<P, Vec<u8>>>
where
    P: image::Pixel<Subpixel = u8>,
{
    let (largura, altura) = (foto.width() as usize, foto.height() as usize);
    let (w, h) = (largura as isize, altura as isize);
    // De onde vem o pixel `(x, y)` do destino: `x·dx + y·dy + início`, em
    // pixels da origem. Do mesmo jeito que o `image` gira (horário) e espelha.
    let (deitada, inicio, dx, dy) = match orientacao {
        Orientation::NoTransforms => (false, 0, 1, w),
        Orientation::FlipHorizontal => (false, w - 1, -1, w),
        Orientation::FlipVertical => (false, (h - 1) * w, 1, -w),
        Orientation::Rotate180 => (false, h * w - 1, -1, -w),
        // Girar 90° no horário: o destino (x, y) vem de (y, h − 1 − x).
        Orientation::Rotate90 => (true, (h - 1) * w, -w, 1),
        // 270°: de (w − 1 − y, x).
        Orientation::Rotate270 => (true, w - 1, w, -1),
        // 90° e espelho: a transposta, de (y, x).
        Orientation::Rotate90FlipH => (true, 0, w, 1),
        // 270° e espelho: de (w − 1 − y, h − 1 − x).
        Orientation::Rotate270FlipH => (true, h * w - 1, -w, -1),
    };
    let (nova_largura, nova_altura) = if deitada {
        (altura, largura)
    } else {
        (largura, altura)
    };
    let origem = foto
        .as_raw()
        .get(..largura.checked_mul(altura)?.checked_mul(N)?)?;
    let mut destino = vec![0u8; origem.len()];
    let pixels = origem.as_chunks::<N>().0;

    // As linhas `y0..` do destino, em blocos de 64 colunas: as 64 linhas da
    // origem que um bloco lê ficam no cache até ele acabar.
    let faixa = |y0: usize, linhas: &mut [u8]| {
        let saida = linhas.as_chunks_mut::<N>().0;
        let n_linhas = saida.len() / nova_largura.max(1);
        for x0 in (0..nova_largura).step_by(64) {
            let x1 = (x0 + 64).min(nova_largura);
            for j in 0..n_linhas {
                let y = (y0 + j) as isize;
                let linha = &mut saida[j * nova_largura..][x0..x1];
                let mut de = inicio + y * dy + x0 as isize * dx;
                for p in linha {
                    *p = pixels[de as usize];
                    de += dx;
                }
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let bytes_por_linha = nova_largura * N;
        let nucleos = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(16);
        if nucleos > 1 && pixels.len() >= 1 << 18 {
            let por_faixa = nova_altura.div_ceil(nucleos * 2).next_multiple_of(64);
            std::thread::scope(|escopo| {
                for (k, linhas) in destino.chunks_mut(por_faixa * bytes_por_linha).enumerate() {
                    let faixa = &faixa;
                    escopo.spawn(move || faixa(k * por_faixa, linhas));
                }
            });
        } else {
            faixa(0, &mut destino);
        }
    }
    #[cfg(target_arch = "wasm32")]
    faixa(0, &mut destino);

    let mut nova = image::ImageBuffer::from_raw(nova_largura as u32, nova_altura as u32, destino)?;
    nova.set_color_space(foto.color_space()).ok()?;
    Some(nova)
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Um JPEG 4×2 (deitado) com `Orientation = 6` (girar 90° no horário) no
    /// EXIF — o que a câmera grava quando a foto foi feita em pé.
    fn jpeg_com_orientacao(valor: u16) -> Vec<u8> {
        // APP1 com um TIFF mínimo: II*, IFD0 com uma entrada (0x0112).
        let mut tiff = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0];
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // uma
        tiff.extend_from_slice(&valor.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // sem próximo IFD
        jpeg_com_exif(4, 2, [90, 90, 90], &tiff)
    }

    /// Um JPEG liso `largura × altura` da cor dada, com `tiff` no APP1.
    fn jpeg_com_exif(largura: u32, altura: u32, cor: [u8; 3], tiff: &[u8]) -> Vec<u8> {
        let imagem = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            largura,
            altura,
            image::Rgb(cor),
        ));
        let mut jpeg = Vec::new();
        imagem
            .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .expect("codificar");
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(tiff);
        let mut saida = vec![0xFF, 0xD8, 0xFF, 0xE1];
        saida.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        saida.extend_from_slice(&app1);
        saida.extend_from_slice(&jpeg[2..]);
        saida
    }

    /// A foto de uma câmera no modo Adobe RGB (`R03` no EXIF, sem ICC) abre
    /// convertida para sRGB — o laranja fica mais vermelho, como no Lightroom.
    /// O `image` puro, que é o que ignorava a etiqueta, a deixa como está.
    #[test]
    fn a_foto_em_adobe_rgb_abre_em_srgb() {
        let tiff = crate::espaco_de_cor::testes::exif_com_indice(b"R03");
        let jpeg = jpeg_com_exif(16, 16, [200, 120, 60], &tiff);
        let p = decodificar_de_pe(&jpeg)
            .expect("abre")
            .to_rgb8()
            .get_pixel(8, 8)
            .0;
        for (valor, esperado) in p.iter().zip([224, 121, 53]) {
            assert!((*valor as i32 - esperado).abs() <= 3, "saiu {p:?}");
        }
        let cru = image::load_from_memory(&jpeg)
            .unwrap()
            .to_rgb8()
            .get_pixel(8, 8)
            .0;
        assert!((cru[0] as i32 - 200).abs() <= 3, "o image puro deu {cru:?}");
    }

    /// O caminho do site: o navegador decodifica sem converter, e o espaço
    /// lido do cabeçalho mais a conversão do RGBA dão o mesmo pixel do app.
    #[test]
    fn o_espaco_do_cabecalho_e_a_conversao_do_rgba_dao_a_foto_do_app() {
        use crate::espaco_de_cor::{declarado_no_arquivo, rgba_para_srgb};
        let tiff = crate::espaco_de_cor::testes::exif_com_indice(b"R03");
        let jpeg = jpeg_com_exif(16, 16, [200, 120, 60], &tiff);
        let espaco = declarado_no_arquivo(&jpeg);
        assert_eq!(espaco, EspacoDeCor::AdobeRgb);
        assert_eq!(EspacoDeCor::do_codigo(espaco.codigo()), espaco);

        let mut rgba = image::load_from_memory(&jpeg)
            .unwrap()
            .to_rgba8()
            .into_raw();
        rgba_para_srgb(&mut rgba, espaco);
        let do_app = decodificar_de_pe(&jpeg).unwrap().to_rgba8().into_raw();
        assert_eq!(rgba, do_app);

        let sem = jpeg_com_orientacao(1);
        assert_eq!(declarado_no_arquivo(&sem), EspacoDeCor::Srgb);
        assert_eq!(declarado_no_arquivo(b"nem imagem"), EspacoDeCor::Srgb);
    }

    #[test]
    fn a_foto_em_pe_sai_de_pe() {
        let foto = decodificar_de_pe(&jpeg_com_orientacao(6)).expect("abre");
        assert_eq!((foto.width(), foto.height()), (2, 4), "girou 90°");
        // O `image` puro, que era o que o desktop usava, a deixa deitada.
        let crua = image::load_from_memory(&jpeg_com_orientacao(6)).unwrap();
        assert_eq!((crua.width(), crua.height()), (4, 2));
    }

    #[test]
    fn sem_etiqueta_segue_como_veio() {
        let foto = decodificar_de_pe(&jpeg_com_orientacao(1)).expect("abre");
        assert_eq!((foto.width(), foto.height()), (4, 2));
    }

    /// 🚨 **O giro daqui é o do `image`, pixel a pixel**: as oito orientações,
    /// em RGB e RGBA, em lados que não fecham um bloco e numa foto grande o
    /// bastante para ir em faixas.
    #[test]
    fn as_oito_orientacoes_sao_as_do_image() {
        use image::{RgbImage, RgbaImage};
        let todas = [
            Orientation::NoTransforms,
            Orientation::Rotate90,
            Orientation::Rotate180,
            Orientation::Rotate270,
            Orientation::FlipHorizontal,
            Orientation::FlipVertical,
            Orientation::Rotate90FlipH,
            Orientation::Rotate270FlipH,
        ];
        for (largura, altura) in [(1, 1), (7, 5), (64, 3), (130, 67), (701, 409)] {
            let rgba = RgbaImage::from_fn(largura, altura, |x, y| {
                image::Rgba([x as u8, y as u8, (x * 7 + y * 3) as u8, (x ^ y) as u8])
            });
            let rgb = RgbImage::from_fn(largura, altura, |x, y| {
                image::Rgb([(x * 5) as u8, (y * 11) as u8, (x + y) as u8])
            });
            for foto in [DynamicImage::ImageRgba8(rgba), DynamicImage::ImageRgb8(rgb)] {
                for o in todas {
                    let mut daqui = foto.clone();
                    aplicar(&mut daqui, o);
                    let mut do_image = foto.clone();
                    do_image.apply_orientation(o);
                    assert_eq!(
                        daqui,
                        do_image,
                        "{largura}×{altura} {:?} {o:?}",
                        foto.color()
                    );
                    assert_eq!(daqui.color_space(), do_image.color_space());
                }
            }
        }
        // O que não é RGB nem RGBA de 8 bits vai ao `image`.
        let cinza = DynamicImage::ImageLuma8(image::GrayImage::from_fn(5, 3, |x, y| {
            image::Luma([(x * 40 + y) as u8])
        }));
        let mut daqui = cinza.clone();
        aplicar(&mut daqui, Orientation::Rotate90);
        let mut do_image = cinza;
        do_image.apply_orientation(Orientation::Rotate90);
        assert_eq!(daqui, do_image);
    }
}
