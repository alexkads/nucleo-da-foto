//! O fotolivro na galeria do cliente — o mesmo livro do balcão, gerado no
//! navegador dele.
//!
//! # Por que existe
//!
//! 📖 *"No site tem que ter a opção de gerar o book sem as fotos não
//! adquiridas, pois o cliente pode gostar dessa idéia"* (dono, 2026-10-09). O
//! balcão manda o livro com as disponíveis marcadas, para vender; o cliente
//! pode querer o **dele** — só as que já são suas, limpas — para guardar,
//! mostrar à família, imprimir.
//!
//! 🔑 **Um livro só.** A diagramação, a moldura e os links moram no crate
//! `fotolivro`; este é a casca do wasm-bindgen. Duas implementações do livro
//! divergiriam na primeira mudança de molde.
//!
//! Sem GPU e sem rede: a página baixa as prévias (limpas as dele, marcadas as
//! outras — quem decide é o servidor), entrega os bytes aqui uma a uma e
//! recebe o PDF.

use wasm_bindgen::prelude::*;

fn erro(mensagem: impl Into<String>) -> JsValue {
    JsValue::from_str(&mensagem.into())
}

/// O livro em montagem: as fotos entram uma a uma, na ordem da galeria.
#[wasm_bindgen]
pub struct Fotolivro {
    capa: fotolivro::Capa,
    fotos: Vec<fotolivro::FotoDaFolha>,
}

#[wasm_bindgen]
impl Fotolivro {
    /// `galeria` ou `agendar` vazios = sem o botão deles.
    #[wasm_bindgen(constructor)]
    pub fn new(
        titulo: &str,
        lugar_e_data: &str,
        galeria: &str,
        agendar: &str,
        site: &str,
    ) -> Fotolivro {
        console_error_panic_hook::set_once();
        Fotolivro {
            capa: fotolivro::Capa {
                titulo: titulo.to_string(),
                lugar_e_data: lugar_e_data.to_string(),
                galeria: Some(galeria.to_string()).filter(|g| !g.is_empty()),
                agendar: Some(agendar.to_string()).filter(|g| !g.is_empty()),
                site: site.to_string(),
            },
            fotos: Vec::new(),
        }
    }

    /// Uma foto: os bytes como o site os serve (JPEG, WebP, PNG), o nome, se
    /// já é dele e para onde o toque leva (`""` = sem link).
    pub fn adicionar(
        &mut self,
        bytes: &[u8],
        nome: &str,
        levada: bool,
        link: &str,
    ) -> Result<(), JsValue> {
        let imagem = foto_codec::orientacao::decodificar_de_pe(bytes)
            .map_err(|e| erro(format!("a foto {nome} não abriu: {e}")))?;
        self.fotos.push(fotolivro::FotoDaFolha {
            imagem,
            nome: nome.to_string(),
            levada,
            link: Some(link.to_string()).filter(|l| !l.is_empty()),
        });
        Ok(())
    }

    pub fn quantas(&self) -> usize {
        self.fotos.len()
    }

    /// O PDF.
    pub fn gerar(&self) -> Result<Vec<u8>, JsValue> {
        fotolivro::gerar(&self.capa, &self.fotos).map_err(erro)
    }
}
