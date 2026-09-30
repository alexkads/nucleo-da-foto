//! O RAW da câmera e o DNG do Lightroom, no desktop e no navegador.
//!
//! Duas perguntas, e as duas nascem na importação:
//!
//! 1. **Quais são os pixels?** — [`decodificar`]: os dados do sensor, revelados
//!    sem efeito nenhum (balanço da câmera, matriz de cor, sRGB), de pé. É a
//!    base neutra do [Contrato da Foto]: o bruto não carrega revelação.
//! 2. **Que revelação veio junto?** — [`xmp`]: o XMP que o Lightroom grava
//!    dentro do DNG. Quem o traduz para os controles é quem tem os controles
//!    (`lightroom.rs` no desktop, `lightroom.ts` no site).
//!
//! # 🚨 Nunca a prévia embutida
//!
//! No DNG que sai do Lightroom a prévia JPEG de dentro **já tem a revelação
//! aplicada**. Usá-la como bruto e aplicar os parâmetros do XMP por cima seria
//! revelar duas vezes. Por isso aqui só entra o dado do sensor.
//!
//! # Quem usa
//!
//! - O desktop, como **reserva** da LibRaw: só quando ela recusa (o DNG com
//!   perdas, `infrastructure/src/dng.rs`). Os RAW que a LibRaw já abre
//!   continuam com ela — trocar o decodificador mudaria a cor de todo arquivo
//!   que hoje abre, em silêncio.
//! - O site, pelo `raw-web`: é o único decodificador de RAW que existe lá.
//!
//! [Contrato da Foto]: ../../../recordarfotos-e-commerce/docs/CONTRATO_DA_FOTO.md

mod opcodes;
mod tiff;

use image::metadata::Orientation as Giro;
use image::{DynamicImage, RgbImage};
use rawler::decoders::RawDecodeParams;
use rawler::imgop::develop::RawDevelop;
use rawler::rawsource::RawSource;
use rawler::Orientation;

/// As extensões de RAW que chegam num estúdio.
pub const EXTENSOES: &[&str] = &[
    "dng", "nef", "nrw", "cr2", "cr3", "crw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef",
    "srw", "x3f", "3fr", "iiq", "erf", "kdc", "dcr", "mrw", "mos", "raw", "rwl",
];

/// Este nome de arquivo é um RAW?
pub fn eh_raw(nome: &str) -> bool {
    nome.rsplit_once('.')
        .map(|(_, ext)| EXTENSOES.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Os pixels do sensor, revelados sem efeito, de pé, em sRGB de 8 bits.
pub fn decodificar(bytes: &[u8]) -> Result<RgbImage, String> {
    let fonte = RawSource::new_from_slice(bytes);
    let decodificador =
        rawler::get_decoder(&fonte).map_err(|e| format!("formato de RAW desconhecido: {e}"))?;
    let mut raw = decodificador
        .raw_image(&fonte, &RawDecodeParams::default(), false)
        .map_err(|e| format!("não foi possível ler o RAW: {e}"))?;

    if let Some(lista) =
        tiff::Tiff::novo(bytes).and_then(|t| t.da_imagem_inteira(tiff::OPCODE_LIST_2))
    {
        opcodes::aplicar(&mut raw, lista);
    }

    let orientacao = raw.orientation;
    let revelada = RawDevelop::default()
        .develop_intermediate(&raw)
        .map_err(|e| format!("não foi possível revelar o RAW: {e}"))?
        .to_dynamic_image()
        .ok_or("o RAW revelado não virou imagem")?;
    let mut revelada = match revelada {
        // Sensor monocromático: o motor trabalha em RGB.
        DynamicImage::ImageLuma16(_) => DynamicImage::ImageRgb8(revelada.to_rgb8()),
        outra => outra,
    };
    revelada.apply_orientation(giro(orientacao));
    Ok(revelada.to_rgb8())
}

/// A etiqueta de orientação do rawler na do `image` — os dois seguem o EXIF.
fn giro(o: Orientation) -> Giro {
    match o {
        Orientation::Normal | Orientation::Unknown => Giro::NoTransforms,
        Orientation::HorizontalFlip => Giro::FlipHorizontal,
        Orientation::Rotate180 => Giro::Rotate180,
        Orientation::VerticalFlip => Giro::FlipVertical,
        Orientation::Transpose => Giro::Rotate90FlipH,
        Orientation::Rotate90 => Giro::Rotate90,
        Orientation::Transverse => Giro::Rotate270FlipH,
        Orientation::Rotate270 => Giro::Rotate270,
    }
}

/// O XMP de dentro do arquivo: a etiqueta 700 do TIFF (DNG, e os RAW que são
/// TIFF por dentro) ou, na falta dela, o pacote `<x:xmpmeta>` onde estiver (CR3,
/// JPEG exportado).
///
/// `None` quando não há XMP ou quando ele não traz nada do Camera Raw
/// (`crs:`) — um XMP só com data e autor não é revelação.
pub fn xmp(bytes: &[u8]) -> Option<String> {
    let da_etiqueta = tiff::Tiff::novo(bytes).and_then(|t| t.da_ifd0(tiff::XMP));
    let cru = da_etiqueta.or_else(|| pacote_xmp(bytes))?;
    let texto = String::from_utf8_lossy(cru)
        .trim_end_matches('\0')
        .to_string();
    texto.contains("crs:").then_some(texto)
}

/// A orientação EXIF (1–8) gravada no arquivo, quando ele é TIFF por dentro
/// (DNG, NEF, CR2, ARW, ORF, PEF…). É o que diz em que referencial está o corte
/// do Lightroom, que vem no do sensor.
pub fn orientacao_exif(bytes: &[u8]) -> Option<u16> {
    let valor = tiff::Tiff::novo(bytes)?.numero_da_ifd0(tiff::ORIENTACAO)?;
    (1..=8).contains(&valor).then_some(valor as u16)
}

fn pacote_xmp(bytes: &[u8]) -> Option<&[u8]> {
    const ABRE: &[u8] = b"<x:xmpmeta";
    const FECHA: &[u8] = b"</x:xmpmeta>";
    let inicio = bytes.windows(ABRE.len()).position(|w| w == ABRE)?;
    let fim = bytes[inicio..]
        .windows(FECHA.len())
        .position(|w| w == FECHA)?
        + inicio
        + FECHA.len();
    Some(&bytes[inicio..fim])
}

#[cfg(test)]
mod testes {
    use super::*;

    const XMP_DO_LIGHTROOM: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description crs:Exposure2012="+0.46"/></rdf:RDF></x:xmpmeta>"#;

    #[test]
    fn reconhece_raw_pela_extensao() {
        assert!(eh_raw("DSC_0001.NEF"));
        assert!(eh_raw("_CSF7953.dng"));
        assert!(eh_raw("IMG_1.CR3"));
        assert!(!eh_raw("foto.jpg"));
        assert!(!eh_raw("sem-extensao"));
        assert!(!eh_raw("DSC_0001.NEF.xmp"));
    }

    #[test]
    fn acha_o_xmp_na_etiqueta_700() {
        let bytes = tiff::testes::tiff_com(&[(tiff::XMP, 1, XMP_DO_LIGHTROOM.as_bytes())], &[]);
        assert_eq!(xmp(&bytes).as_deref(), Some(XMP_DO_LIGHTROOM));
    }

    #[test]
    fn acha_o_pacote_xmp_fora_de_tiff() {
        let mut bytes = b"\0\0\0\x18ftypcrx ".to_vec();
        bytes.extend(XMP_DO_LIGHTROOM.as_bytes());
        bytes.extend(b"mais coisa");
        assert_eq!(xmp(&bytes).as_deref(), Some(XMP_DO_LIGHTROOM));
    }

    #[test]
    fn xmp_sem_camera_raw_nao_e_revelacao() {
        let so_data = r#"<x:xmpmeta><rdf:Description xmp:CreateDate="2026"/></x:xmpmeta>"#;
        assert_eq!(xmp(so_data.as_bytes()), None);
    }

    #[test]
    fn arquivo_que_nao_e_raw_da_erro_e_nao_panico() {
        assert!(decodificar(b"isto nao e um raw").is_err());
    }
}
