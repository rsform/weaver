// @generated - This file is generated by esquema-codegen (forked from atrium-codegen). DO NOT EDIT.
//!Definitions for the `sh.weaver.embed.defs` namespace.
///Proportional size of the embed relative to the viewport in larger windows. The dimensions are percentage out of 100. Could we use more granularity? Maybe, but come on.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PercentSizeData {
    pub height: i64,
    pub width: i64,
}
pub type PercentSize = atrium_api::types::Object<PercentSizeData>;
///Pixel-exact embed size. The dimensions are logical pixels, subject to scaling, so 200px at X2 scale is 400px.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PixelSizeData {
    pub height: i64,
    pub width: i64,
}
pub type PixelSize = atrium_api::types::Object<PixelSizeData>;
