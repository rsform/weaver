pub async fn serve<T, E>(
    namespace: String,
) -> Result<atrium_xrpc::OutputDataOrBytes<T>, atrium_xrpc::Error<E>>
where
    T: serde::de::DeserializeOwned,
    E: serde::de::DeserializeOwned + std::fmt::Debug,
{
    todo!()
}
