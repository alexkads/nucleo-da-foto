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
//!
//! ## 🔑 Determinístico, e o mesmo no desktop e no navegador
//!
//! Distâncias e votos são **inteiros**, e o gerador aleatório é um xorshift com
//! semente tirada dos parâmetros do retoque. A mesma foto com a mesma receita
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
//! - **Custo em CPU** cresce com a área: dezenas de milissegundos num remendo
//!   de preview, centenas numa área grande a 24 MP. É feito uma vez por
//!   retoque e guardado; só refaz quando o retoque ou a foto mudam.

use crate::locais::Preenchimento;

/// Meio lado do patch: 7×7.
const R: i32 = 3;
/// O buraco no nível mais grosso não passa disto (em pixels).
const LADO_GROSSO: i32 = 24;
const ITERACOES_DO_PATCHMATCH: usize = 4;

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
}

impl Nivel {
    fn i(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    fn reduzido(&self) -> Nivel {
        let (w, h) = ((self.w + 1) / 2, (self.h + 1) / 2);
        let mut cor = vec![[0u8; 3]; (w * h) as usize];
        let mut buraco = vec![false; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut soma = [0u32; 3];
                let mut n = 0u32;
                let mut vazio = false;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                    if sx < self.w && sy < self.h {
                        let j = self.i(sx, sy);
                        vazio |= self.buraco[j];
                        somar(&mut soma, self.cor[j]);
                        n += 1;
                    }
                }
                let k = (y * w + x) as usize;
                cor[k] = [0, 1, 2].map(|c| ((soma[c] + n / 2) / n) as u8);
                buraco[k] = vazio;
            }
        }
        Nivel { w, h, cor, buraco }
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

    /// Centros de patch inteiros dentro da imagem e sem nenhum pixel do buraco.
    fn fontes_validas(&self) -> Vec<bool> {
        let mut valido = vec![false; (self.w * self.h) as usize];
        for y in R..self.h - R {
            for x in R..self.w - R {
                let mut ok = true;
                'patch: for dy in -R..=R {
                    for dx in -R..=R {
                        if self.buraco[self.i(x + dx, y + dy)] {
                            ok = false;
                            break 'patch;
                        }
                    }
                }
                valido[self.i(x, y)] = ok;
            }
        }
        valido
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
                for dy in -R..=R {
                    for dx in -R..=R {
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
    for dy in -R..=R {
        for dx in -R..=R {
            let (px, py) = (p.0 + dx, p.1 + dy);
            if px < 0 || py < 0 || px >= n.w || py >= n.h {
                continue;
            }
            let a = n.cor[n.i(px, py)];
            let b = n.cor[n.i(s.0 + dx, s.1 + dy)];
            for c in 0..3 {
                let e = a[c] as i64 - b[c] as i64;
                d += (e * e) as u64;
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
) {
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
}

/// Cada pixel do buraco vira a média do que os patches que o cobrem dizem.
fn votar(n: &mut Nivel, alvos: &[(i32, i32)], nnf: &[(i32, i32)]) {
    let mut soma = vec![[0u32; 3]; (n.w * n.h) as usize];
    let mut conta = vec![0u32; (n.w * n.h) as usize];
    for (p, s) in alvos.iter().zip(nnf) {
        for dy in -R..=R {
            for dx in -R..=R {
                let (qx, qy) = (p.0 + dx, p.1 + dy);
                if qx < 0 || qy < 0 || qx >= n.w || qy >= n.h {
                    continue;
                }
                let q = n.i(qx, qy);
                if !n.buraco[q] {
                    continue;
                }
                let c = n.cor[n.i(s.0 + dx, s.1 + dy)];
                for k in 0..3 {
                    soma[q][k] += c[k] as u32;
                }
                conta[q] += 1;
            }
        }
    }
    for q in 0..soma.len() {
        if conta[q] > 0 {
            let m = conta[q];
            n.cor[q] = [0, 1, 2].map(|k| ((soma[q][k] + m / 2) / m) as u8);
        }
    }
}

/// Preenche o buraco do `Preenchimento` numa imagem RGBA `largura × altura`.
///
/// `None` quando não há buraco dentro da foto, ou quando a região em volta não
/// tem nenhum patch inteiro para oferecer — aí a GPU deixa o destino como está.
pub fn preencher(rgba: &[u8], largura: u32, altura: u32, p: &Preenchimento) -> Option<Remendo> {
    let (w, h) = (largura as i32, altura as i32);
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
    let tamanho_do_buraco = ((bx1 - bx0).max(by1 - by0)) as f32;
    let margem = (tamanho_do_buraco * 0.75).max(2.0 * raio).max(24.0) as i32 + R;
    let (rx0, ry0) = ((bx0 as i32 - margem).max(0), (by0 as i32 - margem).max(0));
    let (rx1, ry1) = ((bx1 as i32 + margem).min(w), (by1 as i32 + margem).min(h));

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

    let (nw, nh) = (rx1 - rx0, ry1 - ry0);
    let mut base = Nivel {
        w: nw,
        h: nh,
        cor: vec![[0; 3]; (nw * nh) as usize],
        buraco: vec![false; (nw * nh) as usize],
    };
    for y in 0..nh {
        for x in 0..nw {
            let (gx, gy) = (rx0 + x, ry0 + y);
            let j = ((gy * w + gx) * 4) as usize;
            let i = base.i(x, y);
            base.cor[i] = [rgba[j], rgba[j + 1], rgba[j + 2]];
            base.buraco[i] = no_buraco(gx as f32 + 0.5, gy as f32 + 0.5);
        }
    }
    let (hx0, hy0, hx1, hy1) = base.caixa_do_buraco()?;

    // A pirâmide: reduz enquanto o buraco for grande e a região comportar patches.
    let mut piramide = vec![base];
    loop {
        let topo = piramide.last().expect("não vazia");
        let (a, b, c, d) = topo.caixa_do_buraco()?;
        let maior = (c - a + 1).max(d - b + 1);
        if maior <= LADO_GROSSO || topo.w < 8 * R || topo.h < 8 * R {
            break;
        }
        let menor = topo.reduzido();
        if !menor.fontes_validas().iter().any(|&v| v) {
            break;
        }
        piramide.push(menor);
    }

    let mut sorteio = Sorteio(semente(p));
    let mut anterior: Option<(i32, i32, Campo)> = None;
    for nivel in (0..piramide.len()).rev() {
        let n = &mut piramide[nivel];
        let valido = n.fontes_validas();
        let fontes: Vec<(i32, i32)> = (0..n.h)
            .flat_map(|y| (0..n.w).map(move |x| (x, y)))
            .filter(|&(x, y)| valido[(y * n.w + x) as usize])
            .collect();
        if fontes.is_empty() {
            return None;
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
            n.preencher_por_difusao();
        } else {
            votar(n, &alvos, &nnf);
        }
        let rodadas = 2 + (nivel > 0) as usize;
        for _ in 0..rodadas {
            patchmatch(n, &alvos, &mut nnf, &valido, &mut sorteio);
            votar(n, &alvos, &nnf);
        }
        if nivel == 0 {
            // 🔑 O último passo não é voto: a média de patches que discordam
            // por um pixel borra a textura (listras viram degradê). Cada pixel
            // do buraco toma o centro do patch que casou melhor com o dele.
            let mut nitido = n.cor.clone();
            for (p, s) in alvos.iter().zip(&nnf) {
                let q = n.i(p.0, p.1);
                if n.buraco[q] {
                    nitido[q] = n.cor[n.i(s.0, s.1)];
                }
            }
            n.cor = nitido;
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
    Some(Remendo {
        x0: (rx0 + hx0) as u32,
        y0: (ry0 + hy0) as u32,
        largura: largura_r,
        altura: altura_r,
        rgba: saida,
    })
}

#[cfg(test)]
mod testes {
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
}
