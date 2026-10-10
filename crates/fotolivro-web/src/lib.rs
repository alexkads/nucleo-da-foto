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
    montagem: Option<fotolivro::Montagem>,
}

#[wasm_bindgen]
impl Fotolivro {
    /// `previstas`: quantas fotos vão entrar — o teto de 10 MB do livro se
    /// divide por elas.
    #[wasm_bindgen(constructor)]
    pub fn new(
        titulo: &str,
        lugar_e_data: &str,
        galeria: &str,
        agendar: &str,
        site: &str,
        previstas: usize,
    ) -> Fotolivro {
        console_error_panic_hook::set_once();
        let capa = fotolivro::Capa {
            titulo: titulo.to_string(),
            lugar_e_data: lugar_e_data.to_string(),
            galeria: Some(galeria.to_string()).filter(|g| !g.is_empty()),
            agendar: Some(agendar.to_string()).filter(|g| !g.is_empty()),
            site: site.to_string(),
        };
        Fotolivro {
            montagem: Some(fotolivro::Montagem::nova(capa, previstas)),
        }
    }

    /// 🧠 A foto entra no livro na hora, já em JPEG, e a aberta é solta: o
    /// celular nunca segura as fotos do ensaio abertas ao mesmo tempo.
    pub fn adicionar(
        &mut self,
        bytes: &[u8],
        nome: &str,
        levada: bool,
        link: &str,
    ) -> Result<(), JsValue> {
        let montagem = self
            .montagem
            .as_mut()
            .ok_or_else(|| erro("o livro já foi gerado"))?;
        let imagem = foto_codec::orientacao::decodificar_de_pe(bytes)
            .map_err(|e| erro(format!("a foto {nome} não abriu: {e}")))?;
        montagem.adicionar(
            imagem,
            nome.to_string(),
            levada,
            Some(link.to_string()).filter(|l| !l.is_empty()),
        );
        Ok(())
    }

    pub fn quantas(&self) -> usize {
        self.montagem.as_ref().map_or(0, |m| m.quantas())
    }

    /// Gera o livro. Uma vez: as fotos já estão dentro do PDF.
    pub fn gerar(&mut self) -> Result<Vec<u8>, JsValue> {
        self.montagem
            .take()
            .ok_or_else(|| erro("o livro já foi gerado"))?
            .gerar()
            .map_err(erro)
    }
}
