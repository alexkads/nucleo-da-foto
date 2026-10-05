//! A vista: a imagem editada reduzida para a tela, refeita só onde sujou.
//!
//! 🔑 **O traço pinta em resolução cheia e a tela vê uma redução.** Numa foto de
//! 24 MP, recompor e reenviar a foto inteira a cada movimento do ponteiro seria
//! refazer 24 milhões de pixels por evento. Aqui cada gesto devolve o retângulo
//! que sujou; a vista recompõe **só os pixels da tela** que caem nele, e marca
//! os ladrilhos da tela (de [`LADO_DO_LADRILHO`] px) que precisam subir de novo
//! para a GPU. É o `dirty_rect` + `set_partial` do PaintFE, em ladrilhos.
//!
//! A redução é por um fator inteiro, pela média da caixa `fator × fator` da
//! **imagem composta** — o que a tela mostra é a imagem editada reduzida, e não
//! uma aproximação dela.
//!
//! 🔍 **A lupa é uma vista de um pedaço.** Com a foto ampliada, a vista inteira
//! (2048 px no maior lado) vira borrão; a janela pede então uma segunda vista,
//! só da região visível, num fator menor (até 1, a resolução cheia). As contas
//! são as mesmas: a vista inteira é a lupa da foto toda.

use std::collections::BTreeSet;

use image::RgbImage;

use crate::composicao;
use crate::documento::Documento;
use crate::retangulo::Retangulo;

/// O lado dos ladrilhos da tela, em pixels da vista.
pub const LADO_DO_LADRILHO: u32 = 256;

pub struct Vista {
    fator: u32,
    /// O pedaço da foto que a vista cobre, em pixels da foto, com a origem
    /// múltipla do fator.
    regiao: Retangulo,
    imagem: RgbImage,
    sujos: BTreeSet<(u32, u32)>,
}

impl Vista {
    /// A vista inteira, com o maior lado até `lado_maximo`.
    pub fn nova(base: &RgbImage, doc: &Documento, lado_maximo: u32) -> Self {
        let maior = base.width().max(base.height()).max(1);
        let fator = maior.div_ceil(lado_maximo.max(1)).max(1);
        Self::da_regiao(
            base,
            doc,
            &Retangulo::inteiro(base.width(), base.height()),
            fator,
        )
    }

    /// A vista de um pedaço da foto, num fator dado — a lupa. A região é
    /// alargada até a origem cair num múltiplo do fator.
    pub fn da_regiao(base: &RgbImage, doc: &Documento, regiao: &Retangulo, fator: u32) -> Self {
        let fator = fator.max(1);
        let (largura, altura) = (base.width(), base.height());
        let r = regiao.limitado(largura, altura);
        let (x0, y0) = (r.x / fator * fator, r.y / fator * fator);
        let regiao =
            Retangulo::novo(x0, y0, r.direita() - x0, r.baixo() - y0).limitado(largura, altura);
        let mut vista = Self {
            fator,
            regiao,
            imagem: RgbImage::new(
                regiao.largura.div_ceil(fator).max(1),
                regiao.altura.div_ceil(fator).max(1),
            ),
            sujos: BTreeSet::new(),
        };
        vista.refazer(base, doc, &regiao);
        vista
    }

    /// O pedaço da foto que a vista cobre.
    pub fn regiao(&self) -> Retangulo {
        self.regiao
    }

    /// Quantos pixels da foto cada pixel da vista cobre, por lado.
    pub fn fator(&self) -> u32 {
        self.fator
    }

    pub fn imagem(&self) -> &RgbImage {
        &self.imagem
    }

    pub fn colunas(&self) -> u32 {
        self.imagem.width().div_ceil(LADO_DO_LADRILHO)
    }

    pub fn linhas(&self) -> u32 {
        self.imagem.height().div_ceil(LADO_DO_LADRILHO)
    }

    /// Refaz a vista onde a foto mudou em `ret` (pixels da foto). O que cai
    /// fora da região da vista não conta.
    pub fn refazer(&mut self, base: &RgbImage, doc: &Documento, ret: &Retangulo) {
        let (largura, altura) = (base.width(), base.height());
        let ret = composicao::interseccao(&ret.limitado(largura, altura), &self.regiao);
        if ret.vazio() {
            return;
        }
        let f = self.fator;
        let (ox, oy) = (self.regiao.x, self.regiao.y);
        // Os pixels da vista que tocam o retângulo, e a região da foto que eles
        // cobrem inteira (alinhada ao fator).
        let (vx0, vy0) = ((ret.x - ox) / f, (ret.y - oy) / f);
        let (vx1, vy1) = (
            (ret.direita() - ox).div_ceil(f).min(self.imagem.width()),
            (ret.baixo() - oy).div_ceil(f).min(self.imagem.height()),
        );
        // 🔑 **Em faixas de linhas, uma por thread** quando a região é grande:
        // a transformação livre refaz milhões de pixels a cada movimento do
        // ponteiro (medido: 35–52 ms numa thread, para 3–5 MP). Cada faixa
        // compõe e reduz o pedaço dela; as linhas da vista não se cruzam.
        let colunas = (vx1 - vx0) as usize;
        let pixels = colunas as u64 * (vy1 - vy0) as u64 * (f * f) as u64;
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()) as u32;
        let por_faixa = if pixels < 1 << 20 || threads <= 1 {
            vy1 - vy0
        } else {
            (vy1 - vy0).div_ceil(threads).max(8)
        };
        let faixas: Vec<(u32, u32)> = (vy0..vy1)
            .step_by(por_faixa as usize)
            .map(|a| (a, (a + por_faixa).min(vy1)))
            .collect();
        let feitas: Vec<Vec<u8>> = if faixas.len() == 1 {
            vec![self.faixa(base, doc, faixas[0], vx0, vx1)]
        } else {
            let esta = &*self;
            std::thread::scope(|escopo| {
                let tarefas: Vec<_> = faixas
                    .iter()
                    .map(|&faixa| escopo.spawn(move || esta.faixa(base, doc, faixa, vx0, vx1)))
                    .collect();
                tarefas
                    .into_iter()
                    .map(|t| t.join().unwrap_or_default())
                    .collect()
            })
        };
        let largura_da_vista = self.imagem.width() as usize;
        let imagem: &mut [u8] = &mut self.imagem;
        for ((a, b), linhas) in faixas.iter().zip(feitas) {
            for (k, vy) in (*a..*b).enumerate() {
                let destino = (vy as usize * largura_da_vista + vx0 as usize) * 3;
                let fonte = &linhas[k * colunas * 3..(k + 1) * colunas * 3];
                imagem[destino..destino + colunas * 3].copy_from_slice(fonte);
            }
        }
        self.marcar_sujos(vx0, vy0, vx1, vy1);
    }

    /// As linhas `[a, b)` da vista, colunas `[vx0, vx1)`, em RGB: a foto
    /// composta naquele pedaço, reduzida pela média de cada caixa `fator ×
    /// fator` (no fator 1, copiada).
    fn faixa(
        &self,
        base: &RgbImage,
        doc: &Documento,
        (a, b): (u32, u32),
        vx0: u32,
        vx1: u32,
    ) -> Vec<u8> {
        let f = self.fator;
        let (ox, oy) = (self.regiao.x, self.regiao.y);
        let (fim_x, fim_y) = (self.regiao.direita(), self.regiao.baixo());
        let regiao = Retangulo::novo(ox + vx0 * f, oy + a * f, (vx1 - vx0) * f, (b - a) * f)
            .limitado(fim_x, fim_y);
        let composta = composicao::compor_recorte(base, doc, &regiao);
        let colunas = (vx1 - vx0) as usize;
        let mut saida = Vec::with_capacity(colunas * (b - a) as usize * 3);
        if f == 1 {
            for k in 0..(b - a) as usize {
                let linha = k * regiao.largura as usize * 3;
                saida.extend_from_slice(&composta.as_raw()[linha..linha + colunas * 3]);
            }
            return saida;
        }
        for vy in a..b {
            for vx in vx0..vx1 {
                let mut soma = [0u32; 3];
                let mut n = 0u32;
                for y in (oy + vy * f)..(oy + (vy + 1) * f).min(fim_y) {
                    for x in (ox + vx * f)..(ox + (vx + 1) * f).min(fim_x) {
                        let p = composta.get_pixel(x - regiao.x, y - regiao.y).0;
                        soma[0] += p[0] as u32;
                        soma[1] += p[1] as u32;
                        soma[2] += p[2] as u32;
                        n += 1;
                    }
                }
                let n = n.max(1);
                saida.extend_from_slice(&[
                    ((soma[0] + n / 2) / n) as u8,
                    ((soma[1] + n / 2) / n) as u8,
                    ((soma[2] + n / 2) / n) as u8,
                ]);
            }
        }
        saida
    }

    fn marcar_sujos(&mut self, vx0: u32, vy0: u32, vx1: u32, vy1: u32) {
        if vx1 <= vx0 || vy1 <= vy0 {
            return;
        }
        // Um pixel a mais de cada lado: o vizinho leva uma folga de um pixel
        // deste ladrilho (`ladrilho_bgra_com_folga`), e ela também mudou.
        let (vx0, vy0) = (vx0.saturating_sub(1), vy0.saturating_sub(1));
        let vx1 = (vx1 + 1).min(self.imagem.width());
        let vy1 = (vy1 + 1).min(self.imagem.height());
        for ly in (vy0 / LADO_DO_LADRILHO)..=((vy1 - 1) / LADO_DO_LADRILHO) {
            for lx in (vx0 / LADO_DO_LADRILHO)..=((vx1 - 1) / LADO_DO_LADRILHO) {
                self.sujos.insert((lx, ly));
            }
        }
    }

    /// Os ladrilhos que mudaram desde a última pergunta — e esquece-os.
    pub fn levar_os_sujos(&mut self) -> Vec<(u32, u32)> {
        std::mem::take(&mut self.sujos).into_iter().collect()
    }

    /// Todos os ladrilhos sujos de novo (a janela recriou as imagens).
    pub fn sujar_tudo(&mut self) {
        for ly in 0..self.linhas() {
            for lx in 0..self.colunas() {
                self.sujos.insert((lx, ly));
            }
        }
    }

    /// O retângulo (em pixels da vista) de um ladrilho.
    pub fn retangulo_do_ladrilho(&self, ladrilho: (u32, u32)) -> Retangulo {
        Retangulo::novo(
            ladrilho.0 * LADO_DO_LADRILHO,
            ladrilho.1 * LADO_DO_LADRILHO,
            LADO_DO_LADRILHO,
            LADO_DO_LADRILHO,
        )
        .limitado(self.imagem.width(), self.imagem.height())
    }

    /// O ladrilho com **um pixel de folga de cada lado**, tirado dos vizinhos
    /// (na borda da vista, o próprio pixel repetido): `(largura + 2, altura +
    /// 2, bytes)`.
    ///
    /// 🔑 A tela desenha a textura inteira, recortada no retângulo exato do
    /// ladrilho. Ampliada, o GPU lê meio texel além da borda; sem a folga,
    /// esse meio texel vinha do vizinho **no atlas**, e uma linha clara ou
    /// escura aparecia na emenda depois de cada ladrilho reenviado (visto no
    /// app real em 05/out/2026).
    pub fn ladrilho_bgra_com_folga(&self, ladrilho: (u32, u32)) -> (u32, u32, Vec<u8>) {
        let ret = self.retangulo_do_ladrilho(ladrilho);
        let (lv, av) = (self.imagem.width() as i64, self.imagem.height() as i64);
        let (l, a) = (ret.largura + 2, ret.altura + 2);
        let mut bytes = Vec::with_capacity((l * a * 4) as usize);
        for y in -1..=ret.altura as i64 {
            let yy = (ret.y as i64 + y).clamp(0, av - 1) as u32;
            for x in -1..=ret.largura as i64 {
                let xx = (ret.x as i64 + x).clamp(0, lv - 1) as u32;
                let p = self.imagem.get_pixel(xx, yy).0;
                bytes.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        (l, a, bytes)
    }

    /// Os pixels de um ladrilho em **BGRA** — a ordem que o GPUI espera
    /// (`ui-gpui/src/imagem.rs`) — e o tamanho dele.
    pub fn ladrilho_bgra(&self, ladrilho: (u32, u32)) -> (u32, u32, Vec<u8>) {
        let ret = self.retangulo_do_ladrilho(ladrilho);
        let mut bytes = Vec::with_capacity((ret.largura * ret.altura * 4) as usize);
        for y in ret.y..ret.baixo() {
            for x in ret.x..ret.direita() {
                let p = self.imagem.get_pixel(x, y).0;
                bytes.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        (ret.largura, ret.altura, bytes)
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::documento::BaseRef;
    use crate::pincel::{Pincel, Traco};

    #[test]
    fn a_vista_e_a_imagem_editada_reduzida_e_so_suja_onde_mudou() {
        let base = RgbImage::from_fn(1200, 900, |x, y| {
            image::Rgb([(x / 5) as u8, (y / 4) as u8, 90])
        });
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let mut vista = Vista::nova(&base, &doc, 400);
        assert_eq!(vista.fator(), 3);
        assert_eq!(
            (vista.imagem().width(), vista.imagem().height()),
            (400, 300)
        );
        vista.levar_os_sujos();

        let mut traco = Traco::novo(Pincel {
            raio: 30.0,
            dureza: 1.0,
            cor: [255, 255, 255],
            ..Pincel::default()
        });
        let sujo = traco.ate(&mut doc.camadas[0].pixels, 900.0, 150.0);
        traco.terminar(&mut doc.camadas[0].pixels);
        vista.refazer(&base, &doc, &sujo);
        assert_eq!(
            vista.levar_os_sujos(),
            vec![(1, 0)],
            "só o ladrilho do traço"
        );

        // A vista inteira refeita do zero é igual à refeita aos pedaços.
        let do_zero = Vista::nova(&base, &doc, 400);
        assert_eq!(do_zero.imagem().as_raw(), vista.imagem().as_raw());
        assert_eq!(vista.imagem().get_pixel(300, 50).0, [255, 255, 255]);
    }

    #[test]
    fn a_lupa_e_o_mesmo_pedaco_da_vista_em_resolucao_maior() {
        let base = RgbImage::from_fn(1000, 700, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x * y) % 256) as u8])
        });
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let pedaco = Retangulo::novo(301, 155, 400, 300);
        let mut lupa = Vista::da_regiao(&base, &doc, &pedaco, 1);
        assert_eq!(lupa.regiao(), pedaco);
        assert_eq!(lupa.imagem().get_pixel(0, 0).0, base.get_pixel(301, 155).0);

        // Um traço que passa pela lupa e por fora dela: a lupa refaz só o
        // pedaço dela, e fica igual à lupa feita do zero.
        lupa.levar_os_sujos();
        let mut traco = Traco::novo(Pincel {
            raio: 20.0,
            dureza: 1.0,
            cor: [255, 0, 255],
            ..Pincel::default()
        });
        traco.ate(&mut doc.camadas[0].pixels, 250.0, 200.0);
        let sujo = traco.ate(&mut doc.camadas[0].pixels, 360.0, 200.0);
        traco.terminar(&mut doc.camadas[0].pixels);
        lupa.refazer(&base, &doc, &sujo.uniao(&Retangulo::novo(230, 180, 40, 40)));
        assert!(!lupa.levar_os_sujos().is_empty());
        let do_zero = Vista::da_regiao(&base, &doc, &pedaco, 1);
        assert_eq!(lupa.imagem().as_raw(), do_zero.imagem().as_raw());
        assert_eq!(
            lupa.imagem().get_pixel(340 - 301, 200 - 155).0,
            [255, 0, 255]
        );

        // Com fator 2 a origem vai para o par de baixo.
        let meia = Vista::da_regiao(&base, &doc, &pedaco, 2);
        assert_eq!(meia.regiao(), Retangulo::novo(300, 154, 401, 301));
        assert_eq!(meia.imagem().width(), 201);
    }

    #[test]
    fn a_folga_do_ladrilho_vem_do_vizinho() {
        let base = RgbImage::from_fn(600, 300, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, (x / 256) as u8])
        });
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        let vista = Vista::nova(&base, &doc, 600);
        let (l, a, b) = vista.ladrilho_bgra_com_folga((1, 0));
        assert_eq!((l, a), (258, 258));
        // O primeiro pixel da segunda linha é o x = 255 do ladrilho da esquerda.
        let k = (l as usize) * 4;
        assert_eq!(&b[k..k + 3], &[0, 0, 255]);
        // A linha de cima repete a primeira (borda da vista).
        assert_eq!(&b[4..7], &b[k + 4..k + 7]);
    }

    #[test]
    fn o_ladrilho_sai_em_bgra() {
        let base = RgbImage::from_pixel(10, 10, image::Rgb([1, 2, 3]));
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        let vista = Vista::nova(&base, &doc, 100);
        let (l, a, bytes) = vista.ladrilho_bgra((0, 0));
        assert_eq!((l, a), (10, 10));
        assert_eq!(&bytes[..4], &[3, 2, 1, 255]);
    }
}
