//! O JPEG que sai do motor — um codificador só, para todo destino.
//!
//! 🔑 A exportação do desktop, o envio ao pós-venda e a revelação no navegador
//! passam por aqui. Dois codificadores dariam dois arquivos diferentes para a
//! mesma foto revelada, e a exportação deixaria de ser a prova do que o site
//! recebe. É por isso que o navegador **não** usa `canvas.toBlob`.

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType, ImageError};

/// A imagem como JPEG, na qualidade pedida (1–100).
///
/// O canal alfa é descartado: JPEG não o tem, e a revelação nunca o altera.
pub fn codificar(imagem: &DynamicImage, qualidade: u8) -> Result<Vec<u8>, ImageError> {
    let rgb = imagem.to_rgb8();
    let mut bytes = Vec::new();
    let mut codificador = JpegEncoder::new_with_quality(&mut bytes, qualidade);
    codificador.encode(&rgb, rgb.width(), rgb.height(), ExtendedColorType::Rgb8)?;
    Ok(bytes)
}

#[cfg(test)]
mod testes {
    use super::*;

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
