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

use wasm_bindgen::prelude::*;

/// Os pixels do RAW, no formato do `decodificar_imagem` do motor: largura e
/// altura em `u32` little-endian, e o RGBA logo depois.
///
/// ⚠️ Uma foto de 24 MP são 96 MB atravessando para o JavaScript: quem chama
/// trabalha uma foto por vez.
#[wasm_bindgen]
pub fn decodificar_raw(bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    console_error_panic_hook::set_once();
    let imagem = raw_codec::decodificar(bytes).map_err(|e| JsValue::from_str(&e))?;
    let (largura, altura) = imagem.dimensions();
    let mut saida = Vec::with_capacity(8 + (largura * altura * 4) as usize);
    saida.extend(largura.to_le_bytes());
    saida.extend(altura.to_le_bytes());
    for p in imagem.pixels() {
        saida.extend([p[0], p[1], p[2], 255]);
    }
    Ok(saida)
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
