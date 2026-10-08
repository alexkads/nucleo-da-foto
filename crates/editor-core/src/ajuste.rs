//! As camadas de ajuste do Photoshop: uma camada sem pixels que muda a cor de
//! tudo o que está abaixo dela — com a opacidade, o modo e a máscara dela.
//!
//! 🔑 **Nada é destruído**: o ajuste é uma conta aplicada na composição. Mexer
//! no slider, esconder a camada ou pintar a máscara refaz a foto sem perder um
//! pixel, como no Photoshop.
//!
//! As contas trabalham em sRGB de 8 bits (C29). Brilho/contraste, níveis e
//! inverter viram uma tabela por canal ([`Preparado`]), montada uma vez por
//! composição; matiz/saturação passa por HSL pixel a pixel.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Ajuste {
    /// Brilho −150..150 e contraste −50..100, os limites do Photoshop. O
    /// brilho mexe mais nos meios-tons e preserva o preto e o branco.
    BrilhoContraste { brilho: f32, contraste: f32 },
    /// Os níveis de entrada: o preto (0..253), o branco (2..255) e o gama dos
    /// meios-tons (0,10..9,99; 1 = sem mudança).
    Niveis { preto: f32, gama: f32, branco: f32 },
    /// Matiz −180..180 (graus), saturação e luminosidade −100..100.
    MatizSaturacao {
        matiz: f32,
        saturacao: f32,
        luminosidade: f32,
    },
    /// O negativo.
    Inverter,
    /// Curvas: a curva composta (RGB) e uma por canal. Cada canal passa pela
    /// curva dele e depois pela composta (`saída = rgb(canal(entrada))`).
    Curvas {
        rgb: Curva,
        vermelho: Curva,
        verde: Curva,
        azul: Curva,
    },
}

/// Quantos pontos uma curva aceita — os 14 do Photoshop.
pub const PONTOS_DA_CURVA: usize = 14;

/// A menor distância, na entrada, entre dois pontos vizinhos da curva.
pub const DISTANCIA_ENTRE_PONTOS: u8 = 4;

/// Uma curva de tons: de 2 a 14 pontos `(entrada, saída)` em 0..=255, em
/// ordem de entrada, ligados por uma **cúbica monótona** (Fritsch–Carlson):
/// passa por todos os pontos, é lisa, e entre dois pontos nunca sobe acima do
/// maior nem desce abaixo do menor — puxar um ponto não cria o "calombo" de
/// uma spline natural do outro lado.
///
/// 🔑 `Copy` (cabe no `Ajuste`, que anda por valor no histórico): os pontos
/// moram num vetor fixo e saem para o projeto como uma lista.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "Vec<[u8; 2]>", try_from = "Vec<[u8; 2]>")]
pub struct Curva {
    n: u8,
    pontos: [[u8; 2]; PONTOS_DA_CURVA],
}

impl Default for Curva {
    fn default() -> Self {
        Self::identidade()
    }
}

impl From<Curva> for Vec<[u8; 2]> {
    fn from(c: Curva) -> Self {
        c.pontos().to_vec()
    }
}

impl TryFrom<Vec<[u8; 2]>> for Curva {
    type Error = String;
    fn try_from(v: Vec<[u8; 2]>) -> Result<Self, String> {
        if !(2..=PONTOS_DA_CURVA).contains(&v.len()) {
            return Err(format!("curva com {} pontos (de 2 a 14)", v.len()));
        }
        if v.windows(2).any(|w| w[0][0] >= w[1][0]) {
            return Err("curva com entradas fora de ordem".into());
        }
        let mut c = Curva {
            n: v.len() as u8,
            pontos: [[0; 2]; PONTOS_DA_CURVA],
        };
        c.pontos[..v.len()].copy_from_slice(&v);
        Ok(c)
    }
}

impl Curva {
    /// A reta de (0, 0) a (255, 255): não muda nada.
    pub const fn identidade() -> Self {
        let mut pontos = [[0u8; 2]; PONTOS_DA_CURVA];
        pontos[1] = [255, 255];
        Curva { n: 2, pontos }
    }

    pub fn pontos(&self) -> &[[u8; 2]] {
        &self.pontos[..self.n as usize]
    }

    /// Não muda tom nenhum.
    pub fn neutra(&self) -> bool {
        self.pontos().iter().all(|p| p[0] == p[1])
    }

    /// Acrescenta um ponto na entrada `x` com saída `y` e devolve o índice
    /// dele. Na mesma entrada de um ponto (a menos de
    /// [`DISTANCIA_ENTRE_PONTOS`]), move esse ponto. `None` com a curva cheia.
    pub fn com_ponto(&mut self, x: u8, y: u8) -> Option<usize> {
        if let Some(i) = self
            .pontos()
            .iter()
            .position(|p| p[0].abs_diff(x) < DISTANCIA_ENTRE_PONTOS)
        {
            self.pontos[i][1] = y;
            return Some(i);
        }
        if self.n as usize >= PONTOS_DA_CURVA {
            return None;
        }
        let i = self
            .pontos()
            .iter()
            .position(|p| p[0] > x)
            .unwrap_or(self.n as usize);
        let n = self.n as usize;
        self.pontos.copy_within(i..n, i + 1);
        self.pontos[i] = [x, y];
        self.n += 1;
        Some(i)
    }

    /// Leva o ponto `i` para `(x, y)`; a entrada fica entre a dos vizinhos (a
    /// [`DISTANCIA_ENTRE_PONTOS`] deles). Devolve a posição de fato.
    pub fn mover(&mut self, i: usize, x: u8, y: u8) -> [u8; 2] {
        let n = self.n as usize;
        if i >= n {
            return [0, 0];
        }
        let d = DISTANCIA_ENTRE_PONTOS;
        let min = if i == 0 {
            0
        } else {
            self.pontos[i - 1][0].saturating_add(d)
        };
        let max = if i + 1 == n {
            255
        } else {
            self.pontos[i + 1][0].saturating_sub(d)
        };
        let x = if min <= max {
            x.clamp(min, max)
        } else {
            self.pontos[i][0]
        };
        self.pontos[i] = [x, y];
        self.pontos[i]
    }

    /// Tira o ponto `i` (o Photoshop tira o ponto arrastado para fora do
    /// gráfico). Os dois das pontas ficam: a curva tem ao menos dois pontos.
    pub fn sem_ponto(&mut self, i: usize) -> bool {
        let n = self.n as usize;
        if n <= 2 || i >= n {
            return false;
        }
        self.pontos.copy_within(i + 1..n, i);
        self.n -= 1;
        self.pontos[self.n as usize] = [0, 0];
        true
    }

    /// A saída para a entrada `x` (0..=255, contínua).
    pub fn valor(&self, x: f32) -> f32 {
        let p = self.pontos();
        let (x0, y0) = (p[0][0] as f32, p[0][1] as f32);
        let (xn, yn) = (p[p.len() - 1][0] as f32, p[p.len() - 1][1] as f32);
        // Antes do primeiro e depois do último ponto, reta (o ponto preto e o
        // branco do Photoshop).
        if x <= x0 {
            return y0;
        }
        if x >= xn {
            return yn;
        }
        let tangentes = self.tangentes();
        let k = p
            .windows(2)
            .position(|w| x < w[1][0] as f32)
            .unwrap_or(p.len() - 2);
        let (xa, ya) = (p[k][0] as f32, p[k][1] as f32);
        let (xb, yb) = (p[k + 1][0] as f32, p[k + 1][1] as f32);
        let h = xb - xa;
        let t = (x - xa) / h;
        let (t2, t3) = (t * t, t * t * t);
        let (h00, h10, h01, h11) = (
            2.0 * t3 - 3.0 * t2 + 1.0,
            t3 - 2.0 * t2 + t,
            -2.0 * t3 + 3.0 * t2,
            t3 - t2,
        );
        (h00 * ya + h10 * h * tangentes[k] + h01 * yb + h11 * h * tangentes[k + 1])
            .clamp(0.0, 255.0)
    }

    /// As tangentes de Fritsch–Carlson em cada ponto.
    fn tangentes(&self) -> [f32; PONTOS_DA_CURVA] {
        let p = self.pontos();
        let n = p.len();
        let mut d = [0.0f32; PONTOS_DA_CURVA];
        for k in 0..n - 1 {
            d[k] = (p[k + 1][1] as f32 - p[k][1] as f32) / (p[k + 1][0] as f32 - p[k][0] as f32);
        }
        let mut m = [0.0f32; PONTOS_DA_CURVA];
        m[0] = d[0];
        m[n - 1] = d[n - 2];
        for k in 1..n - 1 {
            m[k] = if d[k - 1] * d[k] <= 0.0 {
                0.0
            } else {
                (d[k - 1] + d[k]) / 2.0
            };
        }
        for k in 0..n - 1 {
            if d[k] == 0.0 {
                m[k] = 0.0;
                m[k + 1] = 0.0;
                continue;
            }
            let (a, b) = (m[k] / d[k], m[k + 1] / d[k]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[k] = t * a * d[k];
                m[k + 1] = t * b * d[k];
            }
        }
        m
    }

    /// A tabela dos 256 valores.
    pub fn tabela(&self) -> [u8; 256] {
        let mut t = [0u8; 256];
        for (v, saida) in t.iter_mut().enumerate() {
            *saida = self.valor(v as f32).round() as u8;
        }
        t
    }
}

/// Os tipos, na ordem do menu.
pub const TODOS: [Ajuste; 5] = [
    Ajuste::BrilhoContraste {
        brilho: 0.0,
        contraste: 0.0,
    },
    Ajuste::Niveis {
        preto: 0.0,
        gama: 1.0,
        branco: 255.0,
    },
    CURVAS,
    Ajuste::MatizSaturacao {
        matiz: 0.0,
        saturacao: 0.0,
        luminosidade: 0.0,
    },
    Ajuste::Inverter,
];

/// Curvas sem mexer (o ajuste recém-criado).
pub const CURVAS: Ajuste = Ajuste::Curvas {
    rgb: Curva::identidade(),
    vermelho: Curva::identidade(),
    verde: Curva::identidade(),
    azul: Curva::identidade(),
};

impl Ajuste {
    pub fn nome(&self) -> &'static str {
        match self {
            Ajuste::BrilhoContraste { .. } => "Brilho/Contraste",
            Ajuste::Niveis { .. } => "Níveis",
            Ajuste::MatizSaturacao { .. } => "Matiz/Saturação",
            Ajuste::Inverter => "Inverter",
            Ajuste::Curvas { .. } => "Curvas",
        }
    }

    /// A chave do roteiro e do menu.
    pub fn chave(&self) -> &'static str {
        match self {
            Ajuste::BrilhoContraste { .. } => "brilho",
            Ajuste::Niveis { .. } => "niveis",
            Ajuste::MatizSaturacao { .. } => "matiz",
            Ajuste::Inverter => "inverter",
            Ajuste::Curvas { .. } => "curvas",
        }
    }

    pub fn da_chave(chave: &str) -> Option<Ajuste> {
        TODOS.iter().copied().find(|a| a.chave() == chave)
    }

    /// Não muda cor nenhuma (o ajuste recém-criado, antes de mexer).
    pub fn neutro(&self) -> bool {
        match *self {
            Ajuste::BrilhoContraste { brilho, contraste } => brilho == 0.0 && contraste == 0.0,
            Ajuste::Niveis {
                preto,
                gama,
                branco,
            } => preto == 0.0 && branco == 255.0 && gama == 1.0,
            Ajuste::MatizSaturacao {
                matiz,
                saturacao,
                luminosidade,
            } => matiz == 0.0 && saturacao == 0.0 && luminosidade == 0.0,
            Ajuste::Inverter => false,
            Ajuste::Curvas {
                rgb,
                vermelho,
                verde,
                azul,
            } => rgb.neutra() && vermelho.neutra() && verde.neutra() && azul.neutra(),
        }
    }

    /// Os parâmetros dentro dos limites do Photoshop.
    pub fn limitado(self) -> Ajuste {
        match self {
            Ajuste::BrilhoContraste { brilho, contraste } => Ajuste::BrilhoContraste {
                brilho: brilho.clamp(-150.0, 150.0),
                contraste: contraste.clamp(-50.0, 100.0),
            },
            Ajuste::Niveis {
                preto,
                gama,
                branco,
            } => {
                let preto = preto.clamp(0.0, 253.0);
                Ajuste::Niveis {
                    preto,
                    gama: gama.clamp(0.1, 9.99),
                    branco: branco.clamp(preto + 2.0, 255.0),
                }
            }
            Ajuste::MatizSaturacao {
                matiz,
                saturacao,
                luminosidade,
            } => Ajuste::MatizSaturacao {
                matiz: matiz.clamp(-180.0, 180.0),
                saturacao: saturacao.clamp(-100.0, 100.0),
                luminosidade: luminosidade.clamp(-100.0, 100.0),
            },
            Ajuste::Inverter => Ajuste::Inverter,
            // A curva já nasce dentro dos limites (u8 e em ordem).
            curvas @ Ajuste::Curvas { .. } => curvas,
        }
    }

    /// A conta pronta para a composição.
    pub fn preparar(&self) -> Preparado {
        let tabela = |f: &dyn Fn(f32) -> f32| -> Box<[u8; 256]> {
            let mut t = Box::new([0u8; 256]);
            for (v, saida) in t.iter_mut().enumerate() {
                *saida = (f(v as f32 / 255.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            t
        };
        match *self {
            Ajuste::BrilhoContraste { brilho, contraste } => {
                let b = brilho / 150.0;
                let c = contraste / 100.0;
                Preparado::Tabela(tabela(&|x| {
                    // O brilho como uma curva que prende as pontas; o
                    // contraste em volta do meio.
                    let x = if b >= 0.0 {
                        x + b * x * (1.0 - x) * 2.0 * (1.0 - x * 0.5).max(0.5)
                    } else {
                        x + b * x * (1.0 - x) * 2.0 * (0.5 + x * 0.5).max(0.5)
                    };
                    0.5 + (x - 0.5) * (1.0 + c)
                }))
            }
            Ajuste::Niveis {
                preto,
                gama,
                branco,
            } => {
                let (p, br) = (preto / 255.0, branco / 255.0);
                let largura = (br - p).max(1.0 / 255.0);
                Preparado::Tabela(tabela(&|x| {
                    ((x - p) / largura)
                        .clamp(0.0, 1.0)
                        .powf(1.0 / gama.max(0.01))
                }))
            }
            Ajuste::Inverter => Preparado::Tabela(tabela(&|x| 1.0 - x)),
            Ajuste::Curvas {
                rgb,
                vermelho,
                verde,
                azul,
            } => {
                let geral = rgb.tabela();
                let canal = |c: &Curva| -> Box<[u8; 256]> {
                    let t = c.tabela();
                    Box::new(std::array::from_fn(|v| geral[t[v] as usize]))
                };
                if vermelho.neutra() && verde.neutra() && azul.neutra() {
                    Preparado::Tabela(Box::new(geral))
                } else {
                    Preparado::Tabelas([canal(&vermelho), canal(&verde), canal(&azul)])
                }
            }
            Ajuste::MatizSaturacao {
                matiz,
                saturacao,
                luminosidade,
            } => Preparado::Hsl {
                matiz: matiz / 360.0,
                saturacao: saturacao / 100.0,
                luminosidade: luminosidade / 100.0,
            },
        }
    }
}

/// O ajuste pronto para ser aplicado a milhões de pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Preparado {
    /// A mesma tabela para os três canais.
    Tabela(Box<[u8; 256]>),
    /// Uma tabela por canal (R, G, B) — as Curvas com canal mexido.
    Tabelas([Box<[u8; 256]>; 3]),
    Hsl {
        matiz: f32,
        saturacao: f32,
        luminosidade: f32,
    },
}

impl Preparado {
    #[inline]
    pub fn aplicar(&self, p: [u8; 3]) -> [u8; 3] {
        match self {
            Preparado::Tabela(t) => [t[p[0] as usize], t[p[1] as usize], t[p[2] as usize]],
            Preparado::Tabelas([r, g, b]) => [r[p[0] as usize], g[p[1] as usize], b[p[2] as usize]],
            Preparado::Hsl {
                matiz,
                saturacao,
                luminosidade,
            } => {
                let (h, s, l) = para_hsl(p);
                let h = (h + matiz).rem_euclid(1.0);
                let s = if *saturacao >= 0.0 {
                    s + (1.0 - s) * saturacao * s.min(1.0 - s + 0.5).min(1.0)
                } else {
                    s * (1.0 + saturacao)
                };
                let l = if *luminosidade >= 0.0 {
                    l + (1.0 - l) * luminosidade
                } else {
                    l * (1.0 + luminosidade)
                };
                de_hsl(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
            }
        }
    }
}

fn para_hsl(p: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = p.map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn de_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let canal = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if s <= 0.0 {
        let v = canal(l);
        return [v, v, v];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let tom = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [
        canal(tom(h + 1.0 / 3.0)),
        canal(tom(h)),
        canal(tom(h - 1.0 / 3.0)),
    ]
}

#[cfg(test)]
mod testes {
    use super::*;

    fn curva(pontos: &[[u8; 2]]) -> Curva {
        Curva::try_from(pontos.to_vec()).unwrap()
    }

    #[test]
    fn a_curva_passa_pelos_pontos_e_nao_ultrapassa_entre_eles() {
        let c = curva(&[[0, 0], [64, 30], [128, 128], [255, 255]]);
        for p in c.pontos() {
            assert!((c.valor(p[0] as f32) - p[1] as f32).abs() < 0.01, "{p:?}");
        }
        // Monótona: os pontos sobem, a curva também, sem passar dos vizinhos.
        let t = c.tabela();
        assert!(t.windows(2).all(|w| w[1] >= w[0]));
        assert!((0..=64).all(|v| t[v] <= 30));
        // A identidade é a identidade, byte a byte.
        let id = Curva::identidade().tabela();
        assert!((0..256).all(|v| id[v] == v as u8));
        // O ponto preto e o branco cortam as pontas como retas.
        let c = curva(&[[20, 0], [235, 255]]);
        assert_eq!((c.tabela()[10], c.tabela()[250]), (0, 255));
    }

    #[test]
    fn pontos_entram_em_ordem_mudam_e_saem_e_as_pontas_ficam() {
        let mut c = Curva::identidade();
        assert_eq!(c.com_ponto(100, 80), Some(1));
        assert_eq!(c.com_ponto(50, 30), Some(1));
        assert_eq!(c.pontos(), &[[0, 0], [50, 30], [100, 80], [255, 255]]);
        // Na mesma entrada, move o ponto que já está lá.
        assert_eq!(c.com_ponto(52, 40), Some(1));
        assert_eq!(c.pontos()[1], [50, 40]);
        // Mover não passa do vizinho.
        assert_eq!(c.mover(1, 200, 60), [96, 60]);
        assert!(c.sem_ponto(1));
        assert_eq!(c.pontos().len(), 3);
        assert!(c.sem_ponto(1));
        assert!(!c.sem_ponto(0), "a curva fica com as duas pontas");
        for k in 0..20u8 {
            c.com_ponto(10 + k * 12, 100);
        }
        assert_eq!(c.pontos().len(), PONTOS_DA_CURVA, "14 no máximo");
    }

    #[test]
    fn curvas_escurecem_os_meios_tons_e_cada_canal_vem_antes_da_rgb() {
        let mut rgb = Curva::identidade();
        rgb.com_ponto(128, 96);
        let a = Ajuste::Curvas {
            rgb,
            vermelho: Curva::identidade(),
            verde: Curva::identidade(),
            azul: Curva::identidade(),
        };
        assert!(!a.neutro());
        assert_eq!(a.preparar().aplicar([128, 128, 128]), [96, 96, 96]);
        assert_eq!(
            a.preparar().aplicar([0, 255, 0]),
            [0, 255, 0],
            "pontas presas"
        );
        let mut vermelho = Curva::identidade();
        vermelho.com_ponto(128, 160);
        let a = Ajuste::Curvas {
            rgb,
            vermelho,
            verde: Curva::identidade(),
            azul: Curva::identidade(),
        };
        // 128 → 160 no vermelho, e o 160 pela curva geral.
        let esperado = rgb.tabela()[160];
        assert_eq!(a.preparar().aplicar([128, 128, 128]), [esperado, 96, 96]);
        // Vai e volta pelo JSON do projeto.
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"tipo\":\"curvas\""), "{json}");
        assert_eq!(serde_json::from_str::<Ajuste>(&json).unwrap(), a);
        assert!(serde_json::from_str::<Ajuste>(
            r#"{"tipo":"curvas","rgb":[[0,0],[0,9]],"vermelho":[[0,0],[255,255]],"verde":[[0,0],[255,255]],"azul":[[0,0],[255,255]]}"#
        )
        .is_err(), "entradas fora de ordem não passam");
    }

    #[test]
    fn os_ajustes_novos_nao_mudam_cor_nenhuma() {
        for a in TODOS.iter().filter(|a| !matches!(a, Ajuste::Inverter)) {
            assert!(a.neutro(), "{}", a.nome());
            let p = a.preparar();
            for v in [[0, 0, 0], [255, 255, 255], [12, 200, 77], [128, 128, 128]] {
                let saida = p.aplicar(v);
                for i in 0..3 {
                    assert!(
                        (saida[i] as i32 - v[i] as i32).abs() <= 1,
                        "{}: {v:?} → {saida:?}",
                        a.nome()
                    );
                }
            }
        }
    }

    #[test]
    fn cada_ajuste_faz_o_que_o_nome_diz() {
        let cinza = [100, 100, 100];
        let mais = Ajuste::BrilhoContraste {
            brilho: 100.0,
            contraste: 0.0,
        }
        .preparar();
        assert!(mais.aplicar(cinza)[0] > 130);
        assert_eq!(mais.aplicar([0, 0, 0]), [0, 0, 0], "o preto fica");
        assert_eq!(
            mais.aplicar([255, 255, 255]),
            [255, 255, 255],
            "o branco fica"
        );
        let contraste = Ajuste::BrilhoContraste {
            brilho: 0.0,
            contraste: 100.0,
        }
        .preparar();
        assert!(contraste.aplicar([90, 90, 90])[0] < 70);
        assert!(contraste.aplicar([170, 170, 170])[0] > 190);

        let niveis = Ajuste::Niveis {
            preto: 50.0,
            gama: 1.0,
            branco: 200.0,
        }
        .preparar();
        assert_eq!(niveis.aplicar([50, 200, 140]), [0, 255, 153]);
        assert_eq!(niveis.aplicar([10, 250, 30]), [0, 255, 0]);
        let gama = Ajuste::Niveis {
            preto: 0.0,
            gama: 2.0,
            branco: 255.0,
        }
        .preparar();
        assert!(
            gama.aplicar([64, 64, 64])[0] > 120,
            "gama 2 clareia os meios-tons"
        );

        assert_eq!(
            Ajuste::Inverter.preparar().aplicar([0, 100, 255]),
            [255, 155, 0]
        );

        let cinza_total = Ajuste::MatizSaturacao {
            matiz: 0.0,
            saturacao: -100.0,
            luminosidade: 0.0,
        }
        .preparar();
        let [r, g, b] = cinza_total.aplicar([200, 40, 40]);
        assert!(r == g && g == b, "sem saturação, cinza ({r},{g},{b})");
        let giro = Ajuste::MatizSaturacao {
            matiz: 120.0,
            saturacao: 0.0,
            luminosidade: 0.0,
        }
        .preparar();
        assert_eq!(
            giro.aplicar([255, 0, 0]),
            [0, 255, 0],
            "vermelho + 120° = verde"
        );
        let claro = Ajuste::MatizSaturacao {
            matiz: 0.0,
            saturacao: 0.0,
            luminosidade: 100.0,
        }
        .preparar();
        assert_eq!(claro.aplicar([30, 60, 90]), [255, 255, 255]);
    }

    #[test]
    fn o_hsl_ida_e_volta_devolve_a_cor() {
        for p in [
            [255, 0, 0],
            [12, 34, 56],
            [200, 200, 10],
            [0, 0, 0],
            [77, 77, 77],
        ] {
            let (h, s, l) = para_hsl(p);
            assert_eq!(de_hsl(h, s, l), p);
        }
    }
}
