use std::{
    error::Error,
    fs,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use openssl::{
    asn1::Asn1Time,
    pkey::{PKey, Private},
    sha::sha256,
    ssl::{SslAcceptor, SslMethod},
    stack::Stack,
    x509::{
        store::X509StoreBuilder, verify::X509VerifyFlags, X509PurposeId, X509StoreContext, X509,
    },
};
use pingora::{
    listeners::TlsAccept, protocols::tls::TlsRef, server::ShutdownWatch,
    services::background::BackgroundService,
};
use tracing::{error, info};

type ReloadResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct Certificate {
    chain: Vec<X509>,
    key: PKey<Private>,
    fingerprint: ([u8; 32], [u8; 32]),
}

#[derive(Clone)]
pub struct CertificateReloader {
    cert_path: PathBuf,
    key_path: PathBuf,
    current: Arc<RwLock<Arc<Certificate>>>,
}

impl CertificateReloader {
    pub fn new(cert_path: PathBuf, key_path: PathBuf) -> ReloadResult<Self> {
        let (cert_pem, key_pem) = Self::read_pair(&cert_path, &key_path)?;
        let initial = Self::parse_pair(&cert_pem, &key_pem)?;
        Ok(Self {
            cert_path,
            key_path,
            current: Arc::new(RwLock::new(Arc::new(initial))),
        })
    }

    fn read_pair(cert_path: &PathBuf, key_path: &PathBuf) -> ReloadResult<(Vec<u8>, Vec<u8>)> {
        // Resolve the projection again on every poll: Kubernetes replaces ..data,
        // not the symlinks or inodes that a file watcher might still be following.
        let cert_target = fs::canonicalize(cert_path)?;
        let key_target = fs::canonicalize(key_path)?;
        let cert_pem = fs::read(&cert_target)?;
        let key_pem = fs::read(&key_target)?;
        if fs::canonicalize(cert_path)? != cert_target || fs::canonicalize(key_path)? != key_target
        {
            return Err("TLS files changed during reload; retrying on next poll".into());
        }
        Ok((cert_pem, key_pem))
    }

    fn parse_pair(cert_pem: &[u8], key_pem: &[u8]) -> ReloadResult<Certificate> {
        let chain = X509::stack_from_pem(cert_pem)?;
        let leaf = chain.first().ok_or("empty certificate chain")?;
        let now = Asn1Time::days_from_now(0)?;
        for cert in &chain {
            if cert.not_before() > now || cert.not_after() < now {
                return Err("TLS certificate is outside its validity period".into());
            }
        }
        for pair in chain.windows(2) {
            let issuer_key = pair[1].public_key()?;
            if pair[0].issuer_name().try_cmp(pair[1].subject_name())? != std::cmp::Ordering::Equal
                || !pair[0].verify(&issuer_key)?
            {
                return Err("TLS certificate chain has an invalid issuer or signature".into());
            }
        }
        // Servers commonly omit the root. Anchor at the last supplied certificate
        // to check path constraints without requiring a client trust store here.
        // This checks chain consistency, not authorization to serve a hostname.
        let mut store = X509StoreBuilder::new()?;
        store.add_cert(chain.last().ok_or("empty certificate chain")?.clone())?;
        store.set_flags(X509VerifyFlags::PARTIAL_CHAIN)?;
        store.set_purpose(X509PurposeId::SSL_SERVER)?;
        let mut intermediates = Stack::new()?;
        for cert in chain.iter().skip(1) {
            intermediates.push(cert.clone())?;
        }
        let mut context = X509StoreContext::new()?;
        if !context.init(&store.build(), leaf, &intermediates, |ctx| {
            ctx.verify_cert()
        })? {
            return Err(format!("invalid TLS certificate chain: {}", context.error()).into());
        }
        let key = PKey::private_key_from_pem(key_pem)?;
        // Validate the complete candidate before publishing any of it to handshakes.
        let mut validation = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls())?;
        validation.set_certificate(leaf)?;
        validation.set_private_key(&key)?;
        validation.check_private_key()?;
        for intermediate in chain.iter().skip(1) {
            validation.add_extra_chain_cert(intermediate.clone())?;
        }
        Ok(Certificate {
            chain,
            key,
            fingerprint: (sha256(cert_pem), sha256(key_pem)),
        })
    }

    fn reload(&self) -> ReloadResult<bool> {
        let (cert_pem, key_pem) = Self::read_pair(&self.cert_path, &self.key_path)?;
        let fingerprint = (sha256(&cert_pem), sha256(&key_pem));
        if fingerprint
            == self
                .current
                .read()
                .map_err(|_| "TLS certificate lock poisoned")?
                .fingerprint
        {
            return Ok(false);
        }
        let candidate = Self::parse_pair(&cert_pem, &key_pem)?;
        *self
            .current
            .write()
            .map_err(|_| "TLS certificate lock poisoned")? = Arc::new(candidate);
        Ok(true)
    }
}

#[async_trait]
impl TlsAccept for CertificateReloader {
    async fn certificate_callback(&self, ssl: &mut TlsRef) {
        let certificate = match self.current.read() {
            Ok(current) => Arc::clone(&current),
            Err(_) => {
                error!("TLS certificate lock poisoned");
                return;
            }
        };
        let result = (|| {
            ssl.set_certificate(&certificate.chain[0])?;
            ssl.set_private_key(&certificate.key)?;
            for intermediate in certificate.chain.iter().skip(1) {
                ssl.add_chain_cert(intermediate.clone())?;
            }
            Ok::<_, openssl::error::ErrorStack>(())
        })();
        if let Err(error) = result {
            error!(%error, "could not configure TLS handshake");
        }
    }
}

#[async_trait]
impl BackgroundService for CertificateReloader {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {
                    if *shutdown.borrow() { break; }
                    let reloader = self.clone();
                    match tokio::task::spawn_blocking(move || reloader.reload()).await {
                        Ok(Ok(true)) => info!("TLS certificate reloaded"),
                        Ok(Ok(false)) => {},
                        Ok(Err(error)) => error!(%error, "TLS reload failed; retaining previous certificate"),
                        Err(error) => error!(%error, "TLS reload task failed; retaining previous certificate"),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openssl::{
        asn1::Asn1Time,
        bn::BigNum,
        hash::MessageDigest,
        rsa::Rsa,
        ssl::{Ssl, SslConnector, SslStream, SslVerifyMode},
        x509::X509NameBuilder,
    };
    use std::{
        net::{TcpListener, TcpStream},
        os::unix::fs::symlink,
    };
    use tempfile::TempDir;

    fn pair(serial: u32) -> (Vec<u8>, Vec<u8>) {
        pair_with_dates(serial, 0, 30)
    }

    fn pair_with_dates(serial: u32, start: i64, end: i64) -> (Vec<u8>, Vec<u8>) {
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "localhost").unwrap();
        let name = name.build();
        let mut cert = X509::builder().unwrap();
        cert.set_version(2).unwrap();
        cert.set_serial_number(&BigNum::from_u32(serial).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        cert.set_subject_name(&name).unwrap();
        cert.set_issuer_name(&name).unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&Asn1Time::from_unix(timestamp() + start * 86400).unwrap())
            .unwrap();
        cert.set_not_after(&Asn1Time::from_unix(timestamp() + end * 86400).unwrap())
            .unwrap();
        cert.sign(&key, MessageDigest::sha256()).unwrap();
        (
            cert.build().to_pem().unwrap(),
            key.private_key_to_pem_pkcs8().unwrap(),
        )
    }

    fn write_pair(dir: &std::path::Path, pair: &(Vec<u8>, Vec<u8>)) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("tls.crt"), &pair.0).unwrap();
        fs::write(dir.join("tls.key"), &pair.1).unwrap();
    }

    fn timestamp() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn connect(reloader: &CertificateReloader) -> String {
        connect_with_trust(reloader, None)
    }

    fn connect_with_trust(reloader: &CertificateReloader, trusted: Option<&X509>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let reloader = reloader.clone();
        let server = std::thread::spawn(move || {
            let acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls())
                .unwrap()
                .build();
            let mut ssl = Ssl::new(acceptor.context()).unwrap();
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(reloader.certificate_callback(&mut ssl));
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            tcp.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut stream = SslStream::new(ssl, tcp).unwrap();
            stream.accept().unwrap();
        });
        let mut connector = SslConnector::builder(SslMethod::tls()).unwrap();
        // Ephemeral self-signed fixtures; the test checks certificate replacement.
        if let Some(ca) = trusted {
            connector.cert_store_mut().add_cert(ca.clone()).unwrap();
        } else {
            connector.set_verify(SslVerifyMode::NONE);
        }
        let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        tcp.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let stream = connector.build().connect("localhost", tcp).unwrap();
        let serial = stream
            .ssl()
            .peer_certificate()
            .unwrap()
            .serial_number()
            .to_bn()
            .unwrap()
            .to_dec_str()
            .unwrap()
            .to_string();
        server.join().unwrap();
        serial
    }

    #[test]
    fn projected_secret_swap_changes_new_handshakes() {
        let dir = TempDir::new().unwrap();
        write_pair(&dir.path().join("generation1"), &pair(1));
        write_pair(&dir.path().join("generation2"), &pair(2));
        symlink("generation1", dir.path().join("..data")).unwrap();
        symlink("..data/tls.crt", dir.path().join("tls.crt")).unwrap();
        symlink("..data/tls.key", dir.path().join("tls.key")).unwrap();
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        assert_eq!(connect(&reloader), "1");
        assert!(!reloader.reload().unwrap());
        symlink("generation2", dir.path().join("..data_tmp")).unwrap();
        fs::rename(dir.path().join("..data_tmp"), dir.path().join("..data")).unwrap();
        assert!(reloader.reload().unwrap());
        assert_eq!(connect(&reloader), "2");
    }

    #[test]
    fn invalid_missing_and_mismatched_updates_keep_previous_pair_then_recover() {
        let dir = TempDir::new().unwrap();
        let first = pair(1);
        let second = pair(2);
        write_pair(dir.path(), &first);
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        fs::write(dir.path().join("tls.crt"), b"broken").unwrap();
        assert!(reloader.reload().is_err());
        assert_eq!(connect(&reloader), "1");
        fs::remove_file(dir.path().join("tls.crt")).unwrap();
        assert!(reloader.reload().is_err());
        assert_eq!(connect(&reloader), "1");
        fs::write(dir.path().join("tls.crt"), &second.0).unwrap();
        assert!(reloader.reload().is_err());
        assert_eq!(connect(&reloader), "1");
        fs::write(dir.path().join("tls.key"), &second.1).unwrap();
        assert!(reloader.reload().unwrap());
        assert_eq!(connect(&reloader), "2");
    }

    #[tokio::test]
    async fn background_service_reloads_and_stops_on_shutdown() {
        let dir = TempDir::new().unwrap();
        write_pair(dir.path(), &pair(1));
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        let service = reloader.clone();
        let (shutdown, receiver) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move { service.start(receiver).await });
        write_pair(dir.path(), &pair(2));
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let serial = reloader.current.read().unwrap().chain[0]
                    .serial_number()
                    .to_bn()
                    .unwrap()
                    .to_dec_str()
                    .unwrap()
                    .to_string();
                if serial == "2" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        shutdown.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn expired_and_future_replacements_keep_the_working_certificate() {
        let dir = TempDir::new().unwrap();
        write_pair(dir.path(), &pair(1));
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        for replacement in [pair_with_dates(2, -30, -1), pair_with_dates(3, 1, 30)] {
            write_pair(dir.path(), &replacement);
            assert!(reloader.reload().is_err());
            assert_eq!(connect(&reloader), "1");
        }
    }

    fn chain_fixture() -> ((Vec<u8>, Vec<u8>), X509) {
        chain_fixture_with_options(true, false)
    }

    fn chain_fixture_with_options(is_ca: bool, mixed_names: bool) -> ((Vec<u8>, Vec<u8>), X509) {
        use openssl::{
            asn1::Asn1Type,
            x509::extension::{BasicConstraints, SubjectAlternativeName},
        };
        let ca_key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let leaf_key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text_with_type("CN", "test CA", Asn1Type::PRINTABLESTRING)
            .unwrap();
        let name = name.build();
        let mut ca = X509::builder().unwrap();
        ca.set_version(2).unwrap();
        ca.set_serial_number(&BigNum::from_u32(100).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        ca.set_subject_name(&name).unwrap();
        ca.set_issuer_name(&name).unwrap();
        ca.set_pubkey(&ca_key).unwrap();
        ca.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        ca.set_not_after(&Asn1Time::days_from_now(30).unwrap())
            .unwrap();
        let mut constraints = BasicConstraints::new();
        constraints.critical();
        if is_ca {
            constraints.ca();
        }
        ca.append_extension(constraints.build().unwrap()).unwrap();
        ca.sign(&ca_key, MessageDigest::sha256()).unwrap();
        let ca = ca.build();
        let mut leaf = X509::builder().unwrap();
        leaf.set_version(2).unwrap();
        leaf.set_serial_number(&BigNum::from_u32(200).unwrap().to_asn1_integer().unwrap())
            .unwrap();
        let mut subject = X509NameBuilder::new().unwrap();
        subject.append_entry_by_text("CN", "localhost").unwrap();
        leaf.set_subject_name(&subject.build()).unwrap();
        if mixed_names {
            let mut issuer = X509NameBuilder::new().unwrap();
            issuer
                .append_entry_by_text_with_type("CN", "test CA", Asn1Type::UTF8STRING)
                .unwrap();
            let issuer = issuer.build();
            assert_ne!(
                issuer.to_der().unwrap(),
                ca.subject_name().to_der().unwrap()
            );
            leaf.set_issuer_name(&issuer).unwrap();
        } else {
            leaf.set_issuer_name(ca.subject_name()).unwrap();
        }
        leaf.set_pubkey(&leaf_key).unwrap();
        leaf.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        leaf.set_not_after(&Asn1Time::days_from_now(30).unwrap())
            .unwrap();
        let san = SubjectAlternativeName::new()
            .dns("localhost")
            .build(&leaf.x509v3_context(Some(&ca), None))
            .unwrap();
        leaf.append_extension(san).unwrap();
        leaf.sign(&ca_key, MessageDigest::sha256()).unwrap();
        let mut pem = leaf.build().to_pem().unwrap();
        pem.extend(ca.to_pem().unwrap());
        ((pem, leaf_key.private_key_to_pem_pkcs8().unwrap()), ca)
    }

    #[test]
    fn unrelated_issuer_is_rejected_and_previous_chain_still_verifies() {
        let dir = TempDir::new().unwrap();
        let (valid, ca) = chain_fixture();
        write_pair(dir.path(), &valid);
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        assert_eq!(connect_with_trust(&reloader, Some(&ca)), "200");
        let (_, other_ca) = chain_fixture();
        let mut bad_chain = X509::stack_from_pem(&valid.0).unwrap()[0].to_pem().unwrap();
        bad_chain.extend(other_ca.to_pem().unwrap());
        write_pair(dir.path(), &(bad_chain, valid.1));
        assert!(reloader.reload().is_err());
        assert_eq!(connect_with_trust(&reloader, Some(&ca)), "200");
    }

    #[test]
    fn equivalent_issuer_encodings_are_accepted() {
        let dir = TempDir::new().unwrap();
        let (valid, ca) = chain_fixture_with_options(true, true);
        write_pair(dir.path(), &valid);
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        assert_eq!(connect_with_trust(&reloader, Some(&ca)), "200");
    }

    #[test]
    fn non_ca_issuer_cannot_replace_a_working_chain() {
        let dir = TempDir::new().unwrap();
        let (valid, ca) = chain_fixture();
        write_pair(dir.path(), &valid);
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        write_pair(dir.path(), &chain_fixture_with_options(false, false).0);
        assert!(reloader.reload().is_err());
        assert_eq!(connect_with_trust(&reloader, Some(&ca)), "200");
    }

    #[test]
    fn shipped_example_certificate_passes_startup_and_hostname_verification() {
        let manifest = include_str!("../../examples/manifest.yaml");
        let extract = |field: &str| -> Vec<u8> {
            let marker = format!("  {field}: |\n");
            let block = manifest.split_once(&marker).unwrap().1;
            block
                .lines()
                .take_while(|line| line.starts_with("    "))
                .map(|line| format!("{}\n", &line[4..]))
                .collect::<String>()
                .into_bytes()
        };
        let fixture = (extract("localhost.crt"), extract("localhost.key"));
        let cert = X509::from_pem(&fixture.0).unwrap();
        let dir = TempDir::new().unwrap();
        write_pair(dir.path(), &fixture);
        let reloader =
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .unwrap();
        assert_eq!(
            connect_with_trust(&reloader, Some(&cert)),
            cert.serial_number()
                .to_bn()
                .unwrap()
                .to_dec_str()
                .unwrap()
                .to_string()
        );
    }

    #[test]
    fn invalid_initial_pair_fails_startup() {
        let dir = TempDir::new().unwrap();
        assert!(
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .is_err()
        );
        write_pair(dir.path(), &(Vec::new(), pair(1).1));
        assert!(
            CertificateReloader::new(dir.path().join("tls.crt"), dir.path().join("tls.key"))
                .is_err()
        );
    }
}
