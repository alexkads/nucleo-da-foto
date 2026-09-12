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
    let imagem = foto_codec::decodificar(bytes)
        .map_err(|e| erro(format!("não foi possível abrir a foto: {e}")))?;
    foto_codec::codificar(&imagem, qualidade.clamp(1, 100))
        .map_err(|e| erro(format!("o JPEG não codificou: {e}")))
}
