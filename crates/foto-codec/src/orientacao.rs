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

/// Decodifica bytes de imagem já com a orientação aplicada.
pub fn decodificar_de_pe(bytes: &[u8]) -> ImageResult<DynamicImage> {
    de_pe(ImageReader::new(Cursor::new(bytes)).with_guessed_format()?)
}

/// Abre um arquivo de imagem já com a orientação aplicada.
pub fn abrir_de_pe(caminho: impl AsRef<Path>) -> ImageResult<DynamicImage> {
    de_pe(ImageReader::open(caminho)?.with_guessed_format()?)
}

fn de_pe<R: BufRead + Seek>(leitor: ImageReader<R>) -> ImageResult<DynamicImage> {
    let mut decodificador = leitor.into_decoder()?;
    let orientacao = decodificador
        .orientation()
        .unwrap_or(Orientation::NoTransforms);
    let mut foto = DynamicImage::from_decoder(decodificador)?;
    foto.apply_orientation(orientacao);
    Ok(foto)
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Um JPEG 4×2 (deitado) com `Orientation = 6` (girar 90° no horário) no
    /// EXIF — o que a câmera grava quando a foto foi feita em pé.
    fn jpeg_com_orientacao(valor: u16) -> Vec<u8> {
        let imagem =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 2, image::Rgb([90, 90, 90])));
        let mut jpeg = Vec::new();
        imagem
            .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .expect("codificar");
        // APP1 com um TIFF mínimo: II*, IFD0 com uma entrada (0x0112).
        let mut tiff = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0];
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // uma
        tiff.extend_from_slice(&valor.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // sem próximo IFD
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut saida = vec![0xFF, 0xD8, 0xFF, 0xE1];
        saida.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        saida.extend_from_slice(&app1);
        saida.extend_from_slice(&jpeg[2..]);
        saida
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
}
