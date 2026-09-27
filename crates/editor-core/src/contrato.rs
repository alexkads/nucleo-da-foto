//! O contrato entre o editor e a Revelação — `docs/editor-em-camadas/02-CONTRATO.md`.
//!
//! O editor produz uma [`VersaoEditada`]; a Revelação a resolve como **entrada**
//! (C32), invalida o que era daquela foto e reaplica a receita atual. A janela do
//! editor nunca escreve no estado da Revelação: ela só anuncia a versão.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A versão da imagem editada que a Revelação deve usar como entrada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersaoEditada {
    pub edicao_id: String,
    /// Monotônica e nunca reusada (C33). `0` não existe: é "o bruto".
    pub revisao: u64,
    /// PNG RGB8 sRGB, de pé (C29).
    pub arquivo: PathBuf,
    /// Do arquivo.
    pub sha256: String,
    /// Iguais às da base (C31).
    pub largura: u32,
    pub altura: u32,
    /// A impressão da base neutra de que partiu.
    pub base_sha256: String,
}

impl VersaoEditada {
    /// A versão serve de entrada para uma base com estas dimensões? (C31)
    ///
    /// 🔑 **Pela proporção, com um pixel de folga**: a cópia de trabalho e a
    /// base inteira são a mesma foto em tamanhos diferentes, e a redução
    /// arredonda. Uma proporção diferente de verdade (a foto girada, outra foto)
    /// desloca corte e máscaras, e é recusada.
    pub fn cabe_em(&self, largura: u32, altura: u32) -> bool {
        mesma_proporcao((self.largura, self.altura), (largura, altura))
    }

    /// A chave da cópia de trabalho desta versão no cache de prévias.
    ///
    /// 🔑 A revisão está na chave: uma versão nova nunca é servida pela cópia da
    /// anterior, e a anterior sai sozinha quando ninguém mais a pedir (C17).
    pub fn chave_da_copia(&self, foto_id: &str) -> String {
        chave_da_copia(foto_id, self.revisao)
    }
}

/// `editada:<foto>:<revisão>`.
pub fn chave_da_copia(foto_id: &str, revisao: u64) -> String {
    format!("editada:{foto_id}:{revisao}")
}

/// As duas dimensões descrevem a mesma proporção, com um pixel de folga no lado
/// menor da conta.
pub fn mesma_proporcao(a: (u32, u32), b: (u32, u32)) -> bool {
    let (al, aa) = (a.0 as u64, a.1 as u64);
    let (bl, ba) = (b.0 as u64, b.1 as u64);
    if al == 0 || aa == 0 || bl == 0 || ba == 0 {
        return false;
    }
    // al/aa == bl/ba  ⇔  al·ba == bl·aa, com a folga de um pixel medida na
    // imagem maior.
    let (grande, pequena) = if al * aa >= bl * ba { (a, b) } else { (b, a) };
    let escala = grande.0 as f64 / pequena.0 as f64;
    let esperada = pequena.1 as f64 * escala;
    (esperada - grande.1 as f64).abs() <= escala.max(1.0) + 0.5
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_proporcao_aceita_a_reducao_e_recusa_a_foto_girada() {
        assert!(mesma_proporcao((6000, 4000), (6000, 4000)));
        assert!(mesma_proporcao((6000, 4000), (2560, 1707)));
        assert!(mesma_proporcao((6000, 4000), (2560, 1706)));
        assert!(!mesma_proporcao((6000, 4000), (4000, 6000)), "girada");
        assert!(!mesma_proporcao((6000, 4000), (6000, 3000)));
        assert!(!mesma_proporcao((6000, 4000), (0, 0)));
    }
}
