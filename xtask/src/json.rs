//! Um leitor de JSON mínimo, para o `cargo metadata`.
//!
//! O `xtask` só usa a biblioteca padrão: ele compila em segundos antes de qualquer checagem, sem
//! esperar o `serde_json`. Lê o JSON inteiro (objetos, listas, textos com escapes, números,
//! `true`, `false` e `null`), mas não o valida além do necessário para não se perder.

/// Um valor JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Nulo,
    Logico(bool),
    /// O número como veio escrito: o `xtask` não faz conta com eles.
    Numero(String),
    Texto(String),
    Lista(Vec<Json>),
    /// Os pares na ordem do arquivo.
    Objeto(Vec<(String, Json)>),
}

impl Json {
    /// Lê um documento JSON inteiro.
    pub fn ler(texto: &str) -> Result<Json, String> {
        let mut leitor = Leitor {
            chars: texto.chars().collect(),
            pos: 0,
        };
        let valor = leitor.valor()?;
        leitor.espacos();
        if leitor.pos != leitor.chars.len() {
            return Err(format!("sobrou texto na posição {}", leitor.pos));
        }
        Ok(valor)
    }

    /// O valor de `chave`, se este é um objeto que a tem.
    pub fn campo(&self, chave: &str) -> Option<&Json> {
        match self {
            Json::Objeto(pares) => pares
                .iter()
                .find(|(nome, _)| nome == chave)
                .map(|(_, valor)| valor),
            _ => None,
        }
    }

    pub fn texto(&self) -> Option<&str> {
        match self {
            Json::Texto(texto) => Some(texto),
            _ => None,
        }
    }

    /// Os itens, se é uma lista; uma lista vazia para qualquer outra coisa.
    pub fn itens(&self) -> &[Json] {
        match self {
            Json::Lista(itens) => itens,
            _ => &[],
        }
    }
}

struct Leitor {
    chars: Vec<char>,
    pos: usize,
}

impl Leitor {
    fn espacos(&mut self) {
        while self.chars.get(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn proximo(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        self.pos += 1;
        c
    }

    fn espera(&mut self, esperado: char) -> Result<(), String> {
        self.espacos();
        match self.proximo() {
            Some(c) if c == esperado => Ok(()),
            outro => Err(format!(
                "esperava {esperado:?} na posição {}, veio {outro:?}",
                self.pos - 1
            )),
        }
    }

    fn palavra(&mut self, palavra: &str, valor: Json) -> Result<Json, String> {
        for esperado in palavra.chars() {
            if self.proximo() != Some(esperado) {
                return Err(format!("esperava {palavra} na posição {}", self.pos - 1));
            }
        }
        Ok(valor)
    }

    fn valor(&mut self) -> Result<Json, String> {
        self.espacos();
        match self.chars.get(self.pos).copied() {
            Some('{') => self.objeto(),
            Some('[') => self.lista(),
            Some('"') => self.texto().map(Json::Texto),
            Some('t') => self.palavra("true", Json::Logico(true)),
            Some('f') => self.palavra("false", Json::Logico(false)),
            Some('n') => self.palavra("null", Json::Nulo),
            Some(c) if c == '-' || c.is_ascii_digit() => {
                let inicio = self.pos;
                while self
                    .chars
                    .get(self.pos)
                    .is_some_and(|c| "+-.eE".contains(*c) || c.is_ascii_digit())
                {
                    self.pos += 1;
                }
                Ok(Json::Numero(self.chars[inicio..self.pos].iter().collect()))
            }
            outro => Err(format!(
                "valor inesperado na posição {}: {outro:?}",
                self.pos
            )),
        }
    }

    fn objeto(&mut self) -> Result<Json, String> {
        self.espera('{')?;
        let mut pares = Vec::new();
        self.espacos();
        if self.chars.get(self.pos) == Some(&'}') {
            self.pos += 1;
            return Ok(Json::Objeto(pares));
        }
        loop {
            self.espacos();
            let chave = self.texto()?;
            self.espera(':')?;
            pares.push((chave, self.valor()?));
            self.espacos();
            match self.proximo() {
                Some(',') => continue,
                Some('}') => return Ok(Json::Objeto(pares)),
                outro => {
                    return Err(format!(
                        "esperava , ou }} na posição {}, veio {outro:?}",
                        self.pos - 1
                    ))
                }
            }
        }
    }

    fn lista(&mut self) -> Result<Json, String> {
        self.espera('[')?;
        let mut itens = Vec::new();
        self.espacos();
        if self.chars.get(self.pos) == Some(&']') {
            self.pos += 1;
            return Ok(Json::Lista(itens));
        }
        loop {
            itens.push(self.valor()?);
            self.espacos();
            match self.proximo() {
                Some(',') => continue,
                Some(']') => return Ok(Json::Lista(itens)),
                outro => {
                    return Err(format!(
                        "esperava , ou ] na posição {}, veio {outro:?}",
                        self.pos - 1
                    ))
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let hex: String = (0..4).filter_map(|_| self.proximo()).collect();
        u32::from_str_radix(&hex, 16)
            .map_err(|_| format!("\\u inválido antes da posição {}", self.pos))
    }

    fn texto(&mut self) -> Result<String, String> {
        self.espera('"')?;
        let mut texto = String::new();
        loop {
            match self.proximo() {
                None => return Err("texto sem fim".to_owned()),
                Some('"') => return Ok(texto),
                Some('\\') => match self.proximo() {
                    Some('n') => texto.push('\n'),
                    Some('t') => texto.push('\t'),
                    Some('r') => texto.push('\r'),
                    Some('b') => texto.push('\u{8}'),
                    Some('f') => texto.push('\u{c}'),
                    Some('u') => {
                        let mut codigo = self.hex4()?;
                        // Fora do plano básico, o JSON escreve o par de substitutos: \ud83d\udcf7.
                        if (0xD800..0xDC00).contains(&codigo)
                            && self.chars.get(self.pos) == Some(&'\\')
                        {
                            self.pos += 2;
                            let baixo = self.hex4()?;
                            codigo = 0x10000 + ((codigo - 0xD800) << 10) + (baixo - 0xDC00);
                        }
                        texto.push(char::from_u32(codigo).unwrap_or('\u{FFFD}'));
                    }
                    Some(outro) => texto.push(outro),
                    None => return Err("escape sem fim".to_owned()),
                },
                Some(c) => texto.push(c),
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn le_o_que_o_cargo_metadata_escreve() {
        let json = r#"{"packages":[{"name":"raw-codec","dependencies":[{"name":"rawler","path":"/a/vendor/rawler","optional":false,"req":"*"}],"rust_version":null,"n":-1.5e3}],
            "target_directory":"C:\\Users\\a \"b\"\u00e9","version":1}"#;
        let valor = Json::ler(json).unwrap();
        assert_eq!(
            valor.campo("target_directory").and_then(Json::texto),
            Some("C:\\Users\\a \"b\"é")
        );
        let pacote = &valor.campo("packages").unwrap().itens()[0];
        assert_eq!(
            pacote.campo("name").and_then(Json::texto),
            Some("raw-codec")
        );
        let dependencia = &pacote.campo("dependencies").unwrap().itens()[0];
        assert_eq!(
            dependencia.campo("path").and_then(Json::texto),
            Some("/a/vendor/rawler")
        );
        assert_eq!(dependencia.campo("optional"), Some(&Json::Logico(false)));
        assert_eq!(pacote.campo("rust_version"), Some(&Json::Nulo));
        assert_eq!(pacote.campo("n"), Some(&Json::Numero("-1.5e3".to_owned())));
        assert_eq!(valor.campo("nada"), None);
    }

    #[test]
    fn le_o_par_de_substitutos_e_os_vazios() {
        assert_eq!(
            Json::ler(r#""\ud83d\udcf7""#).unwrap(),
            Json::Texto("📷".to_owned())
        );
        assert_eq!(Json::ler(" { } ").unwrap(), Json::Objeto(Vec::new()));
        assert_eq!(Json::ler("[]").unwrap(), Json::Lista(Vec::new()));
    }

    #[test]
    fn recusa_o_que_nao_e_json() {
        assert!(Json::ler(r#"{"a":1"#).is_err());
        assert!(Json::ler(r#"{"a" 1}"#).is_err());
        assert!(Json::ler("[1] 2").is_err());
        assert!(Json::ler(r#""sem fim"#).is_err());
    }
}
