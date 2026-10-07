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
}

/// Os tipos, na ordem do menu.
pub const TODOS: [Ajuste; 4] = [
    Ajuste::BrilhoContraste {
        brilho: 0.0,
        contraste: 0.0,
    },
    Ajuste::Niveis {
        preto: 0.0,
        gama: 1.0,
        branco: 255.0,
    },
    Ajuste::MatizSaturacao {
        matiz: 0.0,
        saturacao: 0.0,
        luminosidade: 0.0,
    },
    Ajuste::Inverter,
];

impl Ajuste {
    pub fn nome(&self) -> &'static str {
        match self {
            Ajuste::BrilhoContraste { .. } => "Brilho/Contraste",
            Ajuste::Niveis { .. } => "Níveis",
            Ajuste::MatizSaturacao { .. } => "Matiz/Saturação",
            Ajuste::Inverter => "Inverter",
        }
    }

    /// A chave do roteiro e do menu.
    pub fn chave(&self) -> &'static str {
        match self {
            Ajuste::BrilhoContraste { .. } => "brilho",
            Ajuste::Niveis { .. } => "niveis",
            Ajuste::MatizSaturacao { .. } => "matiz",
            Ajuste::Inverter => "inverter",
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
