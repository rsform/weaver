#!/usr/bin/env bash


#cargo install esquema-cli --locked --git https://github.com/fatfingers23/esquema.git
mkdir -p ./target/lexicons
cp -r ./lexicons ./target/lexicons
cp -r ./atproto/lexicons ./target/lexicons


~/.cargo/bin/esquema-cli generate local --lexdir ./target/lexicons/ --outdir ./crates/weaver-common/src/ --module lexicons
