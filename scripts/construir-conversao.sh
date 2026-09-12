#!/usr/bin/env bash
# Compila o conversor de foto para o navegador do CLIENTE e entrega ao site.
#
# 🔑 **Por que ele é separado do motor de revelação**: este wasm não tem GPU
# nenhuma — 0,5 MB contra 3,3 MB —, e quem o baixa é o cliente, no celular,
# depois de comprar uma foto. Ele faz uma coisa só: abrir o WebP que o acervo
# guarda e devolver o JPEG de 300 DPI que a gráfica aceita.
#
# Uso: scripts/construir-web.sh [caminho do frontend do e-commerce]
#      (padrão: $RECORDARFOTOS_FRONTEND, senão ../recordarfotos-e-commerce/frontend)
#
# O que sai, e para onde:
#
#   frontend/public/conversao/conversao_web.js        o glue do wasm-bindgen (--target web)
#   frontend/public/conversao/conversao_web_bg.wasm   o conversor (~0,5 MB)
#   frontend/public/conversao/VERSAO                  o commit deste repositório
#   frontend/src/.../conversao/conversao_web.d.ts     os tipos do glue, para o TypeScript
#   frontend/src/.../conversao/versao.ts              a mesma VERSAO, importável
#
# 🔑 `--target web` é o que dispensa configurar o bundler do Next: o glue
# exporta `default(init)` e não tem `import` de nada. O site carrega os dois
# arquivos em runtime, de `public/`, com `?v=VERSAO` para o glue e o `.wasm`
# nunca descasarem no cache do navegador.
#
# Os artefatos são commitados no repositório do site: a Vercel não tem Rust.
set -euo pipefail

cd "$(dirname "$0")/.."

FRONTEND="${1:-${RECORDARFOTOS_FRONTEND:-../recordarfotos-e-commerce/frontend}}"
if [[ ! -d "$FRONTEND/public" ]]; then
    echo "frontend não encontrado em $FRONTEND (passe o caminho ou RECORDARFOTOS_FRONTEND)" >&2
    exit 1
fi
PUBLICO="$FRONTEND/public/conversao"
# Os tipos e a VERSAO vão para onde a galeria do cliente os importa.
FONTE="$FRONTEND/src/app/(app)/meus-ensaios/conversao"
SAIDA="target/conversao-web"

for ferramenta in wasm-pack wasm-opt; do
    command -v "$ferramenta" >/dev/null || {
        echo "falta $ferramenta — brew install wasm-pack binaryen" >&2
        exit 1
    }
done
rustup target add wasm32-unknown-unknown >/dev/null

echo "→ wasm-pack build (perfil release-web)"
# `--profile` é o do cargo (`[profile.release-web]` no Cargo.toml do workspace);
# não se passa `--release` junto, o wasm-pack recusa os dois.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+simd128" \
wasm-pack build crates/conversao-web \
    --target web \
    --profile release-web \
    --out-dir "../../$SAIDA" \
    --out-name conversao_web

echo "→ wasm-opt -O3"
mkdir -p "$PUBLICO" "$FONTE"
wasm-opt -O3 --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$PUBLICO/conversao_web_bg.wasm" "$SAIDA/conversao_web_bg.wasm"
cp "$SAIDA/conversao_web.js" "$PUBLICO/conversao_web.js"
cp "$SAIDA/conversao_web.d.ts" "$FONTE/conversao_web.d.ts"

echo "→ VERSAO"
VERSAO="$(git rev-parse --short HEAD)"
# 🚨 **O perfil e este script entram na conta do "sujo".** Antes só os dois
# crates contavam, e isso bastava enquanto a compilação era sempre a mesma. Não
# é mais: o `opt-level` do `[profile.release-web]` e o `+simd128` daqui mudam o
# `.wasm` **sem mudar uma linha de Rust** — e o site agora serve o motor com
# `Cache-Control: immutable`, onde uma VERSAO que não muda é um wasm velho que
# nunca mais se desfaz no navegador de quem já o baixou.
if [[ -n "$(git status --porcelain crates/foto-codec crates/conversao-web Cargo.toml "$0")" ]]; then
    VERSAO="$VERSAO-sujo"
fi
echo "$VERSAO" > "$PUBLICO/VERSAO"
cat > "$FONTE/versao.ts" <<TS
// Gerado por scripts/construir-web.sh do VintageLightbox-Rust — não editar.
// É o commit do motor que está em public/conversao/, e vai na URL dos dois
// arquivos (\`?v=\`) para o glue e o .wasm nunca descasarem no cache.
export const VERSAO = "$VERSAO";
TS

echo
ls -la "$PUBLICO"
echo
echo "motor $VERSAO entregue em $PUBLICO"
