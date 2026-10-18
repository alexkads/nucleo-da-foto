//! O RAW da câmera e o DNG do Lightroom, abertos na importação do site.
//!
//! # Por que um wasm só para isto
//!
//! 🔑 **Quem não importa RAW não paga por ele.** O decodificador de RAW (o
//! rawler, com as tabelas de ~1.000 câmeras) é maior que o motor de revelação
//! inteiro; posto dentro do `revelacao-web`, todo operador que abre a Revelação
//! o baixaria. Aqui ele só desce quando a leva tem um RAW.
//!
//! # O que ele devolve
//!
//! O bruto do [Contrato da Foto]: os dados do sensor revelados **sem efeito**
//! (balanço da câmera, matriz de cor, sRGB), de pé. A revelação do Lightroom
//! não entra nos pixels — ela vem pelo [`xmp_do_raw`] e vira **parâmetros**, no
//! JavaScript, pelo mesmo `lightroom.ts` das predefinições.
//!
//! [Contrato da Foto]: ../../../recordarfotos-e-commerce/docs/CONTRATO_DA_FOTO.md

#![deny(missing_docs)]

use wasm_bindgen::prelude::*;

/// Os pixels do RAW, no formato do `decodificar_imagem` do motor: largura e
/// altura em `u32` little-endian, e o RGBA logo depois.
///
/// ⚠️ Uma foto de 24 MP são 96 MB atravessando para o JavaScript: quem chama
/// trabalha uma foto por vez.
#[wasm_bindgen]
pub fn decodificar_raw(bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    console_error_panic_hook::set_once();
    decodificar(bytes).map_err(|e| JsValue::from_str(&e))
}

/// O `decodificar_raw` sem o `JsValue`, para os testes nativos.
fn decodificar(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let imagem = raw_codec::decodificar(bytes)?;
    let (largura, altura) = imagem.dimensions();
    Ok(empacotar(largura, altura, imagem.as_raw()))
}

/// O formato que o JavaScript lê: largura e altura em `u32` little-endian,
/// e o RGBA opaco logo depois.
fn empacotar(largura: u32, altura: u32, rgb: &[u8]) -> Vec<u8> {
    let mut saida = Vec::with_capacity(8 + rgb.len() / 3 * 4);
    saida.extend(largura.to_le_bytes());
    saida.extend(altura.to_le_bytes());
    for p in rgb.as_chunks::<3>().0 {
        saida.extend([p[0], p[1], p[2], 255]);
    }
    saida
}

/// O XMP do Camera Raw de dentro do arquivo, quando há — `undefined` se não.
#[wasm_bindgen]
pub fn xmp_do_raw(bytes: &[u8]) -> Option<String> {
    raw_codec::xmp(bytes)
}

/// A orientação EXIF (1–8) do arquivo, ou 0 quando ele não diz.
#[wasm_bindgen]
pub fn orientacao_do_raw(bytes: &[u8]) -> u16 {
    raw_codec::orientacao_exif(bytes).unwrap_or(0)
}

/// Este nome de arquivo é um RAW? A mesma lista do desktop.
#[wasm_bindgen]
pub fn eh_raw(nome: &str) -> bool {
    raw_codec::eh_raw(nome)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_cabecalho_tem_largura_e_altura_little_endian_e_depois_o_rgba_opaco() {
        // 2×1: um vermelho e um azul.
        let saida = empacotar(2, 1, &[200, 10, 20, 5, 6, 250]);
        assert_eq!(saida[..4], 2u32.to_le_bytes());
        assert_eq!(saida[4..8], 1u32.to_le_bytes());
        assert_eq!(saida[8..], [200, 10, 20, 255, 5, 6, 250, 255]);
    }

    #[test]
    fn dimensoes_grandes_atravessam_inteiras() {
        // 6000 × 4000 (24 MP): os dois u32 chegam ao JavaScript sem cortar.
        let saida = empacotar(6000, 4000, &[]);
        assert_eq!(u32::from_le_bytes(saida[..4].try_into().unwrap()), 6000);
        assert_eq!(u32::from_le_bytes(saida[4..8].try_into().unwrap()), 4000);
    }

    #[test]
    fn arquivo_que_nao_e_raw_e_erro_com_a_mensagem_do_codec() {
        let e = decodificar(b"nao sou um raw").unwrap_err();
        assert!(e.contains("RAW"), "{e}");
    }

    #[test]
    fn xmp_orientacao_e_extensao_sao_os_do_codec() {
        assert_eq!(xmp_do_raw(b"sem xmp nenhum"), None);
        assert_eq!(orientacao_do_raw(b"nao e tiff"), 0, "0: o arquivo não diz");
        assert!(eh_raw("DSC_0001.NEF"));
        assert!(!eh_raw("foto.webp"));
    }
}
