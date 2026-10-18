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

#![deny(missing_docs)]

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

    const ORIENTACAO_TAG: u16 = tiff::ORIENTACAO;

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

    // ── Um DNG de verdade, mínimo, escrito aqui ──────────────────────────
    //
    // `LinearRaw` (já demosaicado, como o DNG com perdas do Lightroom), sem
    // compressão, 16 bits por canal. A `ColorMatrix1` é a XYZ → sRGB linear:
    // assim o "sensor" já fala sRGB linear e a cor que sai é conferível.

    const LARGURA: u32 = 4;
    const ALTURA: u32 = 2;

    fn curtos(v: &[u16]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn longos(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn racionais_com_sinal(v: &[f64]) -> Vec<u8> {
        v.iter()
            .flat_map(|x| {
                let mut b = ((x * 10000.0).round() as i32).to_le_bytes().to_vec();
                b.extend(10000i32.to_le_bytes());
                b
            })
            .collect()
    }

    /// Metade esquerda vermelha, metade direita branca. `extra`: etiquetas a
    /// mais na IFD0 (orientação, opcodes…); `fotometria`: a interpretação.
    fn dng(fotometria: u16, extra: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        dng_de(LARGURA, 16, fotometria, extra)
    }

    /// Com 16 bits, as amostras vão na ordem do TIFF (little-endian); com
    /// menos, empacotadas do bit mais alto para o mais baixo, como a DNG manda.
    fn dng_de(largura: u32, bits: u32, fotometria: u16, extra: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let cheio = (1u32 << bits) - 1;
        let pixels: Vec<u16> = (0..ALTURA * largura)
            .flat_map(|i| {
                let [r, g, b] = if i % largura < largura / 2 + largura % 2 {
                    [0.61, 0.0, 0.0]
                } else {
                    [0.92, 0.92, 0.92]
                };
                [r, g, b].map(|c: f64| (c * cheio as f64).round() as u16)
            })
            .collect();
        let dados = if bits == 16 {
            curtos(&pixels)
        } else {
            let mut empacotado = Vec::new();
            let (mut acumulado, mut n) = (0u64, 0u32);
            for v in &pixels {
                acumulado = (acumulado << bits) | *v as u64;
                n += bits;
                while n >= 8 {
                    n -= 8;
                    empacotado.push((acumulado >> n) as u8);
                }
            }
            if n > 0 {
                empacotado.push((acumulado << (8 - n)) as u8);
            }
            empacotado
        };
        let xyz_para_srgb = [
            3.2406, -1.5372, -0.4986, -0.9689, 1.8758, 0.0415, 0.0557, -0.2040, 1.0570,
        ];
        let montar = |onde: u32| {
            let mut etiquetas: Vec<(u16, u16, Vec<u8>)> = vec![
                (0x00FE, 4, longos(&[0])),              // NewSubFileType: a imagem inteira
                (0x0100, 4, longos(&[largura])),        // ImageWidth
                (0x0101, 4, longos(&[ALTURA])),         // ImageLength
                (0x0102, 3, curtos(&[bits as u16; 3])), // BitsPerSample
                (0x0103, 3, curtos(&[1])),              // Compression: nenhuma
                (0x0106, 3, curtos(&[fotometria])),     // PhotometricInterpretation
                (0x010F, 2, b"Teste\0".to_vec()),       // Make
                (0x0110, 2, b"Sintetica\0".to_vec()),   // Model
                (0x0111, 4, longos(&[onde])),           // StripOffsets
                (0x0115, 3, curtos(&[3])),              // SamplesPerPixel
                (0x0116, 4, longos(&[ALTURA])),         // RowsPerStrip
                (0x0117, 4, longos(&[dados.len() as u32])), // StripByteCounts
                (0x011C, 3, curtos(&[1])),              // PlanarConfiguration
                (0xC612, 1, vec![1, 4, 0, 0]),          // DNGVersion
                (0xC621, 10, racionais_com_sinal(&xyz_para_srgb)), // ColorMatrix1
                (0xC628, 5, longos(&[1, 1, 1, 1, 1, 1])), // AsShotNeutral 1/1
                (0xC65A, 3, curtos(&[21])),             // CalibrationIlluminant1: D65
                (0xC61D, 4, longos(&[cheio])),          // WhiteLevel
            ];
            etiquetas.extend(extra.iter().cloned());
            etiquetas.sort_by_key(|e| e.0);
            let refs: Vec<(u16, u16, &[u8])> = etiquetas
                .iter()
                .map(|(t, ti, v)| (*t, *ti, v.as_slice()))
                .collect();
            tiff::testes::tiff_com(&refs, &[])
        };
        // O deslocamento dos pixels é o tamanho do TIFF: monta uma vez para
        // medir (o valor cabe na entrada, o tamanho não muda) e de novo.
        let tamanho = montar(0).len() as u32;
        let mut bytes = montar(tamanho);
        assert_eq!(bytes.len() as u32, tamanho);
        bytes.extend(dados);
        bytes
    }

    fn vermelho(p: &image::Rgb<u8>) -> bool {
        p[0] > 150 && p[1] < 60 && p[2] < 60
    }

    fn branco(p: &image::Rgb<u8>) -> bool {
        p.0.iter().all(|c| *c > 200)
    }

    #[test]
    fn decodifica_um_dng_linear_com_a_cor_do_sensor() {
        let foto = decodificar(&dng(34892, &[])).unwrap();
        assert_eq!(foto.dimensions(), (LARGURA, ALTURA));
        for y in 0..ALTURA {
            assert!(vermelho(foto.get_pixel(0, y)), "{:?}", foto.get_pixel(0, y));
            assert!(
                branco(foto.get_pixel(LARGURA - 1, y)),
                "{:?}",
                foto.get_pixel(LARGURA - 1, y)
            );
        }
    }

    #[test]
    fn decodifica_um_dng_de_14_bits_com_linhas_que_nao_comecam_num_byte() {
        // 3 pixels × 3 canais × 14 bits = 126 bits por linha: a segunda linha
        // começa no meio de um byte. O rawler pulava esses bits com a bomba
        // vazia — pânico no debug, lixo no release.
        let foto = decodificar(&dng_de(3, 14, 34892, &[])).unwrap();
        assert_eq!(foto.dimensions(), (3, ALTURA));
        for y in 0..ALTURA {
            assert!(
                vermelho(foto.get_pixel(0, y)),
                "({y}) {:?}",
                foto.get_pixel(0, y)
            );
            assert!(
                vermelho(foto.get_pixel(1, y)),
                "({y}) {:?}",
                foto.get_pixel(1, y)
            );
            assert!(
                branco(foto.get_pixel(2, y)),
                "({y}) {:?}",
                foto.get_pixel(2, y)
            );
        }
    }

    #[test]
    fn a_foto_sai_de_pe_pela_orientacao_do_arquivo() {
        // 6 = girar 90° no sentido horário: a esquerda vermelha vai para cima.
        let bytes = dng(34892, &[(ORIENTACAO_TAG, 3, curtos(&[6]))]);
        assert_eq!(orientacao_exif(&bytes), Some(6));
        let foto = decodificar(&bytes).unwrap();
        assert_eq!(foto.dimensions(), (ALTURA, LARGURA), "de pé");
        assert!(vermelho(foto.get_pixel(0, 0)), "{:?}", foto.get_pixel(0, 0));
        assert!(
            branco(foto.get_pixel(0, LARGURA - 1)),
            "{:?}",
            foto.get_pixel(0, LARGURA - 1)
        );
    }

    #[test]
    fn a_opcode_list_2_do_arquivo_e_aplicada_antes_de_revelar() {
        // Um polinômio constante 0 em todos os planos apaga a foto: se a
        // lista não fosse aplicada, o branco continuaria branco.
        let lista =
            opcodes::testes::lista_de_polinomios(ALTURA, LARGURA, &[[0.0; 4], [0.0; 4], [0.0; 4]]);
        let foto = decodificar(&dng(34892, &[(tiff::OPCODE_LIST_2, 7, lista)])).unwrap();
        assert!(
            foto.pixels().all(|p| p.0.iter().all(|c| *c < 8)),
            "tudo preto"
        );
    }

    #[test]
    fn dng_com_fotometria_que_o_rawler_nao_conhece_da_erro_e_nao_panico() {
        // RGB (2) num DNG: o rawler 0.8.0 tinha `todo!()` aqui, e no site (wasm,
        // `panic = "abort"`) um arquivo estranho derrubava o módulo inteiro.
        assert!(decodificar(&dng(2, &[])).is_err(), "sem CFAPattern");
        // Com o CFAPattern, a leitura chega à fotometria.
        let com_cfa = dng(
            2,
            &[
                (0x828D, 3, curtos(&[2, 2])),  // CFARepeatPatternDim
                (0x828E, 1, vec![0, 1, 1, 2]), // CFAPattern: RGGB
            ],
        );
        let e = decodificar(&com_cfa).unwrap_err();
        assert!(e.contains("fotometria"), "{e}");
    }

    #[test]
    fn dng_com_entrelacamento_que_o_rawler_nao_conhece_da_erro_e_nao_panico() {
        // Só o 1×1 e o 2×2 existem; um 3×3 era `todo!()`.
        let bytes = dng(
            34892,
            &[
                (50975, 3, curtos(&[3])), // RowInterleaveFactor
                (52547, 3, curtos(&[3])), // ColumnInterleaveFactor
            ],
        );
        let e = decodificar(&bytes).unwrap_err();
        assert!(e.contains("entrelaçamento"), "{e}");
    }

    #[test]
    fn a_orientacao_exif_so_vale_de_1_a_8_e_so_em_tiff() {
        for v in 1..=8u16 {
            let t = tiff::testes::tiff_com(&[(ORIENTACAO_TAG, 3, &v.to_le_bytes())], &[]);
            assert_eq!(orientacao_exif(&t), Some(v));
        }
        for v in [0u16, 9] {
            let t = tiff::testes::tiff_com(&[(ORIENTACAO_TAG, 3, &v.to_le_bytes())], &[]);
            assert_eq!(orientacao_exif(&t), None, "{v}");
        }
        assert_eq!(orientacao_exif(b"\xFF\xD8\xFF"), None);
        assert_eq!(orientacao_exif(&tiff::testes::tiff_com(&[], &[])), None);
    }

    #[test]
    fn cada_orientacao_do_rawler_vira_a_mesma_do_exif() {
        let pares = [
            (Orientation::Normal, Giro::NoTransforms),
            (Orientation::Unknown, Giro::NoTransforms),
            (Orientation::HorizontalFlip, Giro::FlipHorizontal),
            (Orientation::Rotate180, Giro::Rotate180),
            (Orientation::VerticalFlip, Giro::FlipVertical),
            (Orientation::Transpose, Giro::Rotate90FlipH),
            (Orientation::Rotate90, Giro::Rotate90),
            (Orientation::Transverse, Giro::Rotate270FlipH),
            (Orientation::Rotate270, Giro::Rotate270),
        ];
        for (o, g) in pares {
            assert_eq!(giro(o), g, "{o:?}");
            if o != Orientation::Unknown {
                // O número EXIF dos dois lados é o mesmo.
                assert_eq!(Giro::from_exif(o.to_u16() as u8), Some(g), "{o:?}");
            }
        }
    }

    #[test]
    fn o_xmp_da_etiqueta_perde_os_nulos_do_fim() {
        let mut com_nulos = XMP_DO_LIGHTROOM.as_bytes().to_vec();
        com_nulos.extend([0, 0, 0]);
        let bytes = tiff::testes::tiff_com(&[(tiff::XMP, 1, &com_nulos)], &[]);
        assert_eq!(xmp(&bytes).as_deref(), Some(XMP_DO_LIGHTROOM));
    }

    #[test]
    fn pacote_xmp_sem_fechamento_nao_e_xmp() {
        let aberto = r#"<x:xmpmeta><rdf:Description crs:Exposure2012="+1"/>"#;
        assert_eq!(xmp(aberto.as_bytes()), None);
        assert_eq!(xmp(b""), None);
    }

    #[test]
    fn as_extensoes_valem_em_qualquer_caixa_e_so_a_ultima_conta() {
        for ext in EXTENSOES {
            assert!(eh_raw(&format!("a.{ext}")));
            assert!(eh_raw(&format!("a.{}", ext.to_uppercase())));
        }
        assert!(eh_raw("viagem.2026.nef"));
        assert!(!eh_raw("nef"));
        assert!(!eh_raw(".jpg"));
    }

    /// O arquivo cortado — cartão com defeito, envio interrompido — em
    /// todo tamanho possível: erro (ou foto), nunca pânico.
    #[test]
    fn dng_cortado_em_qualquer_ponto_da_erro_e_nao_panico() {
        let inteiro = dng(34892, &[(ORIENTACAO_TAG, 3, curtos(&[6]))]);
        let em_panico: Vec<usize> = (0..inteiro.len())
            .filter(|n| {
                let pedaco = &inteiro[..*n];
                std::panic::catch_unwind(|| {
                    let _ = decodificar(pedaco);
                    let _ = xmp(pedaco);
                    let _ = orientacao_exif(pedaco);
                })
                .is_err()
            })
            .collect();
        assert!(
            em_panico.is_empty(),
            "pânico com {} bytes de {}",
            em_panico.len(),
            inteiro.len()
        );
    }

    /// Um byte trocado em cada posição do arquivo, com valores que viram
    /// tamanhos e deslocamentos absurdos: erro (ou foto), nunca pânico.
    #[test]
    fn dng_com_um_byte_trocado_em_qualquer_ponto_da_erro_e_nao_panico() {
        let inteiro = dng(34892, &[]);
        let cabecalho = inteiro.len() - (LARGURA * ALTURA * 6) as usize; // sem os pixels
        let mut em_panico = Vec::new();
        for i in 0..cabecalho {
            for v in [0x00, 0x01, 0x7F, 0xFF] {
                let mut torto = inteiro.clone();
                torto[i] = v;
                if std::panic::catch_unwind(|| decodificar(&torto)).is_err() {
                    em_panico.push((i, v));
                }
            }
        }
        assert!(
            em_panico.is_empty(),
            "pânico em (posição, byte): {em_panico:?}"
        );
    }

    #[test]
    fn arquivo_que_nao_e_raw_da_erro_e_nao_panico() {
        assert!(decodificar(b"isto nao e um raw").is_err());
    }
}
