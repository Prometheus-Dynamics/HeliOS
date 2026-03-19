pub(crate) fn build_http_client(user_agent: &str) -> reqwest::Result<reqwest::Client> {
    let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().map(|cert| reqwest::Certificate::from_der(cert.as_ref())).collect::<Result<Vec<_>, _>>()?;

    reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(3)).user_agent(user_agent).tls_certs_only(roots).build()
}
