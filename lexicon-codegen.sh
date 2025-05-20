#!/usr/bin/env bash


#cargo install esquema-cli --locked --git https://github.com/fatfingers23/esquema.git

rm -rf ./target/lexicons
mkdir -p ./target/lexicons
cd target
git clone -n --depth=1 --filter=tree:0 \
https://github.com/bluesky-social/atproto.git
cd atproto
git sparse-checkout set --no-cone /lexicons
git checkout
cd ..

cp -r ../lexicons ./
cp -r ./atproto/lexicons ./


~/.cargo/bin/esquema-cli generate local --lexdir ./lexicons/ --outdir ../crates/weaver-common/src/ --module lexicons
