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

#![deny(missing_docs)]

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
    /// `previstas`: quantas fotos vão entrar — o teto do livro
    /// (`fotolivro::TETO_DO_LIVRO`) se divide por elas. `galeria` e `agendar`
    /// vazios ficam de fora da capa e do fim.
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
        self.adicionar_foto(bytes, nome, levada, link).map_err(erro)
    }

    /// Quantas fotos já entraram no livro — a página usa para a barra de
    /// progresso. Depois de [`gerar`](Self::gerar) a montagem foi consumida e
    /// a conta volta a 0.
    pub fn quantas(&self) -> usize {
        self.montagem.as_ref().map_or(0, |m| m.quantas())
    }

    /// Gera o livro. Uma vez: as fotos já estão dentro do PDF.
    pub fn gerar(&mut self) -> Result<Vec<u8>, JsValue> {
        self.gerar_livro().map_err(erro)
    }
}

/// Os métodos sem o `JsValue`, para os testes nativos.
impl Fotolivro {
    fn adicionar_foto(
        &mut self,
        bytes: &[u8],
        nome: &str,
        levada: bool,
        link: &str,
    ) -> Result<(), String> {
        let montagem = self.montagem.as_mut().ok_or(JA_GERADO)?;
        let imagem = foto_codec::orientacao::decodificar_de_pe(bytes)
            .map_err(|e| format!("a foto {nome} não abriu: {e}"))?;
        montagem.adicionar(
            imagem,
            nome.to_string(),
            levada,
            Some(link.to_string()).filter(|l| !l.is_empty()),
        );
        Ok(())
    }

    fn gerar_livro(&mut self) -> Result<Vec<u8>, String> {
        self.montagem.take().ok_or(JA_GERADO)?.gerar()
    }
}

const JA_GERADO: &str = "o livro já foi gerado";

#[cfg(test)]
mod testes {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    use super::*;

    fn jpeg(largura: u32, altura: u32) -> Vec<u8> {
        let img = RgbImage::from_fn(largura, altura, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        });
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    fn livro(previstas: usize) -> Fotolivro {
        Fotolivro::new(
            "Ensaio da Ana",
            "Jardim Botânico · 12/out/2026",
            "",
            "",
            "estudio.exemplo",
            previstas,
        )
    }

    #[test]
    fn as_fotos_entram_uma_a_uma_e_o_livro_sai_em_pdf() {
        let mut l = livro(2);
        assert_eq!(l.quantas(), 0);
        l.adicionar_foto(&jpeg(300, 200), "IMG_1.jpg", true, "")
            .unwrap();
        l.adicionar_foto(
            &jpeg(200, 300),
            "IMG_2.jpg",
            false,
            "https://estudio.exemplo/f/2",
        )
        .unwrap();
        assert_eq!(l.quantas(), 2);
        let pdf = l.gerar_livro().unwrap();
        assert!(pdf.starts_with(b"%PDF-"), "um PDF");
        assert!(pdf.windows(5).any(|w| w == b"%%EOF"), "inteiro");
    }

    #[test]
    fn o_livro_sai_uma_vez_so() {
        let mut l = livro(1);
        l.adicionar_foto(&jpeg(64, 64), "a.jpg", true, "").unwrap();
        l.gerar_livro().unwrap();
        assert_eq!(l.quantas(), 0, "as fotos foram para o PDF");
        assert_eq!(l.gerar_livro().unwrap_err(), JA_GERADO);
        assert_eq!(
            l.adicionar_foto(&jpeg(64, 64), "b.jpg", true, "")
                .unwrap_err(),
            JA_GERADO
        );
    }

    #[test]
    fn a_foto_que_nao_abre_diz_qual_e_e_nao_entra() {
        let mut l = livro(1);
        let e = l
            .adicionar_foto(b"nao e jpeg", "IMG_9.jpg", true, "")
            .unwrap_err();
        assert!(e.contains("IMG_9.jpg"), "{e}");
        assert_eq!(l.quantas(), 0);
    }
}
