//! Os 144 ajustes, no layout que o WGSL espera.

use serde::{Deserialize, Serialize};

/// Os ajustes, no layout que o WGSL espera.
///
/// 🚨 **Os nomes e a ordem são os do shader, e ficam em inglês de propósito.**
/// `repr(C)` + `bytemuck` mandam esta struct para a GPU como bytes crus, campo a
/// campo, por **posição**. Um campo fora de lugar aqui não é erro de compilação,
/// é a foto saindo com o ajuste errado aplicado — e poder ler os dois lados um ao
/// lado do outro é a única defesa que existe.
///
/// 🚨 **E ela já falhou uma vez: o `uniform` do outro lado declarava 28 campos.**
/// Do campo 23 em diante o shader lia o do vizinho (o matiz do vermelho virava
/// redução de ruído) e do 28 em diante não lia nada — os 4 controles de Detalhe e
/// os 3 de Lente não faziam efeito nenhum. Defeito herdado do `crates/ui`, que
/// mandava a mesma struct para o mesmo shader, e **consertado em 17/ago/2026**,
/// depois que a fase 5 tirou o outro app do caminho: o `struct Params` passou a
/// declarar os 46 na mesma ordem, e quem prende isso é
/// `o_wgsl_declara_os_mesmos_campos_na_mesma_ordem`.
///
/// ✅ **E desde 17/ago/2026 os 46 campos chegam ao shader e todos têm código que
/// os use.** Chegar e ser aplicado são duas coisas, e as duas custaram um
/// conserto próprio no mesmo dia: o alinhamento do `uniform`, e depois o corpo do
/// shader, que não mencionava matiz, luminância nem lente em lugar nenhum.
///
/// 🔑 **Desde 2026-09-04 a struct viaja também como JSON por nome** (`serde`),
/// para o site guardar a revelação de uma foto. Campo ausente no JSON recebe o
/// neutro de [`Ajustes::default`] — o mesmo `if let Some` de `ajustes_da_entidade`.
///
/// ✅ **Em 2026-09-06 entraram os 7 da Tonalização e do Grão**, e eles entraram
/// **no fim**, depois de `sharpen_radius`. Uma revelação gravada antes disso não
/// tem os campos novos no JSON, e o `serde(default)` os completa com o neutro —
/// que é o mesmo que "sem tonalização e sem grão". Fosse a inserção no meio,
/// toda revelação já gravada leria o campo do vizinho a partir dali.
#[repr(C)]
#[derive(
    Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, Serialize, Deserialize,
)]
#[serde(default)]
pub struct Ajustes {
    // ------------------------------------------------------------ Básico
    /// Exposição, em EV — a `Exposição` do Lightroom. Neutro 0.
    ///
    /// No processo 0 multiplica o valor sRGB por `2^EV`, sem limite; no
    /// processo 1 lê a curva medida no Lightroom, presa em −5 a +5 EV (o
    /// alcance do slider), que guarda o branco em vez de estourá-lo.
    pub exposure: f32,
    /// Contraste, como **multiplicador**: `1 + contraste/100` do Lightroom, de
    /// 0,0 (−100) a 2,0 (+100). 🔑 Neutro **1,0**, e não zero.
    ///
    /// No processo 0 é a reta em torno do cinza 128 (`(v − 128)·c + 128`, e
    /// 0,0 vira cinza chapado); no processo 1, a curva medida, que guarda o
    /// preto e o branco nas duas pontas.
    pub contrast: f32,
    /// Temperatura, de −10 (frio) a +10 (quente); neutro 0 — a `Temperatura`
    /// relativa que o Lightroom mostra num JPEG, dividida por 10.
    ///
    /// No processo 0 soma `10·v` níveis ao vermelho e tira do azul, depois do
    /// contraste; no processo 1 lê o balanço medido (o slider de −100 a +100,
    /// preso nessa faixa), antes da exposição.
    pub temperature: f32,
    /// Matiz do balanço de branco, de −10 (verde) a +10 (magenta); neutro 0 —
    /// o `Matiz` do Lightroom dividido por 10. No processo 0 move `5·v` níveis
    /// (vermelho e azul para um lado, verde para o outro).
    pub tint: f32,
    /// Realces, de −100 (recupera) a +100; neutro 0 — os `Realces` do
    /// Lightroom. Age no alto da escala sem mover o ponto de branco; no
    /// processo 1 é local (age na base, pela guia — ver [`Ajustes::usa_a_guia`]).
    pub highlights: f32,
    /// Sombras, de −100 a +100 (clareia); neutro 0 — as `Sombras` do
    /// Lightroom. No processo 1 é local, como os Realces.
    pub shadows: f32,
    /// Brancos, de −100 a +100; neutro 0 — os `Brancos` do Lightroom, o topo da
    /// escala.
    pub whites: f32,
    /// Pretos, de −100 a +100; neutro 0 — os `Pretos` do Lightroom, o pé da
    /// escala.
    pub blacks: f32,
    /// Claridade, em **fração**: a `Claridade` do Lightroom dividida por 100
    /// (−1 a +1); neutro 0. Contraste local largo (σ = 1,2 % do lado maior),
    /// pela guia — ver [`crate::guia`].
    pub clarity: f32,
    /// Vibração, em **fração**: a `Vibração` do Lightroom dividida por 100
    /// (−1 a +1); neutro 0. Não age com o P&B ligado ([`Ajustes::bw_ativo`]).
    pub vibrance: f32,
    /// Saturação, em **fração**: a `Saturação` do Lightroom dividida por 100
    /// (−1 a +1, e −1 é cinza no processo 0); neutro 0. Não age com o P&B
    /// ligado — para P&B, ligue [`Ajustes::bw_ativo`] em vez de zerar a cor.
    pub saturation: f32,
    // ------------------------------------------ Curva paramétrica
    /// Curva paramétrica, zona das **Sombras** (de 0 até
    /// [`Ajustes::tone_curve_split_shadows`]), de −100 a +100; neutro 0.
    pub tone_curve_shadows: f32,
    /// Curva paramétrica, zona dos **Escuros** (entre a primeira e a segunda
    /// divisão), de −100 a +100; neutro 0.
    pub tone_curve_darks: f32,
    /// Curva paramétrica, zona dos **Claros** (entre a segunda e a terceira
    /// divisão), de −100 a +100; neutro 0.
    pub tone_curve_lights: f32,
    /// Curva paramétrica, zona dos **Realces** (da terceira divisão até o
    /// branco), de −100 a +100; neutro 0.
    pub tone_curve_highlights: f32,
    // ------------------------------------------ HSL: saturação
    // As oito faixas do HSL do Lightroom, com centro e meia-largura em graus
    // de matiz: vermelho 0 (±15), laranja 30 (±15), amarelo 60 (±15), verde
    // 120 (±45), água 187,5 (±22,5), azul 240 (±30), roxo 290 (±20) e magenta
    // 327,5 (±17,5). O peso cai linear do centro à borda da faixa.
    /// HSL, saturação do **vermelho** (matiz 0°, ±15°), de −100 a +100;
    /// neutro 0.
    pub hsl_red_sat: f32,
    /// HSL, saturação do **laranja** (30°, ±15°), de −100 a +100; neutro 0.
    pub hsl_orange_sat: f32,
    /// HSL, saturação do **amarelo** (60°, ±15°), de −100 a +100; neutro 0.
    pub hsl_yellow_sat: f32,
    /// HSL, saturação do **verde** (120°, ±45°), de −100 a +100; neutro 0.
    pub hsl_green_sat: f32,
    /// HSL, saturação da **água** (187,5°, ±22,5°), de −100 a +100; neutro 0.
    pub hsl_aqua_sat: f32,
    /// HSL, saturação do **azul** (240°, ±30°), de −100 a +100; neutro 0.
    pub hsl_blue_sat: f32,
    /// HSL, saturação do **roxo** (290°, ±20°), de −100 a +100; neutro 0.
    pub hsl_purple_sat: f32,
    /// HSL, saturação do **magenta** (327,5°, ±17,5°), de −100 a +100;
    /// neutro 0.
    pub hsl_magenta_sat: f32,
    // ------------------------------------------ HSL: matiz
    /// HSL, giro do matiz do **vermelho**, **em graus** (−180 a +180, aplicado
    /// sem escala); neutro 0. O cinza não gira: o efeito pesa pela saturação do
    /// pixel. 🔑 Não é a escala do Lightroom: o `Matiz` dele (−100 a +100) chega
    /// aqui multiplicado por 0,3 (±30°, o giro medido no extremo do controle),
    /// e é quem traduz o XMP que faz a conta. Vale para os oito `hsl_*_hue`.
    pub hsl_red_hue: f32,
    /// HSL, giro do matiz do **laranja**, em graus (−180 a +180); neutro 0.
    pub hsl_orange_hue: f32,
    /// HSL, giro do matiz do **amarelo**, em graus (−180 a +180); neutro 0.
    pub hsl_yellow_hue: f32,
    /// HSL, giro do matiz do **verde**, em graus (−180 a +180); neutro 0.
    pub hsl_green_hue: f32,
    /// HSL, giro do matiz da **água**, em graus (−180 a +180); neutro 0.
    pub hsl_aqua_hue: f32,
    /// HSL, giro do matiz do **azul**, em graus (−180 a +180); neutro 0.
    pub hsl_blue_hue: f32,
    /// HSL, giro do matiz do **roxo**, em graus (−180 a +180); neutro 0.
    pub hsl_purple_hue: f32,
    /// HSL, giro do matiz do **magenta**, em graus (−180 a +180); neutro 0.
    pub hsl_magenta_hue: f32,
    // ------------------------------------------ HSL: luminância
    /// HSL, luminância do **vermelho**, de −100 (rumo ao preto) a +100 (rumo
    /// ao branco), proporcional e sem estourar; neutro 0. O cinza não muda: o
    /// efeito pesa pela saturação do pixel.
    pub hsl_red_lum: f32,
    /// HSL, luminância do **laranja**, de −100 a +100; neutro 0.
    pub hsl_orange_lum: f32,
    /// HSL, luminância do **amarelo**, de −100 a +100; neutro 0.
    pub hsl_yellow_lum: f32,
    /// HSL, luminância do **verde**, de −100 a +100; neutro 0.
    pub hsl_green_lum: f32,
    /// HSL, luminância da **água**, de −100 a +100; neutro 0.
    pub hsl_aqua_lum: f32,
    /// HSL, luminância do **azul**, de −100 a +100; neutro 0.
    pub hsl_blue_lum: f32,
    /// HSL, luminância do **roxo**, de −100 a +100; neutro 0.
    pub hsl_purple_lum: f32,
    /// HSL, luminância do **magenta**, de −100 a +100; neutro 0.
    pub hsl_magenta_lum: f32,
    // ------------------------------------------ Lente
    /// Distorção da lente, de −100 a +100; neutro 0 — a `Distorção` das
    /// correções manuais do Lightroom. Modelo radial `r' = r·(1 + k·r²)`, com
    /// `k = v·0,005` e raio 1 no canto: negativo puxa os cantos para dentro
    /// (corrige barril), positivo empurra para fora (corrige almofada).
    pub lens_distortion: f32,
    /// Vinheta da lente, quantidade de −100 (escurece os cantos) a +100
    /// (clareia); neutro 0 — a `Vinheta` das correções de lente do Lightroom.
    /// Ganho `1 + v/100·t²`, medido no quadro do arquivo que sai.
    pub lens_vignette_amount: f32,
    /// Vinheta da lente, ponto onde a queda **começa**, de 0 (no centro) a 100
    /// (no canto), em fração da distância centro–canto; neutro **0** (ver
    /// [`Ajustes::default`] para a história do 50).
    pub lens_vignette_midpoint: f32,
    // ------------------------------------------ Detalhe
    /// Redução de ruído da luminância, de 0 a 100; neutro 0 (só age acima de
    /// zero) — a `Luminância` da Redução de ruído do Lightroom. Filtro
    /// bilateral 5×5.
    pub nr_luminance: f32,
    /// Redução de ruído da cor, de 0 a 100; neutro 0 (só age acima de zero) —
    /// a `Cor` da Redução de ruído do Lightroom. Desfoque do U/V numa
    /// vizinhança 5×5.
    pub nr_color: f32,
    /// Nitidez, quantidade de 0 a 100; neutro 0 (só age acima de zero) — a
    /// `Quantidade` da Nitidez do Lightroom. Máscara de nitidez sobre a
    /// luminância.
    pub sharpen_amount: f32,
    /// Nitidez, raio do desfoque da máscara, em pixels (σ, com piso de 0,5 e
    /// a vizinhança 5×5 como teto prático); neutro **1,0** — o `Raio` do
    /// Lightroom. Só conta com [`Ajustes::sharpen_amount`] acima de zero.
    pub sharpen_radius: f32,
    // ------------------------------------------ Tonalização (Divisão de tons)
    /// Divisão de tons, matiz das **sombras**, em graus (0 a 360, dá a volta);
    /// neutro 0 — o matiz da roda de Sombras da Gradação de cores do
    /// Lightroom. Sem [`Ajustes::split_shadow_sat`] não faz nada.
    pub split_shadow_hue: f32,
    /// Divisão de tons, saturação das **sombras**, de 0 a 100 (preso nessa
    /// faixa); neutro 0.
    pub split_shadow_sat: f32,
    /// Divisão de tons, matiz dos **realces**, em graus (0 a 360); neutro 0.
    pub split_highlight_hue: f32,
    /// Divisão de tons, saturação dos **realces**, de 0 a 100; neutro 0.
    pub split_highlight_sat: f32,
    /// Equilíbrio entre sombras e realces, de −100 a +100 (preso); neutro 0 —
    /// o `Equilíbrio` do Lightroom. Positivo dá mais foto aos realces,
    /// negativo às sombras.
    pub split_balance: f32,
    // ------------------------------------------ Grão
    /// Grão, quantidade de 0 a 100 (preso); neutro 0 — a `Quantidade` da
    /// Granulação do Lightroom. Monocromático, e some no preto e no branco.
    pub grain_amount: f32,
    /// Grão, tamanho de 0 a 100 (preso): a aresta da célula vai de 1 pixel
    /// (0) a 5 (100). Neutro 0 — ⚠️ o `Tamanho` do Lightroom abre em 25, aqui
    /// abre no grão fino. Só conta com [`Ajustes::grain_amount`] acima de zero.
    pub grain_size: f32,
    // ------------------------------------------------ Calibração de câmera
    // 🚨 **É a base da maioria dos presets de filme**, e faltava inteira até
    // 2026-09-12 — o operador do estúdio relatou que não conseguia reproduzir
    // os estilos que tem no Lightroom e no darktable, e este era o buraco
    // maior depois da curva por ponto. Ela move os **primários** antes de todo
    // o resto: um preset que gira o vermelho para o laranja e dessatura o azul
    // muda a foto inteira, não uma faixa de matiz como o HSL faz.
    //
    // ⚠️ **É uma aproximação declarada.** A calibração da Adobe é uma matriz no
    // espaço do perfil da câmera, que este motor não tem (ele recebe RGB já
    // revelado). Aqui ela é rotação de matiz e escala de saturação em torno de
    // cada primário, com queda larga — o comportamento visível é o mesmo nos
    // valores que os presets usam; o que muda é o extremo.
    /// Calibração, matiz do **primário vermelho**, de −100 a +100; neutro 0 —
    /// o `Matiz` do Primário vermelho do Lightroom. O extremo gira ~30°
    /// (`v·0,3` graus), com queda larga (120°) em torno do primário.
    pub calib_red_hue: f32,
    /// Calibração, saturação do **primário vermelho**, de −100 a +100 (escala
    /// a saturação por `1 + v/100`); neutro 0.
    pub calib_red_sat: f32,
    /// Calibração, matiz do **primário verde**, de −100 a +100 (~±30°);
    /// neutro 0.
    pub calib_green_hue: f32,
    /// Calibração, saturação do **primário verde**, de −100 a +100; neutro 0.
    pub calib_green_sat: f32,
    /// Calibração, matiz do **primário azul**, de −100 a +100 (~±30°);
    /// neutro 0.
    pub calib_blue_hue: f32,
    /// Calibração, saturação do **primário azul**, de −100 a +100; neutro 0.
    pub calib_blue_sat: f32,
    /// Calibração, matiz das **sombras**, de −100 (verde) a +100 (magenta);
    /// neutro 0 — o `Matiz` de Sombras da Calibração do Lightroom. Até 12
    /// níveis no preto, e some até o meio-tom.
    pub calib_shadow_tint: f32,
    // ------------------------------------- Color Grading: os eixos que faltavam
    // O split toning clássico tem duas pontas; o "Color Grading" do LR 10 em
    // diante tem três mais um global, e os presets modernos usam os quatro. Sem
    // os tons médios, um preset cinematográfico perde justamente onde a pele
    // vive.
    /// Gradação de cores, matiz dos **tons médios**, em graus (0 a 360);
    /// neutro 0.
    pub split_midtone_hue: f32,
    /// Gradação de cores, saturação dos **tons médios**, de 0 a 100; neutro 0.
    pub split_midtone_sat: f32,
    /// Gradação de cores, matiz **global** (a foto inteira), em graus (0 a
    /// 360); neutro 0.
    pub split_global_hue: f32,
    /// Gradação de cores, saturação **global**, de 0 a 100; neutro 0.
    pub split_global_sat: f32,
    /// Quanto as três faixas se misturam — o `Blending` da Adobe, 0 a 100.
    ///
    /// 🔑 Neutro **50**, e não 0: é o valor em que a Adobe abre o controle, e
    /// abrir em 0 daria três faixas de bordas duras numa foto que ninguém
    /// tocou. Ver `Ajustes::default`.
    pub split_blending: f32,
    // ------------------------------------------ Mixer de preto e branco
    // 🚨 **Zero cobertura até 2026-09-12.** Um preset B&W do Lightroom virava
    // `saturation = -1` e perdia a mistura por canal — que é exatamente o que
    // separa um P&B de retrato (pele clara, céu escuro) de um cinza chapado.
    /// Liga o mixer. Sem ele os oito abaixo não fazem nada, como no Lightroom:
    /// o mixer só existe com a foto convertida para P&B.
    ///
    /// 0 desligado, qualquer outro valor ligado (grave 1). Ligado, Saturação,
    /// Vibração e HSL deixam de agir — o Lightroom os apaga no painel.
    pub bw_ativo: f32,
    /// Mixer P&B, luminância do **vermelho** (mesma faixa do HSL), de −100 a
    /// +100; neutro 0. Ganho `1 + v/100·saturação` sobre a luminância do pixel.
    pub bw_red: f32,
    /// Mixer P&B, luminância do **laranja**, de −100 a +100; neutro 0.
    pub bw_orange: f32,
    /// Mixer P&B, luminância do **amarelo**, de −100 a +100; neutro 0.
    pub bw_yellow: f32,
    /// Mixer P&B, luminância do **verde**, de −100 a +100; neutro 0.
    pub bw_green: f32,
    /// Mixer P&B, luminância da **água**, de −100 a +100; neutro 0.
    pub bw_aqua: f32,
    /// Mixer P&B, luminância do **azul**, de −100 a +100; neutro 0.
    pub bw_blue: f32,
    /// Mixer P&B, luminância do **roxo**, de −100 a +100; neutro 0.
    pub bw_purple: f32,
    /// Mixer P&B, luminância do **magenta**, de −100 a +100; neutro 0.
    pub bw_magenta: f32,
    // ---------------------------------------------------- Curva por ponto
    // 🚨 **O maior buraco da importação de presets**: numa amostra de 400
    // presets comerciais, **321 usavam curva por ponto** — e até 2026-09-12 ela
    // era ignorada inteira, o que fazia um preset de filme chegar sem o pé de
    // contraste que o define.
    //
    // 🔑 **Nove alturas em x fixo, e não pontos livres.** A revelação é gravada
    // como um mapa plano de número por nome (é o mesmo formato do `corte_*`), e
    // uma lista de pontos de tamanho variável não cabe nele sem inventar um
    // segundo formato. Nove alturas cabem, não podem sair de ordem no eixo x, e
    // o shader as interpola com Hermite monótono — que por construção não
    // oscila entre dois pontos, o defeito clássico de spline em curva de tom.
    //
    // ⚠️ **O neutro não é zero: é a identidade.** O ponto `i` vale `i·255/8`,
    // que é a reta que devolve a foto como ela entrou. Ver `Ajustes::default`.
    //
    // Cada altura vai de 0 a 255 (a saída é presa nessa faixa); o ponto `i`
    // fica em `x = i·255/8`. Os canais R, G e B são aplicados primeiro e o
    // mestre depois, na ordem da Adobe.
    // Canal mestre.
    /// Curva por ponto, canal **mestre** (RGB), altura em `x = 0` (o preto),
    /// de 0 a 255; neutro 0 ([`Ajustes::CURVA_NEUTRA`]).
    pub curva_m0: f32,
    /// Curva mestre, altura em `x = 31,875` (0–255); neutro 31,875.
    pub curva_m1: f32,
    /// Curva mestre, altura em `x = 63,75` (0–255); neutro 63,75.
    pub curva_m2: f32,
    /// Curva mestre, altura em `x = 95,625` (0–255); neutro 95,625.
    pub curva_m3: f32,
    /// Curva mestre, altura em `x = 127,5` (0–255); neutro 127,5.
    pub curva_m4: f32,
    /// Curva mestre, altura em `x = 159,375` (0–255); neutro 159,375.
    pub curva_m5: f32,
    /// Curva mestre, altura em `x = 191,25` (0–255); neutro 191,25.
    pub curva_m6: f32,
    /// Curva mestre, altura em `x = 223,125` (0–255); neutro 223,125.
    pub curva_m7: f32,
    /// Curva mestre, altura em `x = 255` (o branco), de 0 a 255; neutro 255.
    pub curva_m8: f32,
    // Canal vermelho.
    /// Curva por ponto, canal **vermelho**, altura em `x = 0` (0–255); neutro 0.
    pub curva_r0: f32,
    /// Curva vermelha, altura em `x = 31,875` (0–255); neutro 31,875.
    pub curva_r1: f32,
    /// Curva vermelha, altura em `x = 63,75` (0–255); neutro 63,75.
    pub curva_r2: f32,
    /// Curva vermelha, altura em `x = 95,625` (0–255); neutro 95,625.
    pub curva_r3: f32,
    /// Curva vermelha, altura em `x = 127,5` (0–255); neutro 127,5.
    pub curva_r4: f32,
    /// Curva vermelha, altura em `x = 159,375` (0–255); neutro 159,375.
    pub curva_r5: f32,
    /// Curva vermelha, altura em `x = 191,25` (0–255); neutro 191,25.
    pub curva_r6: f32,
    /// Curva vermelha, altura em `x = 223,125` (0–255); neutro 223,125.
    pub curva_r7: f32,
    /// Curva vermelha, altura em `x = 255` (0–255); neutro 255.
    pub curva_r8: f32,
    // Canal verde.
    /// Curva por ponto, canal **verde**, altura em `x = 0` (0–255); neutro 0.
    pub curva_g0: f32,
    /// Curva verde, altura em `x = 31,875` (0–255); neutro 31,875.
    pub curva_g1: f32,
    /// Curva verde, altura em `x = 63,75` (0–255); neutro 63,75.
    pub curva_g2: f32,
    /// Curva verde, altura em `x = 95,625` (0–255); neutro 95,625.
    pub curva_g3: f32,
    /// Curva verde, altura em `x = 127,5` (0–255); neutro 127,5.
    pub curva_g4: f32,
    /// Curva verde, altura em `x = 159,375` (0–255); neutro 159,375.
    pub curva_g5: f32,
    /// Curva verde, altura em `x = 191,25` (0–255); neutro 191,25.
    pub curva_g6: f32,
    /// Curva verde, altura em `x = 223,125` (0–255); neutro 223,125.
    pub curva_g7: f32,
    /// Curva verde, altura em `x = 255` (0–255); neutro 255.
    pub curva_g8: f32,
    // Canal azul.
    /// Curva por ponto, canal **azul**, altura em `x = 0` (0–255); neutro 0.
    pub curva_b0: f32,
    /// Curva azul, altura em `x = 31,875` (0–255); neutro 31,875.
    pub curva_b1: f32,
    /// Curva azul, altura em `x = 63,75` (0–255); neutro 63,75.
    pub curva_b2: f32,
    /// Curva azul, altura em `x = 95,625` (0–255); neutro 95,625.
    pub curva_b3: f32,
    /// Curva azul, altura em `x = 127,5` (0–255); neutro 127,5.
    pub curva_b4: f32,
    /// Curva azul, altura em `x = 159,375` (0–255); neutro 159,375.
    pub curva_b5: f32,
    /// Curva azul, altura em `x = 191,25` (0–255); neutro 191,25.
    pub curva_b6: f32,
    /// Curva azul, altura em `x = 223,125` (0–255); neutro 223,125.
    pub curva_b7: f32,
    /// Curva azul, altura em `x = 255` (0–255); neutro 255.
    pub curva_b8: f32,
    // ------------------------------------------ Os controles do Lightroom
    // 🔑 **Paridade com o painel do Lightroom** (dono, 2026-09-30: *"na
    // revelação sRGB preciso dos mesmos controles do Lightroom"*, com um DNG
    // revelado lá que não se reproduzia aqui). No fim, como todo campo novo: a
    // revelação gravada antes não os tem, e o `serde(default)` os completa com
    // o neutro — que é, em cada um, o comportamento de antes, bit a bit.
    //
    // Textura e Remover névoa: os dois que o Básico do Lightroom tem e este
    // não tinha. A Claridade (`clarity`) passou a ser contraste local de verdade
    // no mesmo dia — ver `corpo.wgsl`.
    /// Textura, de −100 a +100; neutro 0 — a `Textura` do Lightroom. Contraste
    /// local de vizinhança média (σ = 0,25 % do lado maior), pela guia.
    pub texture: f32,
    /// Remover névoa, de −100 (põe névoa) a +100 (tira); neutro 0 — o
    /// `Remover névoa` do Lightroom. Canal escuro de He et al., pela guia.
    pub dehaze: f32,
    // As três divisões da curva paramétrica (o Lightroom abre em 25/50/75).
    /// Curva paramétrica, divisão entre Sombras e Escuros, de 0 a 100 (em %
    /// da luminância); neutro **25**.
    ///
    /// As três divisões ficam em ordem mesmo quando o JSON traz outra coisa: o
    /// shader prende cada uma entre a anterior e 100.
    pub tone_curve_split_shadows: f32,
    /// Curva paramétrica, divisão entre Escuros e Claros, de 0 a 100; neutro
    /// **50**.
    pub tone_curve_split_midtones: f32,
    /// Curva paramétrica, divisão entre Claros e Realces, de 0 a 100; neutro
    /// **75**.
    pub tone_curve_split_highlights: f32,
    // A luminância das três faixas e do global da Gradação de cores.
    /// Gradação de cores, luminância das **sombras**, de −100 (rumo ao preto) a
    /// +100 (rumo ao branco), sem estourar; neutro 0.
    pub split_shadow_lum: f32,
    /// Gradação de cores, luminância dos **tons médios**, de −100 a +100;
    /// neutro 0.
    pub split_midtone_lum: f32,
    /// Gradação de cores, luminância dos **realces**, de −100 a +100; neutro 0.
    pub split_highlight_lum: f32,
    /// Gradação de cores, luminância **global**, de −100 a +100; neutro 0.
    pub split_global_lum: f32,
    // Nitidez: Detalhe (neutro 25, o do Lightroom) e Máscara.
    /// Nitidez, `Detalhe` do Lightroom, de 0 a 100; neutro **25**. Abaixo de
    /// 25 contém o detalhe grande (tira halo), acima reforça o fino.
    pub sharpen_detail: f32,
    /// Nitidez, `Máscara` do Lightroom, de 0 a 100; neutro 0 (a foto inteira).
    /// Quanto maior, mais a nitidez fica só nas bordas.
    pub sharpen_masking: f32,
    // Redução de ruído: Detalhe e Contraste da luminância, Detalhe e
    // Suavidade da cor (neutros 50/0/50/50, os do Lightroom).
    /// Redução de ruído, `Detalhe` da luminância, de 0 a 100; neutro **50**.
    /// Mais detalhe estreita o filtro. Só conta com [`Ajustes::nr_luminance`].
    pub nr_luminance_detail: f32,
    /// Redução de ruído, `Contraste` da luminância, de 0 a 100; neutro 0.
    /// Devolve parte do contraste fino que o filtro levou (até metade, em 100).
    pub nr_luminance_contrast: f32,
    /// Redução de ruído, `Detalhe` da cor, de 0 a 100; neutro **50**. Só conta
    /// com [`Ajustes::nr_color`].
    pub nr_color_detail: f32,
    /// Redução de ruído, `Suavidade` da cor, de 0 a 100; neutro **50**. Mais
    /// suavidade abre o filtro.
    pub nr_color_smoothness: f32,
    // Vinheta pós-corte: estilo (0 prioridade de realces, 1 prioridade de
    // cores, 2 sobreposição de tinta), quantidade, ponto médio, arredondamento,
    // difusão e realces. A `lens_vignette_*` continua sendo a da Lente.
    /// Vinheta pós-corte, `Estilo`: 0 prioridade de realces, 1 prioridade de
    /// cores, 2 sobreposição de tinta (arredondado ao inteiro); neutro 0. No
    /// processo 1, 0 e 1 dão o mesmo resultado, como no Lightroom.
    pub pcv_style: f32,
    /// Vinheta pós-corte, `Quantidade`, de −100 (escurece) a +100 (clareia),
    /// presa nessa faixa; neutro 0. Medida no quadro do arquivo que sai.
    pub pcv_amount: f32,
    /// Vinheta pós-corte, `Ponto médio`, de 0 a 100; neutro **50**. Leva a
    /// linha de força total da borda (0) até o canto (100).
    pub pcv_midpoint: f32,
    /// Vinheta pós-corte, `Arredondamento`, de −100 (retângulo de cantos
    /// redondos) a +100 (círculo); neutro 0 (a elipse do quadro).
    pub pcv_roundness: f32,
    /// Vinheta pós-corte, `Difusão`, de 0 (borda dura) a 100; neutro **50**.
    pub pcv_feather: f32,
    /// Vinheta pós-corte, `Realces`, de 0 a 100; neutro 0. Poupa os claros ao
    /// escurecer — só no processo 0, nos estilos 0 e 1.
    pub pcv_highlights: f32,
    // Aspereza do grão (neutro 50).
    /// Grão, `Aspereza` do Lightroom, de 0 (liso) a 100 (irregular); neutro
    /// **50**. Só conta com [`Ajustes::grain_amount`] acima de zero.
    pub grain_roughness: f32,
    // ------------------------------------------ A versão de processo
    // 🔑 **0 é a conta de antes; 1 é a do Lightroom medida** (dono, 2026-10-01:
    // *"versão de processo"*, como o PV2012 do Lightroom). No processo 1 a
    // Exposição, o Contraste, os Realces, as Sombras, os Brancos, os Pretos e a
    // vinheta pós-corte leem as tabelas de `lightroom.rs` em vez das contas
    // antigas. O neutro é **0**: a revelação gravada antes não tem o campo, o
    // `serde(default)` o completa com 0, e a foto que o operador já entregou
    // continua igual até alguém a atualizar. Quem liga o 1 é a foto nova e o
    // preset do Lightroom.
    /// A versão de processo: 0 a conta de antes, 1 as tabelas medidas no
    /// Lightroom ([`crate::lightroom`]); 0,5 ou mais conta como 1. Neutro 0.
    ///
    /// No neutro de todos os outros campos os dois processos dão a mesma foto
    /// — por isso [`Ajustes::sem_efeito`] o ignora.
    pub processo: f32,
    // ------------------------------------------ A vinheta do darktable
    // 🎞️ O módulo `vignette` do darktable (`src/iop/vignette.c`,
    // `dt_iop_vignette_params_t`), de volta em 2/out/2026 como controle
    // próprio — a do `RecordarFotos P&B`. As duas do Lightroom não a imitam:
    // aqui o brilho positivo **soma** luz linear, e a forma é a superelipse de
    // expoente `2/forma`. Os campos são os do módulo, com as faixas e os
    // padrões dele: o liga/desliga (o botão do módulo), início e raio do
    // decaimento 0..200 % (80 e 50), brilho e saturação −1..1 (−0,5), centro
    // −1..1, proporção automática 0/1, largura/altura 0..2 (1), forma 0..5 (1)
    // e matização 0 desligada, 1 saída de 8 bits, 2 de 16. O `unbound` não
    // entra: é sempre ligado (o 0–255 do fim do shader prende). Prefixo
    // `darktable_`, e não `dt_`: os `dt_*` são a receita antiga, que
    // `migrar_do_darktable` traduz e apaga.
    /// Vinheta do darktable, o botão do módulo: 0 desligada, 1 ligada (0,5 ou
    /// mais conta como ligada); neutro 0. Sem ele os dez abaixo não fazem nada
    /// — ver [`Ajustes::vinheta_do_darktable_ligada`].
    pub darktable_vignette_ativo: f32,
    /// Vinheta do darktable, início do decaimento (`scale`), de 0 a 200 % do
    /// meio-quadro; padrão do módulo **80**.
    pub darktable_vignette_scale: f32,
    /// Vinheta do darktable, raio do decaimento (`falloff_scale`), de 0 a
    /// 200 % do meio-quadro; padrão **50** (com o piso anti-serrilhado do
    /// darktable).
    pub darktable_vignette_falloff_scale: f32,
    /// Vinheta do darktable, brilho, de −1 a +1; padrão **−0,5**. Negativo
    /// multiplica a luz linear, positivo **soma** (a borda branca).
    pub darktable_vignette_brightness: f32,
    /// Vinheta do darktable, saturação, de −1 a +1; padrão **−0,5**.
    pub darktable_vignette_saturation: f32,
    /// Vinheta do darktable, centro em x, de −1 a +1 (fração do meio-quadro, 0
    /// no meio); padrão 0.
    pub darktable_vignette_center_x: f32,
    /// Vinheta do darktable, centro em y, de −1 a +1 (o `y` cresce para
    /// baixo); padrão 0.
    pub darktable_vignette_center_y: f32,
    /// Vinheta do darktable, proporção automática: 1 segue a proporção do
    /// quadro, 0 usa [`Ajustes::darktable_vignette_whratio`]; padrão 0.
    pub darktable_vignette_autoratio: f32,
    /// Vinheta do darktable, largura/altura, de 0 a 2 (preso em 0,001..1,999);
    /// padrão **1** (círculo). Só conta sem a proporção automática.
    pub darktable_vignette_whratio: f32,
    /// Vinheta do darktable, forma, de 0 a 5 (expoente `2/forma` da
    /// superelipse); padrão **1** (círculo/elipse).
    pub darktable_vignette_shape: f32,
    /// Vinheta do darktable, matização: 0 desligada, 1 ruído de 8 bits, 2 de
    /// 16 bits (arredondado ao inteiro); padrão 0.
    pub darktable_vignette_dithering: f32,
}

/// A cor que a roda da Gradação de cores do Lightroom mostra no matiz `graus`,
/// em sRGB 0–1 — a mesma conta do shader (`roda_do_lightroom`): CIELab com
/// matiz `graus + 28°`, num meio-tom (L* 60) de croma 40. É o trilho do slider.
pub fn cor_da_roda_do_lightroom(graus: f32) -> [f32; 3] {
    let h = (graus + 28.0).to_radians();
    let (l, a, b) = (60.0f32, 40.0 * h.cos(), 40.0 * h.sin());
    let fy = (l + 16.0) / 116.0;
    let inv = |t: f32| {
        if t * t * t > 0.008856 {
            t * t * t
        } else {
            (t - 16.0 / 116.0) / 7.787
        }
    };
    let (x, y, z) = (
        inv(fy + a / 500.0) * 0.95047,
        inv(fy),
        inv(fy - b / 200.0) * 1.08883,
    );
    let srgb = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    [
        srgb(3.2406 * x - 1.5372 * y - 0.4986 * z),
        srgb(-0.9689 * x + 1.8758 * y + 0.0415 * z),
        srgb(0.0557 * x - 0.2040 * y + 1.0570 * z),
    ]
}

/// Quantos campos a struct tem — e quantos `f32` o vetor posicional carrega.
///
/// ⚠️ **Eram 46 até 2026-09-06, e 53 até 2026-09-12** (a Calibração, o Color
/// Grading completo e o mixer P&B levaram a 74; a curva por ponto, a 110; o
/// estágio darktable — exposure, vignetting e color balance rgb —, a 157; shadows
/// and highlights e monochrome, a 171; os controles do Lightroom que faltavam
/// — Textura, Remover névoa, a vinheta pós-corte e o resto do Detalhe —, a 193
/// em 2026-09-30; a versão de processo, a 194 em 2026-10-01; sem o estágio
/// darktable, a 133 em 2026-10-02; a vinheta do darktable, de volta como
/// controle próprio, a 144 no mesmo dia).
/// A Tonalização (5) e o
/// Grão (2) entraram primeiro; depois a Calibração de câmera (7), os eixos que
/// faltavam do Color Grading (5) e o mixer de preto e branco (9). **Todos no
/// fim da lista**, e não perto do que se parece com eles: a posição de um campo
/// é o contrato com o shader, e mover `nr_luminance` para junto do grão faria
/// toda revelação já gravada ler o campo do vizinho.
///
/// 🚨 **A única vez que saiu um bloco do meio** (2/out/2026): os 61 campos
/// `dt_*`, quando o dono decidiu um motor só. Tudo depois deles andou 61 posições
/// — o shader, o wasm e o `nomes.json` do site mudaram juntos, e o banco não
/// sentiu, porque guarda por nome.
pub const QUANTIDADE: usize = 144;

/// O tamanho do buffer de `uniform`, arredondado para múltiplo de 16 bytes.
///
/// 🚨 **Não é `size_of::<Ajustes>()`, e a diferença é uma regra do WGSL.** No
/// endereço `uniform` o alinhamento de uma struct é `roundUp(16, …)`, então os
/// 53 `f32` (212 bytes) ocupam 224 do ponto de vista do shader — e o `bind
/// group` recusa um buffer menor que isso. Enquanto o `struct Params` declarava
/// 28 campos (112 bytes, múltiplo de 16) ninguém precisava saber disto.
///
/// Os 12 bytes de sobra nunca são escritos nem lidos: a CPU manda os 212 do
/// `bytemuck::bytes_of`, e o shader não tem campo além do 52. O enchimento do
/// WGSL acompanha: eram dois `_enchimento` para 184 bytes, são três para 212.
pub(crate) const TAMANHO_DO_UNIFORM: wgpu::BufferAddress = {
    let bytes = std::mem::size_of::<Ajustes>() as wgpu::BufferAddress;
    bytes.next_multiple_of(16)
};

impl Default for Ajustes {
    /// O neutro, conferido campo a campo contra o `crates/ui`.
    ///
    /// ⚠️ **Nem todo neutro é zero**, e é por isso que este `Default` é escrito e
    /// não derivado: `contrast` neutro é `1.0` (é um multiplicador) e
    /// `sharpen_radius` é `1.0` (raio zero seria não ter pixel de vizinhança).
    /// Derivar daria zero nos dois, e a foto abriria já alterada — sem ninguém
    /// ter tocado em nada.
    ///
    /// 🚨 **`lens_vignette_midpoint` era `50.0` aqui, e estava errado.** O 50 veio
    /// de `GpuEditParams::default` do `crates/ui` — que o app de lá **nunca
    /// chama**: o único chamador em todo o repositório é um teste que confere só
    /// os 11 campos do Básico. O que o legado de fato usa é
    /// `AppState::new`/`reset_edits`, e nos dois o meio da vinheta é **`0.0`**.
    /// Copiar a `impl Default` em vez do caminho vivo fazia o slider "Meio da
    /// vinheta" abrir em 50 aqui e em 0 lá, na mesma foto.
    ///
    /// 🔑 A pergunta que separa os dois: não é "qual é o padrão declarado", é
    /// **"qual valor a foto recebe quando ninguém mexeu em nada"**. `impl Default`
    /// responde a primeira, e ela pode ser código morto.
    ///
    /// ⚠️ **E a resposta certa, para foto de verdade, é um quarto lugar: o
    /// schema.** `014_add_hsl_lens_fields.sql` cria a coluna com
    /// `DEFAULT 50.0`, então toda foto importada volta do banco com o meio da
    /// vinheta preenchido — e é esse 50 que os dois apps mostram no slider. O que
    /// este `Default` decide é o resto: o estado antes de abrir qualquer foto, o
    /// futuro "redefinir", e o campo que vier `NULL`. Nos três o legado diz 0.0.
    fn default() -> Self {
        let mut neutro: Self = bytemuck::Zeroable::zeroed();
        neutro.contrast = 1.0;
        neutro.sharpen_radius = 1.0;
        // 🚨 **O terceiro neutro que não é zero** (2026-09-12). A mistura do
        // Color Grading abre em 50 na Adobe; em 0 as três faixas teriam borda
        // dura, e uma foto que ninguém tocou já sairia diferente.
        neutro.split_blending = 50.0;
        // 🚨 **O quarto neutro que não é zero, e o maior deles.** A curva
        // neutra é a IDENTIDADE — o ponto `i` devolve `i·255/8` —, e não uma
        // reta em zero: zerada, ela apagaria a foto. Quem tem revelação
        // gravada antes de 2026-09-12 não tem estes campos no JSON, e o
        // `serde(default)` os completa com esta reta, que é o mesmo que "sem
        // curva".
        let identidade = Self::CURVA_NEUTRA;
        neutro.curva_m0 = identidade[0];
        neutro.curva_m1 = identidade[1];
        neutro.curva_m2 = identidade[2];
        neutro.curva_m3 = identidade[3];
        neutro.curva_m4 = identidade[4];
        neutro.curva_m5 = identidade[5];
        neutro.curva_m6 = identidade[6];
        neutro.curva_m7 = identidade[7];
        neutro.curva_m8 = identidade[8];
        neutro.curva_r0 = identidade[0];
        neutro.curva_r1 = identidade[1];
        neutro.curva_r2 = identidade[2];
        neutro.curva_r3 = identidade[3];
        neutro.curva_r4 = identidade[4];
        neutro.curva_r5 = identidade[5];
        neutro.curva_r6 = identidade[6];
        neutro.curva_r7 = identidade[7];
        neutro.curva_r8 = identidade[8];
        neutro.curva_g0 = identidade[0];
        neutro.curva_g1 = identidade[1];
        neutro.curva_g2 = identidade[2];
        neutro.curva_g3 = identidade[3];
        neutro.curva_g4 = identidade[4];
        neutro.curva_g5 = identidade[5];
        neutro.curva_g6 = identidade[6];
        neutro.curva_g7 = identidade[7];
        neutro.curva_g8 = identidade[8];
        neutro.curva_b0 = identidade[0];
        neutro.curva_b1 = identidade[1];
        neutro.curva_b2 = identidade[2];
        neutro.curva_b3 = identidade[3];
        neutro.curva_b4 = identidade[4];
        neutro.curva_b5 = identidade[5];
        neutro.curva_b6 = identidade[6];
        neutro.curva_b7 = identidade[7];
        neutro.curva_b8 = identidade[8];
        // Os neutros do Lightroom que não são zero: em cada um, o motor faz
        // exatamente o que fazia antes de o controle existir.
        neutro.tone_curve_split_shadows = 25.0;
        neutro.tone_curve_split_midtones = 50.0;
        neutro.tone_curve_split_highlights = 75.0;
        neutro.sharpen_detail = 25.0;
        neutro.nr_luminance_detail = 50.0;
        neutro.nr_color_detail = 50.0;
        neutro.nr_color_smoothness = 50.0;
        neutro.pcv_midpoint = 50.0;
        neutro.pcv_feather = 50.0;
        neutro.grain_roughness = 50.0;
        // Os padrões do `vignette.c`, com o módulo desligado: quem o liga
        // começa onde o darktable começa.
        neutro.darktable_vignette_scale = 80.0;
        neutro.darktable_vignette_falloff_scale = 50.0;
        neutro.darktable_vignette_brightness = -0.5;
        neutro.darktable_vignette_saturation = -0.5;
        neutro.darktable_vignette_whratio = 1.0;
        neutro.darktable_vignette_shape = 1.0;
        neutro
    }
}

impl Ajustes {
    /// A curva que devolve a foto como ela entrou — a reta `y = x`, amostrada
    /// nos nove x fixos.
    ///
    /// 🔑 **Escrita uma vez, e lida pelo `Default` e pelo site.** Dois lugares
    /// decidindo o que é "sem curva" dariam duas fotos diferentes para a mesma
    /// revelação gravada.
    pub const CURVA_NEUTRA: [f32; 9] = [
        0.0, 31.875, 63.75, 95.625, 127.5, 159.375, 191.25, 223.125, 255.0,
    ];

    /// Os 144 nomes, na ordem do `uniform`.
    ///
    /// 🔑 É a ordem que o vetor posicional ([`Ajustes::como_vetor`]) segue, a
    /// que o `struct Params` do WGSL declara, e a que o site recebe em
    /// `nomes.json`. Três lugares, uma lista.
    pub const NOMES: [&'static str; QUANTIDADE] = [
        "exposure",
        "contrast",
        "temperature",
        "tint",
        "highlights",
        "shadows",
        "whites",
        "blacks",
        "clarity",
        "vibrance",
        "saturation",
        "tone_curve_shadows",
        "tone_curve_darks",
        "tone_curve_lights",
        "tone_curve_highlights",
        "hsl_red_sat",
        "hsl_orange_sat",
        "hsl_yellow_sat",
        "hsl_green_sat",
        "hsl_aqua_sat",
        "hsl_blue_sat",
        "hsl_purple_sat",
        "hsl_magenta_sat",
        "hsl_red_hue",
        "hsl_orange_hue",
        "hsl_yellow_hue",
        "hsl_green_hue",
        "hsl_aqua_hue",
        "hsl_blue_hue",
        "hsl_purple_hue",
        "hsl_magenta_hue",
        "hsl_red_lum",
        "hsl_orange_lum",
        "hsl_yellow_lum",
        "hsl_green_lum",
        "hsl_aqua_lum",
        "hsl_blue_lum",
        "hsl_purple_lum",
        "hsl_magenta_lum",
        "lens_distortion",
        "lens_vignette_amount",
        "lens_vignette_midpoint",
        "nr_luminance",
        "nr_color",
        "sharpen_amount",
        "sharpen_radius",
        "split_shadow_hue",
        "split_shadow_sat",
        "split_highlight_hue",
        "split_highlight_sat",
        "split_balance",
        "grain_amount",
        "grain_size",
        "calib_red_hue",
        "calib_red_sat",
        "calib_green_hue",
        "calib_green_sat",
        "calib_blue_hue",
        "calib_blue_sat",
        "calib_shadow_tint",
        "split_midtone_hue",
        "split_midtone_sat",
        "split_global_hue",
        "split_global_sat",
        "split_blending",
        "bw_ativo",
        "bw_red",
        "bw_orange",
        "bw_yellow",
        "bw_green",
        "bw_aqua",
        "bw_blue",
        "bw_purple",
        "bw_magenta",
        "curva_m0",
        "curva_m1",
        "curva_m2",
        "curva_m3",
        "curva_m4",
        "curva_m5",
        "curva_m6",
        "curva_m7",
        "curva_m8",
        "curva_r0",
        "curva_r1",
        "curva_r2",
        "curva_r3",
        "curva_r4",
        "curva_r5",
        "curva_r6",
        "curva_r7",
        "curva_r8",
        "curva_g0",
        "curva_g1",
        "curva_g2",
        "curva_g3",
        "curva_g4",
        "curva_g5",
        "curva_g6",
        "curva_g7",
        "curva_g8",
        "curva_b0",
        "curva_b1",
        "curva_b2",
        "curva_b3",
        "curva_b4",
        "curva_b5",
        "curva_b6",
        "curva_b7",
        "curva_b8",
        "texture",
        "dehaze",
        "tone_curve_split_shadows",
        "tone_curve_split_midtones",
        "tone_curve_split_highlights",
        "split_shadow_lum",
        "split_midtone_lum",
        "split_highlight_lum",
        "split_global_lum",
        "sharpen_detail",
        "sharpen_masking",
        "nr_luminance_detail",
        "nr_luminance_contrast",
        "nr_color_detail",
        "nr_color_smoothness",
        "pcv_style",
        "pcv_amount",
        "pcv_midpoint",
        "pcv_roundness",
        "pcv_feather",
        "pcv_highlights",
        "grain_roughness",
        "processo",
        "darktable_vignette_ativo",
        "darktable_vignette_scale",
        "darktable_vignette_falloff_scale",
        "darktable_vignette_brightness",
        "darktable_vignette_saturation",
        "darktable_vignette_center_x",
        "darktable_vignette_center_y",
        "darktable_vignette_autoratio",
        "darktable_vignette_whratio",
        "darktable_vignette_shape",
        "darktable_vignette_dithering",
    ];

    /// Os [`QUANTIDADE`] valores, por posição — o que a GPU recebe, como `f32`,
    /// na ordem de [`Ajustes::NOMES`].
    pub fn como_vetor(&self) -> [f32; QUANTIDADE] {
        let campos: &[f32] = bytemuck::cast_slice(bytemuck::bytes_of(self));
        campos.try_into().expect("53 campos de quatro bytes")
    }

    /// De um vetor posicional de [`QUANTIDADE`] `f32`, na ordem de
    /// [`Ajustes::NOMES`]; `None` se o tamanho for outro.
    ///
    /// 🚨 É por aqui que o navegador manda os ajustes, e o tamanho é a única
    /// conferência possível: 52 valores seriam 52 campos certos e um lixo.
    pub fn de_vetor(valores: &[f32]) -> Option<Self> {
        if valores.len() != QUANTIDADE {
            return None;
        }
        Some(bytemuck::pod_read_unaligned(bytemuck::cast_slice(valores)))
    }

    /// Não muda pixel nenhum: todo ajuste no neutro.
    ///
    /// 🔑 **A versão de processo não conta.** No neutro os dois processos dão a
    /// mesma foto (o zero de cada tabela é a identidade), e a foto nova nasce
    /// no processo 1: comparar com `Ajustes::default()` faria toda foto nova
    /// parecer revelada — o ponto âmbar aceso, a exportação revelando o que só
    /// precisava copiar.
    pub fn sem_efeito(&self) -> bool {
        Ajustes {
            processo: 0.0,
            ..*self
        } == Ajustes::default()
    }

    /// Claridade, Textura ou Remover névoa — os três que leem a guia
    /// (`guia.rs`), a vizinhança larga que o shader sozinho não alcança.
    pub fn usa_a_guia(&self) -> bool {
        self.clarity != 0.0
            || self.texture != 0.0
            || self.dehaze != 0.0
            // No processo 1, Realces e Sombras são locais: agem na base.
            || (self.processo >= 0.5 && (self.highlights != 0.0 || self.shadows != 0.0))
    }

    /// Alguma das três vinhetas está ligada — a de lente, a pós-corte ou a do
    /// darktable?
    ///
    /// 🔑 **São os únicos ajustes que dependem do enquadramento** (ver
    /// `Motor::definir_corte`): sem nenhuma delas, mudar o corte não muda pixel
    /// revelado nenhum. Quem reprocessaria a cada arrasto de alça pergunta aqui
    /// antes.
    pub fn vinheta_ligada(&self) -> bool {
        self.lens_vignette_amount != 0.0
            || self.pcv_amount != 0.0
            || self.vinheta_do_darktable_ligada()
    }

    /// A vinheta do darktable está ligada — o botão do módulo.
    pub fn vinheta_do_darktable_ligada(&self) -> bool {
        self.darktable_vignette_ativo >= 0.5
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// ⚠️ O neutro **não é zero** em dois campos — e num terceiro parecia não ser.
    ///
    /// `contrast` é multiplicador e `sharpen_radius` zero seria não ter pixel de
    /// vizinhança: um `#[derive(Default)]` daria zero nos dois e a foto abriria
    /// alterada sem ninguém ter tocado em nada.
    ///
    /// 🚨 `lens_vignette_midpoint` está aqui pelo motivo oposto: ele **é** zero, e
    /// já esteve em 50 porque foi copiado de `GpuEditParams::default`, uma `impl`
    /// que o app do legado nunca chama. O que o legado usa — `AppState::new` e
    /// `reset_edits` — diz `0.0`. O teste cobra o valor do caminho vivo, e não o
    /// do padrão declarado.
    #[test]
    fn o_neutro_e_o_do_caminho_vivo_do_legado() {
        let neutro = Ajustes::default();
        assert_eq!(neutro.contrast, 1.0);
        assert_eq!(neutro.sharpen_radius, 1.0);
        assert_eq!(
            neutro.lens_vignette_midpoint, 0.0,
            "é o que `AppState::new` e `reset_edits` põem no slider; \
             o 50 de `GpuEditParams::default` é código morto"
        );
        assert_eq!(neutro.exposure, 0.0);
        assert_eq!(neutro.saturation, 0.0);
    }

    /// O layout que vai para a GPU tem os 144 campos, de quatro bytes cada.
    ///
    /// Campo a mais desloca **todos** os seguintes na leitura do shader, e o
    /// sintoma é a saturação virando nitidez.
    ///
    /// ⚠️ **O número do `uniform` é escrito à mão de propósito.** Derivá-lo aqui
    /// (`size_of().next_multiple_of(16)`) faria o teste concordar com qualquer
    /// mudança, inclusive com a errada — e é justamente o alinhamento de 16
    /// bytes do WebGL2 que já derrubou este shader uma vez. 144 × 4 = 576, já
    /// múltiplo de 16.
    #[test]
    fn o_layout_tem_os_campos_de_quatro_bytes() {
        assert_eq!(std::mem::size_of::<Ajustes>(), QUANTIDADE * 4);
        assert_eq!(TAMANHO_DO_UNIFORM, 576);
    }

    /// Os nomes do `struct Params` do WGSL, na ordem em que ele os declara.
    fn campos_do_wgsl() -> Vec<String> {
        let shader = include_str!("shaders/corpo.wgsl");
        shader
            .split("struct Params {")
            .nth(1)
            .and_then(|resto| resto.split('}').next())
            .expect("o shader tem de declarar `struct Params`")
            .lines()
            .filter_map(|linha| linha.split(':').next())
            .map(str::trim)
            // O enchimento de 16 bytes do WebGL2 começa com `_` e não é campo.
            .filter(|nome| !nome.is_empty() && !nome.starts_with("//") && !nome.starts_with('_'))
            .map(str::to_string)
            .collect()
    }

    /// O `struct Params` do WGSL, com o enchimento, ocupa os mesmos 224 bytes
    /// que `TAMANHO_DO_UNIFORM` reserva — nem mais (o Rust não escreveria o
    /// resto) nem menos (o WebGL2 recusaria o pipeline).
    #[test]
    fn o_enchimento_do_wgsl_fecha_os_192_bytes() {
        let shader = include_str!("shaders/corpo.wgsl");
        let campos = shader
            .split("struct Params {")
            .nth(1)
            .and_then(|resto| resto.split('}').next())
            .expect("o shader tem de declarar `struct Params`")
            .lines()
            .map(str::trim)
            .filter(|l| l.contains(": f32"))
            .count();
        assert_eq!(campos as u64 * 4, TAMANHO_DO_UNIFORM);
    }

    /// O `struct Params` do WGSL declara os mesmos 53 campos do `Ajustes`, na
    /// mesma ordem.
    ///
    /// 🚨 **Este teste substitui um que prendia o defeito oposto.** Até 17/ago o
    /// WGSL declarava **28** campos para os 46 que a CPU manda, e como o
    /// `uniform` chega por **posição** e não por nome, a partir do 23 o shader
    /// lia o campo do vizinho:
    ///
    /// | posição | o Rust mandava | o shader lia como | o usuário via |
    /// |--------:|----------------|-------------------|---------------|
    /// | 23 | `hsl_red_hue` | `nr_luminance` | a foto **borrava** |
    /// | 24 | `hsl_orange_hue` | `nr_luminance` de novo — declarado duas vezes | nada |
    /// | 25 | `hsl_yellow_hue` | `nr_color` | tirava ruído de cor |
    /// | 26 | `hsl_green_hue` | `sharpen_amount` | afiava |
    /// | 27 | `hsl_aqua_hue` | `sharpen_radius` | nada sozinho |
    /// | 28–45 | matiz (3), luminância (8), lente (3), Detalhe (4) | **nada** | nada |
    ///
    /// Nada disso falhava: o buffer é maior que o mínimo que o binding exige,
    /// então o wgpu aceita e ignora a sobra, e a duplicata de `nr_luminance` no
    /// WGSL o naga também aceita. Não havia erro, log nem tela quebrada — havia
    /// um controle que responde e uma foto que muda pelo motivo errado.
    ///
    /// 🔑 **A conferência é por leitura do arquivo, e não por medida na imagem**,
    /// porque campo declarado e campo aplicado são coisas diferentes: quem mede
    /// a segunda são os testes do [`crate::motor`].
    #[test]
    fn o_wgsl_declara_os_mesmos_campos_na_mesma_ordem() {
        assert_eq!(
            campos_do_wgsl(),
            Ajustes::NOMES,
            "o `struct Params` do WGSL divergiu do `Ajustes` — e o `uniform` casa por posição"
        );
    }

    /// 🔑 O vetor posicional e os nomes contam a mesma história.
    ///
    /// `NOMES[i]` tem de ser o campo que `como_vetor()[i]` carrega — senão o site
    /// escreveria "exposure" e a GPU leria contraste. Conferido nos dois que não
    /// são zero no neutro e em dois do meio da lista.
    #[test]
    fn o_vetor_segue_a_ordem_dos_nomes() {
        let posicao = |nome: &str| {
            Ajustes::NOMES
                .iter()
                .position(|n| *n == nome)
                .unwrap_or_else(|| panic!("`{nome}` não está em NOMES"))
        };
        let neutro = Ajustes::default().como_vetor();
        assert_eq!(neutro[posicao("contrast")], 1.0);
        assert_eq!(neutro[posicao("sharpen_radius")], 1.0);
        // 🚨 O terceiro neutro que não é zero, desde 2026-09-12: a mistura do
        // Color Grading abre em 50, como na Adobe. Em 0 as três faixas teriam
        // borda dura e uma foto intocada já sairia diferente.
        assert_eq!(neutro[posicao("split_blending")], 50.0);
        // 🚨 **A curva neutra é a IDENTIDADE, e não uma reta em zero.** São
        // oito alturas não-nulas por canal (a nona é o próprio zero), nos
        // quatro canais: 32 valores que existem justamente para a foto sair
        // como entrou. Zerá-los apagaria a imagem.
        assert_eq!(neutro[posicao("curva_m8")], 255.0);
        assert_eq!(neutro[posicao("curva_b4")], 127.5);
        assert_eq!(neutro[posicao("curva_r0")], 0.0);
        // Os seis padrões da vinheta do darktable (início e raio do
        // decaimento, brilho, saturação, proporção e forma) entraram em
        // 2/out/2026, com o módulo desligado.
        assert_eq!(neutro[posicao("darktable_vignette_scale")], 80.0);
        assert_eq!(neutro[posicao("darktable_vignette_brightness")], -0.5);
        assert_eq!(neutro[posicao("darktable_vignette_ativo")], 0.0);
        assert_eq!(neutro.iter().filter(|v| **v != 0.0).count(), 3 + 32 + 16);

        let com_matiz = Ajustes {
            hsl_green_hue: 33.0,
            lens_distortion: -7.0,
            split_shadow_hue: 210.0,
            grain_amount: 40.0,
            ..Default::default()
        }
        .como_vetor();
        assert_eq!(com_matiz[posicao("hsl_green_hue")], 33.0);
        assert_eq!(com_matiz[posicao("lens_distortion")], -7.0);
        assert_eq!(com_matiz[posicao("split_shadow_hue")], 210.0);
        assert_eq!(com_matiz[posicao("grain_amount")], 40.0);
    }

    /// O vetor volta a ser struct — e só com 46 valores.
    #[test]
    fn de_vetor_exige_exatamente_a_quantidade() {
        let original = Ajustes {
            exposure: 0.5,
            hsl_blue_lum: -20.0,
            ..Default::default()
        };
        assert_eq!(Ajustes::de_vetor(&original.como_vetor()), Some(original));
        assert_eq!(Ajustes::de_vetor(&[0.0; QUANTIDADE - 1]), None);
        assert_eq!(Ajustes::de_vetor(&[0.0; QUANTIDADE + 1]), None);
    }

    /// 🔑 No JSON, campo ausente é o neutro — e `contrast` ausente é `1.0`.
    ///
    /// Um `#[serde(default)]` derivado do zero deixaria a foto cinza chapada
    /// sempre que o site mandasse só a exposição.
    #[test]
    fn o_json_por_nome_completa_com_o_neutro() {
        let lido: Ajustes = serde_json::from_str(r#"{"exposure": 0.5, "hsl_red_sat": 40}"#)
            .expect("JSON parcial é válido");
        assert_eq!(lido.exposure, 0.5);
        assert_eq!(lido.hsl_red_sat, 40.0);
        assert_eq!(
            lido.contrast, 1.0,
            "ausente é o neutro, e o neutro do contraste é 1"
        );
        assert_eq!(lido.sharpen_radius, 1.0);

        let ida_e_volta: Ajustes =
            serde_json::from_str(&serde_json::to_string(&lido).expect("serializa"))
                .expect("desserializa");
        assert_eq!(ida_e_volta, lido);
    }

    /// 🔑 Uma revelação gravada **antes** da Tonalização abre sem tonalização.
    ///
    /// O JSON do site guarda só o que difere do neutro, então toda foto revelada
    /// até 2026-09-06 tem um objeto sem os sete campos novos. Ausente é neutro, e
    /// o neutro dos sete é zero — a foto abre exatamente como foi deixada, e não
    /// com um matiz que ninguém escolheu.
    #[test]
    fn a_revelacao_gravada_antes_da_tonalizacao_abre_sem_ela() {
        let antiga: Ajustes = serde_json::from_str(r#"{"exposure": 0.4, "saturation": -1.0}"#)
            .expect("o JSON de antes continua válido");
        assert_eq!(antiga.split_shadow_sat, 0.0);
        assert_eq!(antiga.split_highlight_sat, 0.0);
        assert_eq!(antiga.split_balance, 0.0);
        assert_eq!(antiga.grain_amount, 0.0);
        assert_eq!(antiga.grain_size, 0.0);
        assert_eq!(antiga.exposure, 0.4);
        assert_eq!(antiga.saturation, -1.0);
    }
}
