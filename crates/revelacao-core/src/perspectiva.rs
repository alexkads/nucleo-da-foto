//! Perspectiva guiada: as retas que o operador marca como verticais ou
//! horizontais viram uma correção projetiva da foto — o *Upright guiado* do
//! Lightroom.
//!
//! ## O que isto é, e o que não é
//!
//! 🔑 **É uma transformação projetiva da imagem 2D**, e não uma reconstrução da
//! cena. Não há profundidade aqui: o que se corrige é a inclinação de uma
//! **câmera virtual** diante da foto já tirada. Para qualquer cena, girar a
//! câmera em torno do próprio centro óptico é exatamente uma homografia
//! `H = K·R·K⁻¹` — `R` a rotação, `K = diag(f, f, 1)` a lente. É por isso que o
//! modelo é esse, e não uma homografia qualquer: ele tem **quatro** graus de
//! liberdade com significado (a rotação em três eixos e o foco), em vez de oito
//! soltos, e o que ele faz com a foto é o que um fotógrafo faria com uma câmera
//! técnica: endireitar sem esticar à toa.
//!
//! ## Graus de liberdade, restrições e regularização
//!
//! As incógnitas são `θ = (rx, ry, rz, φ)`: o vetor de rotação (eixo × ângulo,
//! em radianos) e `φ = ln(f / F0)`. Cada guia dá **uma** restrição — depois de
//! `H`, a reta tem de ficar vertical (`Δx = 0`) ou horizontal (`Δy = 0`). O
//! resíduo é o seno do desvio, `Δx/|Δ|` ou `Δy/|Δ|`: sem unidade, não depende do
//! comprimento nem da posição da reta.
//!
//! Poucas guias não determinam as quatro incógnitas, e muitas podem se
//! contradizer. As duas coisas se resolvem do mesmo jeito: **mínimos quadrados
//! com regularização de Tikhonov** — `λ·|r|` puxa a rotação para zero e `μ·φ`
//! puxa o foco para o padrão. `λ` é pequeno (0,02): numa direção que as guias
//! determinam ele não pesa (o viés é da ordem de `λ²·θ`, milésimos de grau), e
//! numa direção que elas não determinam é ele quem decide — pela **menor
//! rotação** que satisfaz as guias. É o "alterar a foto o mínimo necessário".
//!
//! | Guias válidas | Restrições | O que acontece |
//! |---|---|---|
//! | 0 ou 1 | 0–1 | nenhuma correção (o Lightroom também pede duas) |
//! | 2 verticais | 2 | o ponto de fuga vertical vai ao infinito: incl. vertical + giro; a guinada fica em zero |
//! | 2 horizontais | 2 | o simétrico: guinada + giro; a inclinação vertical fica em zero |
//! | 1 vertical + 1 horizontal | 2 | a menor rotação que endireita as duas |
//! | 3 (2 + 1) | 3 | a rotação inteira é determinada; o foco fica no padrão |
//! | 4 (2 + 2) | 4 | rotação e foco determinados — os dois pontos de fuga ficam ortogonais |
//! | 3 ou 4 do mesmo eixo | 3–4 | sobredeterminado: o melhor compromisso |
//!
//! ## Validações
//!
//! Antes: guia **curta** (menos de 2,5% da diagonal) não diz direção; guia
//! **repetida** (mesmo eixo, mesma reta) não traz informação e só pesa em dobro;
//! guia **inclinada** demais (mais de 40° do eixo pedido) pede uma rotação que
//! nenhuma foto aguenta — as três são ignoradas e contadas.
//!
//! Depois: rotação acima de [`ROTACAO_MAXIMA`] graus, foco fora de `F0/3..3·F0`,
//! ou algum canto da foto indo para trás da câmera (`w ≤ 0`) ou virando do
//! avesso — a correção é **recusada**, e a foto fica como estava.
//!
//! ## Precisão
//!
//! Tudo aqui é `f64`, em coordenadas **centradas e divididas pela meia
//! diagonal** — os cantos da foto ficam a distância 1 do centro, qualquer que
//! seja a resolução. Por isso a mesma solução vale para a cópia de trabalho e
//! para o arquivo de 24 MP. `f32` só aparece no que se guarda e no que vai à
//! GPU.

use nalgebra::{Matrix3, Matrix4, Rotation3, Vector3, Vector4};

/// O foco padrão, em meias diagonais: `f / (diagonal/2)`. 1,6 é uma 35 mm em
/// full frame (21,6 mm de meia diagonal) — o meio do caminho entre a grande
/// angular e a normal, onde a maioria dos retratos de estúdio mora.
pub const F0: f64 = 1.6;
/// A maior rotação que a correção aceita, em graus.
pub const ROTACAO_MAXIMA: f64 = 40.0;
/// O maior ajuste manual, em graus, por eixo.
pub const AJUSTE_MAXIMO: f32 = 30.0;
/// Quantas guias a foto guarda — as mesmas quatro do Lightroom.
pub const MAXIMO_DE_GUIAS: usize = 4;

/// Guia mais curta que isto (em meias diagonais) não diz direção.
const MENOR_GUIA: f64 = 0.05;
/// Guia a mais que isto do eixo pedido pede rotação demais.
const DESVIO_MAXIMO_DA_GUIA: f64 = 40.0;
const LAMBDA: f64 = 0.02;
/// O peso do foco quando as guias podem determiná-lo (duas de cada eixo).
const MU_LIVRE: f64 = 0.01;
/// O peso do foco quando não podem: grande o bastante para prendê-lo no
/// padrão. Sem isto a conta "economiza" rotação trocando de lente — com duas
/// verticais, esticar o foco reduz o ângulo necessário, e a foto sairia
/// corrigida por uma lente que ninguém usou.
const MU_PRESO: f64 = 1e3;

/// O eixo em que a guia deve ficar — **no espaço da foto de pé**, antes de
/// espelho e giro de 90°. A tela troca o rótulo quando o giro é ímpar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eixo {
    Vertical,
    Horizontal,
}

impl Eixo {
    /// O outro eixo — o que um quarto de volta faz com o rótulo.
    pub fn outro(self) -> Self {
        match self {
            Eixo::Vertical => Eixo::Horizontal,
            Eixo::Horizontal => Eixo::Vertical,
        }
    }
}

/// Uma reta de referência, em coordenadas normalizadas (0–1) da foto de pé,
/// antes de espelho e giro — o mesmo espaço das máscaras. Assim ela acompanha o
/// conteúdo quando a foto gira ou espelha depois.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guia {
    pub de: [f64; 2],
    pub ate: [f64; 2],
    pub eixo: Eixo,
}

/// A correção como ela é aplicada — o que a foto guarda e o que o motor usa.
///
/// `rotacao` é o que as guias resolveram, **no espaço da foto de pé** (o vetor
/// de rotação em graus): girar ou espelhar a foto depois não a desfaz.
/// `vertical` e `horizontal` são o ajuste fino à mão, em graus, **nos eixos da
/// tela** — como os sliders do Lightroom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perspectiva {
    pub rotacao: [f32; 3],
    pub foco: f32,
    pub vertical: f32,
    pub horizontal: f32,
}

impl Default for Perspectiva {
    fn default() -> Self {
        Self {
            rotacao: [0.0; 3],
            foco: F0 as f32,
            vertical: 0.0,
            horizontal: 0.0,
        }
    }
}

impl Perspectiva {
    /// Limitada às faixas válidas — o mesmo papel de `Corte::novo`. Número que
    /// não é finito vira o neutro: um `NaN` aqui apagaria a foto inteira.
    pub fn nova(rotacao: [f32; 3], foco: f32, vertical: f32, horizontal: f32) -> Self {
        let finito = |v: f32, padrao: f32| if v.is_finite() { v } else { padrao };
        let mut rotacao = rotacao.map(|v| finito(v, 0.0));
        let norma = rotacao.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norma > ROTACAO_MAXIMA as f32 {
            let s = ROTACAO_MAXIMA as f32 / norma;
            rotacao = rotacao.map(|v| v * s);
        }
        let f0 = F0 as f32;
        Self {
            rotacao,
            foco: finito(foco, f0).clamp(f0 / 3.0, f0 * 3.0),
            vertical: finito(vertical, 0.0).clamp(-AJUSTE_MAXIMO, AJUSTE_MAXIMO),
            horizontal: finito(horizontal, 0.0).clamp(-AJUSTE_MAXIMO, AJUSTE_MAXIMO),
        }
    }

    /// Não corrige nada? O foco sozinho não muda a foto (`K·I·K⁻¹ = I`).
    pub fn e_identidade(&self) -> bool {
        self.rotacao.iter().all(|v| v.abs() < 1e-6)
            && self.vertical.abs() < 1e-6
            && self.horizontal.abs() < 1e-6
    }

    /// A homografia no **espaço orientado** (depois de espelhos e giro de 90°),
    /// em coordenadas centradas e divididas pela meia diagonal — normalizada:
    /// o centro fica no centro e a área da foto é a mesma.
    ///
    /// `lg × ag` são as dimensões do espaço orientado; só o aspecto importa.
    pub fn homografia(&self, orientacao: Orientacao, lg: f64, ag: f64) -> Matrix3<f64> {
        let f = self.foco as f64;
        let resolvida = homografia_de_rotacao(&vetor_em_radianos(self.rotacao), f);
        // Da foto de pé ao espaço orientado: conjugar pela orientação. `K`
        // comuta com ela (giro no plano e espelho não mexem no foco), então o
        // resultado continua sendo uma rotação de câmera.
        let t = orientacao.matriz();
        let t_inv = t.transpose();
        let no_espaco = t * resolvida * t_inv;
        let manual = homografia_de_rotacao_manual(self.vertical as f64, self.horizontal as f64, f);
        normalizar(manual * no_espaco, lg, ag)
    }
}

fn vetor_em_radianos(graus: [f32; 3]) -> Vector3<f64> {
    Vector3::new(
        (graus[0] as f64).to_radians(),
        (graus[1] as f64).to_radians(),
        (graus[2] as f64).to_radians(),
    )
}

/// `K·R·K⁻¹`, com `R` dado pelo vetor de rotação (radianos).
fn homografia_de_rotacao(r: &Vector3<f64>, f: f64) -> Matrix3<f64> {
    let rot = Rotation3::from_scaled_axis(*r);
    let k = Matrix3::new(f, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 1.0);
    let k_inv = Matrix3::new(1.0 / f, 0.0, 0.0, 0.0, 1.0 / f, 0.0, 0.0, 0.0, 1.0);
    k * rot.matrix() * k_inv
}

/// O ajuste à mão: `Rx(vertical)·Ry(horizontal)`, nos eixos da tela.
///
/// Com `y` para baixo e a câmera olhando para `+z`, `Rx(+α)` afasta o topo da
/// câmera — **o topo alarga**, que é o que corrige o prédio fotografado de
/// baixo. `Ry(+β)` alarga a direita.
fn homografia_de_rotacao_manual(vertical: f64, horizontal: f64, f: f64) -> Matrix3<f64> {
    let rx = Rotation3::from_axis_angle(&Vector3::x_axis(), vertical.to_radians());
    let ry = Rotation3::from_axis_angle(&Vector3::y_axis(), horizontal.to_radians());
    let k = Matrix3::new(f, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 1.0);
    let k_inv = Matrix3::new(1.0 / f, 0.0, 0.0, 0.0, 1.0 / f, 0.0, 0.0, 0.0, 1.0);
    k * (rx * ry).matrix() * k_inv
}

/// O centro volta ao centro e a área volta a ser a da foto.
///
/// 🔑 Sem isto a rotação empurra a foto para um lado e a encolhe ou aumenta —
/// o operador veria o retrato fugir do quadro a cada guia.
fn normalizar(h: Matrix3<f64>, lg: f64, ag: f64) -> Matrix3<f64> {
    let r = 0.5 * lg.hypot(ag);
    let (hx, hy) = (0.5 * lg / r, 0.5 * ag / r);
    let centro = aplicar_h(&h, [0.0, 0.0]).unwrap_or([0.0, 0.0]);
    let cantos = [[-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy]];
    let mut q = [[0.0; 2]; 4];
    for (i, c) in cantos.iter().enumerate() {
        match aplicar_h(&h, *c) {
            Some(p) => q[i] = p,
            None => return h,
        }
    }
    let area = area_do_quadrilatero(&q).abs();
    let s = if area > 1e-12 {
        ((4.0 * hx * hy) / area).sqrt()
    } else {
        1.0
    };
    let ajuste = Matrix3::new(
        s,
        0.0,
        -s * centro[0],
        0.0,
        s,
        -s * centro[1],
        0.0,
        0.0,
        1.0,
    );
    let n = ajuste * h;
    n / n[(2, 2)]
}

/// `H·(x, y, 1)`, dividido — `None` quando o ponto vai para trás da câmera.
pub fn aplicar_h(h: &Matrix3<f64>, p: [f64; 2]) -> Option<[f64; 2]> {
    let v = h * Vector3::new(p[0], p[1], 1.0);
    (v.z > 1e-9).then(|| [v.x / v.z, v.y / v.z])
}

/// Área com sinal (fórmula do laço); positiva no sentido horário da tela.
fn area_do_quadrilatero(q: &[[f64; 2]; 4]) -> f64 {
    let mut s = 0.0;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        s += a[0] * b[1] - b[0] * a[1];
    }
    0.5 * s
}

/// Espelhos e giro de 90°, como matriz no plano centrado — a ordem de
/// `espelhar_e_girar`: espelho horizontal, espelho vertical, e depois o giro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Orientacao {
    pub giro_90: i32,
    pub espelho_h: bool,
    pub espelho_v: bool,
}

impl Orientacao {
    /// Da foto de pé ao espaço orientado, nas coordenadas centradas.
    pub fn matriz(&self) -> Matrix3<f64> {
        let mut m = Matrix3::identity();
        if self.espelho_h {
            m = Matrix3::new(-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0) * m;
        }
        if self.espelho_v {
            m = Matrix3::new(1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0) * m;
        }
        // `rotate90`: (x, y) → (−y, x) no plano centrado.
        let quarto = Matrix3::new(0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0);
        for _ in 0..self.giro_90.rem_euclid(4) {
            m = quarto * m;
        }
        m
    }

    /// Um quarto de volta troca vertical com horizontal.
    pub fn troca_os_eixos(&self) -> bool {
        self.giro_90.rem_euclid(4) % 2 == 1
    }
}

/// Por que uma guia ficou de fora da conta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiaIgnorada {
    /// Curta demais para dizer direção.
    Curta,
    /// A mesma reta de outra guia do mesmo eixo.
    Repetida,
    /// Longe demais do eixo pedido: a rotação não caberia.
    Inclinada,
}

/// O que a conta devolveu.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolucao {
    /// Menos de duas guias válidas: a foto fica como está.
    PoucasGuias {
        validas: usize,
    },
    /// A solução existe, mas pede uma correção que a foto não aguenta.
    Excessiva,
    Resolvida(Solucao),
}

/// A correção encontrada.
#[derive(Debug, Clone, PartialEq)]
pub struct Solucao {
    /// O vetor de rotação, em graus, no espaço da foto de pé.
    pub rotacao: [f64; 3],
    pub foco: f64,
    /// O maior desvio que sobrou numa guia, em graus. Perto de zero quando as
    /// guias concordam; grande quando se contradizem (retas curvas, por ex.).
    pub desvio_maximo: f64,
    /// Quantas das quatro incógnitas as guias determinam sozinhas (o resto é a
    /// regularização): 2 com duas guias, 3 com três, 4 com duas de cada eixo.
    pub determinadas: usize,
    /// `σ_max/σ_min` do jacobiano das guias, nas direções determinadas.
    pub condicionamento: f64,
}

impl Solucao {
    pub fn perspectiva(&self, vertical: f32, horizontal: f32) -> Perspectiva {
        Perspectiva::nova(
            self.rotacao.map(|v| v as f32),
            self.foco as f32,
            vertical,
            horizontal,
        )
    }
}

/// O resultado inteiro: a solução e o que ficou de fora.
#[derive(Debug, Clone, PartialEq)]
pub struct Resposta {
    pub resolucao: Resolucao,
    /// Uma entrada por guia recebida, na mesma ordem: `None` é "entrou na conta".
    pub ignoradas: Vec<Option<GuiaIgnorada>>,
}

/// A guia no plano centrado da foto (meias diagonais).
fn centrada(p: [f64; 2], largura: f64, altura: f64) -> [f64; 2] {
    let r = 0.5 * largura.hypot(altura);
    [(p[0] - 0.5) * largura / r, (p[1] - 0.5) * altura / r]
}

/// O desvio da reta em relação ao eixo pedido, em graus (0–90).
fn desvio_do_eixo(d: [f64; 2], eixo: Eixo) -> f64 {
    let (dx, dy) = (d[0].abs(), d[1].abs());
    match eixo {
        Eixo::Vertical => dx.atan2(dy).to_degrees(),
        Eixo::Horizontal => dy.atan2(dx).to_degrees(),
    }
}

/// Resolve a correção que deixa as guias no eixo pedido.
///
/// `largura × altura` são as da foto de pé (qualquer resolução: só o aspecto
/// entra).
pub fn resolver(guias: &[Guia], largura: u32, altura: u32) -> Resposta {
    let (l, a) = (largura.max(1) as f64, altura.max(1) as f64);
    let mut ignoradas = vec![None; guias.len()];
    let mut validas: Vec<([f64; 2], [f64; 2], Eixo)> = Vec::new();

    for (i, g) in guias.iter().enumerate() {
        let (p, q) = (centrada(g.de, l, a), centrada(g.ate, l, a));
        let d = [q[0] - p[0], q[1] - p[1]];
        let comprimento = d[0].hypot(d[1]);
        if !comprimento.is_finite() || comprimento < MENOR_GUIA {
            ignoradas[i] = Some(GuiaIgnorada::Curta);
            continue;
        }
        if desvio_do_eixo(d, g.eixo) > DESVIO_MAXIMO_DA_GUIA {
            ignoradas[i] = Some(GuiaIgnorada::Inclinada);
            continue;
        }
        let repetida = validas.iter().any(|(vp, vq, ve)| {
            if *ve != g.eixo {
                return false;
            }
            let e = [vq[0] - vp[0], vq[1] - vp[1]];
            let le = e[0].hypot(e[1]);
            let cruz = (d[0] * e[1] - d[1] * e[0]).abs() / (comprimento * le);
            // Distância do meio desta guia à reta da outra.
            let meio = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
            let dist = ((meio[0] - vp[0]) * e[1] - (meio[1] - vp[1]) * e[0]).abs() / le;
            cruz < (0.5f64).to_radians().sin() && dist < 0.02
        });
        if repetida {
            ignoradas[i] = Some(GuiaIgnorada::Repetida);
            continue;
        }
        validas.push((p, q, g.eixo));
    }

    if validas.len() < 2 {
        return Resposta {
            resolucao: Resolucao::PoucasGuias {
                validas: validas.len(),
            },
            ignoradas,
        };
    }

    let theta = levenberg_marquardt(&validas);
    let r = Vector3::new(theta[0], theta[1], theta[2]);
    let foco = F0 * theta[3].exp();
    let h = homografia_de_rotacao(&r, foco);

    // O que a conta não pode entregar.
    let excessiva = r.norm().to_degrees() > ROTACAO_MAXIMA + 1e-9
        || !(F0 / 3.0..=F0 * 3.0).contains(&foco)
        || !cantos_sao_validos(&h, l, a);
    if excessiva {
        return Resposta {
            resolucao: Resolucao::Excessiva,
            ignoradas,
        };
    }

    let desvio_maximo = validas
        .iter()
        .map(|g| {
            residuo_da_guia(&h, g)
                .abs()
                .clamp(0.0, 1.0)
                .asin()
                .to_degrees()
        })
        .fold(0.0, f64::max);
    // 🚨 Com três guias ou menos a conta fecha exata; sobrar desvio aqui é o
    // Levenberg–Marquardt parado num mínimo que não serve — a foto sairia
    // "corrigida" com as guias ainda tortas.
    if validas.len() <= 3 && desvio_maximo > 3.0 {
        return Resposta {
            resolucao: Resolucao::Excessiva,
            ignoradas,
        };
    }
    let (determinadas, condicionamento) = condicionamento(&validas, &theta);

    Resposta {
        resolucao: Resolucao::Resolvida(Solucao {
            rotacao: [r.x, r.y, r.z].map(f64::to_degrees),
            foco,
            desvio_maximo,
            determinadas,
            condicionamento,
        }),
        ignoradas,
    }
}

/// Nenhum canto vai para trás da câmera, e a foto não vira do avesso.
fn cantos_sao_validos(h: &Matrix3<f64>, l: f64, a: f64) -> bool {
    let r = 0.5 * l.hypot(a);
    let (hx, hy) = (0.5 * l / r, 0.5 * a / r);
    let cantos = [[-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy]];
    let mut q = [[0.0; 2]; 4];
    for (i, c) in cantos.iter().enumerate() {
        let v = h * Vector3::new(c[0], c[1], 1.0);
        // A margem: um canto quase no horizonte explode para o infinito.
        if v.z < 0.2 {
            return false;
        }
        q[i] = [v.x / v.z, v.y / v.z];
    }
    // Convexo e com a mesma orientação da foto: os quatro produtos vetoriais
    // com o mesmo sinal, e positivos.
    (0..4).all(|i| {
        let (p0, p1, p2) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        let cruz = (p1[0] - p0[0]) * (p2[1] - p1[1]) - (p1[1] - p0[1]) * (p2[0] - p1[0]);
        cruz > 0.0
    })
}

/// O seno do desvio da guia depois de `H`. Ponto atrás da câmera dá 1 — o
/// pior resíduo possível, que empurra a conta para longe dali.
fn residuo_da_guia(h: &Matrix3<f64>, (p, q, eixo): &([f64; 2], [f64; 2], Eixo)) -> f64 {
    let (Some(p), Some(q)) = (aplicar_h(h, *p), aplicar_h(h, *q)) else {
        return 1.0;
    };
    let d = [q[0] - p[0], q[1] - p[1]];
    let n = d[0].hypot(d[1]);
    if n < 1e-12 {
        return 1.0;
    }
    // O sinal da reta não importa (de → até ou até → de): o resíduo usa a
    // componente que devia sumir, com o sinal da outra.
    let s = match eixo {
        Eixo::Vertical => d[1].signum(),
        Eixo::Horizontal => d[0].signum(),
    };
    let s = if s == 0.0 { 1.0 } else { s };
    match eixo {
        Eixo::Vertical => s * d[0] / n,
        Eixo::Horizontal => s * d[1] / n,
    }
}

type Guias<'a> = &'a [([f64; 2], [f64; 2], Eixo)];

/// O foco só é identificável com os **dois** pontos de fuga — duas guias de
/// cada eixo. É a ortogonalidade entre eles que o mede.
fn foco_determinavel(guias: Guias) -> bool {
    let v = guias.iter().filter(|g| g.2 == Eixo::Vertical).count();
    v >= 2 && guias.len() - v >= 2
}

/// Os resíduos: um por guia, e os quatro da regularização.
fn residuos(guias: Guias, theta: &Vector4<f64>) -> Vec<f64> {
    let mu = if foco_determinavel(guias) {
        MU_LIVRE
    } else {
        MU_PRESO
    };
    let r = Vector3::new(theta[0], theta[1], theta[2]);
    let h = homografia_de_rotacao(&r, F0 * theta[3].exp());
    let mut v: Vec<f64> = guias.iter().map(|g| residuo_da_guia(&h, g)).collect();
    v.extend([
        LAMBDA * theta[0],
        LAMBDA * theta[1],
        LAMBDA * theta[2],
        mu * theta[3],
    ]);
    v
}

fn jacobiano(guias: Guias, theta: &Vector4<f64>, so_guias: bool) -> Vec<[f64; 4]> {
    const PASSO: f64 = 1e-6;
    let n = if so_guias {
        guias.len()
    } else {
        guias.len() + 4
    };
    let mut j = vec![[0.0; 4]; n];
    for k in 0..4 {
        let mut mais = *theta;
        let mut menos = *theta;
        mais[k] += PASSO;
        menos[k] -= PASSO;
        let (rm, rn) = (residuos(guias, &mais), residuos(guias, &menos));
        for (i, linha) in j.iter_mut().enumerate() {
            linha[k] = (rm[i] - rn[i]) / (2.0 * PASSO);
        }
    }
    j
}

fn custo(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

/// Levenberg–Marquardt sobre as quatro incógnitas, partindo da foto como está.
///
/// Partir do zero não é só conveniência: é o que faz a regularização escolher,
/// entre as soluções equivalentes, a mais próxima da foto original.
fn levenberg_marquardt(guias: Guias) -> Vector4<f64> {
    let mut theta = Vector4::zeros();
    let mut atual = custo(&residuos(guias, &theta));
    let mut amortecimento = 1e-3;
    for _ in 0..200 {
        let r = residuos(guias, &theta);
        let j = jacobiano(guias, &theta, false);
        let mut jtj = Matrix4::zeros();
        let mut jtr = Vector4::zeros();
        for (linha, ri) in j.iter().zip(&r) {
            for a in 0..4 {
                jtr[a] += linha[a] * ri;
                for b in 0..4 {
                    jtj[(a, b)] += linha[a] * linha[b];
                }
            }
        }
        let mut avancou = false;
        for _ in 0..12 {
            let mut a = jtj;
            for k in 0..4 {
                a[(k, k)] += amortecimento * (jtj[(k, k)] + 1e-12);
            }
            let passo = match a.cholesky() {
                Some(c) => c.solve(&(-jtr)),
                None => match a.svd(true, true).solve(&(-jtr), 1e-14) {
                    Ok(p) => p,
                    Err(_) => break,
                },
            };
            let candidato = theta + passo;
            let novo = custo(&residuos(guias, &candidato));
            if novo.is_finite() && novo < atual {
                let melhora = atual - novo;
                theta = candidato;
                atual = novo;
                amortecimento = (amortecimento / 3.0).max(1e-12);
                avancou = true;
                if passo.norm() < 1e-12 || melhora < 1e-20 {
                    return theta;
                }
                break;
            }
            amortecimento *= 4.0;
        }
        if !avancou {
            break;
        }
    }
    theta
}

/// Quantas direções as guias determinam, e quão bem.
fn condicionamento(guias: Guias, theta: &Vector4<f64>) -> (usize, f64) {
    let j = jacobiano(guias, theta, true);
    let mut m = nalgebra::DMatrix::<f64>::zeros(j.len(), 4);
    for (i, linha) in j.iter().enumerate() {
        for k in 0..4 {
            m[(i, k)] = linha[k];
        }
    }
    let sv = m.svd(false, false).singular_values;
    let maior = sv.iter().cloned().fold(0.0, f64::max);
    if maior <= 0.0 {
        return (0, f64::INFINITY);
    }
    let uteis: Vec<f64> = sv.iter().cloned().filter(|s| *s > 1e-3 * maior).collect();
    let menor = uteis.iter().cloned().fold(f64::INFINITY, f64::min);
    (uteis.len(), maior / menor)
}

#[cfg(test)]
mod testes {
    use super::*;

    const L: u32 = 3000;
    const A: u32 = 2000;

    /// Leva um ponto da foto de pé (normalizado) pela correção, e devolve no
    /// plano centrado.
    fn corrigido(p: &Perspectiva, ponto: [f64; 2]) -> [f64; 2] {
        let h = p.homografia(Orientacao::default(), L as f64, A as f64);
        aplicar_h(&h, centrada(ponto, L as f64, A as f64)).unwrap()
    }

    /// A reta depois da correção: o desvio do eixo, em graus.
    fn desvio_depois(p: &Perspectiva, g: &Guia) -> f64 {
        let (a, b) = (corrigido(p, g.de), corrigido(p, g.ate));
        desvio_do_eixo([b[0] - a[0], b[1] - a[1]], g.eixo)
    }

    /// Uma foto "tirada de baixo": pega pontos de uma cena reta e aplica a
    /// inclinação da câmera. As guias são as retas da cena, já tortas.
    fn cena_inclinada(rotacao_graus: [f64; 3], foco: f64) -> impl Fn([f64; 2]) -> [f64; 2] {
        let h = homografia_de_rotacao(&Vector3::from(rotacao_graus.map(f64::to_radians)), foco);
        let (l, a) = (L as f64, A as f64);
        let r = 0.5 * l.hypot(a);
        move |p: [f64; 2]| {
            let c = centrada(p, l, a);
            let q = aplicar_h(&h, c).unwrap();
            [q[0] * r / l + 0.5, q[1] * r / a + 0.5]
        }
    }

    fn guia(
        torta: &impl Fn([f64; 2]) -> [f64; 2],
        de: [f64; 2],
        ate: [f64; 2],
        eixo: Eixo,
    ) -> Guia {
        Guia {
            de: torta(de),
            ate: torta(ate),
            eixo,
        }
    }

    fn resolvida(r: &Resposta) -> &Solucao {
        match &r.resolucao {
            Resolucao::Resolvida(s) => s,
            outra => panic!("esperava solução, veio {outra:?}"),
        }
    }

    #[test]
    fn sem_correcao_e_a_identidade() {
        let p = Perspectiva::default();
        assert!(p.e_identidade());
        let h = p.homografia(Orientacao::default(), 3000.0, 2000.0);
        assert!((h - Matrix3::identity()).abs().max() < 1e-12, "{h}");
    }

    #[test]
    fn duas_verticais_ficam_verticais() {
        // Câmera apontada 12° para cima: as verticais convergem no topo.
        let torta = cena_inclinada([-12.0, 0.0, 0.0], F0);
        let guias = [
            guia(&torta, [0.2, 0.1], [0.2, 0.9], Eixo::Vertical),
            guia(&torta, [0.8, 0.1], [0.8, 0.9], Eixo::Vertical),
        ];
        for g in &guias {
            assert!(desvio_do_eixo([g.ate[0] - g.de[0], g.ate[1] - g.de[1]], Eixo::Vertical) > 2.0);
        }
        let resposta = resolver(&guias, L, A);
        let s = resolvida(&resposta);
        let p = s.perspectiva(0.0, 0.0);
        for g in &guias {
            assert!(desvio_depois(&p, g) < 0.01, "{}", desvio_depois(&p, g));
        }
        assert_eq!(s.determinadas, 2);
        // A regularização deixa a guinada em paz: só o eixo x gira.
        assert!((s.rotacao[0] - 12.0).abs() < 0.2, "{:?}", s.rotacao);
        assert!(
            s.rotacao[1].abs() < 0.2 && s.rotacao[2].abs() < 0.2,
            "{:?}",
            s.rotacao
        );
    }

    #[test]
    fn duas_horizontais_ficam_horizontais() {
        let torta = cena_inclinada([0.0, 9.0, 0.0], F0);
        let guias = [
            guia(&torta, [0.1, 0.25], [0.9, 0.25], Eixo::Horizontal),
            guia(&torta, [0.1, 0.8], [0.9, 0.8], Eixo::Horizontal),
        ];
        let s = resolvida(&resolver(&guias, L, A)).clone();
        let p = s.perspectiva(0.0, 0.0);
        for g in &guias {
            assert!(desvio_depois(&p, g) < 0.01);
        }
        assert!((s.rotacao[1] + 9.0).abs() < 0.2, "{:?}", s.rotacao);
        assert!(s.rotacao[0].abs() < 0.2, "{:?}", s.rotacao);
    }

    #[test]
    fn tres_guias_determinam_a_rotacao_inteira() {
        let torta = cena_inclinada([-8.0, 5.0, 3.0], F0);
        let guias = [
            guia(&torta, [0.15, 0.1], [0.15, 0.9], Eixo::Vertical),
            guia(&torta, [0.85, 0.1], [0.85, 0.9], Eixo::Vertical),
            // 🔑 Fora do centro: uma horizontal que passa pelo centro óptico
            // não muda com a guinada, e não a determinaria.
            guia(&torta, [0.1, 0.2], [0.9, 0.2], Eixo::Horizontal),
        ];
        let s = resolvida(&resolver(&guias, L, A)).clone();
        assert_eq!(s.determinadas, 3);
        let p = s.perspectiva(0.0, 0.0);
        for g in &guias {
            assert!(desvio_depois(&p, g) < 0.02, "{}", desvio_depois(&p, g));
        }
    }

    #[test]
    fn quatro_guias_determinam_tambem_o_foco() {
        let foco_de_verdade = 1.1; // uma grande angular
        let torta = cena_inclinada([-10.0, 7.0, 2.0], foco_de_verdade);
        let guias = [
            guia(&torta, [0.15, 0.1], [0.15, 0.9], Eixo::Vertical),
            guia(&torta, [0.85, 0.1], [0.85, 0.9], Eixo::Vertical),
            guia(&torta, [0.1, 0.2], [0.9, 0.2], Eixo::Horizontal),
            guia(&torta, [0.1, 0.85], [0.9, 0.85], Eixo::Horizontal),
        ];
        let s = resolvida(&resolver(&guias, L, A)).clone();
        assert_eq!(s.determinadas, 4);
        let p = s.perspectiva(0.0, 0.0);
        for g in &guias {
            assert!(desvio_depois(&p, g) < 0.05, "{}", desvio_depois(&p, g));
        }
        assert!((s.foco - foco_de_verdade).abs() < 0.1, "foco {}", s.foco);
    }

    #[test]
    fn uma_guia_so_nao_corrige() {
        let g = Guia {
            de: [0.3, 0.1],
            ate: [0.35, 0.9],
            eixo: Eixo::Vertical,
        };
        assert_eq!(
            resolver(&[g], L, A).resolucao,
            Resolucao::PoucasGuias { validas: 1 }
        );
        assert_eq!(
            resolver(&[], L, A).resolucao,
            Resolucao::PoucasGuias { validas: 0 }
        );
    }

    #[test]
    fn guia_curta_repetida_ou_inclinada_fica_de_fora() {
        let boa = Guia {
            de: [0.2, 0.1],
            ate: [0.21, 0.9],
            eixo: Eixo::Vertical,
        };
        let curta = Guia {
            de: [0.5, 0.5],
            ate: [0.5, 0.51],
            eixo: Eixo::Vertical,
        };
        let repetida = Guia {
            de: [0.2025, 0.3],
            ate: [0.2075, 0.7],
            eixo: Eixo::Vertical,
        };
        let deitada = Guia {
            de: [0.1, 0.5],
            ate: [0.9, 0.52],
            eixo: Eixo::Vertical,
        };
        let r = resolver(&[boa, curta, repetida, deitada], L, A);
        assert_eq!(
            r.ignoradas,
            vec![
                None,
                Some(GuiaIgnorada::Curta),
                Some(GuiaIgnorada::Repetida),
                Some(GuiaIgnorada::Inclinada)
            ]
        );
        assert_eq!(r.resolucao, Resolucao::PoucasGuias { validas: 1 });
    }

    #[test]
    fn verticais_ja_paralelas_so_giram() {
        // Paralelas e inclinadas 3°: o ponto de fuga já está no infinito, e a
        // menor correção é o giro puro.
        let t = 3f64.to_radians().tan() * (A as f64 / L as f64);
        let guias = [
            Guia {
                de: [0.3, 0.1],
                ate: [0.3 + 0.8 * t, 0.9],
                eixo: Eixo::Vertical,
            },
            Guia {
                de: [0.7, 0.1],
                ate: [0.7 + 0.8 * t, 0.9],
                eixo: Eixo::Vertical,
            },
        ];
        let s = resolvida(&resolver(&guias, L, A)).clone();
        assert!((s.rotacao[2].abs() - 3.0).abs() < 0.1, "{:?}", s.rotacao);
        assert!(
            s.rotacao[0].abs() < 0.1 && s.rotacao[1].abs() < 0.1,
            "{:?}",
            s.rotacao
        );
    }

    #[test]
    fn correcao_impossivel_e_recusada() {
        // Duas "verticais" que se cruzam num ponto de fuga dentro da foto: só
        // uma rotação enorme as deixaria paralelas.
        let guias = [
            Guia {
                de: [0.25, 0.9],
                ate: [0.45, 0.3],
                eixo: Eixo::Vertical,
            },
            Guia {
                de: [0.75, 0.9],
                ate: [0.55, 0.3],
                eixo: Eixo::Vertical,
            },
        ];
        assert_eq!(resolver(&guias, L, A).resolucao, Resolucao::Excessiva);
    }

    #[test]
    fn a_solucao_nao_depende_da_resolucao() {
        let torta = cena_inclinada([-6.0, 4.0, 1.0], F0);
        let guias = [
            guia(&torta, [0.2, 0.1], [0.2, 0.9], Eixo::Vertical),
            guia(&torta, [0.8, 0.1], [0.8, 0.9], Eixo::Vertical),
            guia(&torta, [0.1, 0.5], [0.9, 0.5], Eixo::Horizontal),
        ];
        let grande = resolvida(&resolver(&guias, 6000, 4000)).clone();
        let pequena = resolvida(&resolver(&guias, 600, 400)).clone();
        for k in 0..3 {
            assert!((grande.rotacao[k] - pequena.rotacao[k]).abs() < 1e-6);
        }
    }

    #[test]
    fn o_ajuste_vertical_alarga_o_topo() {
        let p = Perspectiva::nova([0.0; 3], F0 as f32, 10.0, 0.0);
        let topo = corrigido(&p, [1.0, 0.0])[0] - corrigido(&p, [0.0, 0.0])[0];
        let base = corrigido(&p, [1.0, 1.0])[0] - corrigido(&p, [0.0, 1.0])[0];
        assert!(topo > base * 1.05, "topo {topo} base {base}");
        let p = Perspectiva::nova([0.0; 3], F0 as f32, 0.0, 10.0);
        let direita = corrigido(&p, [1.0, 1.0])[1] - corrigido(&p, [1.0, 0.0])[1];
        let esquerda = corrigido(&p, [0.0, 1.0])[1] - corrigido(&p, [0.0, 0.0])[1];
        assert!(
            direita > esquerda * 1.05,
            "direita {direita} esquerda {esquerda}"
        );
    }

    #[test]
    fn a_normalizacao_mantem_centro_e_area() {
        let p = Perspectiva::nova([12.0, -5.0, 2.0], 1.3, 0.0, 0.0);
        let h = p.homografia(Orientacao::default(), 3000.0, 2000.0);
        let c = aplicar_h(&h, [0.0, 0.0]).unwrap();
        assert!(c[0].abs() < 1e-12 && c[1].abs() < 1e-12);
        let r = 0.5 * 3000f64.hypot(2000.0);
        let (hx, hy) = (1500.0 / r, 1000.0 / r);
        let q = [[-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy]].map(|c| aplicar_h(&h, c).unwrap());
        assert!((area_do_quadrilatero(&q) - 4.0 * hx * hy).abs() < 1e-9);
    }

    /// 🔑 A correção resolvida na foto de pé acompanha o conteúdo: girar ou
    /// espelhar depois dá a mesma foto corrigida, girada ou espelhada.
    #[test]
    fn girar_e_espelhar_depois_acompanham_a_correcao() {
        let p = Perspectiva::nova([8.0, -4.0, 2.0], 1.4, 0.0, 0.0);
        let h0 = p.homografia(Orientacao::default(), 3000.0, 2000.0);
        for giro in 0..4 {
            for (eh, ev) in [(false, false), (true, false), (false, true), (true, true)] {
                let o = Orientacao {
                    giro_90: giro,
                    espelho_h: eh,
                    espelho_v: ev,
                };
                let (lg, ag) = if o.troca_os_eixos() {
                    (2000.0, 3000.0)
                } else {
                    (3000.0, 2000.0)
                };
                let h = p.homografia(o, lg, ag);
                let t = o.matriz();
                for ponto in [[0.3, -0.2], [-0.4, 0.25], [0.1, 0.1]] {
                    // orientar depois de corrigir == corrigir no espaço orientado
                    let a = aplicar_h(&(t * h0), ponto).unwrap();
                    let tp = aplicar_h(&t, ponto).unwrap();
                    let b = aplicar_h(&h, tp).unwrap();
                    assert!(
                        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9,
                        "giro {giro} eh {eh} ev {ev}: {a:?} × {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn numero_invalido_vira_o_neutro() {
        let p = Perspectiva::nova([f32::NAN, 0.0, 0.0], f32::INFINITY, f32::NAN, 99.0);
        assert!(p.rotacao[0] == 0.0);
        assert_eq!(p.foco, F0 as f32);
        assert_eq!(p.vertical, 0.0);
        assert_eq!(p.horizontal, AJUSTE_MAXIMO);
        let grande = Perspectiva::nova([80.0, 0.0, 0.0], 1.6, 0.0, 0.0);
        assert!((grande.rotacao[0] - ROTACAO_MAXIMA as f32).abs() < 1e-4);
    }
}
