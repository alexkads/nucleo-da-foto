#!/usr/bin/env bash
# Compila o motor de revelação para o navegador e entrega ao site.
#
# Uso: scripts/construir-web.sh [caminho do frontend do e-commerce]
#      (padrão: $RECORDARFOTOS_FRONTEND, senão ../recordarfotos-e-commerce/frontend)
#
# O que sai, e para onde:
#
#   frontend/public/revelacao/revelacao_web.js        o glue do wasm-bindgen (--target web)
#   frontend/public/revelacao/revelacao_web_bg.wasm   o motor
#   frontend/public/revelacao/nomes.json              os 46 nomes, na ordem do uniform
#   frontend/public/revelacao/VERSAO                  o commit deste repositório
#   frontend/src/.../revelacao/revelacao_web.d.ts     os tipos do glue, para o TypeScript
#   frontend/src/.../revelacao/versao.ts              a mesma VERSAO, importável
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
PUBLICO="$FRONTEND/public/revelacao"
FONTE="$FRONTEND/src/app/(dashboard)/dashboard/sessoes-fotograficas/[id]/revelacao"
SAIDA="target/revelacao-web"

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
wasm-pack build crates/revelacao-web \
    --target web \
    --profile release-web \
    --out-dir "../../$SAIDA" \
    --out-name revelacao_web

echo "→ wasm-opt -O3"
mkdir -p "$PUBLICO" "$FONTE"
wasm-opt -O3 --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$PUBLICO/revelacao_web_bg.wasm" "$SAIDA/revelacao_web_bg.wasm"
cp "$SAIDA/revelacao_web.js" "$PUBLICO/revelacao_web.js"
cp "$SAIDA/revelacao_web.d.ts" "$FONTE/revelacao_web.d.ts"

echo "→ nomes.json e VERSAO"
cargo run -q -p revelacao-core --bin nomes-dos-ajustes > "$PUBLICO/nomes.json"
VERSAO="$(git rev-parse --short HEAD)"
if [[ -n "$(git status --porcelain crates/revelacao-core crates/revelacao-web)" ]]; then
    VERSAO="$VERSAO-sujo"
fi
echo "$VERSAO" > "$PUBLICO/VERSAO"
cat > "$FONTE/versao.ts" <<TS
// Gerado por scripts/construir-web.sh do VintageLightbox-Rust — não editar.
// É o commit do motor que está em public/revelacao/, e vai na URL dos dois
// arquivos (\`?v=\`) para o glue e o .wasm nunca descasarem no cache.
export const VERSAO = "$VERSAO";
TS

echo
ls -la "$PUBLICO"
echo
echo "motor $VERSAO entregue em $PUBLICO"
