#!/usr/bin/env bash
# Compila a tela do cliente do balcão para o navegador e entrega ao site.
#
# Uso: scripts/construir-tela-do-cliente.sh [caminho do frontend do e-commerce]
#      (padrão: $RECORDARFOTOS_FRONTEND, senão ../recordarfotos-e-commerce/frontend)
#
# O que sai, e para onde:
#
#   frontend/public/tela-do-cliente/tela_do_cliente_web.js        o glue do wasm-bindgen (--target web)
#   frontend/public/tela-do-cliente/tela_do_cliente_web_bg.wasm   a tela em wgpu + o revelacao-core
#   frontend/public/tela-do-cliente/VERSAO                o commit deste repositório
#   frontend/src/app/tela-do-cliente/tela_do_cliente_web.d.ts   os tipos do glue
#   frontend/src/app/tela-do-cliente/versao.ts          a mesma VERSAO, importável
#
# 🔑 É o mesmo molde do `construir-biblioteca.sh` e do `construir-web.sh`, e
# pelas mesmas razões: `--target web` dispensa configurar o bundler do Next, e
# os artefatos são commitados no repositório do site porque a Vercel não tem
# Rust.
#
# ⚠️ **O `?v=` não é enfeite**: um glue novo com um `.wasm` velho no cache chama
# exportações que não existem. As duas URLs carregam a versão.
set -euo pipefail

cd "$(dirname "$0")/.."

FRONTEND="${1:-${RECORDARFOTOS_FRONTEND:-../recordarfotos-e-commerce/frontend}}"
if [[ ! -d "$FRONTEND/public" ]]; then
    echo "frontend não encontrado em $FRONTEND (passe o caminho ou RECORDARFOTOS_FRONTEND)" >&2
    exit 1
fi
PUBLICO="$FRONTEND/public/tela-do-cliente"
FONTE="$FRONTEND/src/app/tela-do-cliente"
SAIDA="target/tela-do-cliente-web"

for ferramenta in wasm-pack wasm-opt; do
    command -v "$ferramenta" >/dev/null || {
        echo "falta $ferramenta — brew install wasm-pack binaryen" >&2
        exit 1
    }
done
rustup target add wasm32-unknown-unknown >/dev/null

echo "→ testes do core da revelação (é de onde a foto sai)"
cargo test -q -p revelacao-core --lib

echo "→ wasm-pack build (perfil release-web)"
wasm-pack build crates/tela-do-cliente-web \
    --target web \
    --profile release-web \
    --out-dir "../../$SAIDA" \
    --out-name tela_do_cliente_web

echo "→ wasm-opt -Oz"
mkdir -p "$PUBLICO" "$FONTE"
wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$PUBLICO/tela_do_cliente_web_bg.wasm" "$SAIDA/tela_do_cliente_web_bg.wasm"
cp "$SAIDA/tela_do_cliente_web.js" "$PUBLICO/tela_do_cliente_web.js"
cp "$SAIDA/tela_do_cliente_web.d.ts" "$FONTE/tela_do_cliente_web.d.ts"

echo "→ VERSAO"
VERSAO="$(git rev-parse --short HEAD)"
if [[ -n "$(git status --porcelain crates/tela-do-cliente-web crates/revelacao-core)" ]]; then
    VERSAO="$VERSAO-sujo"
fi
echo "$VERSAO" > "$PUBLICO/VERSAO"
cat > "$FONTE/versao.ts" <<TS
// Gerado por scripts/construir-tela-do-cliente.sh do VintageLightbox-Rust — não editar.
// É o commit da tela do cliente que está em public/tela-do-cliente/, e vai na URL dos
// dois arquivos (\`?v=\`) para o glue e o .wasm nunca descasarem no cache.
export const VERSAO = "$VERSAO";
TS

echo
ls -la "$PUBLICO"
echo
echo "tela do cliente $VERSAO entregue em $PUBLICO"
