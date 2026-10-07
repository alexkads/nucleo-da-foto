//! A camada de pixels, dividida em tiles.
//!
//! 🔑 **Esparsa.** Tile que nunca foi pintado não existe, e ler dele é ler
//! transparente. Um retoque pequeno numa foto de 24 MP ocupa alguns tiles, e não
//! os 96 MB de uma camada cheia — na memória, no desfazer e no disco.
//!
//! 🔑 **Conteúdo fora da foto** (etapa 14, formato 7): a posição do tile tem
//! sinal, e a camada guarda tiles à esquerda, acima, à direita e abaixo da
//! foto — o que o Mover e o ⌘T levam para fora e trazem de volta sem perder.
//! [`CamadaDePixels::existentes`] só mostra os de dentro (quem compõe, desenha
//! e seleciona não precisa saber do resto); [`CamadaDePixels::todos`] mostra
//! todos (quem move, transforma, desfaz e grava).
//!
//! 🔑 **Cópia na escrita.** Cada tile é um `Arc`: o desfazer guarda o `Arc` de
//! antes do traço e a camada escreve numa cópia. Guardar o estado anterior custa
//! um contador, e não 256 KB.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::retangulo::Retangulo;

/// O lado do tile, em pixels.
pub const LADO_DO_TILE: u32 = 256;

/// Os bytes de um tile: `LADO × LADO` pixels RGBA8 de **alfa reto** (não
/// pré-multiplicado) — a cor de um pixel meio apagado continua sendo a cor que
/// foi pintada.
pub const BYTES_DO_TILE: usize = (LADO_DO_TILE * LADO_DO_TILE * 4) as usize;

pub type Tile = Arc<Vec<u8>>;

/// Onde um tile fica: coluna e linha, em tiles — negativas, ou além da última,
/// quando o tile está fora da foto.
pub type Posicao = (i32, i32);

#[derive(Clone, Debug, PartialEq)]
pub struct CamadaDePixels {
    largura: u32,
    altura: u32,
    tiles: BTreeMap<Posicao, Tile>,
}

impl CamadaDePixels {
    /// Transparente, do tamanho da base.
    pub fn nova(largura: u32, altura: u32) -> Self {
        Self {
            largura,
            altura,
            tiles: BTreeMap::new(),
        }
    }

    /// A imagem inteira como camada **opaca** — a camada da fotografia base:
    /// um tile por posição da foto, a parte do tile da borda que passa da foto
    /// transparente. Faixas de tiles em threads (24 MP são 96 MB de tiles).
    pub fn da_imagem(imagem: &image::RgbImage) -> Self {
        let (largura, altura) = imagem.dimensions();
        let mut camada = Self::nova(largura, altura);
        let (colunas, linhas) = (camada.colunas(), camada.linhas());
        let fonte = imagem.as_raw();
        let faixa = |l: u32| -> Vec<(Posicao, Tile)> {
            (0..colunas)
                .map(|c| {
                    let mut tile = vec![0u8; BYTES_DO_TILE];
                    let (x0, y0) = (c * LADO_DO_TILE, l * LADO_DO_TILE);
                    let w = LADO_DO_TILE.min(largura - x0) as usize;
                    for ly in 0..LADO_DO_TILE.min(altura - y0) {
                        let inicio = ((y0 + ly) as usize * largura as usize + x0 as usize) * 3;
                        let linha = &fonte[inicio..inicio + w * 3];
                        let destino = &mut tile[indice(0, ly)..indice(0, ly) + w * 4];
                        for (d, o) in destino.chunks_exact_mut(4).zip(linha.chunks_exact(3)) {
                            d.copy_from_slice(&[o[0], o[1], o[2], 255]);
                        }
                    }
                    ((c as i32, l as i32), Arc::new(tile))
                })
                .collect()
        };
        let faixas: Vec<Vec<(Posicao, Tile)>> = std::thread::scope(|escopo| {
            let tarefas: Vec<_> = (0..linhas)
                .map(|l| {
                    let faixa = &faixa;
                    escopo.spawn(move || faixa(l))
                })
                .collect();
            tarefas
                .into_iter()
                .map(|t| t.join().unwrap_or_default())
                .collect()
        });
        for (posicao, tile) in faixas.into_iter().flatten() {
            camada.tiles.insert(posicao, tile);
        }
        camada
    }

    pub fn largura(&self) -> u32 {
        self.largura
    }

    pub fn altura(&self) -> u32 {
        self.altura
    }

    pub fn colunas(&self) -> u32 {
        self.largura.div_ceil(LADO_DO_TILE)
    }

    pub fn linhas(&self) -> u32 {
        self.altura.div_ceil(LADO_DO_TILE)
    }

    pub fn tile(&self, posicao: Posicao) -> Option<&Tile> {
        self.tiles.get(&posicao)
    }

    /// Troca um tile inteiro — o desfazer e a reabertura. `None` apaga.
    pub fn definir(&mut self, posicao: Posicao, tile: Option<Tile>) {
        match tile {
            Some(tile) => {
                debug_assert_eq!(tile.len(), BYTES_DO_TILE);
                self.tiles.insert(posicao, tile);
            }
            None => {
                self.tiles.remove(&posicao);
            }
        }
    }

    /// O tile para escrever, criado transparente se faltar e **copiado** se
    /// alguém (o desfazer) ainda segura o de antes.
    pub fn tile_mut(&mut self, posicao: Posicao) -> &mut Vec<u8> {
        let tile = self
            .tiles
            .entry(posicao)
            .or_insert_with(|| Arc::new(vec![0; BYTES_DO_TILE]));
        Arc::make_mut(tile)
    }

    /// Tira o tile que ficou todo transparente — a camada volta a ser esparsa
    /// depois da borracha.
    pub fn enxugar(&mut self, posicao: Posicao) {
        if self
            .tiles
            .get(&posicao)
            .is_some_and(|t| t.iter().skip(3).step_by(4).all(|a| *a == 0))
        {
            self.tiles.remove(&posicao);
        }
    }

    /// O tile cai dentro da foto.
    pub fn dentro(&self, posicao: Posicao) -> bool {
        posicao.0 >= 0
            && posicao.1 >= 0
            && (posicao.0 as u32) < self.colunas()
            && (posicao.1 as u32) < self.linhas()
    }

    /// Os tiles existentes **dentro da foto** — o que se vê.
    pub fn existentes(&self) -> impl Iterator<Item = (&Posicao, &Tile)> {
        let (colunas, linhas) = (self.colunas() as i32, self.linhas() as i32);
        self.tiles
            .iter()
            .filter(move |(p, _)| p.0 >= 0 && p.1 >= 0 && p.0 < colunas && p.1 < linhas)
    }

    /// Todos os tiles, também os de fora da foto.
    pub fn todos(&self) -> impl Iterator<Item = (&Posicao, &Tile)> {
        self.tiles.iter()
    }

    /// Há conteúdo fora da foto.
    pub fn tem_fora(&self) -> bool {
        self.tiles.keys().any(|p| !self.dentro(*p))
    }

    /// Quantos tiles, com os de fora.
    pub fn quantos(&self) -> usize {
        self.tiles.len()
    }

    /// Nenhum pixel pintado (dentro ou fora da foto).
    pub fn vazia(&self) -> bool {
        self.tiles
            .values()
            .all(|t| t.iter().skip(3).step_by(4).all(|a| *a == 0))
    }

    /// O pixel RGBA de `(x, y)`, transparente fora dos tiles existentes.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.pixel_em(x as i64, y as i64)
    }

    /// O pixel RGBA de `(x, y)` em qualquer lugar, dentro ou fora da foto.
    pub fn pixel_em(&self, x: i64, y: i64) -> [u8; 4] {
        let lado = LADO_DO_TILE as i64;
        let posicao = (x.div_euclid(lado) as i32, y.div_euclid(lado) as i32);
        let (x, y) = (x.rem_euclid(lado) as u32, y.rem_euclid(lado) as u32);
        match self.tiles.get(&posicao) {
            Some(tile) => {
                let i = indice(x, y);
                [tile[i], tile[i + 1], tile[i + 2], tile[i + 3]]
            }
            None => [0; 4],
        }
    }

    /// As posições dos tiles que cobrem o retângulo.
    pub fn tiles_do_retangulo(&self, ret: &Retangulo) -> Vec<Posicao> {
        let ret = ret.limitado(self.largura, self.altura);
        if ret.vazio() {
            return Vec::new();
        }
        let (c0, c1) = (ret.x / LADO_DO_TILE, (ret.direita() - 1) / LADO_DO_TILE);
        let (l0, l1) = (ret.y / LADO_DO_TILE, (ret.baixo() - 1) / LADO_DO_TILE);
        let mut posicoes = Vec::new();
        for l in l0..=l1 {
            for c in c0..=c1 {
                posicoes.push((c as i32, l as i32));
            }
        }
        posicoes
    }

    /// Quantos bytes os tiles ocupam (os compartilhados contam uma vez por
    /// quem os segura — é a medida do teto do desfazer, não da memória exata).
    pub fn bytes(&self) -> usize {
        self.tiles.len() * BYTES_DO_TILE
    }
}

/// O índice do byte R do pixel `(x, y)` dentro de um tile.
pub fn indice(x: u32, y: u32) -> usize {
    ((y * LADO_DO_TILE + x) * 4) as usize
}

/// O retângulo da foto que um tile ocupa (o último de cada linha/coluna é
/// cortado pela borda; o de fora da foto é vazio).
pub fn retangulo_do_tile(posicao: Posicao, largura: u32, altura: u32) -> Retangulo {
    if posicao.0 < 0 || posicao.1 < 0 {
        return Retangulo::default();
    }
    Retangulo::novo(
        posicao.0 as u32 * LADO_DO_TILE,
        posicao.1 as u32 * LADO_DO_TILE,
        LADO_DO_TILE,
        LADO_DO_TILE,
    )
    .limitado(largura, altura)
}

/// O tile do pixel `(x, y)` da foto.
pub fn tile_de(x: u32, y: u32) -> Posicao {
    ((x / LADO_DO_TILE) as i32, (y / LADO_DO_TILE) as i32)
}

/// O canto de cima à esquerda de um tile, em pixels (com sinal).
pub fn origem_do_tile(posicao: Posicao) -> (i64, i64) {
    let lado = LADO_DO_TILE as i64;
    (posicao.0 as i64 * lado, posicao.1 as i64 * lado)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ler_fora_do_que_foi_pintado_e_transparente() {
        let mut camada = CamadaDePixels::nova(600, 300);
        assert_eq!(camada.colunas(), 3);
        assert_eq!(camada.linhas(), 2);
        assert_eq!(camada.pixel(10, 10), [0; 4]);
        let i = indice(4, 4);
        camada.tile_mut((1, 1))[i..i + 4].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(camada.pixel(260, 260), [1, 2, 3, 4]);
        assert_eq!(camada.quantos(), 1);
    }

    #[test]
    fn escrever_nao_mexe_na_copia_que_o_desfazer_segura() {
        let mut camada = CamadaDePixels::nova(256, 256);
        camada.tile_mut((0, 0))[3] = 255;
        let antes = camada.tile((0, 0)).cloned().unwrap();
        camada.tile_mut((0, 0))[3] = 7;
        assert_eq!(antes[3], 255, "a cópia de antes ficou intacta");
        assert_eq!(camada.pixel(0, 0)[3], 7);
    }

    #[test]
    fn enxugar_tira_o_tile_transparente() {
        let mut camada = CamadaDePixels::nova(256, 256);
        camada.tile_mut((0, 0))[3] = 0;
        camada.enxugar((0, 0));
        assert_eq!(camada.quantos(), 0);
        assert!(camada.vazia());
    }

    #[test]
    fn os_tiles_de_um_retangulo_param_na_borda() {
        let camada = CamadaDePixels::nova(600, 300);
        assert_eq!(
            camada.tiles_do_retangulo(&Retangulo::novo(250, 250, 400, 400)),
            vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]
        );
        assert!(camada
            .tiles_do_retangulo(&Retangulo::novo(700, 0, 5, 5))
            .is_empty());
    }

    #[test]
    fn o_tile_de_fora_existe_mas_nao_aparece() {
        let mut camada = CamadaDePixels::nova(600, 300);
        camada.tile_mut((-1, 0))[3] = 255;
        camada.tile_mut((3, 0))[3] = 255;
        camada.tile_mut((0, 0))[3] = 255;
        assert_eq!(camada.existentes().count(), 1);
        assert_eq!(camada.todos().count(), 3);
        assert!(camada.tem_fora());
        assert_eq!(camada.pixel_em(-256, 0)[3], 255);
        assert_eq!(camada.pixel_em(-1, 0)[3], 0);
        assert!(!camada.vazia());
        assert!(retangulo_do_tile((-1, 0), 600, 300).vazio());
        assert!(retangulo_do_tile((3, 0), 600, 300).vazio());
    }
}
