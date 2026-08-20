pub(crate) fn invalid_data(
    error: impl std::error::Error + Send + Sync + 'static,
) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
}
