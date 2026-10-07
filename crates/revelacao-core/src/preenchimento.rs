//! Content-Aware: preencher uma área com o que a foto tem em volta, sem origem.
//!
//! ## O método
//!
//! Síntese por patches com **PatchMatch** (Barnes et al., 2009) dentro de uma
//! iteração **EM** (Wexler, Shechtman e Irani, 2007), em pirâmide:
//!
//! 1. O buraco é o caminho pintado com o raio do pincel (sem feather — o
//!    feather é só da mistura, na GPU). Em volta dele, uma região de trabalho
//!    com margem de `max(2 × raio, 24 px)`: é dali que saem os patches.
//! 2. A região é reduzida à metade até o buraco caber em ~24 px. No nível mais
//!    grosso, o buraco é preenchido por "casca de cebola": cada pixel recebe a
//!    média dos vizinhos conhecidos, de fora para dentro.
//! 3. Em cada nível, do grosso ao fino: para cada patch 7×7 que toca o
//!    buraco, o PatchMatch acha um patch **inteiramente conhecido** parecido
//!    (propagação + busca aleatória); depois cada pixel do buraco vira a média
//!    do que os patches que o cobrem dizem (o "voto"). Repete algumas vezes, e
//!    o campo de correspondências sobe ao nível seguinte multiplicado por dois.
//!    O voto é o do **patch que casou melhor**, e não a média: a média borra.
//! 4. No nível cheio, a **síntese coerente** (Ashikhmin) refaz o buraco casca
//!    a casca, continuando a fonte dos vizinhos já preenchidos, com o
//!    resultado do EM como guia da estrutura. O EM sozinho acerta a estrutura
//!    e perde a textura: numa estampa grande e de pouco contraste ele
//!    convergia para o liso entre os desenhos (achado no app real, 2026-09-27,
//!    num papel de parede adamascado).
//!
//! ## 🔑 Determinístico, e o mesmo no desktop e no navegador
//!
//! Distâncias e votos são **inteiros**, e o gerador aleatório é um xorshift com
//! semente tirada dos parâmetros do retoque. A mesma foto com a mesma revelação
//! dá o mesmo remendo em toda máquina, nativo ou wasm: é o mesmo Rust.
//!
//! ## ⚠️ Limites (declarados)
//!
//! - **Lê a foto original**, não o resultado dos retoques anteriores: a CPU tem
//!   os pixels de entrada, e os retoques moram na GPU. Um Content-Aware ao lado
//!   de um carimbo sintetiza como se o carimbo não existisse (a mistura, essa,
//!   respeita a ordem da cadeia).
//! - **Não entende estrutura**: continua textura e cor, mas não completa uma
//!   linha reta longa, um rosto ou uma quina — é o limite conhecido do método.
//! - **A resolução muda a escolha**: o preview sintetiza na resolução dele e a
//!   exportação na do arquivo, então o remendo final pode não ser idêntico ao
//!   da tela (parecido na textura e na cor, não pixel a pixel).
//! - **Custo em CPU** cresce com a área: ~0,6 s num remendo de 220 px no
//!   preview de 2048 (release), segundos numa área grande a 24 MP. É feito uma vez por
//!   retoque e guardado; só refaz quando o retoque ou a foto mudam.

use crate::locais::Preenchimento;

/// Meio lado do patch: 7×7.
const R: i32 = 3;
/// O buraco no nível mais grosso não passa disto (em pixels).
const LADO_GROSSO: i32 = 24;
const ITERACOES_DO_PATCHMATCH: usize = 4;
/// Rodadas de busca + voto por nível.
const RODADAS_DO_EM: usize = 4;

/// O resultado: a caixa do buraco, preenchida, em RGBA.
#[derive(Clone, Debug, PartialEq)]
pub struct Remendo {
    pub x0: u32,
    pub y0: u32,
    pub largura: u32,
    pub altura: u32,
    pub rgba: Vec<u8>,
}

struct Nivel {
    w: i32,
    h: i32,
    cor: Vec<[u8; 3]>,
    buraco: Vec<bool>,
    /// Fora da região de amostragem: não serve de fonte (mas é conhecido —
    /// a vizinhança do buraco continua contando na comparação).
    proibido: Vec<bool>,
    /// Meio lado do patch (3 = 7×7).
    r: i32,
    /// O gradiente da luminância (diferença central), quando a distância o
    /// usa — vazio quando não.
    grad: Vec<[i16; 2]>,
    peso_do_gradiente: u64,
}

impl Nivel {
    fn i(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    fn reduzido(&self) -> Nivel {
        let (w, h) = ((self.w + 1) / 2, (self.h + 1) / 2);
        let mut cor = vec![[0u8; 3]; (w * h) as usize];
        let mut buraco = vec![false; (w * h) as usize];
        let mut proibido = vec![false; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut soma = [0u32; 3];
                let mut n = 0u32;
                let mut vazio = false;
                let mut fora = false;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                    if sx < self.w && sy < self.h {
                        let j = self.i(sx, sy);
                        vazio |= self.buraco[j];
                        // Conservador: um pixel proibido no fino proíbe o grosso.
                        fora |= self.proibido[j];
                        somar(&mut soma, self.cor[j]);
                        n += 1;
                    }
                }
                let k = (y * w + x) as usize;
                cor[k] = [0, 1, 2].map(|c| ((soma[c] + n / 2) / n) as u8);
                buraco[k] = vazio;
                proibido[k] = fora;
            }
        }
        let mut menor = Nivel {
            w,
            h,
            cor,
            buraco,
            proibido,
            r: self.r,
            grad: Vec::new(),
            peso_do_gradiente: self.peso_do_gradiente,
        };
        menor.atualizar_gradiente(None);
        menor
    }

    /// Recalcula o gradiente (todo, ou só na caixa) — depois de o buraco
    /// mudar. Sem peso, não guarda nada.
    fn atualizar_gradiente(&mut self, caixa: Option<(i32, i32, i32, i32)>) {
        if self.peso_do_gradiente == 0 {
            return;
        }
        if self.grad.len() != self.cor.len() {
            self.grad = vec![[0; 2]; self.cor.len()];
        }
        let (x0, y0, x1, y1) = match caixa {
            Some((a, b, c, d)) => (
                (a - 1).max(0),
                (b - 1).max(0),
                (c + 1).min(self.w - 1),
                (d + 1).min(self.h - 1),
            ),
            None => (0, 0, self.w - 1, self.h - 1),
        };
        let luma =
            |c: [u8; 3]| ((77 * c[0] as i32 + 150 * c[1] as i32 + 29 * c[2] as i32) >> 8) as i16;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let l = |x: i32, y: i32| {
                    luma(self.cor[self.i(x.clamp(0, self.w - 1), y.clamp(0, self.h - 1))])
                };
                let i = self.i(x, y);
                self.grad[i] = [l(x + 1, y) - l(x - 1, y), l(x, y + 1) - l(x, y - 1)];
            }
        }
    }

    fn caixa_do_buraco(&self) -> Option<(i32, i32, i32, i32)> {
        let mut caixa: Option<(i32, i32, i32, i32)> = None;
        for y in 0..self.h {
            for x in 0..self.w {
                if self.buraco[self.i(x, y)] {
                    caixa = Some(match caixa {
                        None => (x, y, x, y),
                        Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
                    });
                }
            }
        }
        caixa
    }

    /// Centros de patch cujo **suporte inteiro** (os 7×7) está dentro da
    /// imagem, fora do buraco e dentro da região de amostragem — e não só o
    /// pixel do meio: um patch que encosta no objeto a remover traria um
    /// pedaço dele de volta.
    ///
    /// 🔑 Pela imagem integral dos pixels que não servem: uma conta por pixel,
    /// e não 49 — a região de amostragem pode ser a foto inteira.
    fn fontes_validas(&self) -> Vec<bool> {
        let (w, h) = (self.w as usize, self.h as usize);
        let mut valido = vec![false; w * h];
        if self.w <= 2 * self.r || self.h <= 2 * self.r {
            return valido;
        }
        // integral[(y + 1) * (w + 1) + x + 1] = quantos ruins em [0..=x]×[0..=y].
        let mut integral = vec![0u32; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut linha = 0u32;
            for x in 0..w {
                let i = y * w + x;
                linha += (self.buraco[i] || self.proibido[i]) as u32;
                integral[(y + 1) * (w + 1) + x + 1] = integral[y * (w + 1) + x + 1] + linha;
            }
        }
        let r = self.r as usize;
        for y in r..h - r {
            for x in r..w - r {
                let (x0, y0, x1, y1) = (x - r, y - r, x + r + 1, y + r + 1);
                let ruins = integral[y1 * (w + 1) + x1] + integral[y0 * (w + 1) + x0]
                    - integral[y0 * (w + 1) + x1]
                    - integral[y1 * (w + 1) + x0];
                valido[y * w + x] = ruins == 0;
            }
        }
        valido
    }

    /// Casca de cebola **com patches** (Criminisi, Pérez e Toyama, 2004): de
    /// fora para dentro, cada pixel da borda do buraco recebe o centro do
    /// patch inteiro que mais se parece com o que já se conhece em volta dele.
    ///
    /// 🚨 **A difusão (média dos vizinhos) lavava a textura** (achado no app
    /// real, 2026-09-27): o nível grosso nascia cinza e liso, a busca de
    /// patches preferia os trechos lisos para casar com ele, e o papel de
    /// parede saía sem estampa. Começando de patches de verdade, o EM só
    /// refina.
    fn preencher_por_patches(
        &mut self,
        fontes: &[(i32, i32)],
        controle: &Controle,
    ) -> Result<(), Falha> {
        let mut conhecido: Vec<bool> = self.buraco.iter().map(|b| !b).collect();
        loop {
            controle.parar()?;
            let mut borda = Vec::new();
            for y in 0..self.h {
                for x in 0..self.w {
                    if conhecido[self.i(x, y)] {
                        continue;
                    }
                    let encosta = (-1..=1).any(|dy| {
                        (-1..=1).any(|dx| {
                            let (vx, vy) = (x + dx, y + dy);
                            vx >= 0
                                && vy >= 0
                                && vx < self.w
                                && vy < self.h
                                && conhecido[self.i(vx, vy)]
                        })
                    });
                    if encosta {
                        borda.push((x, y));
                    }
                }
            }
            if borda.is_empty() {
                // Sem vizinho conhecido nenhum (não acontece com fontes).
                self.preencher_por_difusao();
                let caixa = self.caixa_do_buraco();
                self.atualizar_gradiente(caixa);
                return Ok(());
            }
            let mut novos = Vec::with_capacity(borda.len());
            for &(x, y) in &borda {
                let mut melhor = (u64::MAX, fontes[0]);
                for &f in fontes {
                    let mut d = 0u64;
                    let mut n = 0u64;
                    let r = self.r;
                    let area = ((2 * r + 1) * (2 * r + 1)) as u64;
                    'patch: for dy in -r..=r {
                        for dx in -r..=r {
                            let (px, py) = (x + dx, y + dy);
                            if px < 0 || py < 0 || px >= self.w || py >= self.h {
                                continue;
                            }
                            let i = self.i(px, py);
                            if !conhecido[i] {
                                continue;
                            }
                            let a = self.cor[i];
                            let b = self.cor[self.i(f.0 + dx, f.1 + dy)];
                            for c in 0..3 {
                                let e = a[c] as i64 - b[c] as i64;
                                d += (e * e) as u64;
                            }
                            n += 1;
                            // Com no máximo `area` pixels, o custo final é
                            // pelo menos d·64/area: se isso já perde, desiste.
                            if d * 64 / area >= melhor.0 {
                                break 'patch;
                            }
                        }
                    }
                    // Normalizado pelos pixels conhecidos: o patch da quina,
                    // com poucos, não ganha por ter menos a somar.
                    let custo = (d * 64).checked_div(n).unwrap_or(u64::MAX);
                    if custo < melhor.0 {
                        melhor = (custo, f);
                    }
                }
                novos.push((self.i(x, y), self.cor[self.i(melhor.1 .0, melhor.1 .1)]));
            }
            for (i, c) in novos {
                self.cor[i] = c;
                conhecido[i] = true;
            }
        }
    }

    /// Casca de cebola: de fora para dentro, a média dos vizinhos conhecidos.
    fn preencher_por_difusao(&mut self) {
        let mut conhecido: Vec<bool> = self.buraco.iter().map(|b| !b).collect();
        loop {
            let mut novos = Vec::new();
            for y in 0..self.h {
                for x in 0..self.w {
                    let i = self.i(x, y);
                    if conhecido[i] {
                        continue;
                    }
                    let mut soma = [0u32; 3];
                    let mut n = 0u32;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (vx, vy) = (x + dx, y + dy);
                            if (dx, dy) != (0, 0)
                                && vx >= 0
                                && vy >= 0
                                && vx < self.w
                                && vy < self.h
                            {
                                let j = self.i(vx, vy);
                                if conhecido[j] {
                                    somar(&mut soma, self.cor[j]);
                                    n += 1;
                                }
                            }
                        }
                    }
                    if n > 0 {
                        novos.push((i, [0, 1, 2].map(|c| ((soma[c] + n / 2) / n) as u8)));
                    }
                }
            }
            if novos.is_empty() {
                return;
            }
            for (i, c) in novos {
                self.cor[i] = c;
                conhecido[i] = true;
            }
        }
    }
}

fn somar(soma: &mut [u32; 3], cor: [u8; 3]) {
    for (s, c) in soma.iter_mut().zip(cor) {
        *s += c as u32;
    }
}

/// O campo de correspondências de um nível: alvo → centro do patch fonte.
type Campo = std::collections::HashMap<(i32, i32), (i32, i32)>;

/// xorshift64* — o mesmo número em toda máquina.
struct Sorteio(u64);

impl Sorteio {
    fn proximo(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn ate(&mut self, n: usize) -> usize {
        (self.proximo() % n.max(1) as u64) as usize
    }
}

/// A semente de um preenchimento: os bits dos parâmetros.
pub fn semente(p: &Preenchimento) -> u64 {
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut misturar = |v: f32| {
        s ^= v.to_bits() as u64;
        s = s.wrapping_mul(0x100_0000_01B3).rotate_left(17);
    };
    misturar(p.raio);
    for q in p.caminho.iter().chain(&p.laco) {
        misturar(q[0]);
        misturar(q[1]);
    }
    s | 1
}

/// Os pixels (centros de patch) cujo patch toca o buraco.
fn alvos(n: &Nivel) -> Vec<(i32, i32)> {
    let mut perto = vec![false; (n.w * n.h) as usize];
    for y in 0..n.h {
        for x in 0..n.w {
            if n.buraco[n.i(x, y)] {
                for dy in -n.r..=n.r {
                    for dx in -n.r..=n.r {
                        let (a, b) = (x + dx, y + dy);
                        if a >= 0 && b >= 0 && a < n.w && b < n.h {
                            perto[n.i(a, b)] = true;
                        }
                    }
                }
            }
        }
    }
    (0..n.h)
        .flat_map(|y| (0..n.w).map(move |x| (x, y)))
        .filter(|&(x, y)| perto[(y * n.w + x) as usize])
        .collect()
}

/// A distância entre o patch em `p` e o de `s`, parando quando passa de `teto`.
fn distancia(n: &Nivel, p: (i32, i32), s: (i32, i32), teto: u64) -> u64 {
    let mut d = 0u64;
    let com_gradiente = n.peso_do_gradiente > 0;
    for dy in -n.r..=n.r {
        for dx in -n.r..=n.r {
            let (px, py) = (p.0 + dx, p.1 + dy);
            if px < 0 || py < 0 || px >= n.w || py >= n.h {
                continue;
            }
            let (i, j) = (n.i(px, py), n.i(s.0 + dx, s.1 + dy));
            let (a, b) = (n.cor[i], n.cor[j]);
            for c in 0..3 {
                let e = a[c] as i64 - b[c] as i64;
                d += (e * e) as u64;
            }
            if com_gradiente {
                let (ga, gb) = (n.grad[i], n.grad[j]);
                let (ex, ey) = ((ga[0] - gb[0]) as i64, (ga[1] - gb[1]) as i64);
                d += (ex * ex + ey * ey) as u64 * n.peso_do_gradiente;
            }
        }
        if d >= teto {
            return d;
        }
    }
    d
}

fn patchmatch(
    n: &Nivel,
    alvos: &[(i32, i32)],
    nnf: &mut [(i32, i32)],
    valido: &[bool],
    sorteio: &mut Sorteio,
) -> Vec<u64> {
    let ok = |s: (i32, i32)| {
        s.0 >= 0 && s.1 >= 0 && s.0 < n.w && s.1 < n.h && valido[(s.1 * n.w + s.0) as usize]
    };
    let indice: std::collections::HashMap<(i32, i32), usize> =
        alvos.iter().enumerate().map(|(k, p)| (*p, k)).collect();
    let mut custo: Vec<u64> = alvos
        .iter()
        .zip(nnf.iter())
        .map(|(p, s)| distancia(n, *p, *s, u64::MAX))
        .collect();
    for iteracao in 0..ITERACOES_DO_PATCHMATCH {
        let frente = iteracao % 2 == 0;
        let passo = if frente { 1 } else { -1 };
        let ordem: Box<dyn Iterator<Item = usize>> = if frente {
            Box::new(0..alvos.len())
        } else {
            Box::new((0..alvos.len()).rev())
        };
        for k in ordem {
            let p = alvos[k];
            // Propagação: o vizinho já visitado sugere o vizinho do patch dele.
            for viz in [(p.0 - passo, p.1), (p.0, p.1 - passo)] {
                if let Some(&j) = indice.get(&viz) {
                    let s = nnf[j];
                    let candidato = (s.0 + (p.0 - viz.0), s.1 + (p.1 - viz.1));
                    if ok(candidato) {
                        let d = distancia(n, p, candidato, custo[k]);
                        if d < custo[k] {
                            custo[k] = d;
                            nnf[k] = candidato;
                        }
                    }
                }
            }
            // Busca aleatória em raios que caem pela metade.
            let mut raio = n.w.max(n.h);
            while raio >= 1 {
                let s = nnf[k];
                let candidato = (
                    s.0 + sorteio.ate((2 * raio + 1) as usize) as i32 - raio,
                    s.1 + sorteio.ate((2 * raio + 1) as usize) as i32 - raio,
                );
                if ok(candidato) {
                    let d = distancia(n, p, candidato, custo[k]);
                    if d < custo[k] {
                        custo[k] = d;
                        nnf[k] = candidato;
                    }
                }
                raio /= 2;
            }
        }
    }
    custo
}

/// Meio lado da vizinhança da síntese coerente: 9×9, maior que o patch do
/// EM para enxergar o desenho de uma estampa grande.
const R_COERENTE: i32 = 4;
/// O peso do guia do EM (×64 como a distância): uma diferença de cor ao
/// centro pesa tanto quanto a mesma diferença em cada pixel da vizinhança.
const PESO_DO_GUIA: u64 = 12;
/// Candidatos sorteados por pixel na síntese coerente.
const SORTEADOS: usize = 24;

/// A distância entre a vizinhança **já conhecida** de `p` e a de `f`, por
/// pixel contado (×64), parando quando passa de `teto`.
fn distancia_mascarada(
    n: &Nivel,
    conhecido: &[bool],
    p: (i32, i32),
    f: (i32, i32),
    teto: u64,
) -> u64 {
    let mut d = 0u64;
    let mut conta = 0u64;
    for dy in -R_COERENTE..=R_COERENTE {
        for dx in -R_COERENTE..=R_COERENTE {
            let (px, py) = (p.0 + dx, p.1 + dy);
            let (fx, fy) = (f.0 + dx, f.1 + dy);
            if px < 0
                || py < 0
                || px >= n.w
                || py >= n.h
                || fx < 0
                || fy < 0
                || fx >= n.w
                || fy >= n.h
            {
                continue;
            }
            let i = n.i(px, py);
            if !conhecido[i] {
                continue;
            }
            let (a, b) = (n.cor[i], n.cor[n.i(fx, fy)]);
            for c in 0..3 {
                let e = a[c] as i64 - b[c] as i64;
                d += (e * e) as u64;
            }
            conta += 1;
        }
        // Com no máximo 81 pixels, o custo final é pelo menos d·64/81.
        if d * 64 / 81 >= teto {
            return u64::MAX;
        }
    }
    (d * 64).checked_div(conta).unwrap_or(u64::MAX)
}

/// 🔑 **Síntese coerente, casca a casca, na resolução cheia** (Ashikhmin,
/// 2001, com a propagação do PatchMatch): cada pixel da borda do que falta
/// procura, entre os candidatos, a vizinhança conhecida mais parecida — e os
/// candidatos são sobretudo *continuar a fonte dos vizinhos já preenchidos*.
/// Assim uma estampa grande e de pouco contraste continua desenhada, em vez
/// de o EM convergir para os trechos lisos entre os desenhos (achado no app
/// real, 2026-09-27: o papel de parede saía cinza). O resultado do EM entra
/// como candidato também: ele traz a estrutura grossa.
fn sintese_coerente(
    n: &mut Nivel,
    valido: &[bool],
    fontes: &[(i32, i32)],
    sorteio: &mut Sorteio,
    controle: &Controle,
    fracao_antes: f32,
) -> Result<Vec<Option<(i32, i32)>>, Falha> {
    let total = n.buraco.iter().filter(|b| **b).count().max(1);
    let mut feitos = 0usize;
    // O guia: o que o EM deixou no buraco — a estrutura grossa, sem a
    // textura. Sem ele a síntese continuava o objeto que sobrou na borda
    // (o resto do violino entrava no buraco).
    let guia = n.cor.clone();
    let custo_do_guia = |p: (i32, i32), c: (i32, i32), cor: &[[u8; 3]]| -> u64 {
        let (a, b) = (
            guia[(p.1 * n.w + p.0) as usize],
            cor[(c.1 * n.w + c.0) as usize],
        );
        (0..3)
            .map(|k| {
                let e = a[k] as i64 - b[k] as i64;
                (e * e) as u64
            })
            .sum::<u64>()
            * PESO_DO_GUIA
    };
    let ok = |c: (i32, i32)| {
        c.0 >= 0 && c.1 >= 0 && c.0 < n.w && c.1 < n.h && valido[(c.1 * n.w + c.0) as usize]
    };
    let mut conhecido: Vec<bool> = n.buraco.iter().map(|b| !b).collect();
    let mut origem: Vec<Option<(i32, i32)>> = vec![None; (n.w * n.h) as usize];
    loop {
        let mut borda = Vec::new();
        for y in 0..n.h {
            for x in 0..n.w {
                let i = n.i(x, y);
                if conhecido[i] {
                    continue;
                }
                let encosta = (-1..=1).any(|dy: i32| {
                    (-1..=1).any(|dx: i32| {
                        let (vx, vy) = (x + dx, y + dy);
                        vx >= 0 && vy >= 0 && vx < n.w && vy < n.h && conhecido[n.i(vx, vy)]
                    })
                });
                if encosta {
                    borda.push((x, y));
                }
            }
        }
        if borda.is_empty() {
            return Ok(origem);
        }
        controle.parar()?;
        let mut novos = Vec::with_capacity(borda.len());
        for &(x, y) in &borda {
            let mut candidatos: Vec<(i32, i32)> = Vec::with_capacity(SORTEADOS + 30);
            // Continuar a fonte dos vizinhos já preenchidos (raio 2).
            for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    let (vx, vy) = (x + dx, y + dy);
                    if vx < 0 || vy < 0 || vx >= n.w || vy >= n.h {
                        continue;
                    }
                    if let Some(o) = origem[n.i(vx, vy)] {
                        let c = (o.0 - dx, o.1 - dy);
                        if ok(c) {
                            candidatos.push(c);
                        }
                    }
                }
            }
            for _ in 0..SORTEADOS {
                candidatos.push(fontes[sorteio.ate(fontes.len())]);
            }
            let mut melhor = (u64::MAX, candidatos[0]);
            for &c in &candidatos {
                let g = custo_do_guia((x, y), c, &n.cor);
                if g >= melhor.0 {
                    continue;
                }
                let d = distancia_mascarada(n, &conhecido, (x, y), c, melhor.0 - g);
                if d != u64::MAX && d + g < melhor.0 {
                    melhor = (d + g, c);
                }
            }
            // Busca em volta do melhor, com raios que caem pela metade.
            let mut raio = 32;
            while raio >= 1 {
                let c = (
                    melhor.1 .0 + sorteio.ate((2 * raio + 1) as usize) as i32 - raio,
                    melhor.1 .1 + sorteio.ate((2 * raio + 1) as usize) as i32 - raio,
                );
                if ok(c) {
                    let g = custo_do_guia((x, y), c, &n.cor);
                    if g < melhor.0 {
                        let d = distancia_mascarada(n, &conhecido, (x, y), c, melhor.0 - g);
                        if d != u64::MAX && d + g < melhor.0 {
                            melhor = (d + g, c);
                        }
                    }
                }
                raio /= 2;
            }
            novos.push(((x, y), melhor.1));
        }
        feitos += novos.len();
        for ((x, y), c) in novos {
            let i = n.i(x, y);
            n.cor[i] = n.cor[n.i(c.0, c.1)];
            origem[i] = Some(c);
            conhecido[i] = true;
        }
        controle.avisar(
            Etapa::Textura,
            fracao_antes + (1.0 - fracao_antes) * feitos as f32 / total as f32,
        );
    }
}

/// Cada pixel do buraco toma a cor do patch que o cobre **e casou melhor**.
///
/// 🚨 **Não a média** (achado no app real, 2026-09-27): a média de até 49
/// patches desalinhados por um pixel borra, o borrado alimenta a busca
/// seguinte, que passa a preferir os trechos lisos — e o papel de parede
/// saía cinza, sem estampa. O melhor patch mantém a textura nítida.
fn votar_pelo_melhor(n: &mut Nivel, alvos: &[(i32, i32)], nnf: &[(i32, i32)], custo: &[u64]) {
    let mut melhor = vec![(u64::MAX, [0u8; 3]); (n.w * n.h) as usize];
    for ((p, s), &c) in alvos.iter().zip(nnf).zip(custo) {
        for dy in -n.r..=n.r {
            for dx in -n.r..=n.r {
                let (qx, qy) = (p.0 + dx, p.1 + dy);
                if qx < 0 || qy < 0 || qx >= n.w || qy >= n.h {
                    continue;
                }
                let q = n.i(qx, qy);
                // Empate: o de menor índice, para ser determinístico.
                if n.buraco[q] && c < melhor[q].0 {
                    melhor[q] = (c, n.cor[n.i(s.0 + dx, s.1 + dy)]);
                }
            }
        }
    }
    for (q, (c, cor)) in melhor.into_iter().enumerate() {
        if c != u64::MAX {
            n.cor[q] = cor;
        }
    }
    let caixa = n.caixa_do_buraco();
    n.atualizar_gradiente(caixa);
}

/// Preenche o buraco do `Preenchimento` numa imagem RGBA `largura × altura`.
///
/// `None` quando não há buraco dentro da foto, ou quando a região em volta não
/// tem nenhum patch inteiro para oferecer — aí a GPU deixa o destino como está.
pub fn preencher(rgba: &[u8], largura: u32, altura: u32, p: &Preenchimento) -> Option<Remendo> {
    let lado = largura.max(altura) as f32;
    let raio = p.raio * lado;
    let caminho: Vec<[f32; 2]> = p
        .caminho
        .iter()
        .map(|q| [q[0] * largura as f32, q[1] * altura as f32])
        .collect();
    // O buraco: o laço (com a metade de fora do feather, onde a máscara ainda
    // mistura), ou o caminho pintado com o raio.
    let poligono =
        (!p.laco.is_empty()).then(|| crate::locais::laco_em_pixels(&p.laco, largura, altura));
    let meia_borda = p.feather * lado * 0.5 + 0.5;
    let (bx0, by0, bx1, by1) = match &poligono {
        Some(poly) => {
            if poly.len() < 3 {
                return None;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for q in poly {
                x0 = x0.min(q[0]);
                y0 = y0.min(q[1]);
                x1 = x1.max(q[0]);
                y1 = y1.max(q[1]);
            }
            let caixa = (
                (x0 - meia_borda).floor().max(0.0) as u32,
                (y0 - meia_borda).floor().max(0.0) as u32,
                (x1 + meia_borda).ceil().min(largura as f32) as u32,
                (y1 + meia_borda).ceil().min(altura as f32) as u32,
            );
            if caixa.2 <= caixa.0 || caixa.3 <= caixa.1 {
                return None;
            }
            caixa
        }
        None => crate::locais::caixa_de_pontos(
            caminho
                .iter()
                .map(|q| [q[0] / largura as f32, q[1] / altura as f32]),
            p.raio,
            largura,
            altura,
        )?,
    };
    let no_buraco = |x: f32, y: f32| {
        if let Some(poly) = &poligono {
            return crate::locais::distancia_ao_laco(poly, [x, y]) > -meia_borda;
        }
        let trechos: Vec<([f32; 2], [f32; 2])> = if caminho.len() == 1 {
            vec![(caminho[0], caminho[0])]
        } else {
            caminho.windows(2).map(|q| (q[0], q[1])).collect()
        };
        trechos.iter().any(|(a, b)| {
            let ab = [b[0] - a[0], b[1] - a[1]];
            let ap = [x - a[0], y - a[1]];
            let l2 = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if l2 > 1e-9 {
                ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = [ap[0] - ab[0] * t, ap[1] - ab[1] * t];
            (d[0] * d[0] + d[1] * d[1]).sqrt() < raio + 0.5
        })
    };
    preencher_na_regiao(
        rgba,
        largura,
        altura,
        (bx0, by0, bx1, by1),
        raio,
        &no_buraco,
        semente(p),
    )
}

/// A margem de trabalho em volta de um buraco — de onde saem os patches.
pub fn margem_de_trabalho(caixa: (u32, u32, u32, u32), raio: f32) -> u32 {
    let (bx0, by0, bx1, by1) = caixa;
    let tamanho_do_buraco = ((bx1 - bx0).max(by1 - by0)) as f32;
    (tamanho_do_buraco * 0.75).max(2.0 * raio).max(24.0) as u32 + R as u32
}

/// Preenche um buraco qualquer — o que `no_buraco(x, y)` diz, com o centro do
/// pixel `(x, y)` em `(x + 0,5, y + 0,5)` — dentro da caixa `(x0, y0, x1,
/// y1)`. É o caminho rápido do editor (⇧⌫ e o pincel de correção): a mesma
/// síntese, o buraco vindo de fora, a amostragem automática em volta.
pub fn preencher_buraco(
    rgba: &[u8],
    largura: u32,
    altura: u32,
    caixa: (u32, u32, u32, u32),
    no_buraco: &dyn Fn(f32, f32) -> bool,
    semente: u64,
) -> Option<Remendo> {
    preencher_na_regiao(rgba, largura, altura, caixa, 0.0, no_buraco, semente | 1)
}

fn preencher_na_regiao(
    rgba: &[u8],
    largura: u32,
    altura: u32,
    caixa: (u32, u32, u32, u32),
    raio: f32,
    no_buraco: &dyn Fn(f32, f32) -> bool,
    semente: u64,
) -> Option<Remendo> {
    let (w, h) = (largura as i32, altura as i32);
    let (bx0, by0, bx1, by1) = caixa;
    let margem = margem_de_trabalho(caixa, raio) as i32;
    let (rx0, ry0) = ((bx0 as i32 - margem).max(0), (by0 as i32 - margem).max(0));
    let (rx1, ry1) = ((bx1 as i32 + margem).min(w), (by1 as i32 + margem).min(h));
    let (nw, nh) = (rx1 - rx0, ry1 - ry0);
    if nw <= 0 || nh <= 0 {
        return None;
    }
    let mut recorte = Vec::with_capacity((nw * nh * 4) as usize);
    let mut destino = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        let (gy, j0) = (ry0 + y, (((ry0 + y) * w + rx0) * 4) as usize);
        recorte.extend_from_slice(&rgba[j0..j0 + (nw * 4) as usize]);
        for x in 0..nw {
            destino.push(no_buraco((rx0 + x) as f32 + 0.5, gy as f32 + 0.5));
        }
    }
    let pedido = Pedido {
        rgba: &recorte,
        largura: nw as u32,
        altura: nh as u32,
        destino: &destino,
        amostragem: None,
        qualidade: Qualidade::default(),
        semente,
    };
    let r = sintetizar(&pedido, &Controle::sem_controle()).ok()?;
    Some(Remendo {
        x0: r.x0 + rx0 as u32,
        y0: r.y0 + ry0 as u32,
        ..r
    })
}

// ------------------------------------------------------------ a API explícita

/// O que a síntese recebe: uma região da foto e as **duas máscaras**, que não
/// se confundem — nem com a máscara de camada do editor, nem com o peso com
/// que o remendo é aplicado (esse é de quem aplica).
pub struct Pedido<'a> {
    /// A região de trabalho, RGBA `largura × altura` (o alfa não conta).
    pub rgba: &'a [u8],
    pub largura: u32,
    pub altura: u32,
    /// A máscara de **destino**: o que será reconstruído (um por pixel).
    pub destino: &'a [bool],
    /// A máscara de **amostragem**: onde os patches de origem podem estar.
    /// `None` = toda a região fora do destino. Um patch só serve se o suporte
    /// inteiro estiver aqui e fora do destino.
    pub amostragem: Option<&'a [bool]>,
    pub qualidade: Qualidade,
    pub semente: u64,
}

/// Os botões de qualidade da síntese — o padrão é o medido como melhor
/// (`docs/editor-em-camadas/17-PREENCHIMENTO.md`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Qualidade {
    /// Rodadas de busca + voto por nível da pirâmide.
    pub rodadas: usize,
    /// A síntese coerente no nível cheio (a que guarda a textura).
    pub sintese_final: bool,
    /// Meio lado do patch da busca e do voto no nível cheio (3 = 7×7).
    pub meio_lado: i32,
    /// O mesmo nos níveis reduzidos, onde se decide a estrutura.
    pub meio_lado_grosso: i32,
    /// O peso do gradiente na distância entre patches (0 = só a cor).
    pub peso_do_gradiente: u64,
    /// Ajustar o tom do remendo à borda (a membrana): sem emenda num céu ou
    /// numa parede em degradê.
    pub harmonizar: bool,
}

impl Qualidade {
    /// A do editor em camadas, medida na bancada
    /// (`examples/bancada_do_preenchimento.rs`; os números em
    /// `docs/editor-em-camadas/17-PREENCHIMENTO.md`): patch 9×9 nos níveis
    /// grossos (a estrutura: linhas e quinas atravessam o buraco), 7×7 no
    /// cheio (a textura), o gradiente na distância e a membrana na borda (sem
    /// emenda de tom). O `Default` é o de antes, que a Revelação usa — o
    /// mesmo remendo no desktop e no site.
    pub fn recomendada() -> Self {
        Self {
            meio_lado_grosso: 4,
            peso_do_gradiente: 1,
            harmonizar: true,
            ..Self::default()
        }
    }
}

impl Default for Qualidade {
    fn default() -> Self {
        Self {
            rodadas: RODADAS_DO_EM,
            sintese_final: true,
            meio_lado: R,
            meio_lado_grosso: R,
            peso_do_gradiente: 0,
            harmonizar: false,
        }
    }
}

/// Por que não saiu remendo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Falha {
    /// Não há nada a reconstruir na região.
    SemDestino,
    /// Nenhum patch inteiro cabe na região de amostragem: é preciso ampliá-la.
    SemFontes,
    /// Quem pediu desistiu (`Controle::cancelado`).
    Cancelado,
}

/// Onde a síntese está, para a barra de progresso: etapas reais do método.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progresso {
    pub etapa: Etapa,
    /// `0..=1` dentro da síntese inteira.
    pub fracao: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Etapa {
    /// Montando a pirâmide e as fontes.
    Preparando,
    /// O nível `n` de `de` (1 = o mais grosso), na busca + voto.
    Nivel { n: usize, de: usize },
    /// A síntese coerente, casca a casca, na resolução cheia.
    Textura,
}

/// O cancelamento cooperativo e o aviso de progresso. A síntese olha o
/// `cancelado` entre as etapas e entre as cascas da síntese final.
pub struct Controle<'a> {
    pub cancelado: Option<&'a std::sync::atomic::AtomicBool>,
    pub progresso: Option<&'a (dyn Fn(Progresso) + Sync)>,
}

impl Controle<'_> {
    pub fn sem_controle() -> Self {
        Self {
            cancelado: None,
            progresso: None,
        }
    }

    fn parar(&self) -> Result<(), Falha> {
        match self.cancelado {
            Some(c) if c.load(std::sync::atomic::Ordering::Relaxed) => Err(Falha::Cancelado),
            _ => Ok(()),
        }
    }

    fn avisar(&self, etapa: Etapa, fracao: f32) {
        if let Some(p) = self.progresso {
            p(Progresso {
                etapa,
                fracao: fracao.clamp(0.0, 1.0),
            });
        }
    }
}

/// A síntese: o remendo cobre a caixa do destino, em coordenadas da região.
/// Fora do destino, o remendo traz os pixels da própria região — quem aplica
/// usa o peso dele para misturar.
pub fn sintetizar(pedido: &Pedido, controle: &Controle) -> Result<Remendo, Falha> {
    let (nw, nh) = (pedido.largura as i32, pedido.altura as i32);
    let n = (nw * nh) as usize;
    assert_eq!(pedido.rgba.len(), n * 4, "rgba do tamanho da região");
    assert_eq!(pedido.destino.len(), n, "destino do tamanho da região");
    controle.avisar(Etapa::Preparando, 0.0);
    let base = Nivel {
        w: nw,
        h: nh,
        cor: pedido
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect(),
        buraco: pedido.destino.to_vec(),
        proibido: match pedido.amostragem {
            Some(a) => {
                assert_eq!(a.len(), n, "amostragem do tamanho da região");
                a.iter().map(|ok| !ok).collect()
            }
            None => vec![false; n],
        },
        r: pedido.qualidade.meio_lado_grosso.clamp(2, 6),
        grad: Vec::new(),
        peso_do_gradiente: pedido.qualidade.peso_do_gradiente,
    };
    let mut base = base;
    base.atualizar_gradiente(None);
    let (hx0, hy0, hx1, hy1) = base.caixa_do_buraco().ok_or(Falha::SemDestino)?;
    if !base.fontes_validas().iter().any(|&v| v) {
        return Err(Falha::SemFontes);
    }

    // A pirâmide: reduz enquanto o buraco for grande e a região comportar patches.
    let mut piramide = vec![base];
    loop {
        let topo = piramide.last().expect("não vazia");
        let (a, b, c, d) = topo.caixa_do_buraco().ok_or(Falha::SemDestino)?;
        let maior = (c - a + 1).max(d - b + 1);
        if maior <= LADO_GROSSO || topo.w < 8 * topo.r || topo.h < 8 * topo.r {
            break;
        }
        let menor = topo.reduzido();
        if !menor.fontes_validas().iter().any(|&v| v) {
            break;
        }
        piramide.push(menor);
    }
    controle.parar()?;

    // O peso de cada etapa na barra: os níveis pelo tamanho, a textura pela
    // metade (é a mais cara).
    let niveis = piramide.len();
    let parte_dos_niveis = if pedido.qualidade.sintese_final {
        0.5
    } else {
        1.0
    };
    let mut sorteio = Sorteio(pedido.semente | 1);
    let mut anterior: Option<(i32, i32, Campo)> = None;
    for nivel in (0..niveis).rev() {
        let numero = niveis - nivel;
        let fracao_antes = parte_dos_niveis * (numero - 1) as f32 / niveis as f32;
        controle.avisar(
            Etapa::Nivel {
                n: numero,
                de: niveis,
            },
            fracao_antes,
        );
        let n = &mut piramide[nivel];
        // O patch do nível: o grosso decide a estrutura, o fino a textura.
        n.r = if nivel == 0 {
            pedido.qualidade.meio_lado
        } else {
            pedido.qualidade.meio_lado_grosso
        }
        .clamp(2, 6);
        let valido = n.fontes_validas();
        let fontes: Vec<(i32, i32)> = (0..n.h)
            .flat_map(|y| (0..n.w).map(move |x| (x, y)))
            .filter(|&(x, y)| valido[(y * n.w + x) as usize])
            .collect();
        if fontes.is_empty() {
            return Err(Falha::SemFontes);
        }
        let alvos = alvos(n);
        let mut nnf: Vec<(i32, i32)> = alvos
            .iter()
            .map(|&(x, y)| {
                let herdado = anterior.as_ref().and_then(|(_, _, mapa)| {
                    let s = mapa.get(&(x / 2, y / 2))?;
                    let c = (s.0 * 2 + x % 2, s.1 * 2 + y % 2);
                    (c.0 >= 0
                        && c.1 >= 0
                        && c.0 < n.w
                        && c.1 < n.h
                        && valido[(c.1 * n.w + c.0) as usize])
                        .then_some(c)
                });
                herdado.unwrap_or_else(|| fontes[sorteio.ate(fontes.len())])
            })
            .collect();
        if anterior.is_none() {
            // 🔑 O caminho sem amostragem explícita (a Revelação, o ⇧⌫) fica
            // idêntico ao de antes: o mesmo remendo no desktop e no site.
            let iniciais = if pedido.amostragem.is_some() {
                amostra_das_fontes(&fontes)
            } else {
                fontes.clone()
            };
            n.preencher_por_patches(&iniciais, controle)?;
        } else {
            let custo: Vec<u64> = alvos
                .iter()
                .zip(&nnf)
                .map(|(p, s)| distancia(n, *p, *s, u64::MAX))
                .collect();
            votar_pelo_melhor(n, &alvos, &nnf, &custo);
        }
        for rodada in 0..pedido.qualidade.rodadas {
            controle.parar()?;
            let custo = patchmatch(n, &alvos, &mut nnf, &valido, &mut sorteio);
            votar_pelo_melhor(n, &alvos, &nnf, &custo);
            controle.avisar(
                Etapa::Nivel {
                    n: numero,
                    de: niveis,
                },
                fracao_antes
                    + parte_dos_niveis * (rodada + 1) as f32
                        / (pedido.qualidade.rodadas * niveis) as f32,
            );
        }
        if nivel == 0 && pedido.qualidade.sintese_final {
            controle.avisar(Etapa::Textura, parte_dos_niveis);
            let origem = sintese_coerente(
                n,
                &valido,
                &fontes,
                &mut sorteio,
                controle,
                parte_dos_niveis,
            )?;
            if pedido.qualidade.harmonizar {
                harmonizar(n, &origem);
            }
        }
        anterior = Some((n.w, n.h, alvos.into_iter().zip(nnf).collect()));
    }

    let n = &piramide[0];
    let (largura_r, altura_r) = ((hx1 - hx0 + 1) as u32, (hy1 - hy0 + 1) as u32);
    let mut saida = Vec::with_capacity((largura_r * altura_r * 4) as usize);
    for y in hy0..=hy1 {
        for x in hx0..=hx1 {
            let c = n.cor[n.i(x, y)];
            saida.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    controle.avisar(Etapa::Textura, 1.0);
    Ok(Remendo {
        x0: hx0 as u32,
        y0: hy0 as u32,
        largura: largura_r,
        altura: altura_r,
        rgba: saida,
    })
}

/// 🔑 **A membrana** (a colagem sem emenda de Pérez, Gangnet e Blake, 2003,
/// pela interpolação rápida de Farbman et al., 2009): cada pixel do remendo
/// veio de uma origem; na borda do buraco, a diferença entre o pixel
/// conhecido da foto e o que a origem "esperava" ali é o desvio de tom. O
/// desvio é espalhado suavemente pelo buraco (push-pull numa pirâmide) e
/// somado ao remendo: a textura copiada fica, o fundo acompanha a borda.
///
/// Sem isso, num degradê (céu, parede iluminada de lado) os patches vinham de
/// outro trecho e traziam a cor de lá — uma mancha dentro do buraco (a
/// bancada, `docs/editor-em-camadas/17-PREENCHIMENTO.md`).
fn harmonizar(n: &mut Nivel, origem: &[Option<(i32, i32)>]) {
    let (w, h) = (n.w as usize, n.h as usize);
    // O desvio na borda: somado por pixel do buraco, sobre os vizinhos conhecidos.
    let mut valor = vec![[0f32; 3]; w * h];
    let mut peso = vec![0f32; w * h];
    for y in 0..n.h {
        for x in 0..n.w {
            let i = n.i(x, y);
            if !n.buraco[i] {
                continue;
            }
            let Some(o) = origem[i] else {
                continue;
            };
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (qx, qy) = (x + dx, y + dy);
                let (ex, ey) = (o.0 + dx, o.1 + dy);
                if qx < 0
                    || qy < 0
                    || qx >= n.w
                    || qy >= n.h
                    || ex < 0
                    || ey < 0
                    || ex >= n.w
                    || ey >= n.h
                {
                    continue;
                }
                let q = n.i(qx, qy);
                if n.buraco[q] {
                    continue;
                }
                let (conhecido, esperado) = (n.cor[q], n.cor[n.i(ex, ey)]);
                for c in 0..3 {
                    valor[i][c] += conhecido[c] as f32 - esperado[c] as f32;
                }
                peso[i] += 1.0;
            }
        }
    }
    if !peso.iter().any(|p| *p > 0.0) {
        return;
    }
    let desvio = puxar_e_empurrar(&valor, &peso, w, h);
    for (i, d) in desvio.iter().enumerate() {
        if n.buraco[i] {
            let c = n.cor[i];
            n.cor[i] = [0, 1, 2].map(|k| (c[k] as f32 + d[k]).round().clamp(0.0, 255.0) as u8);
        }
    }
}

/// Interpolação suave de amostras esparsas (`valor` somado, `peso` a
/// quantidade), pelo push-pull de Gortler et al. (1996): reduz somando até
/// o nível de um pixel, e volta preenchendo o que faltava com o nível de cima.
fn puxar_e_empurrar(valor: &[[f32; 3]], peso: &[f32], w: usize, h: usize) -> Vec<[f32; 3]> {
    if w <= 1 && h <= 1 {
        return vec![if peso[0] > 0.0 {
            valor[0].map(|v| v / peso[0])
        } else {
            [0.0; 3]
        }];
    }
    let (w2, h2) = (w.div_ceil(2), h.div_ceil(2));
    let mut v2 = vec![[0f32; 3]; w2 * h2];
    let mut p2 = vec![0f32; w2 * h2];
    for y in 0..h {
        for x in 0..w {
            let (i, j) = (y * w + x, (y / 2) * w2 + x / 2);
            for c in 0..3 {
                v2[j][c] += valor[i][c];
            }
            p2[j] += peso[i];
        }
    }
    let grosso = puxar_e_empurrar(&v2, &p2, w2, h2);
    let mut saida = vec![[0f32; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            // O de cima, bilinear (pelos centros dos pixels grossos).
            let (fx, fy) = ((x as f32 - 0.5) / 2.0, (y as f32 - 0.5) / 2.0);
            let (x0, y0) = (fx.floor().max(0.0) as usize, fy.floor().max(0.0) as usize);
            let (x1, y1) = ((x0 + 1).min(w2 - 1), (y0 + 1).min(h2 - 1));
            let (tx, ty) = (
                (fx - x0 as f32).clamp(0.0, 1.0),
                (fy - y0 as f32).clamp(0.0, 1.0),
            );
            let em = |a: usize, b: usize| grosso[b * w2 + a];
            let a = peso[i].min(1.0);
            for c in 0..3 {
                let topo = em(x0, y0)[c] * (1.0 - tx) + em(x1, y0)[c] * tx;
                let baixo = em(x0, y1)[c] * (1.0 - tx) + em(x1, y1)[c] * tx;
                let de_cima = topo * (1.0 - ty) + baixo * ty;
                // Onde há amostra, ela pesa conforme a quantidade (até 1).
                let proprio = if peso[i] > 0.0 {
                    valor[i][c] / peso[i]
                } else {
                    0.0
                };
                saida[i][c] = proprio * a + de_cima * (1.0 - a);
            }
        }
    }
    saida
}

/// No nível mais grosso, a casca de cebola compara cada pixel com **todas**
/// as fontes. Com a amostragem na foto inteira, seriam dezenas de milhares;
/// uma amostra regular (determinística) de até 4096 basta para começar — o
/// EM refina depois com a busca do PatchMatch, que vê todas.
fn amostra_das_fontes(fontes: &[(i32, i32)]) -> Vec<(i32, i32)> {
    const TETO: usize = 4096;
    if fontes.len() <= TETO {
        return fontes.to_vec();
    }
    let passo = fontes.len() as f64 / TETO as f64;
    (0..TETO)
        .map(|k| fontes[(k as f64 * passo) as usize])
        .collect()
}

#[cfg(test)]
mod testes {

    /// 🚨 **Estampa grande e de pouco contraste não pode sair lisa** (o papel
    /// de parede adamascado do app real, 2026-09-27): o EM sozinho convergia
    /// para o liso entre os desenhos. O remendo tem de guardar pelo menos
    /// metade da variação que a estampa tem em volta.
    #[test]
    fn numa_estampa_grande_e_suave_o_remendo_nao_fica_liso() {
        let (w, h) = (192u32, 192u32);
        let img: Vec<u8> = (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    // Bolinhas de raio 9 numa grade de 40 px, 24 níveis acima do fundo.
                    let (cx, cy) = ((x % 40) as i32 - 20, (y % 40) as i32 - 20);
                    let v = 120
                        + if cx * cx + cy * cy < 81 { 24 } else { 0 }
                        + ((x * 7 + y * 13) % 5) as u8;
                    [v, v, v, 255]
                })
            })
            .collect();
        let p = buraco(vec![[0.45, 0.5], [0.55, 0.5]], 0.12);
        let r = preencher(&img, w, h, &p).expect("remendo");
        let desvio = |vals: &[f32]| {
            let m = vals.iter().sum::<f32>() / vals.len() as f32;
            (vals.iter().map(|v| (v - m).powi(2)).sum::<f32>() / vals.len() as f32).sqrt()
        };
        // Todo o buraco (o caminho com o raio) contra a foto inteira.
        let raio = 0.12 * w as f32;
        let (ax, bx, cy) = (0.45 * w as f32, 0.55 * w as f32, 0.5 * h as f32);
        let miolo: Vec<f32> = (0..r.altura)
            .flat_map(|y| (0..r.largura).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let (gx, gy) = ((r.x0 + x) as f32 + 0.5, (r.y0 + y) as f32 + 0.5);
                let dx = gx - gx.clamp(ax, bx);
                (dx * dx + (gy - cy).powi(2)).sqrt() < raio - 2.0
            })
            .map(|(x, y)| r.rgba[((y * r.largura + x) * 4) as usize] as f32)
            .collect();
        let foto: Vec<f32> = img.iter().step_by(4).map(|&v| v as f32).collect();
        let (dm, df) = (desvio(&miolo), desvio(&foto));
        assert!(
            dm > 0.5 * df,
            "o miolo ficou liso: desvio {dm:.1} contra {df:.1} na estampa"
        );
    }
    use super::*;

    /// Listras verticais de 4 px (escuro/claro) com um ruído fixo por cima.
    fn listras(w: u32, h: u32) -> Vec<u8> {
        (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    let v = if (x / 4) % 2 == 0 { 60 } else { 180 } + ((x * 7 + y * 13) % 9) as u8;
                    [v, v, v, 255]
                })
            })
            .collect()
    }

    fn buraco(caminho: Vec<[f32; 2]>, raio: f32) -> Preenchimento {
        Preenchimento {
            caminho,
            raio,
            feather: 0.3,
            opacidade: 1.0,
            laco: Vec::new(),
        }
    }

    /// Numa textura regular, o preenchimento continua a textura: as listras
    /// seguem pelo buraco, e não viram uma mancha cinza (que é o que a média,
    /// ou a difusão sozinha, dariam).
    #[test]
    fn continua_a_textura_em_vez_de_borrar() {
        let (w, h) = (128, 96);
        let img = listras(w, h);
        let r = preencher(&img, w, h, &buraco(vec![[0.5, 0.5]], 0.06)).expect("remendo");
        let meio = r.altura / 2;
        let linha: Vec<u8> = (0..r.largura)
            .map(|x| r.rgba[((meio * r.largura + x) * 4) as usize])
            .collect();
        let escuros = linha.iter().filter(|&&v| v < 100).count();
        let claros = linha.iter().filter(|&&v| v > 150).count();
        assert!(
            escuros > 3 && claros > 3,
            "listras no meio do remendo: {linha:?}"
        );
        let cinzas = linha.iter().filter(|&&v| (100..=150).contains(&v)).count();
        assert!(
            cinzas * 4 < linha.len(),
            "quase nada de cinza intermediário: {linha:?}"
        );
    }

    /// Numa área lisa, o remendo tem a cor da área.
    #[test]
    fn numa_area_lisa_o_remendo_tem_a_cor_dela() {
        let (w, h) = (100, 80);
        let mut img = vec![0u8; (w * h * 4) as usize];
        for p in img.chunks_mut(4) {
            p.copy_from_slice(&[30, 120, 200, 255]);
        }
        // Uma "sujeira" no meio, que o buraco cobre.
        for y in 38..42 {
            for x in 48..52 {
                img[((y * w + x) * 4) as usize] = 255;
            }
        }
        let r = preencher(&img, w, h, &buraco(vec![[0.5, 0.5]], 0.05)).unwrap();
        assert!(
            r.rgba.chunks(4).all(|p| p[..3] == [30, 120, 200]),
            "a sujeira sumiu"
        );
    }

    #[test]
    fn e_deterministico() {
        let (w, h) = (96, 64);
        let img = listras(w, h);
        let p = buraco(vec![[0.3, 0.4], [0.6, 0.5]], 0.04);
        assert_eq!(preencher(&img, w, h, &p), preencher(&img, w, h, &p));
    }

    #[test]
    fn buraco_fora_da_foto_nao_da_remendo() {
        let img = listras(40, 40);
        assert!(preencher(&img, 40, 40, &buraco(vec![[3.0, 3.0]], 0.05)).is_none());
    }

    /// O buraco vindo de fora (o editor em camadas): um quadrado num fundo
    /// listrado sai listrado, e o resultado não depende da máquina.
    #[test]
    fn preencher_um_buraco_qualquer() {
        let (w, h) = (160u32, 120u32);
        let mut img = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let k = ((y * w + x) * 4) as usize;
                let v = if (x / 6) % 2 == 0 { 200 } else { 40 };
                img[k..k + 4].copy_from_slice(&[v, v, v, 255]);
                if (70..90).contains(&x) && (50..70).contains(&y) {
                    img[k..k + 4].copy_from_slice(&[255, 0, 0, 255]);
                }
            }
        }
        let no = |x: f32, y: f32| (70.0..90.0).contains(&x) && (50.0..70.0).contains(&y);
        let r = preencher_buraco(&img, w, h, (70, 50, 90, 70), &no, 7).expect("remendo");
        assert_eq!((r.x0, r.y0, r.largura, r.altura), (70, 50, 20, 20));
        let vermelhos = r
            .rgba
            .chunks(4)
            .filter(|p| p[0] == 255 && p[1] == 0)
            .count();
        assert_eq!(vermelhos, 0, "nada do vermelho de antes");
        let escuros = r.rgba.chunks(4).filter(|p| p[0] < 100).count();
        assert!(
            escuros > 50 && escuros < 350,
            "listras: {escuros} escuros de 400"
        );
        assert_eq!(
            preencher_buraco(&img, w, h, (70, 50, 90, 70), &no, 7),
            Some(r),
            "determinístico"
        );
    }

    fn cena(w: u32, h: u32, k: u32) -> Vec<u8> {
        (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    let v = ((x * (3 + k) + y * 7 + (x / 9) * 40 * k) % 251) as u8;
                    [v, v.wrapping_mul(3), 255 - v, 255]
                })
            })
            .collect()
    }

    fn digest(r: &Option<Remendo>) -> u64 {
        let Some(r) = r else { return 0 };
        let mut s = 1469598103934665603u64;
        for b in [r.x0, r.y0, r.largura, r.altura] {
            s = (s ^ b as u64).wrapping_mul(1099511628211);
        }
        for b in &r.rgba {
            s = (s ^ *b as u64).wrapping_mul(1099511628211);
        }
        s
    }

    /// 🔑 **O caminho antigo não muda** (a Revelação, no desktop e no site, e o
    /// ⇧⌫ do editor): os digests do remendo de antes da API explícita (0.1.105),
    /// conferidos lado a lado com a versão antiga ao introduzi-la. Uma mudança
    /// aqui muda o remendo de um retoque já salvo — tem de ser de propósito.
    #[test]
    fn o_caminho_sem_amostragem_explicita_e_o_de_antes() {
        let mut todos = Vec::new();
        for (k, (w, h)) in [(160u32, 120u32), (300, 200), (96, 64)]
            .into_iter()
            .enumerate()
        {
            let img = cena(w, h, k as u32 + 1);
            for p in [
                Preenchimento {
                    caminho: vec![[0.5, 0.5]],
                    raio: 0.06,
                    feather: 0.3,
                    opacidade: 1.0,
                    laco: vec![],
                },
                Preenchimento {
                    caminho: vec![[0.2, 0.3], [0.7, 0.6]],
                    raio: 0.05,
                    feather: 0.2,
                    opacidade: 1.0,
                    laco: vec![],
                },
                Preenchimento {
                    caminho: vec![],
                    raio: 0.0,
                    feather: 0.1,
                    opacidade: 1.0,
                    laco: vec![[0.3, 0.3], [0.7, 0.35], [0.6, 0.8], [0.25, 0.7]],
                },
            ] {
                todos.push(digest(&preencher(&img, w, h, &p)));
            }
            let no = |x: f32, y: f32| (x - w as f32 / 2.0).hypot(y - h as f32 / 2.0) < 18.0;
            let caixa = (w / 2 - 19, h / 2 - 19, w / 2 + 19, h / 2 + 19);
            todos.push(digest(&preencher_buraco(&img, w, h, caixa, &no, 99)));
        }
        assert_eq!(
            todos,
            [
                18398676620468618431,
                13096456220126987480,
                13893262335008878189,
                10498634739965498386,
                10845913225970734763,
                16140487258107637557,
                8930530941849441264,
                15473149926752919257,
                13996332391340220872,
                202300192443387874,
                6934185511868767335,
                5722963151573765387
            ]
        );
    }

    // ------------------------------------------------ a API explícita

    /// Uma região listrada em cinza com uma faixa **vermelha** à direita do
    /// buraco: a cor que não pode aparecer quando a faixa é proibida.
    fn com_faixa_proibida(w: u32, h: u32) -> (Vec<u8>, Vec<bool>, Vec<bool>) {
        let img: Vec<u8> = (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    if (90..110).contains(&x) {
                        [230, 20, 20, 255]
                    } else {
                        let v = if (x / 5 + y / 5) % 2 == 0 { 70 } else { 170 };
                        [v, v, v, 255]
                    }
                })
            })
            .collect();
        let destino: Vec<bool> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (60..88).contains(&x) && (40..70).contains(&y)))
            .collect();
        // Amostragem: tudo, menos a faixa vermelha.
        let amostragem: Vec<bool> = (0..h)
            .flat_map(|_| (0..w).map(|x| !(90..110).contains(&x)))
            .collect();
        (img, destino, amostragem)
    }

    fn vermelhos(r: &Remendo) -> usize {
        r.rgba.chunks(4).filter(|p| p[0] > 200 && p[1] < 60).count()
    }

    #[test]
    fn a_regiao_proibida_nunca_vira_fonte_nem_pela_borda_do_patch() {
        let (w, h) = (160u32, 110u32);
        let (img, destino, amostragem) = com_faixa_proibida(w, h);
        let base = Pedido {
            rgba: &img,
            largura: w,
            altura: h,
            destino: &destino,
            amostragem: Some(&amostragem),
            qualidade: Qualidade::default(),
            semente: 5,
        };
        let r = sintetizar(&base, &Controle::sem_controle()).unwrap();
        assert_eq!((r.x0, r.y0, r.largura, r.altura), (60, 40, 28, 30));
        assert_eq!(vermelhos(&r), 0, "nada da faixa proibida");
        // Os patches que só encostam nela também não servem: o suporte
        // inteiro conta.
        let livre = sintetizar(
            &Pedido {
                amostragem: None,
                ..base
            },
            &Controle::sem_controle(),
        )
        .unwrap();
        let _ = livre; // pode ou não trazer vermelho; o que importa é o de cima.
        let valido = {
            let n = Nivel {
                w: w as i32,
                h: h as i32,
                cor: vec![[0; 3]; (w * h) as usize],
                buraco: destino.clone(),
                proibido: amostragem.iter().map(|a| !a).collect(),
                r: R,
                grad: Vec::new(),
                peso_do_gradiente: 0,
            };
            n.fontes_validas()
        };
        for y in 0..h as usize {
            for x in 84..113 {
                if valido[y * w as usize + x] {
                    assert!(
                        x + 3 < 90 || x >= 110 + 3,
                        "o patch em {x} encosta na faixa"
                    );
                }
            }
        }
    }

    #[test]
    fn sem_fontes_validas_a_falha_e_clara_e_cancelar_para() {
        let (w, h) = (160u32, 110u32);
        let (img, destino, _) = com_faixa_proibida(w, h);
        let nada = vec![false; (w * h) as usize];
        let pedido = Pedido {
            rgba: &img,
            largura: w,
            altura: h,
            destino: &destino,
            amostragem: Some(&nada),
            qualidade: Qualidade::default(),
            semente: 5,
        };
        assert_eq!(
            sintetizar(&pedido, &Controle::sem_controle()),
            Err(Falha::SemFontes)
        );
        let sem_destino = Pedido {
            destino: &nada,
            amostragem: None,
            ..pedido
        };
        assert_eq!(
            sintetizar(&sem_destino, &Controle::sem_controle()),
            Err(Falha::SemDestino)
        );

        let cancelado = std::sync::atomic::AtomicBool::new(true);
        let pedido = Pedido {
            amostragem: None,
            destino: &destino,
            ..sem_destino
        };
        let controle = Controle {
            cancelado: Some(&cancelado),
            progresso: None,
        };
        assert_eq!(sintetizar(&pedido, &controle), Err(Falha::Cancelado));
    }

    #[test]
    fn o_progresso_anda_pelas_etapas_ate_o_fim() {
        let (w, h) = (200u32, 160u32);
        let img = listras(w, h);
        let destino: Vec<bool> = (0..h)
            .flat_map(|y| {
                (0..w).map(move |x| (x as i32 - 100).pow(2) + (y as i32 - 80).pow(2) < 40 * 40)
            })
            .collect();
        let visto = std::sync::Mutex::new(Vec::new());
        let anotar = |p: Progresso| visto.lock().unwrap().push(p);
        let controle = Controle {
            cancelado: None,
            progresso: Some(&anotar),
        };
        let pedido = Pedido {
            rgba: &img,
            largura: w,
            altura: h,
            destino: &destino,
            amostragem: None,
            qualidade: Qualidade::default(),
            semente: 3,
        };
        sintetizar(&pedido, &controle).unwrap();
        let visto = visto.into_inner().unwrap();
        assert_eq!(visto.first().unwrap().etapa, Etapa::Preparando);
        assert!(visto
            .iter()
            .any(|p| matches!(p.etapa, Etapa::Nivel { n: 1, de } if de >= 2)));
        assert!(visto.iter().any(|p| p.etapa == Etapa::Textura));
        assert_eq!(visto.last().unwrap().fracao, 1.0);
        assert!(
            visto.windows(2).all(|p| p[1].fracao >= p[0].fracao),
            "nunca anda para trás"
        );
    }

    #[test]
    fn buracos_pequenos_grandes_desconectados_e_na_borda_da_foto() {
        let (w, h) = (240u32, 180u32);
        let img = listras(w, h);
        type Caso = (&'static str, Box<dyn Fn(u32, u32) -> bool>);
        let casos: Vec<Caso> = vec![
            ("um pixel", Box::new(|x, y| (x, y) == (120, 90))),
            (
                "grande",
                Box::new(|x, y| (40..200).contains(&x) && (50..130).contains(&y)),
            ),
            (
                "dois pedaços",
                Box::new(|x, y| {
                    ((20..40).contains(&x) && (20..40).contains(&y))
                        || ((180..210).contains(&x) && (120..150).contains(&y))
                }),
            ),
            ("na quina", Box::new(|x, y| x < 25 && y < 30)),
            (
                "na borda de baixo",
                Box::new(|x, y| (100..140).contains(&x) && y >= 160),
            ),
        ];
        for (nome, no) in casos {
            let destino: Vec<bool> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| no(x, y))
                .collect();
            let pedido = Pedido {
                rgba: &img,
                largura: w,
                altura: h,
                destino: &destino,
                amostragem: None,
                qualidade: Qualidade::default(),
                semente: 11,
            };
            let r = sintetizar(&pedido, &Controle::sem_controle())
                .unwrap_or_else(|e| panic!("{nome}: {e:?}"));
            // Fora do destino, o remendo é a própria foto.
            for y in 0..r.altura {
                for x in 0..r.largura {
                    let (gx, gy) = (r.x0 + x, r.y0 + y);
                    if !no(gx, gy) {
                        let i = ((gy * w + gx) * 4) as usize;
                        let j = ((y * r.largura + x) * 4) as usize;
                        assert_eq!(
                            r.rgba[j..j + 3],
                            img[i..i + 3],
                            "{nome}: ({gx}, {gy}) fora do destino mudou"
                        );
                    }
                }
            }
            // E dentro, as listras continuam (claros e escuros, pouco cinza).
            let dentro: Vec<u8> = (0..r.altura)
                .flat_map(|y| (0..r.largura).map(move |x| (x, y)))
                .filter(|&(x, y)| no(r.x0 + x, r.y0 + y))
                .map(|(x, y)| r.rgba[((y * r.largura + x) * 4) as usize])
                .collect();
            if dentro.len() > 50 {
                let cinza = dentro.iter().filter(|v| (100..=150).contains(*v)).count();
                assert!(
                    cinza * 4 < dentro.len(),
                    "{nome}: {cinza} de {} cinza",
                    dentro.len()
                );
            }
        }
    }
}
