//! O JPEG que sai do estúdio — um codificador só, para todo destino.
//!
//! 🔑 A exportação do desktop, o envio ao pós-venda, a revelação no navegador e
//! o **download do cliente** passam por aqui. Dois codificadores dariam dois
//! arquivos diferentes para a mesma foto revelada, e a exportação deixaria de
//! ser a prova do que o site recebe. É por isso que o navegador **não** usa
//! `canvas.toBlob` para produzir o arquivo final.
//!
//! # Por que ele saiu do `revelacao-core` — 2026-09-12
//!
//! 🚨 **Para poder ir ao navegador do cliente sem levar uma GPU junto.** Ele
//! morava ao lado do motor de revelação, que depende do `wgpu`; qualquer wasm
//! que quisesse só *converter um arquivo* carregava 3,3 MB de motor de
//! renderização. Numa página que o cliente abre no celular depois de comprar
//! uma foto, isso é a diferença entre o download começar e o cliente desistir.
//!
//! O `revelacao_core::jpeg` continua existindo e apontando para cá: quem já
//! chamava, chama igual.
//!
//! # Exemplo
//!
//! A ida e volta que todo destino faz — bytes do arquivo em pixels, pixels em
//! JPEG:
//!
//! ```rust
//! use image::{DynamicImage, ImageFormat, RgbImage};
//!
//! // Um PNG 4×3 feito na hora, no lugar do arquivo que viria do disco.
//! let mut png = Vec::new();
//! DynamicImage::ImageRgb8(RgbImage::new(4, 3))
//!     .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
//!     .unwrap();
//!
//! let foto = foto_codec::decodificar(&png).unwrap();
//! let jpeg = foto_codec::codificar(&foto, 90).unwrap();
//! assert_eq!(&jpeg[..2], &[0xFF, 0xD8]); // SOI: é JPEG
//!
//! let de_volta = foto_codec::decodificar(&jpeg).unwrap();
//! assert_eq!((de_volta.width(), de_volta.height()), (4, 3));
//! ```

#![deny(missing_docs)]

mod codificador;
pub mod espaco_de_cor;
pub mod orientacao;

use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::error::{ParameterError, ParameterErrorKind};
use image::{
    DynamicImage, ExtendedColorType, ImageBuffer, ImageError, Pixel, PixelWithColorType, Rgba,
};

/// A densidade gravada no cabeçalho JFIF de tudo o que sai daqui.
///
/// # 🖨️ Por que um número no cabeçalho muda a impressão
///
/// 🚨 **DPI não é propriedade dos pixels; é o que o arquivo declara.** Um JPEG
/// sem densidade JFIF é lido como **72 DPI** por boa parte dos programas de
/// laboratório — e uma foto de 4000 px que deveria sair 33 cm abre como 141 cm,
/// com o operador da gráfica "ajustando para caber" no chute. Os pixels são os
/// mesmos; o que muda é a régua que vem com eles.
///
/// 300 é o padrão de impressão fotográfica e a régua que o dono usa para
/// decidir o tamanho da importação (`impressao.ts`, 20×30 cm a 300 DPI). Com
/// isto gravado, o arquivo abre no tamanho certo sozinho.
const DPI: u16 = 300;

/// Abre um arquivo de foto — JPEG, PNG ou **WebP** — em pixels.
///
/// 🔑 **O WebP está aqui porque o acervo passou a guardá-lo** (2026-09-12): é
/// 38% menor que o JPEG na mesma foto, e o que o cliente leva ao laboratório
/// continua sendo JPEG, convertido no navegador dele por este crate.
///
/// Devolve os pixels **como estão no arquivo**: sem a orientação do EXIF e
/// sem converter o espaço de cor. Para abrir a foto como o fotógrafo a viu,
/// use [`orientacao::decodificar_de_pe`].
///
/// # Erros
///
/// [`ImageError`] se o formato não for reconhecido ou os bytes estiverem
/// corrompidos.
pub fn decodificar(bytes: &[u8]) -> Result<DynamicImage, ImageError> {
    image::load_from_memory(bytes)
}

/// A imagem como JPEG, na qualidade pedida (1–100).
///
/// O canal alfa é descartado: JPEG não o tem, e a revelação nunca o altera.
///
/// Sai com 300 DPI no cabeçalho JFIF: sem isso, boa parte dos programas de
/// laboratório lê 72 DPI e a foto abre quatro vezes maior que o tamanho de
/// impressão.
///
/// # Erros
///
/// [`ImageError`] se o codificador recusar a imagem — na prática, um lado
/// maior que os 65 535 px que o JPEG admite.
pub fn codificar(imagem: &DynamicImage, qualidade: u8) -> Result<Vec<u8>, ImageError> {
    // 🔑 **Sem cópia quando os pixels já são RGB8 ou RGBA8** — o caso de toda
    // foto revelada e de todo JPEG aberto. O codificador lê pixel a pixel e
    // descarta o alfa ele mesmo, do mesmo jeito que o `to_rgb8` descartava: o
    // arquivo sai byte a byte igual, sem a cópia da foto inteira (72 MB a 24 MP)
    // só para jogar o quarto canal fora.
    match imagem {
        DynamicImage::ImageRgb8(rgb) => codificar_buffer(rgb, qualidade, COM_DPI),
        DynamicImage::ImageRgba8(rgba) => codificar_buffer(rgba, qualidade, COM_DPI),
        outra => codificar_buffer(&outra.to_rgb8(), qualidade, COM_DPI),
    }
}

/// O mesmo JPEG de [`codificar`], direto de pixels RGBA soltos — sem montar
/// uma [`DynamicImage`] dona deles.
///
/// `rgba` tem `largura × altura × 4` bytes, linha a linha. Serve a quem recebe
/// os pixels emprestados (o navegador, a GPU) e teria de copiá-los inteiros
/// só para chamar [`codificar`].
///
/// # Erros
///
/// [`ImageError`] se `rgba` não tiver exatamente `largura × altura × 4` bytes,
/// ou se o codificador recusar a imagem (um lado maior que 65 535 px).
pub fn codificar_rgba(
    rgba: &[u8],
    largura: u32,
    altura: u32,
    qualidade: u8,
) -> Result<Vec<u8>, ImageError> {
    // `checked_mul`: 65 536² × 4 já não cabe em `u64`, e o wasm não pode
    // entrar em pânico.
    let esperado = u64::from(largura)
        .checked_mul(u64::from(altura))
        .and_then(|p| p.checked_mul(4));
    let buffer = (esperado == Some(rgba.len() as u64))
        .then(|| ImageBuffer::<Rgba<u8>, &[u8]>::from_raw(largura, altura, rgba))
        .flatten()
        .ok_or_else(|| {
            ImageError::Parameter(ParameterError::from_kind(
                ParameterErrorKind::DimensionMismatch,
            ))
        })?;
    codificar_buffer(&buffer, qualidade, COM_DPI)
}

/// O JPEG de [`codificar`] **sem os 300 DPI**: o JFIF diz só a proporção de
/// 1:1, que é o que o `JpegEncoder` do `image` grava quando ninguém pede
/// densidade — e o arquivo é o dele, byte a byte.
///
/// É o do fotolivro, que embute a foto no PDF (lá quem diz o tamanho é a
/// página), e o de quem já gravava pelo `image` sem densidade e não pode ver
/// o arquivo mudar.
///
/// # Erros
///
/// Os de [`codificar`].
pub fn codificar_sem_dpi(imagem: &DynamicImage, qualidade: u8) -> Result<Vec<u8>, ImageError> {
    let sem_dpi = codificador::Densidade {
        unidade: 0,
        x: 1,
        y: 1,
    };
    match imagem {
        DynamicImage::ImageRgb8(rgb) => codificar_buffer(rgb, qualidade, sem_dpi),
        DynamicImage::ImageRgba8(rgba) => codificar_buffer(rgba, qualidade, sem_dpi),
        outra => codificar_buffer(&outra.to_rgb8(), qualidade, sem_dpi),
    }
}

/// Os 300 DPI de tudo o que [`codificar`] grava.
const COM_DPI: codificador::Densidade = codificador::Densidade {
    unidade: 1,
    x: DPI,
    y: DPI,
};

/// O codificador de sempre sobre qualquer buffer de 8 bits, com a densidade
/// pedida.
///
/// 🔑 **O arquivo é o do `JpegEncoder` do `image`, byte a byte**, saído do
/// nosso (`codificador`), que divide a foto entre os núcleos: a 24 MP, o do
/// `image` levava 0,27 s por foto exportada, numa thread só. O lado que o JPEG
/// não admite (0 ou mais de 65 535 px) vai ao `image`, para o erro ser o dele.
fn codificar_buffer<P, C>(
    imagem: &ImageBuffer<P, C>,
    qualidade: u8,
    densidade: codificador::Densidade,
) -> Result<Vec<u8>, ImageError>
where
    P: Pixel<Subpixel = u8> + PixelWithColorType,
    C: std::ops::Deref<Target = [u8]>,
{
    let lados = (
        u16::try_from(imagem.width()),
        u16::try_from(imagem.height()),
    );
    let canais = match P::COLOR_TYPE {
        ExtendedColorType::Rgb8 => Some(3),
        ExtendedColorType::Rgba8 => Some(4),
        _ => None,
    };
    if let ((Ok(largura @ 1..), Ok(altura @ 1..)), Some(canais)) = (lados, canais) {
        return Ok(codificador::codificar(
            imagem.as_raw(),
            canais,
            largura,
            altura,
            qualidade,
            densidade,
        ));
    }
    let mut bytes = Vec::new();
    let mut codificador = JpegEncoder::new_with_quality(&mut bytes, qualidade);
    if densidade == COM_DPI {
        codificador.set_pixel_density(PixelDensity::dpi(DPI));
    }
    codificador.encode_image(imagem)?;
    Ok(bytes)
}

#[cfg(test)]
mod testes {
    use super::*;

    /// O JFIF fica nos bytes 14..18 do APP0: densidade X e Y, big-endian, com a
    /// unidade (1 = polegadas) no byte 13. Lido à mão porque o `image` não
    /// devolve densidade na leitura — e o que importa provar é o que **está no
    /// arquivo**, que é o que o laboratório vai ler.
    #[test]
    fn o_jpeg_declara_300_dpi_para_o_laboratorio() {
        let mut origem = image::RgbaImage::new(8, 8);
        for (x, y, p) in origem.enumerate_pixels_mut() {
            *p = image::Rgba([(x * 30) as u8, (y * 30) as u8, 10, 255]);
        }
        let bytes = codificar(&DynamicImage::ImageRgba8(origem), 90).expect("codifica");

        assert_eq!(
            &bytes[6..11],
            b"JFIF\0",
            "o APP0 do JFIF vem logo depois do SOI"
        );
        assert_eq!(bytes[13], 1, "unidade 1 = pontos por polegada");
        let x = u16::from_be_bytes([bytes[14], bytes[15]]);
        let y = u16::from_be_bytes([bytes[16], bytes[17]]);
        assert_eq!(
            (x, y),
            (300, 300),
            "sem isto a grafica abre a foto como 72 DPI"
        );
    }

    /// O caminho de antes de 2026-10-19: copiar para RGB8 e codificar a cópia.
    fn codificado_pela_copia(imagem: &DynamicImage, qualidade: u8) -> Vec<u8> {
        let rgb = imagem.to_rgb8();
        let mut bytes = Vec::new();
        let mut codificador = JpegEncoder::new_with_quality(&mut bytes, qualidade);
        codificador.set_pixel_density(PixelDensity::dpi(DPI));
        codificador
            .encode(
                &rgb,
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .expect("codifica");
        bytes
    }

    /// 🚨 **Sem a cópia, o arquivo tem de sair byte a byte igual** — inclusive
    /// com alfa que não é 255, que o JPEG descarta (não compõe sobre branco).
    #[test]
    fn sem_a_copia_o_jpeg_sai_identico() {
        let mut rgba = image::RgbaImage::new(19, 13);
        for (x, y, p) in rgba.enumerate_pixels_mut() {
            *p = Rgba([
                (x * 13) as u8,
                (y * 19) as u8,
                ((x ^ y) * 7) as u8,
                (x * y) as u8,
            ]);
        }
        let rgba = DynamicImage::ImageRgba8(rgba);
        let rgb = DynamicImage::ImageRgb8(rgba.to_rgb8());
        let cinza = DynamicImage::ImageLuma8(rgba.to_luma8());
        let largo = DynamicImage::ImageRgb16(rgba.to_rgb16());

        for imagem in [&rgba, &rgb, &cinza, &largo] {
            for qualidade in [1, 75, 100] {
                assert_eq!(
                    codificar(imagem, qualidade).expect("codifica"),
                    codificado_pela_copia(imagem, qualidade),
                    "{:?} q{qualidade}",
                    imagem.color()
                );
            }
        }
        let crus = rgba.to_rgba8().into_raw();
        assert_eq!(
            codificar_rgba(&crus, 19, 13, 90).expect("codifica"),
            codificado_pela_copia(&rgba, 90)
        );
    }

    /// Sem DPI, o arquivo é o do `JpegEncoder` do `image` sem densidade — o
    /// que o fotolivro gravava antes de passar por aqui.
    #[test]
    fn sem_dpi_e_o_jpeg_do_image_sem_densidade() {
        let mut rgba = image::RgbaImage::new(21, 11);
        for (x, y, p) in rgba.enumerate_pixels_mut() {
            *p = Rgba([(x * 11) as u8, (y * 23) as u8, ((x + y) * 5) as u8, 200]);
        }
        let rgba = DynamicImage::ImageRgba8(rgba);
        for imagem in [
            rgba.clone(),
            DynamicImage::ImageRgb8(rgba.to_rgb8()),
            DynamicImage::ImageLuma8(rgba.to_luma8()),
        ] {
            for qualidade in [38, 72, 100] {
                let mut do_image = Vec::new();
                JpegEncoder::new_with_quality(&mut do_image, qualidade)
                    .encode_image(&imagem.to_rgb8())
                    .expect("codifica");
                assert_eq!(
                    codificar_sem_dpi(&imagem, qualidade).expect("codifica"),
                    do_image,
                    "{:?} q{qualidade}",
                    imagem.color()
                );
            }
        }
        assert!(codificar_sem_dpi(&DynamicImage::new_rgb8(70_000, 1), 90).is_err());
    }

    #[test]
    fn pixels_soltos_com_o_tamanho_errado_sao_erro() {
        let crus = vec![0u8; 4 * 4 * 4];
        assert!(codificar_rgba(&crus, 4, 4, 90).is_ok());
        assert!(codificar_rgba(&crus[1..], 4, 4, 90).is_err(), "curto");
        assert!(codificar_rgba(&[0u8; 65], 4, 4, 90).is_err(), "longo");
        assert!(codificar_rgba(&[], 0, 0, 90).is_err(), "sem pixel nenhum");
        assert!(
            codificar_rgba(&[], u32::MAX, u32::MAX, 90).is_err(),
            "não cabe"
        );
    }

    #[test]
    fn o_jpeg_abre_com_as_mesmas_dimensoes() {
        let mut origem = image::RgbaImage::new(9, 7);
        for (x, y, p) in origem.enumerate_pixels_mut() {
            *p = image::Rgba([(x * 20) as u8, (y * 30) as u8, 90, 255]);
        }
        let bytes = codificar(&DynamicImage::ImageRgba8(origem), 90).expect("codifica");
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "todo JPEG começa com SOI");

        let lida = image::load_from_memory(&bytes).expect("o JPEG abre");
        assert_eq!((lida.width(), lida.height()), (9, 7));
    }
}
