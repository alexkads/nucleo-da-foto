//! A camada de pixels, dividida em tiles.
//!
//! 🔑 **Esparsa.** Tile que nunca foi pintado não existe, e ler dele é ler
//! transparente. Um retoque pequeno numa foto de 24 MP ocupa alguns tiles, e não
//! os 96 MB de uma camada cheia — na memória, no desfazer e no disco.
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

/// Onde um tile fica: coluna e linha, em tiles.
pub type Posicao = (u32, u32);

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

    /// Todos os tiles existentes.
    pub fn existentes(&self) -> impl Iterator<Item = (&Posicao, &Tile)> {
        self.tiles.iter()
    }

    pub fn quantos(&self) -> usize {
        self.tiles.len()
    }

    /// Nenhum pixel pintado.
    pub fn vazia(&self) -> bool {
        self.tiles
            .values()
            .all(|t| t.iter().skip(3).step_by(4).all(|a| *a == 0))
    }

    /// O pixel RGBA de `(x, y)`, transparente fora dos tiles existentes.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let posicao = (x / LADO_DO_TILE, y / LADO_DO_TILE);
        match self.tiles.get(&posicao) {
            Some(tile) => {
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
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
                posicoes.push((c, l));
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
/// cortado pela borda).
pub fn retangulo_do_tile(posicao: Posicao, largura: u32, altura: u32) -> Retangulo {
    Retangulo::novo(
        posicao.0 * LADO_DO_TILE,
        posicao.1 * LADO_DO_TILE,
        LADO_DO_TILE,
        LADO_DO_TILE,
    )
    .limitado(largura, altura)
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
}
