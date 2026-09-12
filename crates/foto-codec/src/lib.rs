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

use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::{DynamicImage, ExtendedColorType, ImageError};

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
pub fn decodificar(bytes: &[u8]) -> Result<DynamicImage, ImageError> {
    image::load_from_memory(bytes)
}

/// A imagem como JPEG, na qualidade pedida (1–100).
///
/// O canal alfa é descartado: JPEG não o tem, e a revelação nunca o altera.
///
/// Sai com [`DPI`] no cabeçalho — ver a nota lá sobre o que isso muda no
/// laboratório.
pub fn codificar(imagem: &DynamicImage, qualidade: u8) -> Result<Vec<u8>, ImageError> {
    let rgb = imagem.to_rgb8();
    let mut bytes = Vec::new();
    let mut codificador = JpegEncoder::new_with_quality(&mut bytes, qualidade);
    codificador.set_pixel_density(PixelDensity::dpi(DPI));
    codificador.encode(&rgb, rgb.width(), rgb.height(), ExtendedColorType::Rgb8)?;
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
