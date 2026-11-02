use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

pub struct AccessKey(String);
impl AccessKey {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("access key must be 64 hexadecimal characters".into());
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(path)
        {
            Ok(mut file) => {
                let key = random_token()?;
                file.write_all(key.as_bytes())
                    .and_then(|_| file.sync_all())
                    .map_err(|e| format!("cannot save access key: {e}"))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("cannot create access key file: {e}")),
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| format!("cannot open access key file: {e}"))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !(64..=65).contains(&metadata.len())
            || !metadata.is_file()
            || metadata.mode() & 0o777 != 0o600
            || metadata.uid() != unsafe { nix::libc::geteuid() }
        {
            return Err(
                "access key file must be a regular file owned by this user with mode 0600".into(),
            );
        }
        let mut value = String::new();
        file.take(66)
            .read_to_string(&mut value)
            .map_err(|_| "cannot read access key file")?;
        Self::parse(value.trim_end_matches('\n'))
    }
}

pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("cannot obtain secure randomness: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone)]
pub struct Authentication(Arc<Inner>);
struct Inner {
    key: AccessKey,
    sessions: Mutex<HashMap<String, Instant>>,
    attempts: Mutex<(Instant, usize)>,
}
#[derive(Clone)]
pub struct AuthenticatedSession(String);
pub enum LoginError {
    Rejected,
    RateLimited,
    Unavailable,
}
impl Authentication {
    pub const SESSION_SECONDS: u64 = 12 * 60 * 60;
    pub fn new(key: AccessKey) -> Self {
        Self(Arc::new(Inner {
            key,
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new((Instant::now(), 0)),
        }))
    }
    pub fn login(&self, supplied: &str) -> Result<AuthenticatedSession, LoginError> {
        let mut attempts = self.0.attempts.lock().unwrap();
        if attempts.0.elapsed() >= Duration::from_secs(60) {
            *attempts = (Instant::now(), 0);
        }
        if attempts.1 >= 20 {
            return Err(LoginError::RateLimited);
        }
        attempts.1 += 1;
        drop(attempts);
        let key = AccessKey::parse(supplied).map_err(|_| LoginError::Rejected)?;
        if !bool::from(self.0.key.0.as_bytes().ct_eq(key.0.as_bytes())) {
            return Err(LoginError::Rejected);
        }
        let token = random_token().map_err(|_| LoginError::Unavailable)?;
        let now = Instant::now();
        let mut sessions = self.0.sessions.lock().unwrap();
        sessions.retain(|_, expires| *expires > now);
        if sessions.len() >= 64 {
            return Err(LoginError::Unavailable);
        }
        sessions.insert(
            token.clone(),
            now + Duration::from_secs(Self::SESSION_SECONDS),
        );
        Ok(AuthenticatedSession(token))
    }
    pub fn authenticate(&self, token: &str) -> Option<AuthenticatedSession> {
        if self
            .0
            .sessions
            .lock()
            .unwrap()
            .get(token)
            .is_some_and(|expires| *expires > Instant::now())
        {
            Some(AuthenticatedSession(token.into()))
        } else {
            None
        }
    }
    pub fn active(&self, session: &AuthenticatedSession) -> bool {
        self.authenticate(&session.0).is_some()
    }
    pub fn logout(&self, session: &AuthenticatedSession) {
        self.0.sessions.lock().unwrap().remove(&session.0);
    }
}
impl AuthenticatedSession {
    pub(crate) fn token(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sessions_expire_and_logout_revokes_only_the_selected_session() {
        let key = "a".repeat(64);
        let auth = Authentication::new(AccessKey::parse(&key).unwrap());
        let one = auth.login(&key).ok().unwrap();
        let two = auth.login(&key).ok().unwrap();
        assert!(auth.active(&one));
        auth.logout(&one);
        assert!(!auth.active(&one));
        assert!(auth.active(&two));
        auth.0
            .sessions
            .lock()
            .unwrap()
            .insert(two.0.clone(), Instant::now());
        assert!(!auth.active(&two));
    }
    #[test]
    fn key_file_is_private_persistent_and_rejects_unsafe_files() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        let first = AccessKey::load_or_create(&path).unwrap();
        assert_eq!(first.0, AccessKey::load_or_create(&path).unwrap().0);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(AccessKey::load_or_create(&path).is_err());
        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(AccessKey::load_or_create(&link).is_err());
    }
}
