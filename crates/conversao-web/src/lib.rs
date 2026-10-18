//! A foto guardada vira o arquivo que o cliente leva ao laboratório.
//!
//! # Por que existe
//!
//! 🔑 **O acervo guarda WebP; a gráfica quer JPEG** (decisão do dono,
//! 2026-09-12). WebP é 38% menor que JPEG na mesma foto, e num estúdio que
//! guarda duas cópias de cada foto revelada isso é a diferença entre o storage
//! caber e não caber. Mas boa parte dos laboratórios e quiosques de revelação
//! **não abre WebP** — e descobrir isso no balcão, com o cliente segurando o
//! pen drive, custa mais do que o espaço economizado.
//!
//! A saída é converter **no navegador do cliente**, no momento do download: o
//! servidor entrega os bytes que já tem, e o arquivo que chega ao disco dele é
//! JPEG — saído do mesmo codificador do desktop e do editor, com os 300 DPI
//! declarados no cabeçalho.
//!
//! # 🚨 Isto é uma segunda geração de perda, e é deliberado
//!
//! A foto passa por dois codificadores com perda: WebP na importação, JPEG
//! aqui. Duas gerações degradam mais que uma — por isso o operador pode
//! escolher JPEG na importação quando a foto for para impressão grande ou o
//! cliente tiver exigência especial (é o parâmetro de formato da tela de
//! envio). O padrão é WebP porque, no tamanho em que o estúdio imprime, a
//! diferença não aparece no papel; a decisão fica com quem está no balcão.

#![deny(missing_docs)]

use wasm_bindgen::prelude::*;

fn erro(mensagem: impl Into<String>) -> JsValue {
    JsValue::from_str(&mensagem.into())
}

/// Converte qualquer foto que este crate saiba abrir (WebP, JPEG, PNG) no JPEG
/// do estúdio.
///
/// ⚠️ **Um JPEG que entra sai recodificado**, e isso é uma geração a mais de
/// perda à toa: quem chama deve conferir o tipo antes e entregar os bytes
/// originais quando eles já forem JPEG. A tela faz isso — ver o download da
/// galeria do cliente.
#[wasm_bindgen]
pub fn para_jpeg(bytes: &[u8], qualidade: u8) -> Result<Vec<u8>, JsValue> {
    console_error_panic_hook::set_once();
    converter(bytes, qualidade).map_err(erro)
}

/// O `para_jpeg` sem o `JsValue`, para os testes nativos.
fn converter(bytes: &[u8], qualidade: u8) -> Result<Vec<u8>, String> {
    let imagem = foto_codec::decodificar(bytes)
        .map_err(|e| format!("não foi possível abrir a foto: {e}"))?;
    foto_codec::codificar(&imagem, qualidade.clamp(1, 100))
        .map_err(|e| format!("o JPEG não codificou: {e}"))
}

#[cfg(test)]
mod testes {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    use super::*;

    /// Um degradê 48×32 no formato pedido.
    fn foto(formato: ImageFormat) -> Vec<u8> {
        let img = RgbImage::from_fn(48, 32, |x, y| Rgb([(x * 5) as u8, (y * 7) as u8, 128]));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut bytes), formato)
            .unwrap();
        bytes
    }

    fn reabrir(jpeg: &[u8]) -> DynamicImage {
        assert_eq!(&jpeg[..3], [0xFF, 0xD8, 0xFF], "começa como JPEG");
        image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).unwrap()
    }

    #[test]
    fn webp_e_png_viram_jpeg_do_mesmo_tamanho() {
        for formato in [ImageFormat::WebP, ImageFormat::Png] {
            let jpeg = converter(&foto(formato), 90).unwrap();
            let img = reabrir(&jpeg);
            assert_eq!((img.width(), img.height()), (48, 32), "{formato:?}");
        }
    }

    #[test]
    fn o_jpeg_que_sai_declara_300_dpi() {
        // JFIF: "JFIF\0", versão (2 bytes), unidade 1 = pontos por polegada,
        // e as densidades X e Y em big-endian.
        let jpeg = converter(&foto(ImageFormat::Png), 90).unwrap();
        let jfif = jpeg
            .windows(5)
            .position(|w| w == b"JFIF\0")
            .expect("cabeçalho JFIF");
        let densidade = &jpeg[jfif + 7..jfif + 12];
        assert_eq!(densidade, [1, 0x01, 0x2C, 0x01, 0x2C], "300 × 300 DPI");
    }

    #[test]
    fn a_qualidade_e_presa_entre_1_e_100() {
        let png = foto(ImageFormat::Png);
        assert_eq!(converter(&png, 0).unwrap(), converter(&png, 1).unwrap());
        assert_eq!(converter(&png, 255).unwrap(), converter(&png, 100).unwrap());
        assert!(converter(&png, 20).unwrap().len() < converter(&png, 95).unwrap().len());
    }

    #[test]
    fn arquivo_que_nao_e_foto_e_erro_com_mensagem_para_o_cliente() {
        let e = converter(b"isto nao e foto", 90).unwrap_err();
        assert!(e.starts_with("não foi possível abrir a foto"), "{e}");
    }
}
