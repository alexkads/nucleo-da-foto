//! 🖌️ O Pincel misturador do Photoshop (*Mixer Brush*): a tinta do pincel se
//! mistura com a cor que já está na tela — não é um pincel com opacidade
//! baixa.
//!
//! O pincel carrega duas tintas:
//!
//! - o **reservatório**: a cor carregada ("Carregar o pincel", a cor de
//!   frente) e quanto dela ainda resta (`quantidade`, de 1 a 0). A **carga**
//!   diz quanto dura: com carga alta o reservatório quase não seca; com
//!   carga baixa ele acaba em poucos carimbos;
//! - a **sujeira**: o que o pincel recolheu da tela, pixel a pixel da ponta
//!   (uma imagem do tamanho do carimbo, presa ao centro dele). É ela que
//!   arrasta a cor de um lugar para outro.
//!
//! Em cada carimbo, para cada pixel `k` da ponta:
//!
//! ```text
//! tinta_k = sujeira_k + (reservatório − sujeira_k) · (1 − mistura) · quantidade
//! tela_k ← tela_k + (tinta_k − tela_k) · fluxo · cobertura_k        (pré-multiplicado)
//! sujeira_k ← sujeira_k + (amostra_k − sujeira_k) · umidade          (recolhe)
//! quantidade ← quantidade · (1 − SECAGEM · (1 − carga))
//! ```
//!
//! - **Umidade** 0: o pincel não recolhe nada (pinta só o reservatório,
//!   que seca); 100%: recolhe tudo onde passa (borra);
//! - **Mistura** 0: a tinta é só o reservatório; 100%: só a sujeira;
//! - **Fluxo**: quanto de tinta cada carimbo deposita (vezes a opacidade);
//! - a **amostra** é a camada atual — ou a foto composta, com "Amostrar
//!   todas as camadas" (o fluxo de retoque numa camada vazia por cima), de
//!   um instantâneo do começo do traço: a mesma ponta no mesmo lugar dá a
//!   mesma conta;
//! - "Limpar" esvazia a sujeira e o reservatório; "Carregar" enche o
//!   reservatório com a cor de frente. Os dois podem rodar sozinhos a cada
//!   traço ("Carregar/Limpar após cada traço"); sem eles, o pincel sujo
//!   continua sujo no traço seguinte.
//!
//! A conta da Adobe não é publicada; este é o modelo de reservatório +
//! recolha que a descrição dela ("Wet", "Load", "Mix", "Flow") define.

/// O quanto o reservatório seca por carimbo com carga 0.
const SECAGEM: f32 = 0.15;

/// As opções da barra do Pincel misturador.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpcoesDoMisturador {
    /// `0..=1`: quanto o pincel recolhe da tela.
    pub umidade: f32,
    /// `0..=1`: quanto o reservatório dura.
    pub carga: f32,
    /// `0..=1`: a parte da sujeira (da tela) na tinta; o resto é o
    /// reservatório.
    pub mistura: f32,
    /// Encher o reservatório com a cor de frente no começo de cada traço.
    pub carregar_apos: bool,
    /// Esvaziar o pincel no começo de cada traço.
    pub limpar_apos: bool,
    /// Amostrar a foto composta, e não só a camada atual.
    pub todas_as_camadas: bool,
}

impl Default for OpcoesDoMisturador {
    /// O "Úmido" do Photoshop: umidade, carga e mistura em 50%.
    fn default() -> Self {
        Self {
            umidade: 0.5,
            carga: 0.5,
            mistura: 0.5,
            carregar_apos: true,
            limpar_apos: true,
            todas_as_camadas: false,
        }
    }
}

/// O que o pincel carrega entre um carimbo e outro (e entre traços, sem
/// "Limpar após cada traço").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tinta {
    /// A cor do reservatório (`None`: o pincel está limpo).
    pub cor: Option<[u8; 3]>,
    /// `0..=1`: quanto do reservatório resta.
    pub quantidade: f32,
    /// A sujeira: RGBA pré-multiplicado em `0..=1`, `lado × lado`, centrada
    /// na ponta. `None`: limpa (a primeira amostra entra como está).
    sujeira: Option<(usize, Vec<[f32; 4]>)>,
}

impl Tinta {
    /// "Carregar o pincel" com `cor`: reservatório cheio.
    pub fn carregar(&mut self, cor: [u8; 3]) {
        self.cor = Some(cor);
        self.quantidade = 1.0;
    }

    /// "Limpar o pincel": sem reservatório e sem sujeira.
    pub fn limpar(&mut self) {
        self.cor = None;
        self.quantidade = 0.0;
        self.sujeira = None;
    }

    /// O pincel está limpo (nada a depositar até recolher).
    pub fn limpa(&self) -> bool {
        self.cor.is_none() && self.sujeira.is_none()
    }

    /// A cor que a amostra de agora do pincel mostra (o reservatório que
    /// resta sobre a sujeira do centro) — o quadrado da barra.
    pub fn cor_da_ponta(&self, mistura: f32) -> Option<[u8; 3]> {
        let centro = self.sujeira.as_ref().and_then(|(lado, px)| {
            let s = px[lado / 2 * lado + lado / 2];
            (s[3] > 1e-3).then(|| [s[0] / s[3], s[1] / s[3], s[2] / s[3]])
        });
        let reservatorio = self.cor.map(|c| c.map(|v| v as f32 / 255.0));
        let k = (1.0 - mistura) * self.quantidade;
        let cor = match (centro, reservatorio) {
            (Some(s), Some(r)) => [0, 1, 2].map(|i| s[i] + (r[i] - s[i]) * k),
            (Some(s), None) => s,
            (None, Some(r)) => r,
            (None, None) => return None,
        };
        Some(cor.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
    }

    /// Um carimbo. `lado` é o lado do quadrado da ponta (ímpar), `cobertura`
    /// a cobertura de cada pixel dele (0..=1, já com a seleção e o fluxo, e a
    /// opacidade), `amostra` o que a tela mostra em cada pixel (RGBA reto, ou
    /// `None` fora da foto) e `tela` os pixels da camada, que mudam no lugar
    /// (RGBA reto; `None` fora da foto). `alfa_travado` guarda o alfa da
    /// camada.
    pub fn carimbar(
        &mut self,
        opcoes: &OpcoesDoMisturador,
        lado: usize,
        cobertura: &[f32],
        amostra: &[Option<[u8; 4]>],
        tela: &mut [Option<[u8; 4]>],
        alfa_travado: bool,
    ) {
        let n = lado * lado;
        debug_assert!(cobertura.len() == n && amostra.len() == n && tela.len() == n);
        let pm = |p: [u8; 4]| {
            let a = p[3] as f32 / 255.0;
            [
                p[0] as f32 / 255.0 * a,
                p[1] as f32 / 255.0 * a,
                p[2] as f32 / 255.0 * a,
                a,
            ]
        };
        if self.sujeira.as_ref().is_none_or(|(l, _)| *l != lado) {
            // Pincel limpo (ou de outro tamanho): a primeira amostra entra
            // como sujeira — com o reservatório vazio, o primeiro carimbo não
            // muda a tela.
            let px = amostra
                .iter()
                .map(|a| a.map_or([0.0; 4], pm))
                .collect::<Vec<_>>();
            self.sujeira = Some((lado, px));
        }
        let (_, sujeira) = self.sujeira.as_mut().expect("sujeira");
        let reservatorio = self.cor.map(|c| {
            let c = c.map(|v| v as f32 / 255.0);
            [c[0], c[1], c[2], 1.0]
        });
        let k = (1.0 - opcoes.mistura.clamp(0.0, 1.0)) * self.quantidade;
        let umidade = opcoes.umidade.clamp(0.0, 1.0);
        for i in 0..n {
            let a = cobertura[i];
            let Some(atual) = tela[i] else {
                continue;
            };
            if a > 0.0 {
                let s = sujeira[i];
                let tinta = match reservatorio {
                    Some(r) => [0, 1, 2, 3].map(|c| s[c] + (r[c] - s[c]) * k),
                    None => s,
                };
                let t = pm(atual);
                let mut novo = [0, 1, 2, 3].map(|c| t[c] + (tinta[c] - t[c]) * a);
                if alfa_travado {
                    // Só a cor: o alfa da camada fica.
                    let alfa = t[3];
                    if novo[3] > 1e-6 {
                        for c in 0..3 {
                            novo[c] = novo[c] / novo[3] * alfa;
                        }
                    }
                    novo[3] = alfa;
                }
                tela[i] = Some(reto(novo));
            }
            // A ponta recolhe o que está embaixo dela (só onde ela toca).
            if cobertura[i] > 0.0 {
                if let Some(am) = amostra[i] {
                    let am = pm(am);
                    let s = &mut sujeira[i];
                    for c in 0..4 {
                        s[c] += (am[c] - s[c]) * umidade;
                    }
                }
            }
        }
        if self.cor.is_some() {
            self.quantidade *= 1.0 - SECAGEM * (1.0 - opcoes.carga.clamp(0.0, 1.0));
        }
    }
}

/// Pré-multiplicado em `0..=1` → RGBA reto de 8 bits.
fn reto(p: [f32; 4]) -> [u8; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 1e-6 {
        return [0; 4];
    }
    let q = |v: f32| ((v / a).clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(p[0]), q(p[1]), q(p[2]), (a * 255.0).round() as u8]
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Uma ponta 3 × 3 cheia.
    fn cheia() -> Vec<f32> {
        vec![1.0; 9]
    }

    fn tela(cor: [u8; 4]) -> Vec<Option<[u8; 4]>> {
        vec![Some(cor); 9]
    }

    #[test]
    fn seco_e_sem_mistura_pinta_o_reservatorio_que_vai_secando() {
        let o = OpcoesDoMisturador {
            umidade: 0.0,
            carga: 0.0,
            mistura: 0.0,
            ..Default::default()
        };
        let mut tinta = Tinta::default();
        tinta.carregar([255, 0, 0]);
        let amostra = tela([0, 0, 255, 255]);
        let mut t = tela([0, 0, 255, 255]);
        tinta.carimbar(&o, 3, &cheia(), &amostra, &mut t, false);
        // Sujeira = azul (primeira amostra), k = 1: tinta = vermelho puro.
        assert_eq!(t[4], Some([255, 0, 0, 255]));
        assert!((tinta.quantidade - (1.0 - SECAGEM)).abs() < 1e-6);
        // O reservatório seca: depois de muitos carimbos a tinta é a
        // sujeira (azul, que nunca recolheu nada).
        for _ in 0..60 {
            let mut t = tela([0, 0, 255, 255]);
            tinta.carimbar(&o, 3, &cheia(), &amostra, &mut t, false);
        }
        let mut t = tela([0, 0, 255, 255]);
        tinta.carimbar(&o, 3, &cheia(), &amostra, &mut t, false);
        assert_eq!(t[4], Some([0, 0, 255, 255]));
    }

    #[test]
    fn umido_e_mistura_cheia_borra_a_cor_de_um_lugar_para_outro() {
        let o = OpcoesDoMisturador {
            umidade: 1.0,
            mistura: 1.0,
            ..Default::default()
        };
        let mut tinta = Tinta::default();
        // Primeiro carimbo sobre o vermelho: recolhe vermelho, não muda.
        let mut t = tela([255, 0, 0, 255]);
        tinta.carimbar(&o, 3, &cheia(), &tela([255, 0, 0, 255]), &mut t, false);
        assert_eq!(t[4], Some([255, 0, 0, 255]));
        // O segundo, sobre o branco, deposita o vermelho recolhido.
        let mut t = tela([255, 255, 255, 255]);
        tinta.carimbar(&o, 3, &cheia(), &tela([255, 255, 255, 255]), &mut t, false);
        assert_eq!(t[4], Some([255, 0, 0, 255]));
        // E recolheu o branco: o terceiro deposita branco.
        let mut t = tela([0, 0, 0, 255]);
        tinta.carimbar(&o, 3, &cheia(), &tela([0, 0, 0, 255]), &mut t, false);
        assert_eq!(t[4], Some([255, 255, 255, 255]));
    }

    #[test]
    fn mistura_e_fluxo_dao_a_proporcao() {
        let o = OpcoesDoMisturador {
            umidade: 0.0,
            mistura: 0.5,
            carga: 1.0,
            ..Default::default()
        };
        let mut tinta = Tinta::default();
        tinta.carregar([200, 200, 200]);
        let mut t = tela([0, 0, 0, 255]);
        // Sujeira preta, reservatório 200: tinta 100. Fluxo 50%: 50.
        tinta.carimbar(&o, 3, &[0.5; 9], &tela([0, 0, 0, 255]), &mut t, false);
        assert_eq!(t[4], Some([50, 50, 50, 255]));
        assert_eq!(tinta.quantidade, 1.0, "carga cheia não seca");
        assert_eq!(tinta.cor_da_ponta(0.5), Some([100, 100, 100]));
    }

    #[test]
    fn fora_da_cobertura_e_fora_da_foto_nada_muda() {
        let o = OpcoesDoMisturador::default();
        let mut tinta = Tinta::default();
        tinta.carregar([255, 255, 255]);
        let mut cobertura = cheia();
        cobertura[0] = 0.0;
        let mut t = tela([10, 20, 30, 255]);
        t[8] = None;
        tinta.carimbar(&o, 3, &cobertura, &tela([10, 20, 30, 255]), &mut t, false);
        assert_eq!(t[0], Some([10, 20, 30, 255]));
        assert_eq!(t[8], None);
        assert_ne!(t[4], Some([10, 20, 30, 255]));
    }

    #[test]
    fn numa_camada_vazia_a_amostra_da_foto_vira_pixel() {
        // "Amostrar todas as camadas" numa camada transparente: a tinta é a
        // foto recolhida, e a camada ganha alfa pelo fluxo.
        let o = OpcoesDoMisturador {
            umidade: 1.0,
            mistura: 1.0,
            ..Default::default()
        };
        let mut tinta = Tinta::default();
        let foto = tela([120, 80, 60, 255]);
        let mut t = tela([0, 0, 0, 0]);
        tinta.carimbar(&o, 3, &[0.6; 9], &foto, &mut t, false);
        assert_eq!(t[4], Some([120, 80, 60, 153]));
    }

    #[test]
    fn com_o_alfa_travado_so_a_cor_muda() {
        let o = OpcoesDoMisturador {
            umidade: 0.0,
            mistura: 0.0,
            ..Default::default()
        };
        let mut tinta = Tinta::default();
        tinta.carregar([255, 0, 0]);
        let mut t = tela([0, 0, 255, 100]);
        tinta.carimbar(&o, 3, &cheia(), &tela([0, 0, 255, 100]), &mut t, true);
        assert_eq!(t[4], Some([255, 0, 0, 100]));
    }

    #[test]
    fn limpar_esvazia_tudo() {
        let mut tinta = Tinta::default();
        tinta.carregar([1, 2, 3]);
        let mut t = tela([9, 9, 9, 255]);
        tinta.carimbar(
            &OpcoesDoMisturador::default(),
            3,
            &cheia(),
            &tela([9, 9, 9, 255]),
            &mut t,
            false,
        );
        assert!(!tinta.limpa());
        tinta.limpar();
        assert!(tinta.limpa());
        assert_eq!(tinta.cor_da_ponta(0.5), None);
    }
}
